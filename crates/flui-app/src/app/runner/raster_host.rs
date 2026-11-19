//! The loop's raster host (ADR-0305 §12): one [`RasterExecutor`] over the
//! application's one GPU context, a slot for each window that draws, turned
//! inline on the owner thread or on its own raster thread (ADR-0305 §10).
//!
//! The first window to attach places the executor: its surface picks the
//! adapter (ADR-0305 §2). Every later window is a slot on the same device.
//! Inline, a frame is sent to the window's slot and the executor turns at
//! once, so the frame's verdict is in hand before the UI runtime's frame
//! transaction returns. A frame dispatched while the host is already turning
//! (a platform callback re-entering from inside the GPU work) is answered
//! [`SubmitVerdict::Retry`] and keeps its work for the next wake. Threaded,
//! the frame is answered queued and settles later: the thread's wake asks
//! every window for a frame, and each window's wake drains its own slot's
//! settlements into its UI runtime ([`RasterSlot::settle`]).
//!
//! The host also keeps what only the owner knows about each slot: the size
//! it last sent, what the window's surfaces are made over, the version on
//! screen that damage is measured from, and the presentation's external
//! textures, whose updates ride each frame. A frame is stamped with the
//! configuration generation the executor's record requires at that moment.

use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;

use web_time::Instant;

use flui_engine::{
    DeviceHealth, EngineError, PainterResets, RasterBackend, RasterCommand, RasterExecutor,
    RasterPlacement, RasterSendError, RasterSender, RasterVerdict, SendReceipt, SlotChange,
    SlotConfig, SlotFailure, SlotId, SlotSettlement, SlotSize, SlotTarget, SlotVisibility,
    SurfaceFactory, SurfaceHold,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{FrameEpoch, FrameStamp, PresentationAddress, SurfaceGeneration};
use flui_layer::{DamageMode, RasterBase, ScenePublication, SceneSnapshot, SurfaceKey};
use flui_platform::{DisplayColor, FramePacing, PlatformWindow, RedrawRequester, SurfaceTarget};
use flui_runtime::pump::SampledClock;
use flui_runtime::sink::{FrameSink, SubmitVerdict};

/// Which backend the loop's executor is placed on.
#[derive(Clone, Default)]
pub(crate) enum RasterFactory {
    /// The application's GPU context, bootstrapped on the first window.
    #[default]
    Gpu,
    /// A GPU-free stand-in, for an application run on the headless platform.
    #[cfg(all(
        feature = "testing",
        not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
    ))]
    StandIn(super::stand_in_renderer::StandInRenderers),
    /// The engine's scripted slots, which record what each command asked,
    /// for this module's tests.
    #[cfg(all(test, feature = "testing"))]
    Scripted(flui_engine::testing::ScriptedSlots),
}

impl std::fmt::Debug for RasterFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu => f.write_str("Gpu"),
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
            ))]
            Self::StandIn(_) => f.write_str("StandIn"),
            #[cfg(all(test, feature = "testing"))]
            Self::Scripted(_) => f.write_str("Scripted"),
        }
    }
}

/// The loop's raster host, owner-local (ADR-0323 §1).
///
/// The host is busy while it turns the executor, which runs code that is not
/// ours (a tracing subscriber, a scene's or target's `Drop`). Work that
/// finds it busy and must not be lost (a window's detach, a memory trim) is
/// kept here and done when the turn that holds the host ends.
#[derive(Clone)]
pub(crate) struct SharedRasterHost {
    host: Rc<RefCell<RasterHost>>,
    deferred: Rc<Deferred>,
    /// Where the executor turns, read by each frame's sink for its in-flight
    /// limit.
    placement: RasterPlacement,
    /// Scratch for the settlements a window's wake drains, kept between
    /// wakes.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    settled: Rc<RefCell<Vec<flui_runtime::sink::FrameSettlement>>>,
    /// The raster thread and its slots, shared with the host, so a window's
    /// detach that meets the host busy still waits for the thread before
    /// the window goes.
    #[cfg(not(target_arch = "wasm32"))]
    link: Rc<RefCell<ThreadLink>>,
}

impl Default for SharedRasterHost {
    /// The application's GPU context, turned inline: the host every runner
    /// but the desktop loop keeps.
    fn default() -> Self {
        Self::placed_on(RasterFactory::default(), RasterPlacement::default())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod bounded_wait;
mod device_retry;
#[cfg(not(target_arch = "wasm32"))]
use bounded_wait::{PIPELINE_CACHE_LOAD_BOUND, wait_within};

/// What a window's detach needs of a raster thread without the host: the
/// thread, its sender and each window's slot. The host writes it as it
/// places the executor and attaches and detaches slots, with no code of
/// another's running.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct ThreadLink {
    /// Weak: the executor the host retires stops when the host lets go of
    /// it, not when the last window lets go of the link.
    thread: Option<(
        std::rc::Weak<flui_engine::ExecutorHandle>,
        RasterSender,
        Redraws,
    )>,
    /// Each window's slot, with the target its surfaces were made over, which a
    /// detach that finds the thread wedged keeps past the window's close.
    slots: rustc_hash::FxHashMap<PresentationAddress, (SlotId, Arc<dyn SurfaceTarget>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ThreadLink {
    /// The wait for `address`'s slot to come down, when a raster thread
    /// holds it.
    fn detach_wait(&self, address: PresentationAddress) -> Option<DetachWait> {
        let (handle, sender, redraws) = self.thread.as_ref()?;
        let (slot, target) = self.slots.get(&address)?;
        Some(DetachWait {
            handle: handle.upgrade()?,
            sender: sender.clone(),
            slot: *slot,
            redraws: redraws.clone(),
            target: Arc::clone(target),
        })
    }
}

/// Work that met a busy host.
#[derive(Default)]
struct Deferred {
    detach: RefCell<Vec<PresentationAddress>>,
    /// Surfaces the platform released or gave back, in the order it did.
    surfaces: RefCell<Vec<(PresentationAddress, bool)>>,
    trim: std::cell::Cell<Option<flui_foundation::memory::MemoryPressureLevel>>,
    /// The first panic of that work, held until a window's wake resumes it
    /// ([`SharedRasterHost::take_deferred_panic`]) instead of in place of
    /// the answer of whatever ran before the work.
    panic: RefCell<FirstPanic>,
}

/// Resume `caught`, or, on a thread already unwinding (a window dropped by
/// another panic), say it before `FirstPanic` discards it.
fn resume_unless_unwinding(caught: FirstPanic) {
    if std::thread::panicking() {
        if let Some(payload) = caught.into_payload() {
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a GPU panic met another panic's unwind"
            );
            flui_foundation::panic::discard_payload(payload);
        }
        return;
    }
    caught.resume();
}

impl Drop for Deferred {
    fn drop(&mut self) {
        // The run ended before a wake resumed it: said, not lost silently.
        if let Some(payload) = std::mem::take(self.panic.get_mut()).into_payload() {
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a closed window's GPU panic outlived the run"
            );
            flui_foundation::panic::discard_payload(payload);
        }
    }
}

impl SharedRasterHost {
    /// A host whose executor will be placed on `factory`'s backend, turned
    /// in `placement`.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(
            dead_code,
            reason = "the mobile and web runners keep the default GPU host; only the desktop loop picks a factory"
        )
    )]
    pub(crate) fn new(factory: RasterFactory, placement: RasterPlacement) -> Self {
        Self::placed_on(factory, placement)
    }

    fn placed_on(factory: RasterFactory, placement: RasterPlacement) -> Self {
        let host = RasterHost::new(factory, placement);
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            link: Rc::clone(&host.link),
            host: Rc::new(RefCell::new(host)),
            deferred: Rc::default(),
            placement,
            settled: Rc::default(),
        }
    }

    /// Run `with` on the host, then the work that met it busy meanwhile;
    /// `None` while the host is turning.
    pub(crate) fn try_with<R>(&self, with: impl FnOnce(&mut RasterHost) -> R) -> Option<R> {
        let mut host = self.host.try_borrow_mut().ok()?;
        let answer = with(&mut host);
        self.run_deferred(&mut host);
        // A raster thread's detaches are waited for with the host released,
        // and so is the wake a death asks of every window; a panic either
        // meets is held as the deferred work's is.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let detaching = std::mem::take(&mut host.detaching);
            let death_wake = host.death_wake.take();
            drop(host);
            let mut caught = FirstPanic::new();
            let mut wedged = death_wake.is_some();
            for wait in detaching {
                wedged |= !wait.wait(&mut caught);
            }
            if wedged {
                self.keep_every_window(&mut caught);
            }
            if let Some(redraws) = death_wake {
                let _woken = caught.contain(|| redraws.wake());
            }
            self.hold_deferred(caught);
        }
        Some(answer)
    }

    /// Ask every attached window's host to keep its native window past its
    /// close: the raster thread is wedged (it did not answer a detach or an
    /// attach), and a wedged thread may
    /// still present over any surface it holds. Asked while the windows are
    /// open, so a close that comes later, by any path, already finds it
    /// (ADR-0305 §9). A host's panic is held in `caught`, and every other
    /// window is still asked.
    #[cfg(not(target_arch = "wasm32"))]
    fn keep_every_window(&self, caught: &mut FirstPanic) {
        // Read, then let go of the link before any host code runs.
        let Ok(link) = self.link.try_borrow() else {
            return;
        };
        let targets: Vec<Arc<dyn SurfaceTarget>> = link
            .slots
            .values()
            .map(|(_, target)| Arc::clone(target))
            .collect();
        drop(link);
        for target in targets {
            let _kept = caught.contain(|| target.keep_past_close());
        }
    }

    /// Read the host; `None` while it is turning.
    pub(crate) fn try_read<R>(&self, read: impl FnOnce(&RasterHost) -> R) -> Option<R> {
        let host = self.host.try_borrow().ok()?;
        Some(read(&host))
    }

    /// Release `address`'s slot now, or when the turn that holds the host
    /// ends.
    pub(crate) fn detach(&self, address: PresentationAddress) {
        if self.try_with(|host| host.detach(address)).is_none() {
            self.deferred.detach.borrow_mut().push(address);
            // A raster thread draws on concurrently, and the window goes when
            // this returns: its slot comes down first, and the host forgets it
            // when the turn that holds it ends.
            #[cfg(not(target_arch = "wasm32"))]
            {
                // Read, then let go of the link before the wait, which removes
                // the slot from it.
                let wait = self
                    .link
                    .try_borrow()
                    .ok()
                    .and_then(|link| link.detach_wait(address));
                if let Some(wait) = wait {
                    // The slot it took down is not the link's any more: the
                    // host's deferred detach then expects the thread to answer
                    // it stale, and still says so of any other slot.
                    let mut caught = FirstPanic::new();
                    if wait.wait(&mut caught) {
                        if let Ok(mut link) = self.link.try_borrow_mut() {
                            let _forgotten = link.slots.remove(&address);
                        }
                    } else {
                        self.keep_every_window(&mut caught);
                    }
                    self.hold_deferred(caught);
                }
            }
            return;
        }
        // A detach answers nothing, so the deferred work it ran resumes its
        // panic here: the window it closed may have been the last to wake.
        resume_unless_unwinding(self.take_deferred_panic());
    }

    /// The platform gave `address`'s window a surface (`available`) or is
    /// taking it away ([`RasterHost::set_surface`]), now, or when the turn
    /// that holds the host ends; a release then lands after the platform's
    /// callback returned, which only a platform callback re-entering the GPU
    /// work can cause. Answers whether the window's next frame repaints in
    /// full: a surface given back while the host is busy is taken as on its
    /// way.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "only Android and iOS take a window's surface away; no desktop platform does"
        )
    )]
    pub(crate) fn set_surface(&self, address: PresentationAddress, available: bool) -> bool {
        if let Some(restoring) = self.try_with(|host| host.set_surface(address, available)) {
            // A surface change answers nothing the deferred work can take
            // the place of, as a detach does.
            resume_unless_unwinding(self.take_deferred_panic());
            return restoring;
        }
        tracing::warn!("raster host: a window's surface change met the host turning");
        self.deferred
            .surfaces
            .borrow_mut()
            .push((address, available));
        available
    }

    /// Trim the GPU memory behind every slot now, or when the turn that
    /// holds the host ends; the strongest of the trims asked meanwhile.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "no browser reports memory pressure to the page")
    )]
    pub(crate) fn reclaim(&self, level: flui_foundation::memory::MemoryPressureLevel) {
        if self.try_with(|host| host.reclaim(level)).is_none() {
            let strongest = self
                .deferred
                .trim
                .get()
                .map_or(level, |held| held.max(level));
            self.deferred.trim.set(Some(strongest));
            return;
        }
        // A trim answers nothing, as a detach does.
        resume_unless_unwinding(self.take_deferred_panic());
    }

    /// Take the first panic of the work that met the host busy, for a
    /// window's wake to resume once it has nothing left to answer.
    pub(crate) fn take_deferred_panic(&self) -> FirstPanic {
        self.deferred
            .panic
            .try_borrow_mut()
            .map_or_else(|_| FirstPanic::new(), |mut held| std::mem::take(&mut *held))
    }

    /// Run the work that met the host busy. Each item runs whatever an
    /// earlier one did; the first panic is held for
    /// [`take_deferred_panic`](Self::take_deferred_panic), so it does not
    /// take the place of the answer `try_with` returns.
    fn run_deferred(&self, host: &mut RasterHost) {
        let mut caught = FirstPanic::new();
        loop {
            let surfaces = std::mem::take(&mut *self.deferred.surfaces.borrow_mut());
            let detach = std::mem::take(&mut *self.deferred.detach.borrow_mut());
            let trim = self.deferred.trim.take();
            if surfaces.is_empty() && detach.is_empty() && trim.is_none() {
                break;
            }
            for (address, available) in surfaces {
                let _restoring = host.set_surface_held(address, available, &mut caught);
            }
            for address in detach {
                host.detach_held(address, &mut caught);
            }
            if let Some(level) = trim {
                host.reclaim_held(level, &mut caught);
            }
        }
        self.hold_deferred(caught);
    }

    /// Hold `caught` for [`take_deferred_panic`](Self::take_deferred_panic).
    fn hold_deferred(&self, caught: FirstPanic) {
        if let Ok(mut held) = self.deferred.panic.try_borrow_mut() {
            held.absorb(caught);
        } else {
            // Only a payload's own `Drop` runs while it is borrowed.
            resume_unless_unwinding(caught);
        }
    }
}

