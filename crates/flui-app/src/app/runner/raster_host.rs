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
//! [`SubmitVerdict::Retry`](flui_runtime::sink::SubmitVerdict::Retry) and keeps its work for the next wake. Threaded,
//! the frame is answered queued and settles later: the thread's wake asks
//! every window for a frame, and each window's wake drains its own slot's
//! settlements into its UI runtime ([`RasterSlot::settle`]).
//!
//! The host also keeps what only the owner knows about each slot: the size
//! it last sent, what the window's surfaces are made over, the version on
//! screen that damage is measured from, and the presentation's external
//! textures, whose updates ride each frame. A frame is stamped with the
//! configuration generation the executor's record requires at that moment.

#[cfg(not(target_arch = "wasm32"))]
use std::cell::RefCell;
use std::num::NonZeroUsize;
#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;
use std::sync::Arc;

use web_time::Instant;

use flui_engine::{
    EngineError, PainterResets, RasterBackend, RasterExecutor, RasterPlacement, RasterSendError,
    RasterSender, SlotId, SlotSettlement, SlotSize, SlotVisibility, SurfaceFactory, SurfaceHold,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{FrameEpoch, PresentationAddress};
use flui_layer::{DamageMode, RasterBase, SurfaceKey};
use flui_platform::{DisplayColor, FramePacing, RedrawRequester, SurfaceTarget};

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

#[cfg(not(target_arch = "wasm32"))]
mod bounded_wait;
mod device_retry;
mod executor_turn;
mod frame_submit;
mod raster_slot;
mod shared_host;
mod slot_lifecycle;
mod slot_surface;
#[cfg(not(target_arch = "wasm32"))]
mod thread_link;
pub(crate) use raster_slot::RasterSlot;
#[cfg(all(test, feature = "testing"))]
use raster_slot::SlotFrame;
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub(crate) use raster_slot::ThreadSettled;
pub(crate) use shared_host::SharedRasterHost;
#[cfg(not(target_arch = "wasm32"))]
use thread_link::{DetachWait, ThreadLink};

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
    /// The working vectors of a texture frame's damage, kept between
    /// frames.
    texture_damage: flui_layer::TextureDamageScratch,
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
            texture_damage: flui_layer::TextureDamageScratch::default(),
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

#[cfg(all(test, feature = "testing"))]
pub(super) mod tests;
