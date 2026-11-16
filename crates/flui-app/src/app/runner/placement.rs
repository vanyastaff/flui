//! The desktop runner's window placements: read from the application's storage
//! before a window opens, recorded while it lives, and written when it moves,
//! closes or the session ends (the model is
//! [`placement_recorder`](crate::app::placement_recorder)).
//!
//! The storage is the one the run's capability registry provides, the
//! application's override of `StorageCapability` when it has one: the host
//! acquires it through [`HostCapabilities`], as a widget would, with no window.
//! Nothing here blocks the owner thread on storage, except the final flush at
//! the end of the loop or of the session, within that end's deadline.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Wake, Waker};
use std::time::Instant;

use flui_foundation::panic::{FirstPanic, drop_contained};
use flui_foundation::{MonotonicClock, PresentationAddress, SystemClock};
use flui_platform::{DisplaySnapshot, StorageName, WindowOptions};
use flui_runtime::capability::{CapabilityRegistry, HostCapabilities};
use flui_view::persist::{StorageCapability, StorageHandle};
use rustc_hash::FxHashSet;

use super::desktop::OpeningWindow;
use super::host::{APP_RUNTIME, with_owner_platform};
use crate::app::AppConfig;
use crate::app::placement_recorder::{
    FlushState, PlacementRecall, PlacementRecorder, RECALL_BOUND, Recalled, earliest_due,
    restore_into, settle,
};

/// The run's window placements, held by the application runtime for the
/// loop's life.
pub(in crate::app) struct PlacementHost {
    /// The clock the placement read's bound and the write interval are
    /// measured on: the system's, a manual one in tests.
    clock: Rc<dyn MonotonicClock>,
    /// Where the host acquires the application's storage, from the launch
    /// registry; `None` before a loop installs it and after it retires.
    capabilities: Option<Rc<HostCapabilities>>,
    /// The names a window holds, open or opening.
    claims: Claims,
    /// The open windows that remember their placement, by presentation.
    live: Vec<LiveWindow>,
    /// The windows that closed while their last placement was being written.
    closing: Vec<Rc<PlacementRecorder>>,
    /// Windows that wait for their placement read before they open.
    awaiting: Vec<AwaitingOpen>,
}

struct LiveWindow {
    address: PresentationAddress,
    recorder: Rc<PlacementRecorder>,
    /// Released when the window closes, so the next window may take the name.
    _claim: NameClaim,
}

/// A window waiting for its placement read, and what opens it once the read
/// settles.
struct AwaitingOpen {
    opening: OpeningWindow,
    open: Box<dyn FnOnce(OpeningWindow)>,
}

impl Default for PlacementHost {
    fn default() -> Self {
        Self {
            clock: Rc::new(SystemClock),
            capabilities: None,
            claims: Claims::default(),
            live: Vec::new(),
            closing: Vec::new(),
            awaiting: Vec::new(),
        }
    }
}

impl PlacementHost {
    /// When the owner should wake for the placements, seen from `now`: a held
    /// write whose interval ends, a write storage answered, or a window whose
    /// wait for its read ends.
    pub(in crate::app) fn next_due(&self) -> Option<Instant> {
        let now = self.clock.now();
        let writes = earliest_due(
            now,
            self.live
                .iter()
                .map(|window| &window.recorder)
                .chain(&self.closing),
        );
        let reads = self
            .awaiting
            .iter()
            .filter_map(|awaiting| awaiting.opening.recall_deadline())
            .min();
        writes.into_iter().chain(reads).min()
    }

    /// Measure placements on `clock` from now on.
    #[cfg(all(test, feature = "testing"))]
    pub(in crate::app) fn set_clock(&mut self, clock: Rc<dyn MonotonicClock>) {
        self.clock = clock;
    }
}

/// The host's placement clock, now.
pub(super) fn now() -> Instant {
    let clock = APP_RUNTIME.with(|slot| Rc::clone(&slot.borrow().placement.clock));
    clock.now()
}