/// The executor, once placed, and every window's slot on it.
#[derive(Default)]
pub(crate) struct RasterHost {
    factory: RasterFactory,
    placement: RasterPlacement,
    placed: Option<Placed>,
    /// The textures each window's slot kept on an executor its owner
    /// retired, for the slot attached for it next (the web, ADR-0305 §8).
    carried: rustc_hash::FxHashMap<PresentationAddress, flui_layer::TextureUpdates>,
    /// The detaches a raster thread owes, which the shared host waits for
    /// once its borrow is released.
    #[cfg(not(target_arch = "wasm32"))]
    detaching: Vec<DetachWait>,
    /// The windows a death an attach found wakes, once the shared host's
    /// borrow is released (ADR-0305 §9).
    #[cfg(not(target_arch = "wasm32"))]
    death_wake: Option<Redraws>,
    /// What a window's detach needs of the raster thread when this host is
    /// busy, shared with the [`SharedRasterHost`].
    #[cfg(not(target_arch = "wasm32"))]
    link: Rc<RefCell<ThreadLink>>,
}

/// A window's detach a raster thread has to answer before the window's
/// surface is given back.
#[cfg(not(target_arch = "wasm32"))]
struct DetachWait {
    handle: Rc<flui_engine::ExecutorHandle>,
    sender: RasterSender,
    slot: SlotId,
    /// The windows a death this wait finds wakes, each to settle what the
    /// executor will not and to report it dead.
    redraws: Redraws,
    /// What the window's surfaces were made over: a thread that does not
    /// answer may still present over it.
    target: Arc<dyn SurfaceTarget>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DetachWait {
    /// Wait until the thread took the slot down, at most
    /// [`flui_engine::DETACH_DEADLINE`]; answers whether it did. A thread
    /// that does not answer dies: the window's native window is kept past its
    /// close and the target is leaked, and every window still attached is
    /// asked for a frame, whose wake quarantines it (ADR-0305 §9). A panic of
    /// the host's keep, the log's subscriber or a window's redraw requester
    /// is held in `caught`, and the target is leaked after any of them.
    fn wait(self, caught: &mut FirstPanic) -> bool {
        match self
            .handle
            .detach(&self.sender, self.slot, flui_engine::DETACH_DEADLINE)
        {
            Ok(()) => true,
            Err(failure) => {
                // First, before any code that is not the host's: the native
                // window the wedged thread may still present to outlives
                // the close.
                let _kept = caught.contain(|| self.target.keep_past_close());
                let _logged = caught.contain(|| {
                    tracing::error!(%failure, "raster host: the raster thread did not take a window's slot down");
                });
                let _woken = caught.contain(|| self.redraws.wake());
                // The target lives until the process exits, so the handles
                // the wedged thread holds stay the host's (ADR-0305 §9).
                #[expect(
                    clippy::mem_forget,
                    reason = "ADR-0305 §9: a raster thread that did not answer may still present over this target"
                )]
                std::mem::forget(self.target);
                false
            }
        }
    }
}

struct Placed {
    executor: Executor,
    sender: RasterSender,
    surfaces: SurfaceFactory,
    slots: rustc_hash::FxHashMap<PresentationAddress, HostSlot>,
    /// Scratch for a slot's settlements, kept between frames.
    settlements: Vec<SlotSettlement>,
    /// The windows a raster thread's settlement wakes: every attached
    /// window's redraw requester, read by the thread's wake.
    redraws: Redraws,
    /// The `RetryDevice` commands sent, each with a probed device: while
    /// more are sent than the executor served, a retry is on its way and no
    /// second device is probed.
    retries_sent: u64,
    /// The executor's count of unadopted retries when the owner last read
    /// it: one more since is a retry whose adoption panicked, answered once.
    retries_unadopted: u64,
}

/// The executor as the owner holds it in each placement (ADR-0305 §10).
enum Executor {
    /// Turned by the owner, within the call that sent it work.
    Inline(RasterExecutor<Box<dyn RasterBackend>>),
    /// Turned by its own raster thread, which the handle stops. Shared so
    /// a window's detach waits for the thread with no borrow of the host
    /// held.
    #[cfg(not(target_arch = "wasm32"))]
    Threaded(Rc<flui_engine::ExecutorHandle>),
}

impl Executor {
    /// Turn an inline executor; a raster thread turns on its own, rung by
    /// every send.
    fn turn(&mut self) {
        match self {
            Self::Inline(executor) => {
                let _outcome = executor.turn();
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Threaded(_) => {}
        }
    }

    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            clippy::unnecessary_wraps,
            reason = "the web turns inline only; every other target may hold a raster thread"
        )
    )]
    fn inline(&self) -> Option<&RasterExecutor<Box<dyn RasterBackend>>> {
        match self {
            Self::Inline(executor) => Some(executor),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Threaded(_) => None,
        }
    }

    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            clippy::unnecessary_wraps,
            reason = "the web turns inline only; every other target may hold a raster thread"
        )
    )]
    fn inline_mut(&mut self) -> Option<&mut RasterExecutor<Box<dyn RasterBackend>>> {
        match self {
            Self::Inline(executor) => Some(executor),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Threaded(_) => None,
        }
    }
}

/// The redraw requesters a raster thread's wake or a device request's answer
/// calls, as one list it clones
/// out under the lock and calls with the lock released: a requester is the
/// platform's code.
#[derive(Clone, Default)]
struct Redraws(Arc<std::sync::Mutex<RedrawList>>);

/// The redraw requesters of every attached window, shared with the wake.
type RedrawList = Arc<[Arc<dyn RedrawRequester>]>;

impl Redraws {
    /// Ask every listed window for a frame.
    fn wake(&self) {
        let listed = Arc::clone(
            &*self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        // Every window is asked whatever an earlier requester did; the first
        // panic goes on to the thread, which hands it to the owner.
        let mut caught = FirstPanic::new();
        for redraw in listed.iter() {
            let _asked = caught.contain(|| redraw.request_redraw());
        }
        caught.resume();
    }

    /// List the requesters of `slots`; the list replaced is dropped with the
    /// lock released.
    fn follow(&self, slots: &rustc_hash::FxHashMap<PresentationAddress, HostSlot>) {
        let listed: RedrawList = slots
            .values()
            .filter_map(|slot| slot.redraw.clone())
            .collect();
        let replaced = std::mem::replace(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            listed,
        );
        drop(replaced);
    }
}

/// What the owner keeps about one window's slot.
struct HostSlot {
    slot: SlotId,
    /// What the window's surfaces are made over: a surface the executor
    /// dropped (a device rebuild the window's surface did not survive) is
    /// made again over it.
    surface_target: Arc<dyn SurfaceTarget>,
    /// The version on screen, which a publication's damage starts from
    /// (ADR-0304 §7). Moved only by a frame the slot showed.
    base: RasterBase,
    /// Whether the platform gives the window a surface: `false` from the
    /// platform's release (Android's `Stop` and `TerminateWindow`, a UIKit
    /// scene's backgrounding) until it gives one back. Only then is a
    /// surface the owner released made again.
    available: bool,
    /// When the window's next surface may be made, after attempts that did
    /// not give it one.
    restore: RestorePace,
    textures: Option<flui_layer::TextureRegistry>,
    /// The newest image of each texture the context this slot replaced
    /// kept, given to the slot with its first frame.
    carried: flui_layer::TextureUpdates,
    display: Option<DisplayColor>,
    visibility: SlotVisibility,
    /// The size the window's frames last asked for: a surface owed at a
    /// size the device refused waits for the window's next size, not its
    /// pace.
    wanted: Option<DeviceSize>,
    /// The surface a frame of the slot last settled stale on with the slot's
    /// panic (its own mark, or a taint): the first such frame is retried, and
    /// another on that surface is refused, the size's or the driver's failure
    /// rather than the moment's. Any other settlement but a panicked render's
    /// ends it. Kept by surface, not by frame, so a frame queued behind the
    /// panicked one and the retry count as one retry, in whatever order they
    /// settle.
    stale_panic: Option<SurfaceKey>,
    /// The first panic of the slot's frames since the window last reported
    /// one to its UI runtime ([`RasterSlot::report_frame_panic`]).
    panic: flui_foundation::panic::FirstPanic,
    /// The window's redraw requester, which a raster thread's settlement
    /// calls so the window's next wake drains it.
    redraw: Option<Arc<dyn RedrawRequester>>,
    /// How many frames of the window the executor may hold unsettled
    /// ([`in_flight_limit`]), by who paces the surface target, which says so
    /// once, when the slot is attached.
    in_flight_limit: NonZeroUsize,
    /// The frames a raster thread holds for the slot, oldest first: what the
    /// verdict of each needs once it settles (ADR-0305 §4).
    pending: Vec<PendingFrame>,
    /// The executor's painter resets as the window's last frame found them:
    /// a frame that finds another count repaints in full
    /// ([`RasterHost::before_frame`]).
    painters: PainterResets,
}

/// What the owner keeps of a frame a raster thread holds, to settle it.
#[derive(Clone, Copy, Debug)]
struct PendingFrame {
    epoch: FrameEpoch,
    /// The version the frame shows, which the slot's base moves to.
    shown: flui_layer::SceneVersionId,
    /// The surface it was measured on.
    key: SurfaceKey,
    /// Whether the owner took a panic of the slot while the frame was held:
    /// its settlement may have lost the mark that pairs it with the panic,
    /// so its settlement forgets the base and the next frame draws in full.
    /// A frame that replaces it inherits it.
    tainted: bool,
}

impl HostSlot {
    /// Whether the window is owed a surface it can be given now: one the
    /// executor dropped or refused, or one the platform released and gave
    /// back.
    fn needs_surface(&self, hold: SurfaceHold) -> bool {
        self.available
            && matches!(
                hold,
                SurfaceHold::Wanted | SurfaceHold::Refused | SurfaceHold::Released
            )
    }

    /// Whether the window's pace, rather than its next frame, brings its
    /// next surface attempt: it is owed one, it is shown, and the size its
    /// frames last asked for is not one the device refused.
    ///
    /// A window at a refused size waits for its next change, whose frame
    /// asks for a surface again; a timer would meet the same refusal on
    /// every attempt. A hidden window runs no frame whose wake would make
    /// the attempt, so a deadline for it would be reported and never
    /// served; it is asked for when the window is shown again.
    fn surface_on_pace(&self, surface: &flui_engine::SlotSurface) -> bool {
        let at_refused_size =
            matches!(surface.size, SlotSize::Refused(refused) if Some(refused) == self.wanted);
        self.needs_surface(surface.hold)
            && self.visibility == SlotVisibility::Shown
            && !at_refused_size
    }
}

/// The pace of a window's attempts at a new surface: the first at once, and
/// each after one that left the window without a surface later than the one
/// before, from a frame at 60 Hz up to a second, as ADR-0267's backoff climbs.
/// Reset once the window holds a surface or the platform reports it again.
#[derive(Clone, Copy, Debug, Default)]
struct RestorePace {
    attempts: u32,
    next: Option<Instant>,
}

impl RestorePace {
    const FIRST: std::time::Duration = std::time::Duration::from_millis(16);
    const CEILING: std::time::Duration = std::time::Duration::from_secs(1);

