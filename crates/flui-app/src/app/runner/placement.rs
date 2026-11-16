//! The desktop runner's window placements: read from the application's storage
//! before a window opens, recorded while it lives, and written when it moves,
//! closes or the session ends (the model is
//! [`placement_recorder`](crate::app::placement_recorder)).
//!
//! The storage is the one the run's capability registry provides, the
//! application's override of `StorageCapability` when it has one: the host
//! acquires it through [`HostCapabilities`] (ADR-0338). Nothing here blocks the
//! owner thread on storage, except the final flush at the end of the loop or of
//! the session, within that end's deadline. The owner's turns come from storage's
//! answers and from the loop's turn deadline (`AppHandler::turn_deadline`,
//! `AppRuntime::turn_deadline`), which a loop serves with or without a window
//! open.
//!
//! The state lives in one [`PlacementShared`] that the application runtime and
//! every window's close handle hold, each list in a cell of its own that is
//! borrowed only for a step that runs none of the application's code: storage,
//! a provider and a window's subscription always run with nothing borrowed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Wake, Waker};
use std::time::{Duration, Instant};

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
    FlushState, HandOver, PlacementRecall, PlacementRecorder, Recalled, Succession, earliest_due,
    restore_into, settle,
};

/// How long the end of a session waits, at most, for the window placements
/// still owed, after the application's own work at that end: window metadata
/// takes what the session's budget has left and never more than this.
pub(crate) const SESSION_END_PLACEMENT_BOUND: Duration = Duration::from_millis(250);

/// The run's window placements, held by the application runtime for the
/// loop's life.
#[derive(Default)]
pub(in crate::app) struct PlacementHost {
    shared: Rc<PlacementShared>,
}