/// The loop starts: the host acquires storage from `registry`, the one the
/// run's conflicts were checked against, and what a provider posts back
/// wakes the owner. Forgets what an earlier loop on this thread left.
pub(super) fn install(registry: CapabilityRegistry) {
    let Some(proxy) = with_owner_platform(flui_native::OwnerPlatform::proxy) else {
        return;
    };
    let capabilities = Rc::new(HostCapabilities::new(registry, move || {
        let _ = proxy.wake();
    }));
    let replaced = APP_RUNTIME.with(|slot| {
        let placement = &mut slot.borrow_mut().placement;
        (
            placement.capabilities.replace(capabilities),
            std::mem::take(&mut placement.live),
            std::mem::take(&mut placement.closing),
            std::mem::take(&mut placement.awaiting),
        )
    });
    drop_contained(replaced);
}

/// What a window that remembers its placement opens with: the storage it is
/// kept in, its name, held for this window alone, and the read of it.
pub(super) struct RememberedPlacement {
    storage: StorageHandle,
    claim: NameClaim,
    recall: PlacementRecall,
}

impl RememberedPlacement {
    /// What `config`'s window remembers: the read of its placement, started
    /// now from the application's storage. `None` when the configuration asks
    /// for no placement, when another window holds its name, or when the run
    /// has no storage to keep it in.
    ///
    /// A window that closed this run under the same name leaves its newest
    /// placement in memory: the window opens from that, which no read racing
    /// the closed window's last write could give.
    pub(super) fn recall(config: &AppConfig) -> Option<Self> {
        let name = config.placement?;
        let (claim, fresher, capabilities, clock) = APP_RUNTIME.with(|slot| {
            let runtime = slot.borrow();
            let placement = &runtime.placement;
            let fresher = placement
                .closing
                .iter()
                .rev()
                .find(|recorder| recorder.name() == name)
                .and_then(|recorder| recorder.latest());
            (
                placement.claims.claim(name),
                fresher,
                placement.capabilities.clone(),
                Rc::clone(&placement.clock),
            )
        });
        let Some(claim) = claim else {
            tracing::warn!(
                storage = name.as_str(),
                "another open window remembers its placement under this name; this window \
                 opens from its configuration and is not remembered"
            );
            return None;
        };
        let storage = acquire_storage(capabilities.as_deref(), name)?;
        let recall = match fresher {
            Some(bytes) => PlacementRecall::settled(name, Recalled::of_bytes(&bytes, &name)),
            None => PlacementRecall::start(&storage, name, clock.now(), owner_waker()?),
        };
        if recall.deadline().is_some() {
            wake_owner_at_bound();
        }
        Some(Self {
            storage,
            claim,
            recall,
        })
    }

    /// Whether the read settled, taking storage's answer or giving up on it at
    /// the bound.
    pub(super) fn poll(&mut self) -> bool {
        self.recall.poll(now()).is_some()
    }

    /// When the window stops waiting for the read; `None` once it settled.
    pub(super) fn deadline(&self) -> Option<Instant> {
        self.recall.deadline()
    }

    /// Open `options` where the window was last time, on `owner`; nothing when
    /// the read found no placement or did not settle.
    ///
    /// The fit uses what the platform says of its windows before one exists:
    /// the capabilities (a platform that cannot place a window at open keeps
    /// size and mode only) and the frame a decorated window adds to its
    /// content size.
    pub(super) fn restore_into(
        &self,
        options: &mut WindowOptions,
        owner: &flui_native::OwnerPlatform,
    ) {
        let Some(placement) = self.recall.outcome().and_then(Recalled::placement) else {
            return;
        };
        let Ok(displays) = owner.display_snapshot() else {
            tracing::debug!(
                storage = self.recall.name().as_str(),
                "no displays to fit the stored window placement to"
            );
            return;
        };
        restore_into(
            options,
            placement,
            &displays,
            &owner.window_capabilities(),
            |scale| owner.window_frame_inset(scale),
        );
    }