    /// Whether an attempt may be made at `now`.
    fn due(self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    /// An attempt was made at `now`: the next waits longer.
    fn attempted(&mut self, now: Instant) {
        let wait = Self::FIRST
            .saturating_mul(1 << self.attempts.min(6))
            .min(Self::CEILING);
        self.attempts = self.attempts.saturating_add(1);
        self.next = Some(now.checked_add(wait).unwrap_or(now));
    }
}

/// How many frames of one window an executor placed as `placement` may hold
/// unsettled, where `pacing` says who paces the window's surface (ADR-0305
/// §5).
///
/// A surface its display paces takes the placement's depth: a raster thread
/// holds one frame rendering and one in its inbox, and the present, which
/// waits for the display, paces both. A surface its compositor paces takes
/// one frame whatever the placement. The host withholds such a window's next
/// frame only from the present that arms its frame callback
/// (`SurfaceTarget::pre_present_notify`) until the compositor answers it, and
/// a raster thread presents after the wake that queued the frame has
/// returned: a second frame taken meanwhile would be presented right behind
/// the first, two in one refresh, of which the compositor shows one
/// (ADR-0356). A pacing this host does not know yet is held as the display's
/// is, as the engine presents it.
fn in_flight_limit(placement: RasterPlacement, pacing: FramePacing) -> NonZeroUsize {
    match pacing {
        FramePacing::Compositor => NonZeroUsize::MIN,
        FramePacing::Display | _ => placement.depth(),
    }
}

/// What a window hands its slot: what its surfaces are made over, and, for a
/// raster thread, the requester its settlements wake the window through.
struct WindowHandles {
    surface_target: Arc<dyn SurfaceTarget>,
    redraw: Option<Arc<dyn RedrawRequester>>,
}

/// The raster executor refused a window's slot.
#[derive(Debug, thiserror::Error)]
#[error("the raster executor refused the window's slot: {0}")]
pub(crate) struct SlotRefused(&'static str);

fn refused(reason: &'static str) -> EngineError {
    EngineError::surface_creation(SlotRefused(reason))
}

/// The refusal of a window's slot asked for while the host is turning.
pub(crate) fn host_busy() -> EngineError {
    refused("the raster host is turning")
}

/// Whether a turn ran the slot's own commands into a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotTurn {
    Clean,
    Panicked,
}

impl RasterHost {
    /// A host whose executor will be placed on `factory`'s backend, turned
    /// in `placement`.
    pub(crate) fn new(factory: RasterFactory, placement: RasterPlacement) -> Self {
        Self {
            factory,
            placement,
            placed: None,
            carried: rustc_hash::FxHashMap::default(),
            #[cfg(not(target_arch = "wasm32"))]
            detaching: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            death_wake: None,
            #[cfg(not(target_arch = "wasm32"))]
            link: Rc::default(),
        }
    }