/// The placement state the runtime and every window's close handle share.
pub(in crate::app) struct PlacementShared {
    /// The clock the placement read's bound and the write interval are
    /// measured on: the system's, a manual one in tests.
    clock: RefCell<Rc<dyn MonotonicClock>>,
    /// Where the host acquires the application's storage, from the launch
    /// registry; `None` before a loop installs it and after it retires.
    capabilities: RefCell<Option<Rc<HostCapabilities>>>,
    /// The names a window holds, open or opening.
    claims: Claims,
    /// The open windows that remember their placement, by presentation.
    live: RefCell<Vec<LiveWindow>>,
    /// The windows that closed while their last placement was being written.
    closing: RefCell<Vec<Rc<PlacementRecorder>>>,
    /// Windows that wait for their placement read before they open.
    awaiting: RefCell<Vec<AwaitingOpen>>,
    /// Closes that found [`Self::live`] busy: carried out at the next tick.
    deferred_closes: RefCell<Vec<PresentationAddress>>,
    /// The turn deadline the loop was last told ([`PlacementHost::next_due`]).
    announced: Cell<Option<Instant>>,
    /// Asks the owner for a turn: set when the loop installs the host's
    /// capabilities.
    wake: RefCell<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Default for PlacementShared {
    fn default() -> Self {
        Self {
            clock: RefCell::new(Rc::new(SystemClock)),
            capabilities: RefCell::new(None),
            claims: Claims::default(),
            live: RefCell::new(Vec::new()),
            closing: RefCell::new(Vec::new()),
            awaiting: RefCell::new(Vec::new()),
            deferred_closes: RefCell::new(Vec::new()),
            announced: Cell::new(None),
            wake: RefCell::new(None),
        }
    }
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

impl PlacementHost {
    /// When the owner needs a turn for the placements: a held write whose
    /// interval ends, a write storage answered, a close carried over, or a
    /// window whose wait for its read ends. `None` while a list is busy:
    /// the turn that holds it asks again.
    pub(in crate::app) fn next_due(&self) -> Option<Instant> {
        let due = self.shared.next_due();
        // What the loop is told now: a change made outside an owner turn and
        // already announced is not announced again.
        self.shared.announced.set(due);
        due
    }

    /// Measure placements on `clock` from now on.
    #[cfg(all(test, feature = "testing"))]
    pub(in crate::app) fn set_clock(&self, clock: Rc<dyn MonotonicClock>) {
        self.shared.clock.replace(clock);
    }

    /// The state every window's close handle shares.
    #[cfg(all(test, feature = "testing"))]
    pub(in crate::app) fn shared(&self) -> Rc<PlacementShared> {
        Rc::clone(&self.shared)
    }
}

impl PlacementShared {
    /// When the owner needs a turn for the placements; `None` while a list
    /// is busy: the turn that holds it asks again.
    fn next_due(&self) -> Option<Instant> {
        let now = self.now();
        let (Ok(live), Ok(closing), Ok(awaiting), Ok(deferred)) = (
            self.live.try_borrow(),
            self.closing.try_borrow(),
            self.awaiting.try_borrow(),
            self.deferred_closes.try_borrow(),
        ) else {
            return None;
        };
        if !deferred.is_empty() {
            return Some(now);
        }
        let writes = earliest_due(
            now,
            live.iter().map(|window| &window.recorder).chain(&*closing),
        );
        let reads = awaiting
            .iter()
            .filter_map(|awaiting| awaiting.opening.recall_deadline())
            .min();
        writes.into_iter().chain(reads).min()
    }

    /// The placements' turn deadline may have moved outside an owner turn
    /// (a window's report, a close, a display change): when it differs from
    /// what the loop was last told, ask for an owner turn, after which every
    /// loop reads the turn deadline again (`AppHandler::turn_deadline`).
    fn announce_turn_deadline(&self) {
        let due = self.next_due();
        if self.announced.replace(due) == due {
            return;
        }
        let wake = self.wake.borrow().clone();
        if let Some(wake) = wake {
            wake();
        }
    }

    fn now(&self) -> Instant {
        let clock = Rc::clone(&*self.clock.borrow());
        clock.now()
    }

    /// The open windows' recorders, cloned out of the list.
    fn live_recorders(&self) -> Vec<Rc<PlacementRecorder>> {
        self.live
            .borrow()
            .iter()
            .map(|window| Rc::clone(&window.recorder))
            .collect()
    }
}

/// The run's placement state; `None` while the application runtime is
/// mutably borrowed.
fn shared() -> Option<Rc<PlacementShared>> {
    APP_RUNTIME.with(|slot| {
        slot.try_borrow()
            .ok()
            .map(|runtime| Rc::clone(&runtime.placement.shared))
    })
}

/// The loop starts: the host acquires storage from `registry`, the one the
/// run's conflicts were checked against. What a provider posts back wakes
/// the owner for a turn, and unparks the owner thread while it waits for the
/// last writes at the end of the loop or of the session ([`settle`]).
/// Forgets what an earlier loop on this thread left.
pub(super) fn install(registry: CapabilityRegistry) {
    let Some(proxy) = with_owner_platform(flui_native::OwnerPlatform::proxy) else {
        return;
    };
    let Some(shared) = shared() else {
        return;
    };
    let owner = std::thread::current();
    let turn = proxy.clone();
    shared.wake.replace(Some(Arc::new(move || {
        let _ = turn.wake();
    })));
    shared.announced.set(None);
    let capabilities = Rc::new(HostCapabilities::new(registry, move || {
        let _ = proxy.wake();
        owner.unpark();
    }));
    let replaced = (
        shared.capabilities.replace(Some(capabilities)),
        std::mem::take(&mut *shared.live.borrow_mut()),
        std::mem::take(&mut *shared.closing.borrow_mut()),
        std::mem::take(&mut *shared.awaiting.borrow_mut()),
    );
    shared.deferred_closes.borrow_mut().clear();
    drop_contained(replaced);
}

/// What a window that remembers its placement opens with: the storage it is
/// kept in, its name, held for this window alone, and the read of it.
pub(super) struct RememberedPlacement {
    shared: Rc<PlacementShared>,
    storage: StorageHandle,
    claim: NameClaim,
    recall: PlacementRecall,
    /// What a closed window of the same name handed over when the window
    /// opens from that window's placement instead of a read: its stored state
    /// and its write in flight, which the new recorder takes over.
    succession: Option<Succession>,
}

impl RememberedPlacement {
    /// What `config`'s window remembers: the read of its placement, started
    /// now from the application's storage. `None` when the configuration asks
    /// for no placement, when another window holds its name, or when the run
    /// has no storage to keep it in.
    ///
    /// A window that closed this run under the same name while its last write
    /// is still out hands over its newest placement, what storage holds and
    /// that write: the window opens from that placement, which no read racing
    /// the write could give, and its recorder is the name's one writer from
    /// then on ([`PlacementRecorder::hand_over`]).
    pub(super) fn recall(config: &AppConfig) -> Option<Self> {
        let name = config.placement?;
        let shared = shared()?;
        let Some(claim) = shared.claims.claim(name) else {
            tracing::warn!(
                storage = name.as_str(),
                "another open window remembers its placement under this name; this window \
                 opens from its configuration and is not remembered"
            );
            return None;
        };
        let closing = {
            let mut closing = shared.closing.borrow_mut();
            let at = closing.iter().rposition(|recorder| recorder.name() == name);
            at.map(|at| closing.remove(at))
        };
        let handed = match &closing {
            Some(recorder) => recorder.hand_over(),
            None => HandOver::Nothing,
        };
        if let HandOver::Busy = handed {
            // The closed window's write is with storage's code right now:
            // moving it would race. It stays the name's writer.
            shared.closing.borrow_mut().extend(closing);
            tracing::warn!(
                storage = name.as_str(),
                "a window reopened while storage was answering its predecessor's write; it \
                 opens from its configuration and is not remembered"
            );
            return None;
        }
        // The predecessor owes nothing now; its drop is the storage's code
        // when it held a write, which was moved.
        drop_contained(closing);
        let capabilities = shared.capabilities.borrow().clone();
        let storage = acquire_storage(capabilities.as_deref(), name)?;
        let (recall, succession) = match handed {
            HandOver::Taken(succession) => (
                PlacementRecall::settled(name, Recalled::of_bytes(&succession.latest, &name)),
                Some(succession),
            ),
            HandOver::Nothing | HandOver::Busy => (
                PlacementRecall::start(&storage, name, shared.now(), owner_waker()?),
                None,
            ),
        };
        Some(Self {
            shared,
            storage,
            claim,
            recall,
            succession,
        })
    }

    /// Whether the read settled, taking storage's answer or giving up on it at
    /// the bound.
    pub(super) fn poll(&mut self) -> bool {
        let now = self.shared.now();
        self.recall.poll(now).is_some()
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
    /// which opened with this; the handle its close goes through.
    pub(super) fn keep(self, address: PresentationAddress) -> Option<PlacementClose> {
        let Self {
            shared,
            storage,
            claim,
            recall,
            succession,
        } = self;
        let name = recall.name();
        let stored = recall
            .outcome()
            .map_or(Recalled::TimedOut.stored(), Recalled::stored);
        drop_contained(recall);
        let handle = APP_RUNTIME.with(|slot| {
            slot.try_borrow()
                .ok()
                .and_then(|runtime| runtime.window_commands.handle_for(address))
        })?;
        // Storage answers on its own thread: wake the owner, which takes the answer
        // at its next turn.
        let proxy = with_owner_platform(flui_native::OwnerPlatform::proxy)?;
        let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.wake();
        });
        let recorder = Rc::new(match succession {
            Some(succession) => {
                PlacementRecorder::succeeding(storage, name, succession, wake_owner)
            }
            None => PlacementRecorder::new(storage, name, stored, wake_owner),
        });
        let weak = Rc::downgrade(&recorder);
        // Weak: the recorder, in the shared lists, holds this subscription.
        let clock = Rc::downgrade(&shared);
        let Ok((state, subscription)) = handle.subscribe(move |state| {
            if let (Some(recorder), Some(shared)) = (weak.upgrade(), clock.upgrade()) {
                recorder.observe(state, &current_displays(), shared.now());
                shared.announce_turn_deadline();
            }
        }) else {
            return None;
        };
        recorder.hold(subscription);
        recorder.observe(&state, &current_displays(), shared.now());
        shared.live.borrow_mut().push(LiveWindow {
            address,
            recorder,
            _claim: claim,
        });
        Some(PlacementClose { shared, address })
    }
}

/// The handle a remembered window's close goes through: it holds the
/// placement state itself, so a close reached from inside a borrow of the
/// application runtime still finds it.
pub(super) struct PlacementClose {
    shared: Rc<PlacementShared>,
    address: PresentationAddress,
}

/// What a window's close did with its placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Closing {
    /// The window's name is free and its last placement is with storage.
    Started,
    /// The placement state was busy: the close is carried out at the owner's
    /// next turn, which the turn deadline asks for.
    Deferred,
    /// The window was not recorded, or its close was carried out already.
    NotKept,
}