    /// Start recording the placement of the window presented at `address`,
    /// which opened with this.
    pub(super) fn keep(self, address: PresentationAddress) {
        let Self {
            storage,
            claim,
            recall,
        } = self;
        let name = recall.name();
        let baseline = recall
            .outcome()
            .map_or(Recalled::TimedOut.baseline(), Recalled::baseline);
        drop_contained(recall);
        let Some(handle) =
            APP_RUNTIME.with(|slot| slot.borrow().window_commands.handle_for(address))
        else {
            return;
        };
        // Storage answers on its own thread: wake the owner, which takes the answer
        // at its next turn.
        let Some(proxy) = with_owner_platform(flui_native::OwnerPlatform::proxy) else {
            return;
        };
        let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.wake();
        });
        let recorder = Rc::new(PlacementRecorder::new(storage, name, baseline, wake_owner));
        let weak = Rc::downgrade(&recorder);
        let Ok((state, subscription)) = handle.subscribe(move |state| {
            if let Some(recorder) = weak.upgrade() {
                recorder.observe(state, &current_displays(), now());
            }
        }) else {
            return;
        };
        recorder.hold(subscription);
        recorder.observe(&state, &current_displays(), now());
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut().placement.live.push(LiveWindow {
                address,
                recorder,
                _claim: claim,
            });
        });
    }
}

/// The storage the run's registry provides, acquired for the host now. A
/// refusal or a panic of the provider costs the window its placement and
/// nothing more.
fn acquire_storage(
    capabilities: Option<&HostCapabilities>,
    name: StorageName,
) -> Option<StorageHandle> {
    let Some(capabilities) = capabilities else {
        tracing::debug!(
            storage = name.as_str(),
            "no loop has installed the host's capabilities; the window is not remembered"
        );
        return None;
    };
    let mut first = FirstPanic::new();
    let acquired = first.contain(|| capabilities.acquire::<StorageCapability>());
    if let Some(payload) = first.into_payload() {
        tracing::error!(
            storage = name.as_str(),
            panic = flui_foundation::panic::payload_text(payload.as_ref()),
            "the storage provider panicked; the window opens from its configuration"
        );
        flui_foundation::panic::discard_payload(payload);
    }
    match acquired? {
        Ok(storage) => Some(storage),
        Err(refused) => {
            tracing::warn!(
                storage = name.as_str(),
                %refused,
                "a window asks to remember its placement but the application has no storage; \
                 give it an identifier, set `AppConfig::storage_dir` or provide \
                 `StorageCapability`"
            );
            None
        }
    }
}

/// A waker that wakes the owner loop, from any thread.
fn owner_waker() -> Option<Waker> {
    let proxy = with_owner_platform(flui_native::OwnerPlatform::proxy)?;
    Some(Waker::from(Arc::new(OwnerWake(proxy))))
}

/// Wake the owner once [`RECALL_BOUND`] has passed, for the window waiting on
/// its placement read.
///
/// The loop's wake deadline alone does not give the owner a turn: a desktop
/// loop serves a due deadline by asking its windows for a frame, and the main
/// window waiting here has none yet. So a short-lived thread sleeps out the
/// bound and wakes the owner, whose turn then opens the window at its default
/// if the read still has not answered. A wake that reaches a loop already gone
/// is refused and ignored.
fn wake_owner_at_bound() {
    let Some(proxy) = with_owner_platform(flui_native::OwnerPlatform::proxy) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("flui-placement-bound".into())
        .spawn(move || {
            std::thread::sleep(RECALL_BOUND);
            let _ = proxy.wake();
        });
    if let Err(error) = spawned {
        tracing::warn!(
            %error,
            "no thread to wake the owner at the placement read's bound; the window \
             opens at the next owner turn after it"
        );
    }
}

struct OwnerWake(flui_native::PlatformProxy);

impl Wake for OwnerWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.wake();
    }
}