    /// Give `window`, presenting `address`, a slot: unconfigured until its
    /// first size. Places the executor if this is the first window.
    ///
    /// # Errors
    /// The window's surface cannot be made, no GPU gives a device for the
    /// first window, or the executor refuses the slot.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the web attaches on a context it awaited, with attach_bootstrapped"
        )
    )]
    pub(crate) fn attach(
        &mut self,
        window: &dyn PlatformWindow,
        address: PresentationAddress,
        config: &crate::app::AppConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        let surface_target = self.surface_target(window);
        // Every window's requester is listed: the raster thread's wake asks
        // for frames through it, and so does a device request's answer on
        // every placement (ADR-0305 §8).
        let redraw = Some(window.redraw_requester());
        self.attach_target(surface_target, redraw, address, config, textures, display)
    }

    /// Give the window whose surfaces are made over `surface_target` a slot,
    /// as [`attach`](Self::attach) does.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the web attaches on a context it awaited, with attach_bootstrapped"
        )
    )]
    fn attach_target(
        &mut self,
        surface_target: Arc<dyn SurfaceTarget>,
        redraw: Option<Arc<dyn RedrawRequester>>,
        address: PresentationAddress,
        config: &crate::app::AppConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        let slot_config = SlotConfig::new()
            .color(config.surface_color, display)
            .latency(config.frame_latency);
        self.attach_on(
            WindowHandles {
                surface_target,
                redraw,
            },
            address,
            slot_config,
            textures,
            display,
            None,
        )
    }

    /// Give `window`, presenting `address`, the first slot of an executor
    /// placed on `bootstrap`, a GPU context awaited over the window: the web
    /// awaits its device, which the owner thread cannot block on (ADR-0305
    /// §2).
    ///
    /// # Errors
    /// The executor is placed already, or refuses the slot.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(
            dead_code,
            reason = "only the web awaits its GPU context; a native runner blocks on it in `attach`"
        )
    )]
    pub(crate) fn attach_bootstrapped(
        &mut self,
        bootstrap: flui_engine::GpuBootstrap,
        window: &dyn PlatformWindow,
        address: PresentationAddress,
        slot_config: SlotConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        if self.placed.is_some() {
            return Err(refused("the raster executor is placed already"));
        }
        let surface_target = self.surface_target(window);
        let attached = self.attach_on(
            WindowHandles {
                surface_target,
                redraw: None,
            },
            address,
            slot_config,
            textures,
            display,
            Some(bootstrap),
        );
        // An executor placed for a slot it refused serves no window: it goes,
        // so the next context awaited over the window is placed instead.
        if attached.is_err()
            && self
                .placed
                .as_ref()
                .is_some_and(|placed| placed.slots.is_empty())
        {
            self.retire_executor();
        }
        attached
    }

    /// Give the window whose surfaces are made over `surface_target` a slot
    /// configured by `slot_config`, placing the executor on `bootstrap`, or
    /// on a context bootstrapped now, if this is the first window.
    fn attach_on(
        &mut self,
        window: WindowHandles,
        address: PresentationAddress,
        slot_config: SlotConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
        bootstrap: Option<flui_engine::GpuBootstrap>,
    ) -> Result<(), EngineError> {
        let WindowHandles {
            surface_target,
            redraw,
        } = window;
        // The target's word, read before anything is placed or attached: it
        // is the platform's code.
        let limit = in_flight_limit(self.placement, surface_target.frame_pacing());
        let target = if let Some(placed) = self.placed.as_ref() {
            SlotTarget::Window(placed.surfaces.create(Arc::clone(&surface_target))?)
        } else {
            let (placed, first) = self.place(Arc::clone(&surface_target), bootstrap)?;
            self.placed = Some(placed);
            first
        };
        let placed = self
            .placed
            .as_mut()
            .expect("BUG: the executor was placed above");
        let slot = placed.sender.mint_slot()?;
        placed
            .sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target,
                config: slot_config,
            })
            .map_err(|error| refused(send_refusal(&error)))?;
        // The turn runs code that is not ours; whatever stops the slot from
        // attaching (a refusal, a panic) detaches it before it is answered,
        // so no executor slot outlives its window unnamed.
        let mut caught = FirstPanic::new();
        let _turned = caught.contain(|| placed.executor.turn());
        // A raster thread attaches on its own turn: the owner waits for its
        // answer, so a window the thread refuses is refused here as it is
        // inline. A thread that dies or does not answer within the deadline
        // (wedged, ADR-0305 §9) leaves the executor dead. The window is not
        // refused: its surface may still be in the thread's hands or in its
        // lifecycle queue, which lives as long as the host, and the native
        // window must outlive it. The answer decides, not a second read of
        // the record, which a thread that answers just after the deadline
        // changes.
        #[cfg(not(target_arch = "wasm32"))]
        let unanswered = match &placed.executor {
            Executor::Threaded(handle) => {
                match handle.await_attach(&placed.sender, slot, flui_engine::DETACH_DEADLINE) {
                    Ok(()) => false,
                    Err(failure) => {
                        let _logged = caught.contain(|| {
                            tracing::error!(%failure, "raster host: the raster thread died or did not answer a window's attach; the window is kept and draws nothing");
                        });
                        true
                    }
                }
            }
            Executor::Inline(_) => false,
        };
        #[cfg(target_arch = "wasm32")]
        let unanswered = false;
        let refusal = match placed.sender.record(slot).map(|record| record.phase) {
            _ if unanswered => None,
            // A panic of the turn alone (not of the slot's attach) leaves the
            // slot attached; the window reports it after its first frame.
            Some(flui_engine::SlotPhase::Attached) => None,
            _ => match placed.sender.take_failure(slot) {
                Some(SlotFailure::Refused(error)) => Some(error),
                Some(SlotFailure::Panicked(payload)) => {
                    caught.record(payload);
                    Some(refused("attaching the slot panicked"))
                }
                // A failure added later is a refusal until this match names it.
                Some(_) => Some(refused("the raster executor refused the slot")),
                None => Some(refused("the raster executor did not attach the slot")),
            },
        };
        if let Some(error) = refusal {
            match placed.sender.send(RasterCommand::Detach { slot }) {
                Ok(_) => {
                    let _turned = caught.contain(|| placed.executor.turn());
                }
                Err(send) => {
                    tracing::warn!(error = %send, "raster host: a refused slot's detach was refused");
                }
            }
            caught.resume();
            return Err(error);
        }
        let carried = self.carried.remove(&address).unwrap_or_default();
        let mut base = RasterBase::default();
        base.set_mode(damage_mode());
        #[cfg(not(target_arch = "wasm32"))]
        let link_target = Arc::clone(&surface_target);
        placed.slots.insert(
            address,
            HostSlot {
                slot,
                surface_target,
                base,
                textures,
                carried,
                display,
                visibility: SlotVisibility::Shown,
                wanted: None,
                stale_panic: None,
                available: true,
                restore: RestorePace::default(),
                panic: caught,
                redraw,
                in_flight_limit: limit,
                pending: Vec::with_capacity(self.placement.depth().get()),
                painters: placed.sender.painter_resets(),
            },
        );
        placed.redraws.follow(&placed.slots);
        #[cfg(not(target_arch = "wasm32"))]
        if placed.executor.inline().is_none() {
            let _replaced = self
                .link
                .borrow_mut()
                .slots
                .insert(address, (slot, link_target));
        }
        // A death the attach found wakes every window, this one included,
        // once the host is released: each wake quarantines its window.
        #[cfg(not(target_arch = "wasm32"))]
        if unanswered {
            self.death_wake = Some(placed.redraws.clone());
        }
        Ok(())
    }

    /// The executor over the first window, whose surfaces are made over
    /// `first`, and that window's target: on `bootstrap` when the owner
    /// awaited one over `first`, otherwise on a context bootstrapped now.
    fn place(
        &self,
        first: Arc<dyn SurfaceTarget>,
        bootstrap: Option<flui_engine::GpuBootstrap>,
    ) -> Result<(Placed, SlotTarget), EngineError> {
        let (backend, surfaces, target): (Box<dyn RasterBackend + Send>, _, _) = match &self.factory
        {
            RasterFactory::Gpu => {
                let bootstrap = match bootstrap {
                    Some(bootstrap) => bootstrap,
                    None => Self::bootstrap_now(first)?,
                };
                let surface = bootstrap.first.ok_or_else(|| {
                    refused("the GPU context answered without the first window's surface")
                })?;
                (
                    Box::new(bootstrap.context),
                    bootstrap.factory,
                    SlotTarget::Window(surface),
                )
            }
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
            ))]
            RasterFactory::StandIn(renderers) => {
                let surfaces = flui_engine::testing::noop_surface_factory(Arc::new(
                    flui_engine::testing::FakeDisplay,
                ));
                let target = SlotTarget::Window(surfaces.create(first)?);
                (Box::new(renderers.backend()), surfaces, target)
            }
            #[cfg(all(test, feature = "testing"))]
            RasterFactory::Scripted(slots) => {
                let surfaces = flui_engine::testing::noop_surface_factory(Arc::new(
                    flui_engine::testing::FakeDisplay,
                ));
                let target = SlotTarget::Window(surfaces.create(first)?);
                (Box::new(slots.clone()), surfaces, target)
            }
        };
        let redraws = Redraws::default();
        let (executor, sender) = match self.placement {
            RasterPlacement::Inline => {
                // The owner drains a slot's settlements right after its turn,
                // and reads the device's rebuild deadline for its wake.
                let clock: Rc<dyn flui_foundation::MonotonicClock> =
                    Rc::new(flui_foundation::SystemClock);
                let backend: Box<dyn RasterBackend> = backend;
                // A lost device's replacement is asked for on a worker, which
                // asks every attached window for a frame: the frame gate
                // counts the request's deadline, so that frame's turn installs
                // the device (ADR-0305 §8).
                let woken = redraws.clone();
                let (executor, sender) =
                    RasterExecutor::new(backend, clock, || {}, move || woken.wake());
                (Executor::Inline(executor), sender)
            }
            #[cfg(not(target_arch = "wasm32"))]
            RasterPlacement::Threaded => {
                // A settlement, a device's health or a failure wakes every
                // attached window; each window's wake drains its own slot.
                let woken = redraws.clone();
                // The owner waits for the thread's answers (an attach, a
                // detach, the stop) through its platform's wait, which on
                // Win32 serves the messages the thread's DXGI calls send to
                // the windows meanwhile (ADR-0305 §9).
                let owner_wait =
                    super::host::with_owner_platform(flui_native::OwnerPlatform::owner_wait)
                        .flatten();
                let (handle, sender) = RasterExecutor::spawn(
                    backend,
                    Arc::new(flui_foundation::SystemClock),
                    move || woken.wake(),
                    owner_wait,
                )
                .map_err(EngineError::surface_creation)?;
                let handle = Rc::new(handle);
                *self.link.borrow_mut() = ThreadLink {
                    thread: Some((Rc::downgrade(&handle), sender.clone(), redraws.clone())),
                    slots: rustc_hash::FxHashMap::default(),
                };
                (Executor::Threaded(handle), sender)
            }
            #[cfg(target_arch = "wasm32")]
            RasterPlacement::Threaded => {
                return Err(refused("the web has no raster thread"));
            }
        };
        Ok((Placed::new(executor, sender, surfaces, redraws), target))
    }

    /// The application's GPU context over `first`, the first window,
    /// bootstrapped on the owner thread, which waits for it, its pipeline
    /// cache loaded from the host's storage before any window attaches, so
    /// a driver without a disk cache of its own does not compile every
    /// pipeline again at each launch.
    #[cfg(not(target_arch = "wasm32"))]
    fn bootstrap_now(
        first: Arc<dyn SurfaceTarget>,
    ) -> Result<flui_engine::GpuBootstrap, EngineError> {
        let display =
            super::host::with_owner_platform(flui_native::OwnerPlatform::display_connection)
                .ok_or_else(|| refused("no platform owns this thread"))?;
        let mut bootstrap =
            pollster::block_on(flui_engine::GpuContext::bootstrap(display, Some(first)))?;
        if let Some(storage) = super::host::host_storage() {
            // The read runs on the application's IO pool, which may be one
            // the owner thread itself drives: past the bound the context
            // starts with a cold cache rather than wait for a read that
            // cannot come, and its warm-up still saves for the next launch.
            let loaded = wait_within(
                bootstrap.context.load_pipeline_cache(Arc::clone(&storage)),
                PIPELINE_CACHE_LOAD_BOUND,
            );
            if loaded.is_none() {
                tracing::warn!(
                    target: "flui.gpu",
                    event = "pipeline_cache_load_timed_out",
                    "the pipeline cache was not read in time; the GPU context starts it cold"
                );
                bootstrap.context.start_pipeline_cache_cold(storage);
            }
        }
        Ok(bootstrap)
    }

    /// The web cannot wait on its owner thread: its runner awaits the
    /// context and attaches with [`attach_bootstrapped`](Self::attach_bootstrapped).
    #[cfg(target_arch = "wasm32")]
    fn bootstrap_now(
        _first: Arc<dyn SurfaceTarget>,
    ) -> Result<flui_engine::GpuBootstrap, EngineError> {
        Err(refused(
            "the web awaits its GPU context before its first window attaches",
        ))
    }

    /// Detach every window's slot and drop the executor with its GPU
    /// context: the web's context was lost, and the next one is bootstrapped
    /// over the canvas again (ADR-0305 §8). A panic of the slots' detach or
    /// of the context's `Drop` is resumed once the executor is gone.
    #[cfg_attr(
        not(any(target_arch = "wasm32", all(test, feature = "testing"))),
        expect(
            dead_code,
            reason = "only the web bootstraps its context again; a native device is rebuilt in place"
        )
    )]
    pub(crate) fn retire_executor(&mut self) {
        let mut caught = FirstPanic::new();
        let addresses: Vec<PresentationAddress> = self
            .placed
            .as_ref()
            .map_or_else(Vec::new, |placed| placed.slots.keys().copied().collect());
        if let Some(placed) = self.placed.as_mut() {
            // A frame still waiting in a slot's inbox hands its texture
            // frames to the backend in this turn, so they are kept below.
            let _turned = caught.contain(|| placed.executor.turn());
            // The registry hands a frame over once, so the images the context
            // kept are what the next one draws until their producers submit
            // again; images carried here that no frame took yet are newer.
            for (address, slot) in &mut placed.slots {
                // A raster thread's backend is its own: only the web retires
                // an executor, and the web turns inline.
                let mut kept = placed
                    .executor
                    .inline()
                    .map_or_else(flui_layer::TextureUpdates::default, |executor| {
                        executor.backend().kept_textures(slot.slot)
                    });
                kept.merge(std::mem::take(&mut slot.carried));
                self.carried.insert(*address, kept);
            }
        }
        for address in addresses {
            self.detach_held(address, &mut caught);
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(std::mem::take(&mut *self.link.borrow_mut()));
        let retired = self.placed.take();
        caught.contain(|| drop(retired));
        resume_unless_unwinding(caught);
    }

    /// Whether `address`'s window has a slot.
    #[cfg_attr(
        not(any(target_arch = "wasm32", all(test, feature = "testing"))),
        expect(
            dead_code,
            reason = "only the web attaches after its context arrives and asks whether it has"
        )
    )]
    pub(crate) fn has_slot(&self, address: PresentationAddress) -> bool {
        self.placed
            .as_ref()
            .is_some_and(|placed| placed.slots.contains_key(&address))
    }

    /// What `window`'s surfaces are made over: the window itself, or for the
    /// stand-in, which never presents, a fake window, after naming the
    /// platform window so the test's handle finds the slot.
    fn surface_target(&self, window: &dyn PlatformWindow) -> Arc<dyn SurfaceTarget> {
        match &self.factory {
            RasterFactory::Gpu => window.surface_target(),
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
            ))]
            RasterFactory::StandIn(renderers) => {
                renderers.expect_window(window.id());
                Arc::new(flui_engine::testing::FakeWindow::new(1))
            }
            #[cfg(all(test, feature = "testing"))]
            RasterFactory::Scripted(_) => Arc::new(flui_engine::testing::FakeWindow::new(1)),
        }
    }

    /// Release `address`'s slot and its GPU target. A panic of the turn
    /// (a target's `Drop` is not ours) is held in `caught` once the slot is
    /// gone.
    fn detach_held(&mut self, address: PresentationAddress, caught: &mut FirstPanic) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        let Some(mut slot) = placed.slots.remove(&address) else {
            return;
        };
        if let Some(payload) = std::mem::replace(&mut slot.panic, FirstPanic::new()).into_payload()
        {
            // A window that closed before it reported its frame's panic (a
            // wake that unwound first, a window hidden since): the closing
            // window has no handler left to tell, and its close must not
            // throw a panic that is not the close's.
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a closing window's GPU panic was never reported"
            );
            flui_foundation::panic::discard_payload(payload);
        }
        // A slot the busy path's wait already took down answers stale; any
        // other stale answer is a fault the log keeps.
        #[cfg(not(target_arch = "wasm32"))]
        let taken_down = matches!(placed.executor, Executor::Threaded(_))
            && self
                .link
                .try_borrow()
                .is_ok_and(|link| !link.slots.contains_key(&address));
        #[cfg(target_arch = "wasm32")]
        let taken_down = false;
        match placed
            .sender
            .send(RasterCommand::Detach { slot: slot.slot })
        {
            Ok(_) => {}
            Err(RasterSendError::StaleSlot(_)) if taken_down => {}
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's detach was refused");
            }
        }
        placed.redraws.follow(&placed.slots);
        #[cfg(not(target_arch = "wasm32"))]
        let _forgotten = self.link.borrow_mut().slots.remove(&address);
        // A raster thread takes the slot down on its own turn; the window's
        // surface is given back once it did, which the shared host waits for
        // with its borrow released (ADR-0305 §9).
        #[cfg(not(target_arch = "wasm32"))]
        if let Executor::Threaded(handle) = &placed.executor {
            self.detaching.push(DetachWait {
                handle: Rc::clone(handle),
                sender: placed.sender.clone(),
                slot: slot.slot,
                redraws: placed.redraws.clone(),
                target: Arc::clone(&slot.surface_target),
            });
        }
        let _turned = caught.contain(|| placed.executor.turn());
    }

    /// Trim the GPU memory behind every slot, holding a panic of the turn in
    /// `caught`.
    fn reclaim_held(
        &mut self,
        level: flui_foundation::memory::MemoryPressureLevel,
        caught: &mut FirstPanic,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        if let Err(error) = placed.sender.send(RasterCommand::Trim(level)) {
            tracing::warn!(%error, "raster host: a memory trim was refused");
        }
        let _turned = caught.contain(|| placed.executor.turn());
    }

    /// Release `address`'s slot and its GPU target.
    pub(crate) fn detach(&mut self, address: PresentationAddress) {
        // A window that closes before its next slot is attached draws nothing.
        drop(self.carried.remove(&address));
        let mut caught = FirstPanic::new();
        self.detach_held(address, &mut caught);
        // Reached from a window's `Drop`, which may run during an unwind.
        resume_unless_unwinding(caught);
    }

    /// Trim the GPU memory behind every slot.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "no browser reports memory pressure to the page")
    )]
    pub(crate) fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        let mut caught = FirstPanic::new();
        self.reclaim_held(level, &mut caught);
        // Reached from a window's `Drop`, which may run during an unwind.
        resume_unless_unwinding(caught);
    }

    /// Turn the executor for `address`'s window with no frame of its own: a
    /// lost device whose rebuild is due is asked for, and one whose request
    /// answered is installed, whether or not a window
    /// has anything new to draw. What the turn left for the window (a
    /// refusal, a panic) is settled now, so the frame's own turn finds its
    /// failure slot empty.
    pub(crate) fn service(&mut self, address: PresentationAddress) {
        if let Some(placed) = self.placed.as_mut() {
            let _settled = Self::turn_for(placed, address);
        }
    }

    /// Send `command` for `address`'s window; one the full lifecycle queue
    /// refused is sent again once a turn has made room, since a change the
    /// owner made (a release before the native window goes, a visibility, a
    /// device retry) is not asked for again. A panic of that turn is held in
    /// `caught`.
    fn send_with_room(
        placed: &mut Placed,
        address: PresentationAddress,
        command: RasterCommand,
        caught: &mut FirstPanic,
    ) -> Result<SendReceipt, RasterSendError> {
        match placed.sender.send(command) {
            Err(RasterSendError::Full(command)) => {
                let _turned = caught.contain(|| Self::turn_for(placed, address));
                placed.sender.send(*command)
            }
            sent => sent,
        }
    }

    /// Turn the executor on `address`'s behalf and settle what the turn left
    /// for its slot: a refusal is logged, and a panic of the slot's commands
    /// or of the turn itself is held for the window's report after its
    /// frame. Answers whether the slot's own commands panicked. What the
    /// window does next (a new surface, another size, nothing until it
    /// changes) is read from the slot's record, not from the failure.
    fn turn_for(placed: &mut Placed, address: PresentationAddress) -> SlotTurn {
        let mut caught = FirstPanic::new();
        let _turned = caught.contain(|| placed.executor.turn());
        // A raster thread's failures are taken with its settlements alone
        // (`drain_settled`), so the frame they came with is the one refused.
        let Some(slot) = placed
            .slots
            .get_mut(&address)
            .filter(|_| placed.executor.inline().is_some())
        else {
            caught.resume();
            return SlotTurn::Clean;
        };
        let failure = placed.sender.take_failure(slot.slot);
        // The turn's own panic (a retired scene's `Drop`, a trim, a device
        // rebuild) goes to the window that turned it, for its report. The
        // slot's own panic takes precedence over it, as before this host
        // took a thread's failures with their settlements: it is this
        // window's, while the turn's may be another window's work, and the
        // executor records no order between the two.
        Self::hold_slot_failure(slot, failure, caught, placed.sender.painter_resets())
    }

    /// Settle `failure`, taken from `slot`'s executor slot, with `turned`,
    /// the panic of the turn that left it: a panic of the slot's commands is
    /// held for the window's report in precedence over the turn's (not by time:
    /// the executor records no order between them), and a refusal
    /// is logged last. The log and the refusal's drop run contained, their
    /// panic held with the slot's (a subscriber is user code), so a caller
    /// that drained settlements with the failure still hands them on. `painters` is the
    /// executor's painter-reset count as the slot's failure was taken.
    fn hold_slot_failure(
        slot: &mut HostSlot,
        failure: Option<SlotFailure>,
        turned: FirstPanic,
        painters: PainterResets,
    ) -> SlotTurn {
        let refusal = match failure {
            Some(SlotFailure::Panicked(payload)) => {
                // The slot's own panic settles its frame `Retry`, whose retry repaints in full
                // after the frame-failure backoff (ADR-0258 §6), so the painter resets counted
                // by now (`painters`) are that retry's to answer: read again before the next
                // frame, they would repaint at once, into the same panic.
                slot.painters = painters;
                slot.panic.record(payload);
                slot.panic.absorb(turned);
                return SlotTurn::Panicked;
            }
            refusal => refusal,
        };
        slot.panic.absorb(turned);
        match refusal {
            Some(SlotFailure::Refused(error)) => {
                let _logged = slot.panic.contain(move || {
                    tracing::warn!(%error, "raster host: the executor refused a window's command");
                    drop(error);
                });
            }
            Some(failure) => {
                let _logged = slot.panic.contain(move || {
                    tracing::warn!(?failure, "raster host: a window's slot failed");
                    drop(failure);
                });
            }
            None => {}
        }
        SlotTurn::Clean
    }

    /// Give the executor a device probed in fresh instances in place of one
    /// it gave up on (ADR-0305 §8, ADR-0267), on `address`'s behalf: a full
    /// lifecycle queue is turned for that window's slot and sent to again.
    fn retry_device(&mut self, address: PresentationAddress) -> device_retry::Retry {
        let Some(placed) = self.placed.as_mut() else {
            return device_retry::Retry::NotGivenUp;
        };
        let mut caught = FirstPanic::new();
        let retry = device_retry::retry_given_up(&self.factory, placed, |placed, command| {
            Self::send_with_room(placed, address, command, &mut caught)
        });
        // The turn that made room ran under the window's frame: its panic is
        // the frame's to report, resumed here as any turn's is.
        resume_unless_unwinding(caught);
        retry
    }

    /// The first panic of `address`'s frames since the last one was taken.
    fn take_frame_panic(
        &mut self,
        address: PresentationAddress,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        let slot = self.placed.as_mut()?.slots.get_mut(&address)?;
        std::mem::replace(&mut slot.panic, FirstPanic::new()).into_payload()
    }

    /// Where the context's device stands.
    ///
    /// A backend that cannot rebuild where the executor runs (the web) is
    /// replaced by its owner, which retires the executor (ADR-0305 §8): the
    /// executor's give-up is then the rebuild the owner owes, not one the
    /// application must ask for, and a device the backend reports lost is
    /// lost before any turn has seen it.
    pub(crate) fn device_health(&self) -> DeviceHealth {
        let Some(placed) = self.placed.as_ref() else {
            return DeviceHealth::Healthy;
        };
        let health = placed.sender.device_health();
        // A raster thread's backend rebuilds where it runs: a native one.
        let Some(backend) = placed.executor.inline().map(RasterExecutor::backend) else {
            return health;
        };
        if backend.requests_devices() || !(health.is_lost() || backend.device_lost()) {
            return health;
        }
        DeviceHealth::Rebuilding { attempts: 0 }
    }

    /// When the executor must next turn for a lost device (its next rebuild,
    /// or its request's deadline), for the
    /// loop's wake;
    /// `None` once it died, since a dead executor rebuilds nothing.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn device_retry_at(&self) -> Option<web_time::Instant> {
        self.placed
            .as_ref()
            .filter(|_| !self.executor_died())
            .and_then(|placed| placed.sender.device_retry_at())
    }

    /// Whether the executor died (ADR-0305 §9): terminal for every window.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the web has no raster thread to die")
    )]
    fn executor_died(&self) -> bool {
        self.placed
            .as_ref()
            .is_some_and(|placed| placed.sender.health() == flui_engine::ExecutorHealth::Died)
    }

    /// Follow the display `address`'s window is on: a changed colour profile
    /// sets its surface up again before the next frame. Answers whether it
    /// changed, so the caller repaints in full.
    pub(crate) fn follow_display(
        &mut self,
        address: PresentationAddress,
        display: Option<DisplayColor>,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        let Some(color) = display else {
            return false;
        };
        if slot.display == Some(color) {
            return false;
        }
        // Recorded whether or not it is taken: a send is refused only for a
        // slot that is gone or an executor that died, which asking again
        // cannot change.
        slot.display = Some(color);
        match placed.sender.send(RasterCommand::Configure {
            slot: slot.slot,
            change: SlotChange::Color(color),
        }) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "raster host: a display colour change was refused");
                false
            }
        }
    }

    /// Tell the executor whether `address`'s window can be seen: a slot
    /// that cannot is not drawn, and draws in full when it shows again.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "a hidden tab runs no frames, so the page's slot stays shown"
        )
    )]
    pub(crate) fn set_visibility(
        &mut self,
        address: PresentationAddress,
        visibility: SlotVisibility,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return;
        };
        if slot.visibility == visibility {
            return;
        }
        slot.visibility = visibility;
        // A window shown again tries for a surface it is owed at once, not
        // at the end of a backoff run up while it could not be seen.
        if visibility == SlotVisibility::Shown {
            slot.restore = RestorePace::default();
        }
        let command = RasterCommand::Configure {
            slot: slot.slot,
            change: SlotChange::Shown(visibility),
        };
        // A configuration is folded into the slot's inbox, never queued, so
        // no full queue refuses it; an executor that stopped, or a slot it
        // no longer holds, draws nothing whatever the window's visibility.
        if let Err(error) = placed.sender.send(command) {
            tracing::warn!(%error, "raster host: a visibility change was refused");
        }
    }

    /// Bring `address`'s slot to `size`, and a surface if the executor
    /// dropped its own, and answer the configuration generation its next
    /// frame is stamped with: `None` while the slot has no size the executor
    /// accepted or no surface.
    ///
    /// The generation is the executor's, read after the size is sent: a
    /// device rebuild or a lost surface moves it without the owner asking,
    /// so a cached one would stamp every later frame stale.
    fn configure(
        placed: &mut Placed,
        address: PresentationAddress,
        size: (u32, u32),
    ) -> Option<SurfaceGeneration> {
        let slot = placed.slots.get(&address)?;
        let record = placed.sender.record(slot.slot)?;
        // A size past `i32` is one no device configures: unconfigured.
        let (Ok(width), Ok(height)) = (i32::try_from(size.0), i32::try_from(size.1)) else {
            return None;
        };
        let size = DeviceSize::new(width, height);
        placed
            .slots
            .get_mut(&address)
            .expect("BUG: the slot was found above")
            .wanted = Some(size);
        let slot = placed.slots.get(&address)?;
        // Nothing is drawn at a size the device refused, and asking again
        // is refused the same way: the window waits for another size, or
        // for a rebuilt device, which the record shows pending again.
        if record.surface.size == SlotSize::Refused(size) {
            return None;
        }
        // A surface released by the platform waits for it; one on its way is
        // not asked for again; another attempt waits out its pace.
        if record.surface.hold == SurfaceHold::Held {
            placed
                .slots
                .get_mut(&address)
                .expect("BUG: the slot was found above")
                .restore = RestorePace::default();
        } else if slot.needs_surface(record.surface.hold) {
            if !Self::restore_surface(placed, address, web_time::Instant::now()) {
                return None;
            }
        } else if record.surface.hold != SurfaceHold::Restoring {
            // Released, and the platform has not given it back.
            return None;
        }
        let slot = placed.slots.get(&address)?;
        if record.surface.size.newest() != Some(size) {
            match placed.sender.send(RasterCommand::Configure {
                slot: slot.slot,
                change: SlotChange::Size(size),
            }) {
                Ok(SendReceipt::Configured(_)) => {}
                Ok(
                    SendReceipt::Unconfigured | SendReceipt::Accepted | SendReceipt::Replaced(_),
                ) => {
                    return None;
                }
                Err(error) => {
                    tracing::warn!(%error, "raster host: a window's size was refused");
                    return None;
                }
            }
        }
        let generation = placed.sender.record(slot.slot)?.required_generation;
        (generation != SurfaceGeneration::ZERO).then_some(generation)
    }

    /// Make `address`'s window a new surface and send it to its slot, when
    /// the window's pace allows an attempt at `now`: answers whether one is
    /// on its way. A window whose native surface is not there yet (an
    /// Android `Resume` before its `InitWindow`) is taken as released by the
    /// platform until it reports one again.
    fn restore_surface(placed: &mut Placed, address: PresentationAddress, now: Instant) -> bool {
        // A dead executor restores nothing, whoever asks (the restore pace, a
        // surface the platform gives back, a lent frame): a surface made for
        // it would only be refused and dropped again.
        if placed.sender.health() == flui_engine::ExecutorHealth::Died {
            return false;
        }
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        if !slot.restore.due(now) {
            return false;
        }
        slot.restore.attempted(now);
        let surface = match placed.surfaces.create(Arc::clone(&slot.surface_target)) {
            Ok(surface) => surface,
            Err(EngineError::SurfaceTargetUnavailable { .. }) => {
                slot.available = false;
                slot.restore = RestorePace::default();
                tracing::trace!("raster host: the window has no native surface yet");
                return false;
            }
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's surface could not be made again");
                return false;
            }
        };
        match placed.sender.send(RasterCommand::Restore {
            slot: slot.slot,
            surface,
        }) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's new surface was refused");
                false
            }
        }
    }

    /// The platform gave `address`'s window a surface (`available`) or is
    /// taking it away. A release is applied before this returns, since the
    /// native window behind the surface may go as soon as the platform's
    /// callback does; a surface given back is made and sent at once, and a
    /// device the executor gave up on is asked to rebuild, the window's
    /// return being the moment to try again. Answers whether the window has a
    /// surface on its way, so its next frame repaints in full.
    #[cfg_attr(
        not(any(
            target_os = "android",
            target_os = "ios",
            all(test, feature = "testing")
        )),
        expect(
            dead_code,
            reason = "only Android and iOS take a window's surface away; no desktop platform does"
        )
    )]
    pub(crate) fn set_surface(&mut self, address: PresentationAddress, available: bool) -> bool {
        let mut caught = FirstPanic::new();
        let restored = self.set_surface_held(address, available, &mut caught);
        // Reached from a platform callback, which may run during an unwind.
        resume_unless_unwinding(caught);
        restored
    }

    fn set_surface_held(
        &mut self,
        address: PresentationAddress,
        available: bool,
        caught: &mut FirstPanic,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        slot.available = available;
        slot.restore = RestorePace::default();
        let id = slot.slot;
        let restoring = if available {
            // A given-up device is replaced before the surface is made: a
            // restore configured on the given-up device would be refused, and
            // the surface is made with the factory of the device's instance.
            let retry = device_retry::retry_given_up(&self.factory, placed, |placed, command| {
                Self::send_with_room(placed, address, command, caught)
            });
            if retry == device_retry::Retry::Sent {
                let _turned = caught.contain(|| Self::turn_for(placed, address));
            }
            Self::restore_surface(placed, address, Instant::now())
        } else {
            // The native window behind the surface goes when the platform's
            // callback returns. An executor that stopped draws nothing more,
            // and a slot it no longer holds has no surface to release.
            let sent =
                Self::send_with_room(placed, address, RasterCommand::Release { slot: id }, caught);
            if let Err(error) = sent {
                tracing::warn!(%error, "raster host: a window's surface release was refused");
            }
            false
        };
        let _turned = caught.contain(|| Self::turn_for(placed, address));
        restoring
            && placed
                .sender
                .record(id)
                .is_some_and(|record| record.surface.hold == SurfaceHold::Held)
    }

    /// Draw `scene`, lent for this call alone, into `address`'s window at
    /// `size`, outside its frames: the development reload plugin's scene.
    /// Answers whether it was shown. The window's next frame draws in full.
    #[cfg_attr(
        not(any(target_os = "android", all(test, feature = "testing"))),
        expect(dead_code, reason = "only the Android runner drives a scene plugin")
    )]
    pub(crate) fn render_lent(
        &mut self,
        address: PresentationAddress,
        size: (u32, u32),
        scene: &flui_layer::Scene,
        reset_fonts: bool,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        if Self::configure(placed, address, size).is_none() {
            return false;
        }
        // The size the configuration sent is applied before the scene is.
        let _settled = Self::turn_for(placed, address);
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        // The screen shows a scene no publication described.
        slot.base.forget();
        let id = slot.slot;
        // A panic of the render is the slot's, held for the window's report.
        let mut caught = FirstPanic::new();
        // A lent scene is drawn within this call, which only an inline
        // executor can do; only Android lends one, and it turns inline.
        let Some(executor) = placed.executor.inline_mut() else {
            tracing::warn!("raster host: a lent scene met a raster thread, which cannot draw it");
            return false;
        };
        let rendered = caught.contain(|| executor.render_lent(id, scene, reset_fonts));
        slot.panic.absorb(caught);
        match rendered {
            Some(Ok(shown)) => shown == flui_engine::PresentDisposition::Presented,
            Some(Err(error)) => {
                tracing::error!(%error, "raster host: the plugin's scene failed to render");
                false
            }
            None => false,
        }
    }

    /// Make `address`'s window the surface it is owed once its pace is due at
    /// `now`, with no frame of its own: the deadline
    /// [`window_deadline`](Self::window_deadline) reports is served by the
    /// wake it schedules, and the attempt moves it on, so a window the
    /// backend keeps refusing does not hand the loop the same past instant
    /// again. Answers whether the window holds a surface again.
    fn restore_due(&mut self, address: PresentationAddress, now: Instant) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get(&address) else {
            return false;
        };
        let id = slot.slot;
        let owed = placed
            .sender
            .record(id)
            .is_some_and(|record| slot.surface_on_pace(&record.surface));
        if !owed || !Self::restore_surface(placed, address, now) {
            return false;
        }
        let _settled = Self::turn_for(placed, address);
        placed
            .sender
            .record(id)
            .is_some_and(|record| record.surface.hold == SurfaceHold::Held)
    }

    /// What `address`'s window does before its frame at `now`, with no
    /// frame of its own: a retry of a given-up device the application
    /// asked for (`retry`), a surface it is owed whose pace is due, the
    /// colour of the display it is on, and a turn of the executor, which
    /// asks for a lost device that is due and installs one whose request
    /// answered. Answers what came of it ([`BeforeFrame`]).
    fn before_frame(
        &mut self,
        address: PresentationAddress,
        now: Instant,
        display: Option<DisplayColor>,
        retry: bool,
    ) -> BeforeFrame {
        let retry_failed = retry && self.retry_device(address) == device_retry::Retry::Failed;
        let restored = self.restore_due(address, now);
        let changed = self.follow_display(address, display);
        self.service(address);
        let painter_reset = self.follow_painter(address);
        BeforeFrame {
            repaint: changed | restored | painter_reset,
            retry_failed,
        }
    }

    /// Whether the executor let its painter go since `address`'s window last
    /// asked (a device rebuilt, or a render of any window that panicked,
    /// ADR-0305 §7): the recordings the window's last scene was drawn from
    /// went with it, so its frame repaints in full rather than republish a
    /// version it holds for a retry (ADR-0304 §7). Read after the turn, which
    /// may rebuild the device.
    fn follow_painter(&mut self, address: PresentationAddress) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let now = placed.sender.painter_resets();
        placed
            .slots
            .get_mut(&address)
            .is_some_and(|slot| std::mem::replace(&mut slot.painters, now) != now)
    }

    /// The loop's next wake for raster work: the earliest
    /// [`window_deadline`](Self::window_deadline) of the windows whose frames
    /// run (`frames_run`). A window whose frames are off is left out: only
    /// its own frame serves its surface, so its deadline would wake the loop
    /// for a past instant no wake moves; the device's next rebuild is still
    /// counted through every window whose frames run.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn wake_deadline(
        &self,
        frames_run: impl Fn(PresentationAddress) -> bool,
    ) -> Option<Instant> {
        let placed = self.placed.as_ref()?;
        placed
            .slots
            .keys()
            .copied()
            .filter(|&address| frames_run(address))
            .filter_map(|address| self.window_deadline(address))
            .min()
    }

    /// When `address`'s own window may try for the surface it is owed: the
    /// deadline only that window's frame serves.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    fn surface_retry_at_for(&self, address: PresentationAddress) -> Option<Instant> {
        let placed = self.placed.as_ref()?;
        let slot = placed.slots.get(&address)?;
        let record = placed.sender.record(slot.slot)?;
        slot.surface_on_pace(&record.surface)
            .then_some(slot.restore.next)
            .flatten()
    }

    /// The deadline `address`'s own frame serves, which its frame's dirty
    /// gate counts and the loop wakes for while its frames run
    /// ([`wake_deadline`](Self::wake_deadline)): the device's next turn,
    /// which any window's turn runs, and the next attempt at the surface
    /// this window is owed. Another window's surface deadline is that
    /// window's: counted here, it would render this window on every wake
    /// while the other's frames are off, and never move.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn window_deadline(&self, address: PresentationAddress) -> Option<Instant> {
        // A dead executor rebuilds nothing and restores nothing: a deadline it
        // left in its record would wake the loop at a past instant for good.
        if self.executor_died() {
            return None;
        }
        super::host::merge_wake_deadlines(
            self.device_retry_at(),
            self.surface_retry_at_for(address),
        )
    }

    /// Whether the device refused `surface`, the size `address`'s window is
    /// at, and nothing has asked it again since.
    fn size_refused(placed: &Placed, address: PresentationAddress, surface: (u32, u32)) -> bool {
        let (Ok(width), Ok(height)) = (i32::try_from(surface.0), i32::try_from(surface.1)) else {
            return false;
        };
        placed
            .slots
            .get(&address)
            .and_then(|slot| placed.sender.record(slot.slot))
            .is_some_and(|record| {
                record.surface.size == SlotSize::Refused(DeviceSize::new(width, height))
            })
    }

    /// Send `publication`, the frame `epoch` names in its UI runtime's
    /// sequence, to `address`'s slot, measured on `surface`, turn
    /// the executor and answer what became of the frame. A panic of the
    /// frame is held for the window to report to its UI runtime after the
    /// frame ([`RasterSlot::report_frame_panic`]); the frame is refused
    /// (`SubmitVerdict::Refused`), and the window's next change draws in full.
    /// A size past the device's limit refuses the frame too, until the
    /// platform announces another size.
    fn submit(
        &mut self,
        address: PresentationAddress,
        surface: (u32, u32),
        epoch: FrameEpoch,
        publication: ScenePublication,
    ) -> SubmitVerdict {
        let Some(placed) = self.placed.as_mut() else {
            tracing::error!("BUG: raster host: a frame for a window with no executor");
            return SubmitVerdict::Failed;
        };
        // A dead executor takes no frame (ADR-0305 §9): the frame is finished
        // as far as the window can tell, and commits, so input and the agent
        // keep reading the tree while the window draws nothing.
        if placed.sender.health() == flui_engine::ExecutorHealth::Died {
            return SubmitVerdict::NoPresent;
        }
        let Some(generation) = Self::configure(placed, address, surface) else {
            // Past the device's limit, a retry is refused the same way.
            // Otherwise (a zero size, no surface yet) nothing is drawn until
            // the next resize.
            return if Self::size_refused(placed, address, surface) {
                SubmitVerdict::Refused
            } else {
                SubmitVerdict::NotShown
            };
        };
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: `configure` found the slot");
        let key = SurfaceKey {
            size: surface,
            generation,
        };
        let stamp = FrameStamp::new(address, epoch, generation);
        let damage = slot.base.damage_for(&publication, key);
        let shown = publication.scene.id();
        let mut snapshot =
            SceneSnapshot::of_version(stamp, publication.base, damage, publication.scene);
        // The images a replaced context kept go first, so the registry's
        // newer frames replace them.
        let mut updates = std::mem::take(&mut slot.carried);
        if let Some(textures) = &slot.textures {
            updates.merge(textures.take_updates());
        }
        if !updates.is_empty() {
            snapshot = snapshot.textures(updates);
        }
        let id = slot.slot;
        let receipt = match placed
            .sender
            .send(RasterCommand::Frame { slot: id, snapshot })
        {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::error!(%error, "raster host: a frame was refused");
                // The texture frames it took from the registry and the carry
                // reached no backend: the window's next frame takes them.
                if let RasterCommand::Frame { snapshot, .. } = error.into_command() {
                    slot.carried = snapshot.textures;
                }
                return SubmitVerdict::Failed;
            }
        };
        let frame = PendingFrame {
            epoch,
            shown,
            key,
            tainted: false,
        };
        if placed.executor.inline().is_none() {
            return Self::queue(placed, address, frame, receipt);
        }
        // The turn runs code that is not ours; its settlements are drained
        // whatever it did, so a panic leaves none for the next frame to trip
        // over. A panic of the turn itself (a retired scene's `Drop`) is held
        // for the window's report and does not change what the frame became.
        let turned = Self::turn_for(placed, address);
        placed.settlements.clear();
        placed.sender.drain_settlements(id, &mut placed.settlements);
        let settled = placed
            .settlements
            .iter()
            .find(|settlement| settlement.epoch == epoch)
            .map(|settlement| settlement.verdict);
        let Some(verdict) = Self::settle_one(placed, address, frame, settled, turned) else {
            let slot = placed
                .slots
                .get_mut(&address)
                .expect("BUG: the window an inline frame settled for holds its slot");
            let _logged = slot.panic.contain(move || {
                tracing::error!(?epoch, "raster host: an inline frame settled nothing");
            });
            return SubmitVerdict::Failed;
        };
        verdict
    }

    /// Hold `frame`, which a raster thread took with `receipt`, until it
    /// settles: answered [`SubmitVerdict::Queued`], or
    /// [`SubmitVerdict::QueuedReplacing`] naming the frame it took the place
    /// of, which never settles and is no longer held (ADR-0305 §4).
    fn queue(
        placed: &mut Placed,
        address: PresentationAddress,
        mut frame: PendingFrame,
        receipt: SendReceipt,
    ) -> SubmitVerdict {
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: `configure` found the slot");
        let verdict = match receipt {
            SendReceipt::Replaced(replaced) => {
                // The replacement inherits the replaced frame's taint, as it
                // inherits its damage.
                if slot
                    .pending
                    .iter()
                    .any(|held| held.epoch == replaced && held.tainted)
                {
                    frame.tainted = true;
                }
                slot.pending.retain(|held| held.epoch != replaced);
                SubmitVerdict::QueuedReplacing(replaced)
            }
            SendReceipt::Accepted | SendReceipt::Configured(_) | SendReceipt::Unconfigured => {
                SubmitVerdict::Queued
            }
        };
        slot.pending.push(frame);
        verdict
    }

    /// What `frame` of `address`'s window became, once the executor settled
    /// it `settled` in a turn that `turned`, with the slot's base moved to
    /// what the screen now holds; `None` when nothing settled it and nothing
    /// refuses it.
    fn settle_one(
        placed: &mut Placed,
        address: PresentationAddress,
        frame: PendingFrame,
        settled: Option<RasterVerdict>,
        turned: SlotTurn,
    ) -> Option<SubmitVerdict> {
        let PendingFrame {
            shown,
            key,
            tainted,
            ..
        } = frame;
        let id = placed.slots.get(&address)?.slot;
        // No surface the GPU takes, or no size it draws at: the frame is
        // refused, and nothing until the window's next change (or a rebuilt
        // device) asks again. Retried, the frame would meet the same refusal.
        let stuck = settled != Some(RasterVerdict::Presented)
            && (placed
                .sender
                .record(id)
                .is_some_and(|record| record.surface.hold == SurfaceHold::Refused)
                || Self::size_refused(placed, address, key.size));
        let slot = placed.slots.get_mut(&address)?;
        if turned == SlotTurn::Panicked {
            let verdict = match settled {
                // The render itself panicked, which the executor settles as
                // a retry. Refused: a renderer that panics on this scene
                // panics on its retry too, and the window's next change
                // draws again.
                Some(RasterVerdict::Retry) | None => SubmitVerdict::Refused,
                // A command before the frame panicked (a resize, an upload)
                // and the frame settled otherwise: what it became stands, so
                // a stale surface is retried and applies what is still
                // pending. Once per surface: another frame on it that meets
                // the panic again (the retry, or one queued behind the first)
                // is the size's or the driver's failure, not the moment's,
                // and the window's next change tries it again.
                Some(RasterVerdict::SurfaceStale) if slot.stale_panic == Some(key) => {
                    SubmitVerdict::Refused
                }
                Some(verdict) => {
                    let verdict = Self::verdict(&mut slot.base, verdict, shown, key);
                    slot.stale_panic = (verdict == SubmitVerdict::SurfaceStale).then_some(key);
                    verdict
                }
            };
            // The image may be torn or miss an upload: the next frame is
            // drawn in full.
            slot.base.forget();
            return Some(verdict);
        }
        match settled {
            // Nothing settled says nothing of the surface.
            None => {}
            // A frame held when its slot's panic was taken may have met the
            // panic and lost its mark: settled stale, it counts as the
            // panicked frame does.
            Some(RasterVerdict::SurfaceStale) if tainted => {
                if slot.stale_panic == Some(key) {
                    slot.base.forget();
                    return Some(SubmitVerdict::Refused);
                }
                slot.stale_panic = Some(key);
            }
            // Any other settlement ends the surface's retry.
            Some(_) => slot.stale_panic = None,
        }
        if stuck {
            slot.base.forget();
            return Some(SubmitVerdict::Refused);
        }
        Some(Self::verdict(&mut slot.base, settled?, shown, key))
    }

    /// Drain what a raster thread settled of `address`'s frames into
    /// `into`, each with the verdict its UI runtime settles it with
    /// (`UiRuntime::settle_frame`). A frame whose turn panicked (its
    /// settlement's mark) settles as a panicked one, its panic held for the window's
    /// report; a frame settled with no record of it held (one the
    /// owner forgot on a detach) is dropped. An inline executor settles each
    /// frame in its own submit and leaves nothing here.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn drain_settled(
        &mut self,
        address: PresentationAddress,
        into: &mut Vec<flui_runtime::sink::FrameSettlement>,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        if placed.executor.inline().is_some() {
            return;
        }
        let Some(id) = placed.slots.get(&address).map(|slot| slot.slot) else {
            return;
        };
        let mut settlements = std::mem::take(&mut placed.settlements);
        settlements.clear();
        // The slot's failure is taken with its settlements, under one lock,
        // for its payload, which the window reports. Which frame a panic came
        // with is each settlement's own mark; taking the panic ends the mark,
        // so a frame held now and not settled in this drain may settle
        // unmarked though the panic came with its turn (or with a command
        // before it): it is tainted, its settlement does not move the base,
        // and settled stale it counts toward the surface's retry.
        let failure = placed
            .sender
            .drain_settlements_and_failure(id, &mut settlements);
        // Read after the drain, which settles what the death left in the
        // inboxes: a death the drain saw is seen here.
        let died = placed.sender.health() == flui_engine::ExecutorHealth::Died;
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: the window whose slot drained still holds it");
        let panicked = Self::hold_slot_failure(
            slot,
            failure,
            FirstPanic::new(),
            placed.sender.painter_resets(),
        ) == SlotTurn::Panicked;
        if panicked {
            for held in &mut slot.pending {
                held.tainted |= !settlements
                    .iter()
                    .any(|settlement| settlement.epoch == held.epoch);
            }
        }
        let turned = |settlement: &SlotSettlement| {
            if settlement.panicked {
                SlotTurn::Panicked
            } else {
                SlotTurn::Clean
            }
        };
        // A frame drawn after a panicked one of this drain was drawn over its
        // image, which may be torn or miss an upload: it does not move the
        // base either (after a panicked render, whose next frame the executor
        // draws in full anyway, that costs one more full frame).
        let mut torn = false;
        for settlement in &settlements {
            torn |= settlement.panicked;
            let Some(slot) = placed.slots.get_mut(&address) else {
                break;
            };
            let Some(at) = slot
                .pending
                .iter()
                .position(|held| held.epoch == settlement.epoch)
            else {
                let epoch = settlement.epoch;
                let _logged = slot.panic.contain(move || {
                    tracing::warn!(
                        ?epoch,
                        "raster host: a settlement for a frame the window does not hold"
                    );
                });
                continue;
            };
            let frame = slot.pending.remove(at);
            let forget = frame.tainted || torn;
            let verdict = Self::settle_one(
                placed,
                address,
                frame,
                Some(settlement.verdict),
                turned(settlement),
            )
            .unwrap_or(SubmitVerdict::Failed);
            if forget && let Some(slot) = placed.slots.get_mut(&address) {
                slot.base.forget();
            }
            // After the death no later frame shows the tree: one that will not
            // be shown (the executor's own `Failed` at the death, a stale or
            // withheld one) is finished with nothing to present, so it commits
            // and its held input replays (ADR-0305 §9).
            let shown = matches!(verdict, SubmitVerdict::Presented | SubmitVerdict::NoPresent);
            let verdict = if died && !shown {
                SubmitVerdict::NoPresent
            } else {
                verdict
            };
            into.push(flui_runtime::sink::FrameSettlement::new(
                address,
                settlement.epoch,
                verdict,
            ));
        }
        // A dead executor's frames still held never settle, or settle only
        // if a wedged render returns: the window settles them now, as
        // finished with nothing to present, so its UI runtime commits the
        // tree they were painted from and the input held for that commit
        // replays. The window is quarantined, not closed (ADR-0305 §9): its
        // frames go on against the current tree while it draws nothing. A
        // settlement the thread still sends for one of them is a frame the
        // window no longer holds.
        if died
            && let Some(slot) = placed.slots.get_mut(&address)
            && !slot.pending.is_empty()
        {
            slot.base.forget();
            into.extend(slot.pending.drain(..).map(|frame| {
                flui_runtime::sink::FrameSettlement::new(
                    address,
                    frame.epoch,
                    SubmitVerdict::NoPresent,
                )
            }));
        }
        // After a panic the next frame the window asks for draws in full,
        // whatever settled clean in this drain.
        if panicked && let Some(slot) = placed.slots.get_mut(&address) {
            slot.base.forget();
        }
        placed.settlements = settlements;
    }

    /// How many frames of `address`'s window its sink may hold unsettled
    /// ([`in_flight_limit`]); one for a window with no slot, whose frames
    /// are refused.
    fn in_flight_limit(&self, address: PresentationAddress) -> NonZeroUsize {
        self.placed
            .as_ref()
            .and_then(|placed| placed.slots.get(&address))
            .map_or(NonZeroUsize::MIN, |slot| slot.in_flight_limit)
    }

    /// Whether the in-flight floor of `address`'s presentation still guards
    /// a settlement (ADR-0305 §5): `false` only once a raster thread has
    /// settled every frame it holds of the window and its wake has returned
    /// after them, so the window was asked for the frame that drains them,
    /// which its host delivers once the window can take one: woken at the
    /// floor's pace before then, the loop would only ask the other windows
    /// for frames they do not owe. Inline, with no slot, with a frame still
    /// on the thread, or with a settlement whose wake has not returned (it
    /// has not run yet, or a requester panicked), the floor stands.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn floor_owed(&self, address: PresentationAddress) -> bool {
        let Some(placed) = self.placed.as_ref() else {
            return true;
        };
        if placed.executor.inline().is_some() {
            return true;
        }
        let Some(slot) = placed.slots.get(&address) else {
            return true;
        };
        slot.pending.is_empty()
            || placed.sender.settlements_announced(slot.slot) < slot.pending.len()
    }

    /// Whether `address`'s window holds frames a raster thread has not
    /// settled yet.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn has_pending(&self, address: PresentationAddress) -> bool {
        self.placed
            .as_ref()
            .and_then(|placed| placed.slots.get(&address))
            .is_some_and(|slot| !slot.pending.is_empty())
    }

    /// What a settled frame of `version`, drawn on `surface`, became, with
    /// the slot's base moved to what the screen now holds.
    fn verdict(
        base: &mut RasterBase,
        verdict: RasterVerdict,
        version: flui_layer::SceneVersionId,
        surface: SurfaceKey,
    ) -> SubmitVerdict {
        match verdict {
            RasterVerdict::Presented => {
                base.shown(version, surface);
                SubmitVerdict::Presented
            }
            RasterVerdict::NoPresent => {
                base.shown(version, surface);
                SubmitVerdict::NoPresent
            }
            RasterVerdict::NotShown => SubmitVerdict::NotShown,
            RasterVerdict::Retry => SubmitVerdict::Retry,
            RasterVerdict::SurfaceStale => {
                // The surface the image was on is gone.
                base.forget();
                SubmitVerdict::SurfaceStale
            }
            RasterVerdict::DeviceLost => {
                // The image went with the device: the next frame is drawn in full.
                base.forget();
                SubmitVerdict::DeviceLost
            }
            RasterVerdict::Failed => SubmitVerdict::Failed,
        }
    }
}
impl Placed {
    fn new(
        executor: Executor,
        sender: RasterSender,
        surfaces: SurfaceFactory,
        redraws: Redraws,
    ) -> Self {
        Self {
            executor,
            sender,
            surfaces,
            slots: rustc_hash::FxHashMap::default(),
            settlements: Vec::new(),
            redraws,
            retries_sent: 0,
            retries_unadopted: 0,
        }
    }
}