impl PlacementClose {
    /// A handle on the close of the window presented at `address`.
    #[cfg(all(test, feature = "testing"))]
    pub(super) fn of(shared: Rc<PlacementShared>, address: PresentationAddress) -> Self {
        Self { shared, address }
    }

    /// The window is closing: free its name and hand its last placement to
    /// storage. The owner does not wait for the answer: [`tick`] takes it,
    /// and the end of the loop waits for what is still out.
    pub(super) fn finish(&self) -> Closing {
        finish_in(&self.shared, self.address)
    }
}

fn finish_in(shared: &PlacementShared, address: PresentationAddress) -> Closing {
    let window = {
        let Ok(mut live) = shared.live.try_borrow_mut() else {
            shared.deferred_closes.borrow_mut().push(address);
            return Closing::Deferred;
        };
        let Some(at) = live.iter().position(|window| window.address == address) else {
            return Closing::NotKept;
        };
        live.remove(at)
    };
    let LiveWindow {
        recorder,
        _claim: claim,
        ..
    } = window;
    drop(claim);
    recorder.release_window();
    let now = shared.now();
    recorder.begin_flush(now);
    if recorder.continue_flush(now) == FlushState::Waiting {
        shared.closing.borrow_mut().push(recorder);
    }
    shared.announce_turn_deadline();
    Closing::Started
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
/// answer or the one the read's bound brings. A loop that ends first drops
/// `open` unrun.
pub(super) fn when_recalled(opening: OpeningWindow, open: Box<dyn FnOnce(OpeningWindow)>) {
    match shared() {
        Some(shared) => {
            shared
                .awaiting
                .borrow_mut()
                .push(AwaitingOpen { opening, open });
            shared.announce_turn_deadline();
        }
        None => drop_contained((opening, open)),
    }
}

/// Write every owed placement now and wait for storage, by `deadline`: the
/// end of the loop, or of the session after the application's own work. What
/// providers post back to the owner meanwhile runs between the looks at the
/// answers. A placement storage did not take by then is reported, by name.
pub(super) fn flush_all_by(deadline: Instant) {
    let Some(shared) = shared() else {
        return;
    };
    let closed = std::mem::take(&mut *shared.closing.borrow_mut());
    let recorders: Vec<Rc<PlacementRecorder>> = shared
        .live_recorders()
        .into_iter()
        .chain(closed.iter().cloned())
        .collect();
    if recorders.is_empty() {
        return;
    }
    let capabilities = shared.capabilities.borrow().clone();
    let mut posted = FirstPanic::new();
    let _: Vec<StorageName> = settle(
        &recorders,
        shared.now(),
        deadline.saturating_duration_since(Instant::now()),
        || {
            if let Some(capabilities) = &capabilities {
                posted.absorb(capabilities.run_posted());
            }
        },
    );
    if let Some(payload) = posted.into_payload() {
        tracing::error!(
            panic = flui_foundation::panic::payload_text(payload.as_ref()),
            "a storage provider's posted work panicked while the window placements were \
             written"
        );
        flui_foundation::panic::discard_payload(payload);
    }
    // The flush polled the writes with a waker that unparks this thread. A
    // session end that does not end the application goes on: the writes still
    // out are polled once more with their own wakers, which wake the owner,
    // and a closed window's unfinished write stays to be taken at a turn.
    let now = shared.now();
    for recorder in shared.live_recorders() {
        recorder.tick(now);
    }
    let unfinished: Vec<Rc<PlacementRecorder>> = closed
        .iter()
        .filter(|recorder| recorder.continue_flush(now) == FlushState::Waiting)
        .cloned()
        .collect();
    shared.closing.borrow_mut().extend(unfinished);
    drop_contained(closed);
    shared.announce_turn_deadline();
}

/// Take what the owner's turn brought: what the host's providers posted,
/// closes carried over, storage's answers to writes and reads, and the held
/// writes whose interval is over; then open the windows whose read settled.
/// The owner calls it after every turn.
pub(super) fn tick() {
    let Some(shared) = shared() else {
        return;
    };
    let mut first = FirstPanic::new();
    let capabilities = shared.capabilities.borrow().clone();
    if let Some(capabilities) = capabilities {
        first.absorb(capabilities.run_posted());
    }
    let deferred = std::mem::take(&mut *shared.deferred_closes.borrow_mut());
    for address in deferred {
        let _: Option<Closing> = first.contain(|| finish_in(&shared, address));
    }
    let now = shared.now();
    for recorder in shared.live_recorders() {
        first.contain(|| recorder.tick(now));
    }
    let closing = shared.closing.borrow().clone();
    let finished: Vec<Rc<PlacementRecorder>> = closing
        .into_iter()
        .filter(|recorder| {
            first
                .contain(|| recorder.continue_flush(now))
                .is_none_or(|state| state != FlushState::Waiting)
        })
        .collect();
    if !finished.is_empty() {
        shared
            .closing
            .borrow_mut()
            .retain(|recorder| !finished.iter().any(|done| Rc::ptr_eq(done, recorder)));
    }
    drop_contained(finished);
    first.absorb(open_recalled(&shared));
    first.resume();
}

/// Open each waiting window whose placement read settled, outside every
/// borrow: opening a window is platform code.
///
/// Each open runs contained: one that panics leaves the others opened, and
/// the first panic is returned.
fn open_recalled(shared: &PlacementShared) -> FirstPanic {
    let awaiting = std::mem::take(&mut *shared.awaiting.borrow_mut());
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
    if shared.capabilities.borrow().is_none() {
        first.contain(|| drop(waiting));
    } else {
        shared.awaiting.borrow_mut().splice(0..0, waiting);
    }
    first
}

/// The host accepted a new display snapshot: measure every open window's
/// placement against it, so a placement kept relative to a display's usable
/// area is written again when that area moved under a window that did not.
pub(super) fn displays_changed() {
    let Some(shared) = shared() else {
        return;
    };
    let live = shared.live_recorders();
    if live.is_empty() {
        return;
    }
    let displays = current_displays();
    let now = shared.now();
    for recorder in live {
        recorder.redisplay(&displays, now);
    }
    shared.announce_turn_deadline();
}

/// The loop ends, after [`flush_all_by`]: the host's capabilities retire, a
/// window still waiting for its read is dropped unopened and every recorder
/// is forgotten. Returns the first panic of that for the loop's teardown.
pub(super) fn retire() -> FirstPanic {
    let mut first = FirstPanic::new();
    let Some(shared) = shared() else {
        return first;
    };
    let capabilities = shared.capabilities.borrow_mut().take();
    let wake = shared.wake.borrow_mut().take();
    first.contain(|| drop(wake));
    let released = (
        std::mem::take(&mut *shared.awaiting.borrow_mut()),
        std::mem::take(&mut *shared.live.borrow_mut()),
        std::mem::take(&mut *shared.closing.borrow_mut()),
    );
    shared.deferred_closes.borrow_mut().clear();
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