/// Open the window `opening` describes with `open` once its placement read
/// settles, which it has not yet: at the owner turn that takes storage's
/// answer or reaches the read's bound. A loop that ends first drops `open`
/// unrun.
pub(super) fn when_recalled(opening: OpeningWindow, open: Box<dyn FnOnce(OpeningWindow)>) {
    APP_RUNTIME.with(|slot| {
        slot.borrow_mut()
            .placement
            .awaiting
            .push(AwaitingOpen { opening, open });
    });
}

/// The window at `address` is closing: its name is free from now on, and its
/// last placement is handed to storage. The owner does not wait for the
/// answer: [`tick`] takes it, and the end of the loop waits for what is still
/// out.
pub(super) fn finish(address: PresentationAddress) {
    let window = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.try_borrow_mut().ok()?;
        let live = &mut runtime.placement.live;
        let at = live.iter().position(|window| window.address == address)?;
        Some(live.remove(at))
    });
    let Some(LiveWindow {
        recorder,
        _claim: claim,
        ..
    }) = window
    else {
        return;
    };
    drop(claim);
    recorder.release_window();
    let now = now();
    recorder.begin_flush(now);
    if recorder.continue_flush(now) == FlushState::Waiting {
        let unkept = APP_RUNTIME.with(|slot| match slot.try_borrow_mut() {
            Ok(mut runtime) => {
                runtime.placement.closing.push(recorder);
                None
            }
            Err(_) => Some(recorder),
        });
        if let Some(recorder) = unkept {
            // Closed from inside a borrow of the runtime (a `Drop` it runs):
            // the write is with storage, but nothing is left to wait for it.
            tracing::warn!(
                storage = recorder.name().as_str(),
                "a closing window's last placement is written unwatched; its loss would go \
                 unreported"
            );
            drop_contained(recorder);
        }
    }
}

/// The session is ending: write every window's placement now, by
/// `deadline`, the session end's own (ADR-0260), so window metadata never
/// spends the documents' time. A placement storage did not take by then is
/// reported, by name.
pub(super) fn flush_all_by(deadline: Instant) {
    let (recorders, closed) = APP_RUNTIME.with(|slot| {
        slot.try_borrow_mut().map_or_default(|mut runtime| {
            let placement = &mut runtime.placement;
            let closed = std::mem::take(&mut placement.closing);
            let recorders: Vec<Rc<PlacementRecorder>> = placement
                .live
                .iter()
                .map(|window| Rc::clone(&window.recorder))
                .chain(closed.iter().cloned())
                .collect();
            (recorders, closed)
        })
    });
    let now = now();
    settle(
        &recorders,
        now,
        deadline.saturating_duration_since(Instant::now()),
    );
    drop_contained(closed);
}

/// Take what the owner's turn brought: what the host's providers posted,
/// storage's answers to writes and reads, and the held writes whose interval
/// is over; then open the windows whose read settled. The owner calls it after
/// every turn, so a wake for any of these does it whatever the frames are
/// doing.
pub(super) fn tick() {
    let (capabilities, live, closing) = APP_RUNTIME.with(|slot| {
        slot.try_borrow().map_or_default(|runtime| {
            let placement = &runtime.placement;
            (
                placement.capabilities.clone(),
                placement
                    .live
                    .iter()
                    .map(|window| Rc::clone(&window.recorder))
                    .collect::<Vec<_>>(),
                placement.closing.clone(),
            )
        })
    });
    let mut first = FirstPanic::new();
    if let Some(capabilities) = capabilities {
        first.absorb(capabilities.run_posted());
    }
    let now = now();
    for recorder in live {
        recorder.tick(now);
    }
    let finished: Vec<Rc<PlacementRecorder>> = closing
        .into_iter()
        .filter(|recorder| recorder.continue_flush(now) != FlushState::Waiting)
        .collect();
    if !finished.is_empty() {
        APP_RUNTIME.with(|slot| {
            slot.borrow_mut()
                .placement
                .closing
                .retain(|recorder| !finished.iter().any(|done| Rc::ptr_eq(done, recorder)));
        });
    }
    drop_contained(finished);
    first.absorb(open_recalled());
    first.resume();
}