/// What a window's work before its frame came to
/// ([`RasterHost::before_frame`]).
#[derive(Debug, Clone, Copy)]
struct BeforeFrame {
    /// The window's surface came back, its display changed or the executor let its
    /// painter go: the frame repaints in full.
    repaint: bool,
    /// The application's retry of a given-up device found no device: the
    /// device stays given up, which the window tells its runtime again.
    retry_failed: bool,
}

/// Why `send` refused a command, for a log line.
fn send_refusal(error: &RasterSendError) -> &'static str {
    match error {
        RasterSendError::Full(_) => "the lifecycle queue is full",
        RasterSendError::StaleSlot(_) => "the command names a stale slot",
        RasterSendError::Gone(_) => "the raster executor has stopped",
        _ => "the raster executor refused the command",
    }
}

/// The environment variable that switches damage production off for every
/// window the process opens: `FLUI_DAMAGE=off` sends every frame as `Full`
/// and never renders a partial frame (ADR-0087 §3). It changes what the host
/// hands the executor and nothing else: the pipeline still retains its
/// repaint boundaries' contents, and the backend still renders into its
/// retained target when the surface needs an intermediate image. It is the
/// fallback for stale pixels in the field, read once per window, and stays
/// until an `AppConfig` switch carries the same choice per app.
const DAMAGE_ENV: &str = "FLUI_DAMAGE";

/// The damage mode [`DAMAGE_ENV`]'s value selects: `off` (any case) turns
/// damage off; unset, empty or `on` keeps the default; anything else keeps
/// the default and says so, since a typo must not silently disable the
/// fallback someone reached for.
fn damage_mode_from(value: Option<&str>) -> DamageMode {
    match value.map(str::trim) {
        None | Some("") => DamageMode::default(),
        Some(value) if value.eq_ignore_ascii_case("off") => DamageMode::Off,
        Some(value) if value.eq_ignore_ascii_case("on") => DamageMode::default(),
        Some(value) => {
            tracing::warn!(value, "{DAMAGE_ENV} takes `on` or `off`; damage stays on");
            DamageMode::default()
        }
    }
}

/// The damage mode the process's [`DAMAGE_ENV`] selects.
fn damage_mode() -> DamageMode {
    damage_mode_from(std::env::var(DAMAGE_ENV).ok().as_deref())
}

/// Where the desktop loop's executor turns on `host` (ADR-0305 §10): on its
/// raster thread on the Wayland host, where the two-window run met the
/// pacing budget, and inline on every other host until a run on it does.
/// X11 waits for a run on a real X server, since Xvfb has no vsync and an
/// unmapped window's `Fifo` present may block the one thread every window
/// shares; the headless host stays inline, as ADR-0305 §10 places it.
#[cfg_attr(
    all(
        any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
        not(all(test, feature = "testing"))
    ),
    expect(dead_code, reason = "only the desktop loop turns a raster thread")
)]
pub(crate) fn desktop_placement(host: flui_native::CurrentHost) -> RasterPlacement {
    #[cfg(target_os = "linux")]
    if host == flui_native::CurrentHost::Wayland {
        return RasterPlacement::Threaded;
    }
    let _ = host;
    RasterPlacement::Inline
}