/// Open each waiting window whose placement read settled, outside every
/// borrow: opening a window is platform code.
///
/// Each open runs contained: one that panics leaves the others opened, and
/// the first panic is returned.
fn open_recalled() -> FirstPanic {
    let awaiting =
        APP_RUNTIME.with(|slot| std::mem::take(&mut slot.borrow_mut().placement.awaiting));
    let mut first = FirstPanic::new();
    let mut waiting = Vec::new();
    for mut entry in awaiting {
        if entry.opening.recall_settled() {
            let AwaitingOpen { opening, open } = entry;
            first.contain(|| open(opening));
        } else {
            waiting.push(entry);
        }
    }
    // An open may have queued another window, or ended the loop: the waiting
    // go back in front of the new ones, and a retired host keeps none.
    let dropped = APP_RUNTIME.with(|slot| {
        let placement = &mut slot.borrow_mut().placement;
        if placement.capabilities.is_none() {
            return waiting;
        }
        placement.awaiting.splice(0..0, waiting);
        Vec::new()
    });
    first.contain(|| drop(dropped));
    first
}

/// The host accepted a new display snapshot: measure every open window's
/// placement against it, so a placement kept relative to a display's usable
/// area is written again when that area moved under a window that did not.
pub(super) fn displays_changed() {
    let live = APP_RUNTIME.with(|slot| {
        slot.try_borrow().map_or_default(|runtime| {
            runtime
                .placement
                .live
                .iter()
                .map(|window| Rc::clone(&window.recorder))
                .collect::<Vec<_>>()
        })
    });
    if live.is_empty() {
        return;
    }
    let displays = current_displays();
    let now = now();
    for recorder in live {
        recorder.redisplay(&displays, now);
    }
}

/// The loop ends, after [`flush_all_by`]: the host's capabilities retire, a
/// window still waiting for its read is dropped unopened and every recorder
/// is forgotten. Returns the first panic of that for the loop's teardown.
pub(super) fn retire() -> FirstPanic {
    let (capabilities, released) = APP_RUNTIME.with(|slot| {
        let mut runtime = slot.borrow_mut();
        let placement = &mut runtime.placement;
        (
            placement.capabilities.take(),
            (
                std::mem::take(&mut placement.awaiting),
                std::mem::take(&mut placement.live),
                std::mem::take(&mut placement.closing),
            ),
        )
    });
    let mut first = FirstPanic::new();
    first.contain(|| drop(released));
    if let Some(capabilities) = capabilities {
        first.absorb(capabilities.retire());
        first.contain(|| drop(capabilities));
    }
    first
}

/// The displays the host last sampled; none while it has not sampled yet.
fn current_displays() -> DisplaySnapshot {
    APP_RUNTIME
        .with(|slot| {
            slot.try_borrow()
                .ok()
                .and_then(|state| state.installed_host.displays().snapshot())
        })
        .unwrap_or_else(DisplaySnapshot::empty)
}

/// The names windows of this run remember their placement under: one window
/// per name, so two windows never overwrite each other's placement.
#[derive(Default, Clone)]
struct Claims(Rc<RefCell<FxHashSet<StorageName>>>);

impl Claims {
    /// Hold `name` for one window; `None` while another window holds it.
    fn claim(&self, name: StorageName) -> Option<NameClaim> {
        self.0.borrow_mut().insert(name).then(|| NameClaim {
            claims: self.clone(),
            name,
        })
    }
}

/// One window's hold on its placement's name, released when it drops.
struct NameClaim {
    claims: Claims,
    name: StorageName,
}

impl Drop for NameClaim {
    fn drop(&mut self) {
        self.claims.0.borrow_mut().remove(&self.name);
    }
}

#[cfg(all(test, feature = "testing"))]
#[path = "placement_tests.rs"]
mod tests;