/// One frame's sink: the window's size measured once at frame start, and the
/// host it submits through.
pub(crate) struct SlotFrame {
    host: SharedRasterHost,
    address: PresentationAddress,
    surface: (u32, u32),
}

impl SlotFrame {
    /// The sink for one frame of `address`, measured at `surface`.
    pub(crate) fn new(
        host: SharedRasterHost,
        address: PresentationAddress,
        surface: (u32, u32),
    ) -> Self {
        Self {
            host,
            address,
            surface,
        }
    }
}

impl FrameSink for SlotFrame {
    fn surface_size(&mut self) -> (u32, u32) {
        self.surface
    }

    fn in_flight_limit(&mut self) -> NonZeroUsize {
        // The slot's limit, fixed at its attach (ADR-0305 §5). A frame
        // dispatched while the host is turning reads the least: its submit
        // is answered `Retry`, so the host holds nothing of it.
        let address = self.address;
        self.host
            .try_read(|host| host.in_flight_limit(address))
            .unwrap_or(NonZeroUsize::MIN)
    }

    fn submit(&mut self, epoch: FrameEpoch, publication: ScenePublication) -> SubmitVerdict {
        // A frame dispatched while the host is turning (a platform callback
        // re-entering from the GPU work) keeps its work for the next wake.
        self.host
            .try_with(|host| host.submit(self.address, self.surface, epoch, publication))
            .unwrap_or(SubmitVerdict::Retry)
    }
}

/// What a window's frames a raster thread finished became, as
/// [`RasterSlot::settle`] answers it.
#[cfg_attr(
    all(
        any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
        not(all(test, feature = "testing"))
    ),
    expect(dead_code, reason = "only the desktop loop turns a raster thread")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadSettled {
    /// No frame of the window settled.
    Nothing,
    /// One presented: the first reveals the window, and the pacing fallback
    /// stands down.
    Presented,
    /// Frames settled and none presented (refused, retried, failed, not
    /// shown): the wake counts toward the first reveal's fallback as a wake
    /// whose own frame presented nothing does.
    NotPresented,
}

impl ThreadSettled {
    /// Fold in one more of the window's frames, `shown` when it presented.
    fn and(self, shown: bool) -> Self {
        if shown || self == Self::Presented {
            Self::Presented
        } else {
            Self::NotPresented
        }
    }
}

/// One window's slot as its frame driver holds it: the host, the window's
/// address on it, the size the platform last announced, and the device's
/// health as the window last reported it to its UI runtime, and whether it
/// reported the executor dead.
pub(crate) struct RasterSlot {
    pub(crate) host: SharedRasterHost,
    pub(crate) address: PresentationAddress,
    pub(crate) size: (u32, u32),
    pub(crate) device: DeviceHealth,
    /// Whether the window told its UI runtime that the executor died
    /// (ADR-0305 §9): terminal, so no device health is reported after it
    /// and no rebuild the application asks for reaches the executor.
    pub(crate) renderer_dead: bool,
}

impl RasterSlot {
    /// The sink of one frame, measured now.
    pub(crate) fn frame(&self) -> SlotFrame {
        SlotFrame::new(self.host.clone(), self.address, self.size)
    }

    /// Report the first panic of the window's frames since the last report
    /// to `ui_runtime`, which hands it to the application's frame-failure
    /// handler, then resume a panic of the work that met the host busy (a
    /// closed window's detach), which no window's handler owns. Run at the
    /// end of a wake, with no borrow of the host held: the handler is not
    /// ours.
    pub(crate) fn report_frame_panic(&self, ui_runtime: &crate::app::ui_runtime::UiRuntime) {
        let address = self.address;
        let mut caught = FirstPanic::new();
        if let Some(payload) = self
            .host
            .try_with(|host| host.take_frame_panic(address))
            .flatten()
        {
            let _reported =
                caught.contain(|| ui_runtime.report_contained_panic(address, &*payload));
            flui_foundation::panic::discard_payload(payload);
        }
        caught.absorb(self.host.take_deferred_panic());
        caught.resume();
    }

    /// Hand `ui_runtime` what a raster thread settled of the window's frames
    /// (`UiRuntime::settle_frame`), with no borrow of the host held. Run
    /// first in every wake of the window, whatever the wake goes on to do,
    /// so a frame the thread finished frees its place before the wake's gate
    /// reads it. Answers what the window's frames became, for its pacing and
    /// its first reveal, and the first panic of the code the settlements
    /// ran, which the window's wake resumes once the rest of it ran: it is
    /// this window's, and no other window's wake reports it. An inline
    /// executor leaves nothing to settle here.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn settle(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
    ) -> (ThreadSettled, FirstPanic) {
        let mut caught = FirstPanic::new();
        if self.host.placement == RasterPlacement::Inline {
            return (ThreadSettled::Nothing, caught);
        }
        let address = self.address;
        let mut drained = self
            .host
            .settled
            .try_borrow_mut()
            .map_or_else(|_| Vec::new(), |mut scratch| std::mem::take(&mut *scratch));
        drained.clear();
        let _drained = self
            .host
            .try_with(|host| host.drain_settled(address, &mut drained));
        // Every settlement reaches the runtime whatever an earlier one ran
        // into: a frame left unsettled holds its place for good.
        let mut settled = ThreadSettled::Nothing;
        for settlement in drained.drain(..) {
            let shown = settlement.verdict == SubmitVerdict::Presented;
            let answer = caught.contain(|| ui_runtime.settle_frame(settlement));
            // A settlement the runtime held no frame for is another life's.
            // One whose settling panicked is still this window's frame, and
            // a presented one is on the window's screen: it reveals the
            // window while its panic resumes at the end of the wake.
            if matches!(answer, Some(flui_runtime::sink::Settled::Stale)) {
                continue;
            }
            settled = settled.and(shown);
        }
        self.give_back(drained);
        // After the settlements, which commit what a dead executor never
        // will: the runtime learns of the death once.
        let _dead = caught.contain(|| self.settle_death(ui_runtime));
        (settled, caught)
    }

    /// Keep `scratch`'s room for the next wake's settlements.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    fn give_back(&self, scratch: Vec<flui_runtime::sink::FrameSettlement>) {
        if let Ok(mut kept) = self.host.settled.try_borrow_mut() {
            *kept = scratch;
        }
    }

    /// Whether a raster thread holds frames of the window it has not
    /// settled: the window's pacing then waits for their settlement, which
    /// wakes it, instead of arming its fallback.
    #[cfg_attr(
        any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn has_pending(&self) -> bool {
        let address = self.address;
        self.host
            .try_read(|host| host.has_pending(address))
            .unwrap_or(false)
    }

    /// The deadline this window's own frame serves, for its wake's dirty
    /// gate: [`RasterHost::window_deadline`], which the loop also wakes for
    /// while the window's frames run.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn frame_deadline(&self) -> Option<Instant> {
        let address = self.address;
        self.host
            .try_read(|host| host.window_deadline(address))
            .flatten()
    }

    /// Tell the executor whether the window can be seen (`shown`): a slot
    /// that cannot is not drawn, and draws in full when it shows again. A host
    /// already turning is told on the next wake.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "a hidden tab runs no frames, so the page's slot stays shown"
        )
    )]
    pub(crate) fn show(&self, shown: bool) {
        let visibility = if shown {
            SlotVisibility::Shown
        } else {
            SlotVisibility::Hidden
        };
        let address = self.address;
        let _told = self
            .host
            .try_with(|host| host.set_visibility(address, visibility));
    }

    /// Whether the application asked, through `ui_runtime`, for a rebuild of
    /// the device the executor gave up on: the runtime no longer reports the
    /// abandonment this window last told it of
    /// (`UiRuntime::retry_render_device`). A dead executor takes no rebuild.
    fn device_retry_asked(&self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        !self.renderer_dead
            && matches!(self.device, DeviceHealth::GivenUp { .. })
            && !ui_runtime
                .primary_device_recovery()
                .is_some_and(flui_runtime::frame_failure::DeviceRecovery::is_abandoned)
    }

    /// Run the window's frame at `now` through its slot and answer whether it
    /// presented.
    ///
    /// Before the frame: a probe for a device to replace a given-up one,
    /// when the application asked for it (a probe that answers none tells
    /// the application the device is given up again), a surface the window is owed whose pace is due
    /// ([`RasterHost::restore_due`]), the colour of the display the window is
    /// on (`display`; a changed profile sets the surface up again, ADR-0256
    /// §6), and a turn of the executor, which asks for a lost device that is
    /// due, or installs one whose request answered, even when the frame has
    /// nothing new to draw. A device or a surface
    /// that came back, a display that changed, or a painter the executor let go (a device rebuilt, a render that panicked), repaints the window in
    /// full. After it: the frame's GPU
    /// panic goes to the application's frame-failure handler, and a device
    /// the frame lost is reported with the frames it drops; its next turn is
    /// due at the executor's deadline, which [`RasterHost::window_deadline`] reads.
    pub(crate) fn pump(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        now: Instant,
        display: Option<DisplayColor>,
    ) -> bool {
        let address = self.address;
        let retry = self.device_retry_asked(ui_runtime);
        // A host already turning is asked again on the next wake: the
        // device stays given up and the runtime's request stands.
        let before = self
            .host
            .try_with(|host| host.before_frame(address, now, display, retry));
        if let (
            Some(BeforeFrame {
                retry_failed: true, ..
            }),
            DeviceHealth::GivenUp { attempts },
        ) = (before, self.device)
        {
            // The request is answered: the device is given up again, and
            // the next frame probes nothing until the application asks.
            ui_runtime.note_primary_device_recovery(Some(
                flui_runtime::frame_failure::DeviceRecovery::Abandoned { attempts },
            ));
        }
        let repaint = before.is_some_and(|before| before.repaint);
        if repaint | self.settle_device(ui_runtime) {
            ui_runtime.mark_primary_needs_full_repaint();
        }
        let mut sink = self.frame();
        let presented = ui_runtime
            .pump(&mut SampledClock(now), &mut sink)
            .presented();
        self.report_frame_panic(ui_runtime);
        let _ = self.settle_device(ui_runtime);
        presented
    }

    /// Tell `ui_runtime` where the device's rebuild stands after a frame, so
    /// the frames it drops carry it (ADR-0267). Answers whether the device
    /// came back, which repaints the window in full.
    pub(crate) fn settle_device(&mut self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        use flui_runtime::frame_failure::DeviceRecovery;

        // A host still turning up the stack leaves the device unseen, for the
        // next settle to see. The executor's failure is the whole executor's:
        // the first window to settle logs it.
        let Some((health, failure, executor_failure)) = self.host.try_read(|host| {
            let placed = host.placed.as_ref();
            (
                host.device_health(),
                placed.and_then(|placed| placed.sender.take_device_failure()),
                placed.and_then(|placed| placed.sender.take_executor_failure()),
            )
        }) else {
            return false;
        };
        // The death is told before any log line, whose subscriber is not ours.
        let dead = self.settle_death(ui_runtime);
        if let Some(error) = failure {
            tracing::warn!(%error, "GPU device rebuild failed");
        }
        if let Some(error) = executor_failure {
            tracing::error!(%error, "raster executor failed");
        }
        if dead {
            return false;
        }
        if health == self.device {
            return false;
        }
        let was_lost = self.device.is_lost();
        self.device = health;
        match health {
            DeviceHealth::Healthy => {
                ui_runtime.note_primary_device_recovery(None);
                if was_lost {
                    tracing::warn!("GPU device lost — recovered successfully");
                }
                was_lost
            }
            DeviceHealth::Rebuilding { attempts: 0 } => false,
            DeviceHealth::Rebuilding { attempts } => {
                ui_runtime.note_primary_device_recovery(Some(DeviceRecovery::Failed { attempts }));
                false
            }
            DeviceHealth::GivenUp { attempts } => {
                ui_runtime
                    .note_primary_device_recovery(Some(DeviceRecovery::Abandoned { attempts }));
                false
            }
            // A health state added later reports nothing until this match names it.
            _ => false,
        }
    }

    /// Tell `ui_runtime` once that the executor died
    /// ([`DeviceRecovery::Dead`](flui_runtime::frame_failure::DeviceRecovery::Dead),
    /// ADR-0305 §9), and answer whether it is dead: each window learns it
    /// from the executor's health, which stays.
    fn settle_death(&mut self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        if self.renderer_dead {
            return true;
        }
        let died = self
            .host
            .try_read(|host| {
                host.placed.as_ref().is_some_and(|placed| {
                    placed.sender.health() == flui_engine::ExecutorHealth::Died
                })
            })
            .unwrap_or(false);
        if died {
            self.renderer_dead = true;
            ui_runtime.note_primary_device_recovery(Some(
                flui_runtime::frame_failure::DeviceRecovery::Dead,
            ));
        }
        died
    }
}

impl Drop for RasterSlot {
    fn drop(&mut self) {
        self.host.detach(self.address);
    }
}

#[cfg(all(test, feature = "testing"))]
pub(super) mod tests;
