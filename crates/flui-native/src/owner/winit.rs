//! The winit host's owner thread: winit's event loop, driven by
//! [`WinitApp`], the owner lane ([`control`]) that carries requests from other
//! threads to it, and the owner hooks and proxy transport built on that lane.
//!
//! winit is the primary desktop backend on Linux until a native Wayland/X11
//! backend lands, and a fallback on Windows and macOS.
//!
//! # Architecture
//!
//! Unlike GPUI which uses native platform APIs (Win32, Cocoa, Wayland/X11),
//! FLUI uses winit as a cross-platform abstraction. This trade-off provides:
//! - Simpler implementation and maintenance
//! - Faster multi-platform support
//! - Good-enough performance for most use cases
//!
//! The architecture uses winit 0.30's `ApplicationHandler` trait to manage
//! the event loop without consuming ownership.
//!
//! # Same-thread window creation during `on_ready`
//!
//! winit 0.30 requires a live `ActiveEventLoop` to create a window, and that
//! is only reachable from inside an `ApplicationHandler` callback running on
//! the event-loop thread. `Platform::run`'s `on_ready` callback is invoked
//! synchronously from [`WinitApp::resumed`] — one such callback — so a call
//! to `open_window` made from inside `on_ready` cannot go through the
//! cross-thread owner-control lane: that lane's only consumer is a *later*
//! dispatch on the same thread and cannot run
//! until `resumed` (and therefore `on_ready`) returns, which would deadlock
//! forever. [`ACTIVE_EVENT_LOOP`] publishes the live `ActiveEventLoop` for
//! the exact duration of the `on_ready` call so `open_window` can create the
//! window directly instead.
//!
//! # Programmatic close runs the owner's close teardown
//!
//! A window leaves this backend by exactly one body,
//! [`WinitApp::complete_window_close`], whichever way it was asked to go:
//! the compositor's `CloseRequested` (after the should-close veto) runs it
//! in the event arm, and
//! [`WindowCommand::Close`](flui_platform::WindowCommand::Close) — a
//! decision the embedder already made, so no veto — posts a per-window
//! request on the owner
//! control lane ([`ControlSender::request_close_window`]) that the owner
//! runs on its next turn. It cannot run in place: the teardown ends the
//! loop when the last window goes, which needs the live `ActiveEventLoop`
//! only an owner-turn callback holds, and the `WindowHandler::closed` callback must fire
//! on the owner thread whatever thread asked. Before this (issue #919)
//! `close()` hid the window and never left the tracking map, so the exit
//! policy — consulted only against that map — never saw the last window
//! go and the process lingered with nothing on screen.
//!
//! # Vsync pacing
//!
//! [`WinitApp::about_to_wait`] pins the event loop's control flow every
//! iteration rather than trusting winit's own default: `ControlFlow::Wait`
//! when nothing needs a wall-clock wake, `ControlFlow::WaitUntil(deadline)`
//! when the registered wake-deadline hook (issue #556) answers `Some`. This
//! is deliberate, not decorative: `flui-app`'s frame loop is wake-driven (a
//! redraw is requested only from `UiRuntime::wake_frame`/`request_redraw`,
//! never polled), and steady-state pacing for a frame that DOES present
//! comes entirely from the GPU-side blocking Fifo present in
//! `flui-engine`'s `Renderer::render_scene` (see the frame-pacing ADR). If
//! a future winit release changed its own default away from `Wait` (e.g.
//! to `Poll`), that pacing model would silently regress into a busy-spin
//! with no compile or CI signal — pinning the value here turns an upstream
//! default change into a one-line diff to review instead of a surprise.
//!
//! # Wall-clock wake actuation
//!
//! `ControlFlow::WaitUntil` alone does not run a pump: when the deadline
//! expires, winit calls [`WinitApp::new_events`] with
//! `StartCause::ResumeTimeReached`, but nothing about that call delivers a
//! `WindowEvent` — no window is dirty, nothing requested a redraw. Without
//! an explicit actuator, `about_to_wait` would run again immediately,
//! re-consult the SAME hook (nothing has changed, so it answers the same
//! now-past deadline), and re-arm `WaitUntil` at an instant already behind
//! `Instant::now()` — winit fires it again on the very next loop
//! iteration, forever: a wall-clock wake with no actuator on the other
//! side degenerates into an unbounded busy-spin, not a wake. `new_events`
//! closes this: on `ResumeTimeReached` it calls `request_redraw()` on
//! every window this platform currently tracks, which queues a REAL
//! `WindowEvent::RedrawRequested` for the next iteration — the exact same
//! `dispatch_request_frame`/`WindowHandler::frame_requested`/`wake_action` path every
//! other wake in this backend already goes through, so a deadline that
//! resolves nothing new (nothing was actually due, or the UI runtime has no
//! other demand) still costs at most one extra `wake_action::Skip`, never
//! a second spin.

// The same-thread window creation dereferences the published event loop: a
// small `unsafe` island, documented at its site. The workspace lint
// `unsafe_code = "warn"` is opted out here, at the module boundary, rather
// than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use flui_foundation::ClaimSlot;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent as WinitWindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::ModifiersState,
    window::WindowId as WinitWindowId,
};

use flui_foundation::geometry::Point;
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::input_capabilities::{InputCapabilities, PointerKindSet};

use crate::owner::app_delivery::AppDelivery;
use crate::owner::notices::{NoticeChannel, StaleOnWake};
use crate::owner::signal::OwnerSignal;
#[cfg(target_os = "macos")]
use crate::owner::sources::SystemPreferenceSource;
#[cfg(windows)]
use crate::owner::windows::control::OwnerPreferences;
use crate::{
    error::BootstrapError, error::PlatformError, executor::BackgroundExecutor, host, host::Host,
    host_window::HostWindow, owner::OpenWindowError, owner::OwnerHooks, owner::OwnerPlatform,
    owner::PendingWindow, owner::ProxySendError, owner::ProxyTransport, owner::WindowOpen,
    platform::AppHandler, platform::HostPlatform, platform::Platform, platform::PlatformExecutor,
};
use control::{
    ControlCommand, ControlReceiver, ControlSendError, ControlSender, OpenWindowResult,
    control_lane,
};
use flui_platform::{
    DisplayId, DisplaySnapshot, PlatformInput, WindowHandler, WindowId, WindowOptions,
};
use std::rc::Rc;

pub(crate) mod control;
mod file_drops;
#[cfg(target_os = "linux")]
mod session_end;
mod system_sources;

pub(crate) use file_drops::FileDrops;
pub(crate) use system_sources::PreferenceStart;
#[cfg(target_os = "linux")]
pub(crate) use system_sources::{HostNoticeStart, HostNotices, PortalPreferences};

/// What the winit host's table names for its owner thread (ADR-0151 §10): the
/// displays the loop holds, the channel its system notices take, the clipboard
/// and the drag-and-drop transport, and the operating system's own sources
/// winit lacks, which the loop starts as it runs, once its signal exists. The
/// owner reaches them through the hooks and names no feature.
pub(crate) trait WinitHost: 'static {
    /// The displays, made once per loop: the loop holds them for the table's
    /// displays position and hands them to every window it opens.
    fn displays() -> Arc<dyn HostDisplays>;

    /// The channel the host's system notices take to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

    /// The system's preferences, which winit does not report.
    fn preferences() -> Box<dyn PreferenceStart>;

    /// The sources of the host's system notices: logind and the kernel's
    /// memory pressure.
    #[cfg(target_os = "linux")]
    fn host_notices() -> Box<dyn HostNoticeStart>;

    /// The clipboard, made once per loop: the loop holds it and answers the
    /// table's clipboard position from it.
    fn clipboard() -> Arc<dyn flui_platform::Clipboard>;

    /// The drag-and-drop transport, made once per loop: the loop holds it,
    /// answers the table's drag position from it and tells it of the files
    /// winit reports dragged and dropped.
    fn drag() -> Arc<dyn FileDrops>;

    /// The translation of winit's input, made once per loop: the loop holds
    /// it and hands it every pointer, device and key event.
    fn input() -> Box<dyn LoopInput>;
}

/// The translation of what winit reports of the input, as the loop holds it
/// ([`WinitHost::input`]): the loop hands it each pointer, device and key
/// event under its state lock and dispatches what comes back once the lock is
/// released. Nothing here runs user code.
pub(crate) trait LoopInput: Send {
    /// A window opens in `event_loop`'s session: the input it has, with the
    /// device kinds seen so far present.
    fn opening(&mut self, event_loop: &ActiveEventLoop) -> InputCapabilities;

    /// What `event` of `window` at `scale`, with `modifiers` held, delivers
    /// after the move held before it, which may be another window's; `None`
    /// when `event` is no pointer input.
    fn window_event(
        &mut self,
        window: WindowId,
        event: &WinitWindowEvent,
        scale: f64,
        modifiers: ModifiersState,
    ) -> Option<Vec<(WindowId, PlatformInput)>>;

    /// What each of `windows` hears of `event` of the system's `device`: an
    /// arrival or a removal, which ends that device's contacts.
    fn device_event(
        &mut self,
        device: winit::event::DeviceId,
        event: &winit::event::DeviceEvent,
        windows: &[WindowId],
    ) -> Vec<(WindowId, PlatformInput)>;

    /// The device kinds present, when they changed since the last call.
    fn presence_change(&mut self) -> Option<PointerKindSet>;

    /// The move held to see whether the next event replaces it: the loop turn
    /// ended, or something else follows.
    fn flush(&mut self) -> Vec<(WindowId, PlatformInput)>;

    /// End every contact of `window`: it lost focus or went away.
    fn focus_lost(&mut self, window: WindowId) -> Vec<PlatformInput>;

    /// What a key event with `modifiers` held delivers, if anything.
    fn key(
        &self,
        event: winit::event::KeyEvent,
        modifiers: ModifiersState,
    ) -> Option<PlatformInput>;

    /// What an input method event delivers, if anything.
    fn ime(&self, event: &winit::event::Ime) -> Option<PlatformInput>;
}

/// The clipboard position: the clipboard the host's [`WinitHost::clipboard`]
/// made, which the loop holds.
impl host::Clipboard<WinitLoop> for WinitLoop {
    fn clipboard(owner: &WinitLoop) -> Arc<dyn flui_platform::Clipboard> {
        owner.with_state(|state| Arc::clone(&state.clipboard))
    }
}

/// The drag position: the transport the host's [`WinitHost::drag`] made,
/// which the loop holds. Every answer is a clone of that one source (ADR-0038
/// §2), so every id it mints stays redeemable at every clone.
impl host::Drag<WinitLoop> for WinitLoop {
    fn data_transfer(owner: &WinitLoop) -> Arc<dyn DataTransferSource> {
        owner.with_state(|state| Arc::clone(&state.data_transfer) as Arc<dyn DataTransferSource>)
    }
}

/// The host's displays as the winit loop and the windows it opens reach them:
/// the operating system's own source, never winit's monitor list (ADR-0236),
/// which the table makes ([`WinitHost::displays`]).
///
/// Each operating system's source learns of a change its own way, so the
/// methods past `resample` are each one system's: the Win32 source is a
/// subscriber of the loop's broadcast receiver, the AppKit source a
/// notification observer, the Linux source a thread on a connection of its
/// own, which also says whether the windows are Wayland surfaces.
pub(crate) trait HostDisplays: StaleOnWake {
    /// The current snapshot: the last sample unless something invalidated it.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the source cannot be read on this
    /// thread or the snapshot revision space is used up.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    /// The display `window` is on; `None` when the source holds none for it.
    fn id_of_window(&self, window: &winit::window::Window) -> Option<DisplayId>;

    /// Bring the source up to date with `event_loop`, whose owner `signal` the
    /// source may wake from then on: `true` when the snapshot may have
    /// changed, so the owner is woken to sample it again.
    ///
    /// Only the Linux source reads the arguments: it follows the connection
    /// winit opened. The Win32 and AppKit sources read the system apart from
    /// winit, so they mark themselves stale and ignore both.
    fn resample(&self, event_loop: &ActiveEventLoop, signal: Option<&Arc<OwnerSignal>>) -> bool;

    /// The source's part of the system's broadcasts, a subscriber of the
    /// receiver the loop builds on resume; on Windows the displays are told of
    /// a change through it, not through a watch of their own.
    #[cfg(windows)]
    fn broadcasts(
        self: Arc<Self>,
        signal: &Arc<OwnerSignal>,
    ) -> std::rc::Rc<dyn super::windows::broadcast::Subscriber>;

    /// The source's observation of what changes the displays (AppKit's
    /// screen-parameters notification, the compositor's display thread),
    /// which marks the source stale and wakes the owner behind `signal` until
    /// the watch is dropped.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> super::watch::Watch;

    /// Whether the windows open on a Wayland compositor.
    #[cfg(target_os = "linux")]
    fn is_wayland(&self) -> bool;
}

/// The displays position is the loop's: the native source the table made
/// ([`WinitHost::displays`]), kept between calls and read again only after an
/// event marked it stale, since the host asks on every owner wake. It needs no
/// event loop, so it answers before `Platform::run`; on Linux that first
/// answer is read once on a connection that then closes, and the display
/// thread takes over when the loop resumes.
impl host::Displays<WinitLoop> for WinitLoop {
    fn snapshot(owner: &WinitLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner.displays.snapshot()
    }
}

/// Convert a tracked physical cursor position to logical pixels.
fn logical_cursor_point(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
) -> Point<f64> {
    Point::new(position.x / scale_factor, position.y / scale_factor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenWindowStateError {
    NotRunning,
    Starting,
    OwnerWouldBlock,
    Stopped,
}

pub(crate) enum WinitRunState {
    New {
        quit_requested: bool,
    },
    Starting {
        owner_thread: ThreadId,
        control: ControlSender,
    },
    Running {
        owner_thread: ThreadId,
        control: ControlSender,
    },
    Stopped,
}

impl WinitRunState {
    pub(crate) fn control_for_open_window(
        &self,
        caller_thread: ThreadId,
    ) -> Result<ControlSender, OpenWindowStateError> {
        match self {
            Self::New { .. } => Err(OpenWindowStateError::NotRunning),
            Self::Starting { owner_thread, .. } if *owner_thread == caller_thread => {
                Err(OpenWindowStateError::OwnerWouldBlock)
            }
            Self::Starting { .. } => Err(OpenWindowStateError::Starting),
            Self::Running { owner_thread, .. } if *owner_thread == caller_thread => {
                Err(OpenWindowStateError::OwnerWouldBlock)
            }
            Self::Running { control, .. } => Ok(control.clone()),
            Self::Stopped => Err(OpenWindowStateError::Stopped),
        }
    }
}

/// The winit host's loop: the event loop's run state, the windows it tracks
/// and the state every position of the host's table reads.
/// [`WinitPlatform`](crate::WinitPlatform) is its platform.
pub struct WinitLoop {
    pub(crate) owner_signal: Mutex<Option<Arc<crate::owner::signal::OwnerSignal>>>,
    /// The monitors last sampled and the snapshot built from them, as the
    /// table made them. Shared with every window so each can name its
    /// display; outside the state lock so a window's `display_id()` never
    /// contends with the platform state.
    pub(crate) displays: Arc<dyn HostDisplays>,

    /// The window feature's side of the loop: it opens the windows the loop
    /// tracks.
    pub(crate) windows: Arc<dyn LoopWindows>,

    /// Where system notices go, and what a wake marks stale before the host
    /// hears of it.
    pub(crate) notices: Arc<dyn NoticeChannel>,

    /// What the loop delivers to the application's handler, which `run`
    /// binds as the loop resumes.
    pub(crate) app: Arc<AppDelivery>,

    /// Starts the system preference source as the loop resumes, as the table
    /// made it ([`WinitHost::preferences`]).
    preference_start: Box<dyn PreferenceStart>,

    /// Starts the sources of the host's system notices once the owner can
    /// deliver what they queue ([`WinitHost::host_notices`]).
    #[cfg(target_os = "linux")]
    host_notice_start: Box<dyn HostNoticeStart>,

    /// Run where a turn reads what wakes asked of it, before the
    /// application's turn: a test wakes the owner from here.
    #[cfg(test)]
    pub(crate) turn_read_hook_for_test: Mutex<Option<Box<dyn FnMut() + Send>>>,

    state: Arc<Mutex<WinitLoopState>>,
}

/// A window as the winit loop drives it: the window feature's, which the loop
/// holds without naming it (ADR-0151 §10). The loop hands it winit's window
/// events and closes it.
pub(crate) trait LoopWindow: HostWindow {
    /// The loop tracks the window now: it finishes opening (a window created
    /// unmapped, for its window manager to read its mode first, is mapped).
    fn finish_opening(&self);

    /// The winit window behind it.
    fn inner(&self) -> &Arc<winit::window::Window>;

    /// Reads the window again and publishes its state if it changed.
    fn refresh_state(&self);

    /// winit reported the window focused or not.
    fn set_focused(&self, focused: bool);

    /// winit reported the window occluded or not.
    fn set_occluded(&self, occluded: bool);

    /// The window was shown or hidden.
    fn set_visible(&self, visible: bool);

    /// Whether the window moved to another display since the last call.
    fn take_display_change(&self) -> bool;

    /// Whether no display has been reported for the window yet, so the loop
    /// looks again when it goes idle.
    #[cfg(target_os = "linux")]
    fn awaiting_display(&self) -> bool;

    /// The desktop portal's appearance changed: whether the window's did.
    #[cfg(target_os = "linux")]
    fn observe_portal_appearance(&self, appearance: flui_platform::WindowAppearance) -> bool;

    /// The window's id on the X server, when its window manager is watched.
    #[cfg(target_os = "linux")]
    fn x11_id(&self) -> Option<u32>;

    /// The window closed: it refuses commands from now on.
    fn mark_closed(&self);
}

/// What the loop hands the window feature to build a window with.
pub(crate) struct WindowSeed<'a> {
    /// The id the window reports from its creation on.
    pub(crate) id: WindowId,
    /// The lane a programmatic close travels to the owner by.
    pub(crate) close_lane: ControlSender,
    /// The monitors, by which the window names its display.
    pub(crate) displays: &'a Arc<dyn HostDisplays>,
    /// Where a wake marks the window's state stale.
    pub(crate) notices: &'a Arc<dyn NoticeChannel>,
    /// The handler the window delivers to from its first event on.
    pub(crate) handler: Rc<dyn WindowHandler>,
    /// The owner's signal, while the loop runs: what a window manager's view
    /// wakes when a window it watches changes.
    #[cfg(target_os = "linux")]
    pub(crate) signal: Option<&'a Arc<crate::owner::signal::OwnerSignal>>,
}

/// The windows of the winit loop, which the window feature builds
/// (ADR-0151 §10): the host builds the window feature's object and hands it
/// to [`WinitLoop::new`]. The loop numbers, registers and finishes each window
/// it builds.
pub(crate) trait LoopWindows: Send + Sync {
    /// Builds a window on `event_loop` from `seed` and `options`: the native
    /// window and the window around it, which the loop then tracks.
    ///
    /// # Errors
    ///
    /// What `Platform::open_window` reports.
    fn build(
        &self,
        event_loop: &ActiveEventLoop,
        seed: WindowSeed<'_>,
        options: WindowOptions,
    ) -> Result<Arc<dyn LoopWindow>, OpenWindowError>;

    /// The X11 ids of the windows whose state the window manager changed
    /// without a winit event since the last call, which the loop reads again;
    /// empty when no window manager is watched.
    #[cfg(target_os = "linux")]
    fn take_unreported(&self) -> Vec<u32>;
}

impl std::fmt::Debug for WinitLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `WinitLoopState` holds boxed callbacks and channel endpoints
        // that don't implement `Debug`; print the immutable descriptor only.
        f.debug_struct("WinitLoop").finish_non_exhaustive()
    }
}

#[cfg(windows)]
#[derive(Clone)]
pub(crate) enum PreferenceObservation {
    Observed(flui_platform::SystemPreferences),
    Deferred,
    Failed(String),
}

/// Internal state for WinitLoop
pub(crate) struct WinitLoopState {
    /// Delivery values only; native observation remains on the event-loop owner.
    #[cfg(windows)]
    pub(crate) preferences: PreferenceObservation,
    /// The source the owner keeps, once it resumed.
    #[cfg(target_os = "macos")]
    pub(crate) preferences: Option<std::sync::Weak<dyn SystemPreferenceSource + Send + Sync>>,
    /// The source the owner keeps, once it resumed.
    #[cfg(target_os = "linux")]
    pub(crate) preferences: Option<std::sync::Weak<dyn PortalPreferences>>,
    /// The translation of winit's input, as the table made it
    /// ([`WinitHost::input`]); its identities and phases are committed before
    /// dispatch.
    input: Box<dyn LoopInput>,

    /// Background executor
    pub(crate) background_executor: Arc<BackgroundExecutor>,

    /// Clipboard
    pub(crate) clipboard: Arc<dyn flui_platform::Clipboard>,

    /// The one data-transfer source of this platform instance (ADR-0038):
    /// constructed once by the host's table, cloned out by
    /// `Platform::data_transfer()`, never reconstructed — it owns the single
    /// offer table, so ids stay redeemable across every clone.
    pub(crate) data_transfer: Arc<dyn FileDrops>,

    /// Map of winit window IDs to platform window IDs
    window_id_map: HashMap<WinitWindowId, WindowId>,

    /// Map of platform window IDs to the windows the window feature opened
    pub(crate) windows: HashMap<WindowId, Arc<dyn LoopWindow>>,

    /// Active window ID
    pub(crate) active_window: Option<WindowId>,

    /// Next window ID to allocate
    next_window_id: u64,

    /// Event-loop ownership and control-lane lifecycle.
    pub(crate) run_state: WinitRunState,

    /// Current cursor position per window (physical)
    cursor_positions: HashMap<WindowId, winit::dpi::PhysicalPosition<f64>>,

    /// Current keyboard modifiers, held in winit's own raw form.
    ///
    /// Not pre-converted: the input translation converts it with the event it
    /// belongs to ([`LoopInput::key`], [`LoopInput::window_event`]) — one
    /// native value, converted once per read site rather than cached in a
    /// second form that could drift from what `ModifiersChanged` actually
    /// reported.
    current_modifiers: ModifiersState,
}

impl WinitLoopState {
    fn new(
        clipboard: Arc<dyn flui_platform::Clipboard>,
        data_transfer: Arc<dyn FileDrops>,
        input: Box<dyn LoopInput>,
    ) -> Self {
        Self {
            input,
            #[cfg(windows)]
            preferences: PreferenceObservation::Deferred,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            preferences: None,
            background_executor: Arc::new(BackgroundExecutor::new()),
            clipboard,
            data_transfer,
            window_id_map: HashMap::new(),
            windows: HashMap::new(),
            active_window: None,
            next_window_id: 1,
            run_state: WinitRunState::New {
                quit_requested: false,
            },
            cursor_positions: HashMap::new(),
            current_modifiers: ModifiersState::empty(),
        }
    }

    /// The id the next window carries from its creation on.
    fn allocate_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    /// Tracks `window`, which winit knows as `winit_id`: the first window
    /// becomes the active one.
    fn register_window(
        &mut self,
        winit_id: WinitWindowId,
        platform_id: WindowId,
        window: Arc<dyn LoopWindow>,
    ) {
        self.window_id_map.insert(winit_id, platform_id);
        self.windows.insert(platform_id, window);

        if self.active_window.is_none() {
            self.active_window = Some(platform_id);
        }
    }

    fn get_platform_window_id(&self, winit_id: WinitWindowId) -> Option<WindowId> {
        self.window_id_map.get(&winit_id).copied()
    }
}

impl WinitLoop {
    /// A loop that has not run yet, over the displays, the notice channel, the
    /// sources, the clipboard and the drag-and-drop transport `H` makes, whose
    /// windows `windows` opens.
    pub(crate) fn new<H: WinitHost>(windows: Arc<dyn LoopWindows>) -> Self {
        let app = Arc::new(AppDelivery::new());
        let platform = Self {
            owner_signal: Mutex::new(None),
            displays: H::displays(),
            windows,
            notices: H::notices(&app),
            app,
            #[cfg(test)]
            turn_read_hook_for_test: Mutex::new(None),
            preference_start: H::preferences(),
            #[cfg(target_os = "linux")]
            host_notice_start: H::host_notices(),
            state: Arc::new(Mutex::new(WinitLoopState::new(
                H::clipboard(),
                H::drag(),
                H::input(),
            ))),
        };
        #[cfg(target_os = "linux")]
        platform
            .notices
            .mark_held_stale_on_wake(Arc::downgrade(&platform.displays) as _);
        platform
    }

    /// Run the test's read, as the loop's own reads run in a turn.
    #[cfg(test)]
    fn run_turn_read_hook_for_test(&self) {
        if let Some(read) = self.turn_read_hook_for_test.lock().as_mut() {
            read();
        }
    }

    /// Bring the display source up to date with `event_loop` and wake the owner
    /// when that may have changed the snapshot, so the host samples it again
    /// ([`HostDisplays::resample`]).
    fn resample_displays(&self, event_loop: &ActiveEventLoop) {
        let signal = self.owner_signal.lock().clone();
        if self.displays.resample(event_loop, signal.as_ref())
            && let Some(signal) = signal
        {
            let _ = signal.wake();
        }
    }

    /// Get mutable access to state (for internal use)
    pub(crate) fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WinitLoopState) -> R,
    {
        let mut state = self.state.lock();
        f(&mut state)
    }

    /// Read again the windows whose mode the X11 window manager changed, so
    /// `WindowHandler::state_changed` follows a minimize or a maximize that winit reports
    /// no event for. Each window publishes with the state lock released.
    #[cfg(target_os = "linux")]
    fn refresh_unreported(&self) {
        let changed = self.windows.take_unreported();
        if changed.is_empty() {
            return;
        }
        let windows: Vec<_> = self.with_state(|state| {
            state
                .windows
                .values()
                .filter(|window| window.x11_id().is_some_and(|id| changed.contains(&id)))
                .cloned()
                .collect()
        });
        for window in windows {
            window.refresh_state();
        }
    }

    /// Create and run the event loop for `platform`, the
    /// [`HostPlatform`] over this loop that `on_ready`'s
    /// owner capability wraps
    ///
    /// This is the main entry point for the platform. It creates the event
    /// loop, initializes the platform, and runs until quit is requested.
    ///
    /// # Errors
    /// Same contract as [`Platform::run`]: [`PlatformError::Bootstrap`] for
    /// an `on_ready` failure, [`PlatformError::EventLoop`] for a loop-level
    /// failure (including a second `run` on the same platform).
    pub(crate) fn run_event_loop(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        tracing::info!("Creating winit event loop");

        let event_loop =
            EventLoop::builder()
                .build()
                .map_err(|error| PlatformError::EventLoop {
                    message: error.to_string(),
                })?;
        let event_loop_proxy = event_loop.create_proxy();
        let owner_proxy = event_loop_proxy.clone();
        let signal = crate::owner::signal::OwnerSignal::new(Arc::new(move || {
            owner_proxy
                .send_event(())
                .map_err(|error| PlatformError::EventLoop {
                    message: error.to_string(),
                })
        }));
        let previous_signal = self.owner_signal.lock().replace(signal);
        drop(previous_signal);
        let wake_owner = Arc::new(move || {
            if event_loop_proxy.send_event(()).is_err() {
                tracing::trace!("winit event loop closed before its wake was delivered");
            }
        });
        let (control, receiver) = control_lane(wake_owner);
        let owner_thread = thread::current().id();
        let quit_requested = self.install_control_lane(owner_thread, control.clone())?;

        let mut app = WinitApp {
            platform: Arc::clone(&self),
            owner_platform: platform,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            preference_source: None,
            #[cfg(windows)]
            broadcasts: None,
            #[cfg(target_os = "macos")]
            preference_observers: None,
            #[cfg(target_os = "linux")]
            host_notices: None,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            display_watch: None,
            handler: Some(handler),
            owner_hooks: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: self_close_deadline_from_env(),
            self_close_route: self_close_route_from_env(),
        };
        if quit_requested {
            control.request_quit();
        }

        let result = event_loop
            .run_app(&mut app)
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            });
        // Taken before `finish_shutdown` only so the borrow shape stays
        // simple — `finish_shutdown` does not touch this field.
        let bootstrap_error = app.bootstrap_error.take();
        app.finish_shutdown();
        // The loop is over: the handler goes, outside every lock.
        self.app.release();

        combine_shutdown_result(bootstrap_error, result)
    }

    fn install_control_lane(
        &self,
        owner_thread: ThreadId,
        control: ControlSender,
    ) -> Result<bool, PlatformError> {
        self.with_state(|state| {
            let previous = std::mem::replace(&mut state.run_state, WinitRunState::Stopped);
            match previous {
                WinitRunState::New { quit_requested } => {
                    state.run_state = WinitRunState::Starting {
                        owner_thread,
                        control,
                    };
                    Ok(quit_requested)
                }
                other => {
                    state.run_state = other;
                    Err(PlatformError::EventLoop {
                        message: "the winit event loop can only be started once".to_string(),
                    })
                }
            }
        })
    }

    fn mark_running(&self) {
        self.with_state(|state| {
            let previous = std::mem::replace(&mut state.run_state, WinitRunState::Stopped);
            state.run_state = match previous {
                WinitRunState::Starting {
                    owner_thread,
                    control,
                } => WinitRunState::Running {
                    owner_thread,
                    control,
                },
                other => other,
            };
        });
    }

    fn mark_stopped(&self) {
        self.with_state(|state| state.run_state = WinitRunState::Stopped);
    }

    /// Create a winit window immediately using a live `ActiveEventLoop` and
    /// register it in platform state: the window feature builds it. Shared by
    /// the cross-thread control lane and `open_window`'s same-thread fast path
    /// (see the module-level doc on same-thread window creation).
    pub(crate) fn create_window_now(
        &self,
        event_loop: &ActiveEventLoop,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<WindowId, OpenWindowError> {
        // Resolve the lane BEFORE creating the native window: a window whose
        // programmatic close could reach no owner is the shape of issue #919,
        // and a live `ActiveEventLoop` without an installed lane cannot
        // happen (`run_event_loop` installs it before the loop starts) — but
        // the signature already speaks `OpenWindowError`, so the impossible
        // case is a typed refusal, not a panic holding a native window.
        let Some(close_lane) = self.control_sender() else {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        };
        // The platform identity comes before the window, so the window
        // carries its own `id()` from the start rather than being registered
        // under an identity it cannot itself report.
        let id = self.with_state(WinitLoopState::allocate_window_id);
        #[cfg(target_os = "linux")]
        let signal = self.owner_signal.lock().clone();
        let window = self.windows.build(
            event_loop,
            WindowSeed {
                id,
                close_lane,
                displays: &self.displays,
                notices: &self.notices,
                handler,
                #[cfg(target_os = "linux")]
                signal: signal.as_ref(),
            },
            options,
        )?;
        #[cfg(target_os = "linux")]
        if let Some(source) = self.with_state(|state| {
            state
                .preferences
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
        }) {
            // The snapshot was sampled before the portal's scheme was known.
            if window.observe_portal_appearance(source.appearance()) {
                window.refresh_state();
            }
        }
        self.with_state(|state| {
            // The initial snapshot reflects the devices already seen.
            let capabilities = state.input.opening(event_loop);
            window.delivery().set_input_capabilities(capabilities);
            state.register_window(window.inner().id(), id, Arc::clone(&window));
        });
        if let Some(signal) = self.owner_signal.lock().as_ref() {
            // A command applied from another thread publishes there: what it
            // raises waits for an owner turn.
            let resumed = Arc::downgrade(&window);
            window
                .delivery()
                .resume_through(OwnerSignal::window_wake(signal, move || {
                    if let Some(window) = resumed.upgrade() {
                        window.delivery().deliver_queued();
                    }
                }));
        }
        window.finish_opening();
        // A monitor attached since the last sample is first seen here.
        self.resample_displays(event_loop);
        // No lock is held: the handler hears its window's capabilities
        // before anything later, whichever path opened the window.
        window.delivery().deliver_capability_snapshot();

        tracing::info!(?id, "Created window");

        Ok(id)
    }

    /// Look up a previously-created window by [`WindowId`] and return the
    /// exact stored allocation as a [`HostWindow`]. Named `window_by_id`
    /// (not `window_handle`) to avoid colliding with the unrelated
    /// [`PlatformWindow::window_handle`](flui_platform::PlatformWindow::window_handle)
    /// raw GPU-handle accessor.
    pub(crate) fn window_by_id(
        &self,
        window_id: WindowId,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        self.with_state(|state| {
            state
                .windows
                .get(&window_id)
                .ok_or_else(|| OpenWindowError::Backend {
                    message: "Window not found in state".to_string(),
                })
                .map(|window| Arc::clone(window) as Arc<dyn HostWindow>)
        })
    }

    /// Ask the application's handler whether the loop may end, outside the
    /// state lock (ADR-0039): its body (`flui-app`'s `AppRuntime::should_exit`)
    /// drops removed UI runtime state, whose destructors (a dispose hook
    /// opening another window, say) may call back into this platform. `true`
    /// (allow the exit) when no handler is bound.
    fn exit_policy_allows_exit(&self) -> bool {
        self.app.exit_policy().or_resume(true)
    }

    /// The wall-clock instant the application's handler asks to be woken at,
    /// asked through the call point outside the state lock: the handler walks
    /// every hosted UI runtime and takes gesture-arena locks (ADR-0038 §5).
    fn wake_deadline(&self) -> Option<web_time::Instant> {
        self.app.wake_deadline().or_resume(None)
    }

    /// The owner control lane, while a loop is starting or running; `None`
    /// before `run_event_loop` installs one or after the loop stopped.
    fn control_sender(&self) -> Option<ControlSender> {
        self.with_state(|state| match &state.run_state {
            WinitRunState::Starting { control, .. } | WinitRunState::Running { control, .. } => {
                Some(control.clone())
            }
            WinitRunState::New { .. } | WinitRunState::Stopped => None,
        })
    }
}

/// Combines `event_loop.run_app`'s own result with a stashed `on_ready`
/// bootstrap failure (`WinitApp::bootstrap_error`).
///
/// The bootstrap error is the root cause the whole fallible-`on_ready`
/// design exists to surface — it is checked and returned FIRST, never
/// shadowed by a loop-level error that arrives alongside it.
/// Checking `result` first (the bug this function fixes) would silently
/// drop the actual bootstrap failure exactly when
/// [`WinitApp::request_exit`]'s own `event_loop.exit()` call produces a
/// winit-level error of its own — the one scenario this whole design
/// exists to get right. When both are present, the loop error rides along
/// in [`PlatformError::Bootstrap`]'s `loop_error` field on top of the
/// bootstrap error, so neither is lost: the bootstrap error's own chain is
/// still reachable via `Error::source()` on the returned value.
fn combine_shutdown_result(
    bootstrap_error: Option<BootstrapError>,
    result: Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    match bootstrap_error {
        Some(source) => Err(PlatformError::Bootstrap {
            source,
            loop_error: result.err().map(|error| error.to_string()),
        }),
        None => result,
    }
}

thread_local! {
    /// Published only while `on_ready` executes synchronously inside
    /// [`WinitApp::resumed`], on the winit event-loop thread. See the
    /// module-level doc on same-thread window creation.
    pub(crate) static ACTIVE_EVENT_LOOP: Cell<Option<NonNull<ActiveEventLoop>>> = const { Cell::new(None) };
}

/// Publishes `event_loop` to [`ACTIVE_EVENT_LOOP`] for the duration of `f`,
/// then un-publishes it unconditionally (including if `f` panics), so a
/// publication never outlives the call that set it.
///
/// Callers must only pass an `event_loop` that stays valid for the entire
/// call to `f` — `resumed`'s `&ActiveEventLoop` parameter satisfies this
/// because it outlives the whole `resumed` call, which fully contains `f`.
fn with_active_event_loop<R>(event_loop: &ActiveEventLoop, f: impl FnOnce() -> R) -> R {
    // Save/restore rather than unconditionally clearing to `None`: today
    // `on_ready` is the only caller and never nests, but restoring whatever
    // was published before this call (instead of clobbering it to `None`)
    // keeps a hypothetical future nested publication on this thread correct
    // for free.
    struct RestoreOnDrop(Option<NonNull<ActiveEventLoop>>);
    impl Drop for RestoreOnDrop {
        fn drop(&mut self) {
            ACTIVE_EVENT_LOOP.with(|cell| cell.set(self.0));
        }
    }

    let previous = ACTIVE_EVENT_LOOP.with(|cell| cell.replace(Some(NonNull::from(event_loop))));
    let _restore = RestoreOnDrop(previous);
    f()
}

/// Reads the harness self-close deadline from `FLUI_SELF_CLOSE_AFTER_MS`
/// (milliseconds from now). See [`WinitApp::self_close_deadline`] for why
/// this hook exists. A present-but-unparsable value is a harness
/// misconfiguration worth surfacing, not silently ignoring.
fn self_close_deadline_from_env() -> Option<Instant> {
    let raw = std::env::var("FLUI_SELF_CLOSE_AFTER_MS").ok()?;
    match raw.parse::<u64>() {
        Ok(ms) => {
            tracing::info!(after_ms = ms, "harness self-close armed");
            Some(Instant::now() + Duration::from_millis(ms))
        }
        Err(error) => {
            tracing::warn!(%raw, %error, "FLUI_SELF_CLOSE_AFTER_MS is not a millisecond count");
            None
        }
    }
}

/// Which close route the harness self-close drives — see
/// [`WinitApp::self_close_route`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SelfCloseRoute {
    /// Synthesize `WindowEvent::CloseRequested`: the compositor/user path,
    /// veto and all.
    #[default]
    Compositor,
    /// Call [`WindowCommand::Close`](flui_platform::WindowCommand::Close)
    /// on the tracked window: the programmatic path, exactly as an
    /// application closing its own window does (issue #919).
    Programmatic,
}

/// Reads the harness self-close route from `FLUI_SELF_CLOSE_ROUTE`
/// (`compositor`, the default, or `programmatic`). Only meaningful with a
/// deadline armed. An unknown value is a harness misconfiguration worth
/// surfacing, not silently defaulting.
fn self_close_route_from_env() -> SelfCloseRoute {
    let Ok(raw) = std::env::var("FLUI_SELF_CLOSE_ROUTE") else {
        return SelfCloseRoute::default();
    };
    match raw.as_str() {
        "compositor" => SelfCloseRoute::Compositor,
        "programmatic" => SelfCloseRoute::Programmatic,
        _ => {
            tracing::warn!(
                %raw,
                "FLUI_SELF_CLOSE_ROUTE is neither `compositor` nor `programmatic`; using compositor"
            );
            SelfCloseRoute::Compositor
        }
    }
}

/// How long an `about_to_wait` decision parks the loop for, in whole
/// milliseconds from now — `None` for an open-ended `Wait`. Trace-only:
/// the number a pacing investigation needs next to the frame callback's own
/// timing, without which "the loop was idle" and "the loop was blocked in
/// the GPU" are indistinguishable in a log.
fn control_flow_wait_ms(control_flow: ControlFlow) -> Option<u64> {
    match control_flow {
        ControlFlow::WaitUntil(deadline) => Some(
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64,
        ),
        ControlFlow::Wait | ControlFlow::Poll => None,
    }
}

/// The earlier of two optional wall-clock deadlines — `None` only when both
/// are `None`, so an armed deadline is never lost to an idle `Wait`.
fn earliest_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (deadline @ Some(_), None) | (None, deadline @ Some(_)) => deadline,
        (None, None) => None,
    }
}

/// Application handler for winit event loop
///
/// Implements `ApplicationHandler` to receive events from winit without
/// consuming the event loop.
pub(crate) struct WinitApp {
    #[cfg(windows)]
    preference_source: Option<std::rc::Rc<dyn OwnerPreferences>>,
    /// The hidden top-level window that tells the displays and the preference
    /// source the system's display and setting broadcasts; owner-thread only.
    #[cfg(windows)]
    broadcasts: Option<super::windows::broadcast::Receiver>,
    #[cfg(target_os = "macos")]
    preference_source: Option<Arc<dyn SystemPreferenceSource + Send + Sync>>,
    /// Wakes the owner when AppKit reports a preference change; owner-thread only.
    #[cfg(target_os = "macos")]
    preference_observers: Option<super::watch::Watch>,
    #[cfg(target_os = "linux")]
    preference_source: Option<Arc<dyn PortalPreferences>>,
    /// Sleep, shutdown, lock and memory pressure from logind and the kernel;
    /// owner-thread only.
    #[cfg(target_os = "linux")]
    pub(crate) host_notices: Option<Box<dyn HostNotices>>,
    /// The displays' observation of what changes them, started on resume and
    /// ended when dropped; owner-thread only. On Windows the displays hear the
    /// receiver in `broadcasts` instead.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    display_watch: Option<super::watch::Watch>,
    pub(crate) platform: Arc<WinitLoop>,
    /// The platform over `platform` that `on_ready`'s owner capability wraps.
    owner_platform: Arc<dyn Platform>,
    /// The application's handler, until the loop resumes and binds it.
    handler: Option<Rc<dyn AppHandler>>,
    /// The owner hooks of the run, held here for its length: the loop's
    /// delivery holds them weakly.
    owner_hooks: Option<Arc<dyn OwnerHooks>>,
    control: ControlReceiver,
    quit_notified: bool,
    /// Claim slots whose delivery already landed (`Ok`), kept alive past
    /// dequeue so a *later* abandonment (the requester claimed delivery,
    /// then dropped without reading it) can still be swept and unwound —
    /// see [`WinitApp::sweep_settled_replies`]. Bounded by lane capacity
    /// for queued entries; a delivered-but-unclaimed entry persists here
    /// until the requester claims or drops it (ADR-0039 §3's honest
    /// registry-occupancy statement — not admission-time bounded).
    in_flight_replies: Vec<(WindowId, ClaimSlot<OpenWindowResult>)>,
    /// Set when `on_ready` returns `Err` (a fallible bootstrap failure —
    /// window creation, GPU init, root-widget attach — has no other return
    /// path back to `Platform::run`'s caller). `resumed` stashes it
    /// here and requests exit; `run_event_loop` propagates it out of `run`
    /// (wrapped as [`PlatformError::Bootstrap`]) once the loop has actually
    /// unwound, rather than swallowing it into a bare `tracing::error!`
    /// while the loop keeps pumping a half-built app.
    bootstrap_error: Option<BootstrapError>,
    /// Harness self-close deadline, armed from `FLUI_SELF_CLOSE_AFTER_MS` at
    /// loop start. When it expires, [`WinitApp::fire_self_close_if_due`]
    /// synthesizes `WindowEvent::CloseRequested` for one tracked window —
    /// the exact arm a compositor close takes. This exists for Wayland test
    /// harnesses: unlike X11 (where XTEST + a `WM_DELETE_WINDOW`
    /// client message can drive a real close from outside), Wayland has no
    /// protocol a third-party client can
    /// use to close another client's toplevel — `xdg_toplevel.close` is
    /// compositor→client only — so the close-path teardown ordering is
    /// untestable on Wayland without this in-process trigger. Unset (the
    /// default, and always in production) this is `None` and nothing here
    /// runs.
    self_close_deadline: Option<Instant>,
    /// Which close route the armed self-close drives
    /// (`FLUI_SELF_CLOSE_ROUTE`): the synthesized compositor
    /// `CloseRequested` by default, or
    /// [`WindowCommand::Close`](flui_platform::WindowCommand::Close) on the
    /// tracked window — so a harness can prove the
    /// programmatic route's teardown (issue #919) against a real app, real
    /// GPU surface, and the embedder's real exit-policy hook, which no
    /// in-crate test can host. Irrelevant while `self_close_deadline` is
    /// `None`.
    self_close_route: SelfCloseRoute,
}

enum NativeDispatch {
    Input(PlatformInput),
    Hover(bool),
    /// The device kinds present changed: a capability change (ADR-0213).
    Present(flui_platform::input_capabilities::PointerKindSet),
}

/// One `Present` per tracked window, sorted by window.
fn presence_dispatch(
    state: &WinitLoopState,
    present: flui_platform::input_capabilities::PointerKindSet,
) -> Vec<(Arc<dyn LoopWindow>, NativeDispatch)> {
    let mut windows: Vec<_> = state.windows.iter().collect();
    windows.sort_by_key(|(id, _)| id.0);
    windows
        .into_iter()
        .map(|(_, window)| (Arc::clone(window), NativeDispatch::Present(present)))
        .collect()
}

/// `inputs` addressed to the windows still open, after the presence change
/// their readings revealed: a device is present before its input.
fn native_batch(
    state: &mut WinitLoopState,
    inputs: Vec<(WindowId, flui_platform::PlatformInput)>,
) -> Vec<(Arc<dyn LoopWindow>, NativeDispatch)> {
    let mut batch = state
        .input
        .presence_change()
        .map(|present| presence_dispatch(state, present))
        .unwrap_or_default();
    batch.extend(inputs.into_iter().filter_map(|(window, input)| {
        state
            .windows
            .get(&window)
            .map(|window| (Arc::clone(window), NativeDispatch::Input(input)))
    }));
    batch
}

fn dispatch_native_inputs(inputs: Vec<(Arc<dyn LoopWindow>, NativeDispatch)>) {
    let mut first = None;
    for (window, input) in inputs {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match input {
            NativeDispatch::Input(input) => {
                window.delivery().dispatch_input(input);
            }
            NativeDispatch::Hover(hovered) => {
                window.delivery().report_hover(hovered);
            }
            NativeDispatch::Present(present) => {
                let callbacks = window.delivery();
                callbacks
                    .set_input_capabilities(callbacks.input_capabilities().with_present(present));
            }
        }));
        if let Err(payload) = result {
            if first.is_some() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                first = Some(payload);
            }
        }
        // Callback reentry can close the window and release its final other
        // owner. After a failure, its opaque captures must not retire here.
        if first.is_some() {
            std::mem::forget(window);
        } else if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(window)))
        {
            first = Some(payload);
        }
    }
    if let Some(payload) = first {
        std::panic::resume_unwind(payload);
    }
}

/// Completes a dequeued window request even if owner-side processing
/// unwinds. Wraps the claim-slot owner half (ADR-0039 §3) rather than the
/// pre-ADR-0039 buffered `sync_channel(1)` one-shot.
struct OpenWindowReplyGuard {
    reply: Option<ClaimSlot<OpenWindowResult>>,
}

impl OpenWindowReplyGuard {
    fn new(reply: ClaimSlot<OpenWindowResult>) -> Self {
        Self { reply: Some(reply) }
    }

    /// Delivers `result`. Returns the still-live `ClaimSlot` when delivery
    /// landed on `Pending` (the owner should track it in
    /// `in_flight_replies` for a possible later abandonment); `None` when
    /// the requester had already abandoned the slot (the owner should
    /// unwind instead of tracking).
    fn complete(mut self, result: OpenWindowResult) -> Option<ClaimSlot<OpenWindowResult>> {
        let reply = self
            .reply
            .take()
            .expect("BUG: complete called after this guard already completed once");
        match reply.deliver(result) {
            Ok(()) => Some(reply),
            Err(_already_abandoned) => None,
        }
    }
}

impl Drop for OpenWindowReplyGuard {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            // `OwnerGone`, not `Backend`: this guard fires when the owner
            // stops (panics, or drops the guard) before ever completing the
            // request -- consistent with `reject_pending_commands`'
            // identical shutdown-path delivery and with `OwnerGone`'s own
            // documented semantics ("the loop died after the request was
            // already accepted"). `Backend` means the backend tried and
            // failed to create the window; that never happened here.
            let _ = reply.deliver(Err(OpenWindowError::OwnerGone { rejected: None }));
        }
    }
}

impl ApplicationHandler for WinitApp {
    /// Actuates a wall-clock wake (issue #556): `ControlFlow::WaitUntil`
    /// expiring delivers `StartCause::ResumeTimeReached` here with no
    /// accompanying `WindowEvent` — nothing else in this backend turns that
    /// bare wake into a pump. Poke every currently tracked window's
    /// `request_redraw()`, which queues a real
    /// `WindowEvent::RedrawRequested` for the very next iteration and so
    /// re-enters `dispatch_request_frame`/`WindowHandler::frame_requested`/`wake_action`
    /// exactly like any other wake — never a second, parallel produce path
    /// (see the module doc's "Wall-clock wake actuation" section for why an
    /// un-actuated `WaitUntil` degenerates into an unbounded spin instead).
    /// Every other `StartCause` (`Init`, `Poll`, `WaitCancelled`) is a
    /// documented no-op here — `Poll`/`WaitCancelled` never fire without a
    /// caller setting `ControlFlow::Poll`, which this backend never does.
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            #[cfg(windows)]
            if let Some(source) = &self.preference_source {
                let now = Instant::now();
                let preference_due = source.retry_deadline().is_some_and(|time| time <= now);
                if preference_due {
                    source.retry_if_due();
                    let runtime_due = self
                        .platform
                        .wake_deadline()
                        .is_some_and(|time| time <= now);
                    if !runtime_due {
                        // Preference retries post owner work; they do not dirty windows.
                        return;
                    }
                }
            }
            self.platform.with_state(|state| {
                for window in state.windows.values() {
                    window.inner().request_redraw();
                }
            });
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        tracing::info!("Application resumed");

        // Sample the monitors now: a platform that resumes after a monitor
        // change must not keep the list from the previous resume.
        self.platform.resample_displays(event_loop);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.observe_displays();
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Err(error) = self.initialize_preferences() {
            self.bootstrap_error = Some(error.into());
            self.request_exit(event_loop);
            return;
        }

        // Bind the application's handler and run its bootstrap once.
        // `ACTIVE_EVENT_LOOP` is published for this exact nested call so
        // `open_window` can create windows directly instead of deadlocking on
        // the cross-thread lane (see the module-level doc). Mint the
        // owner-thread capability (ADR-0039) for this call: `!Send + !Sync`,
        // so it cannot outlive this stack frame except by the bootstrap
        // keeping it itself (e.g. `flui-app`'s `OWNER_PLATFORM_HOST` TLS slot).
        if let Some(handler) = self.handler.take() {
            tracing::info!("Running the application's bootstrap");
            let owner_thread = thread::current().id();
            let hooks: Arc<dyn OwnerHooks> = Arc::new(WinitOwnerHooks {
                platform: Arc::clone(&self.platform),
                owner_thread,
            });
            let app = Arc::clone(&self.platform.app);
            app.bind(handler, &self.owner_platform, &hooks);
            let signal = self.platform.owner_signal.lock().clone();
            if let Some(signal) = signal {
                app.resume_through_signal(&signal);
            }
            self.owner_hooks = Some(Arc::clone(&hooks));
            let owner = OwnerPlatform::new(Arc::clone(&self.owner_platform), hooks);
            if let Err(error) = with_active_event_loop(event_loop, || app.ready(owner)) {
                // A fallible bootstrap (window creation, GPU init,
                // root-widget attach) failed with no return path back to
                // `Platform::run`'s caller except through this stash and an
                // immediate exit — never continue the loop with a
                // half-built app. `finish_shutdown` (via `request_exit`)
                // fires quit handlers and closes the owner lane exactly as
                // any other shutdown path does.
                //
                // Logged once, at `error` level, from `Platform::run`'s own
                // `inspect_err` once `run_event_loop` has combined this with
                // whatever `event_loop.run_app` itself returns -- not here,
                // to avoid double-logging the same failure under two
                // different (and, at the `run()` site, previously
                // misleading) messages.
                tracing::debug!(
                    "on_ready bootstrap failed; requesting event-loop exit \
                     (see Platform::run's propagated error for the failure)"
                );
                self.bootstrap_error = Some(error);
                self.request_exit(event_loop);
                return;
            }
        }

        self.platform.mark_running();
        let signal = self.platform.owner_signal.lock().clone();
        if let Some(signal) = signal
            && let Err(error) = signal.start()
        {
            tracing::error!(%error, "owner wake start failed");
            self.request_exit(event_loop);
        } else {
            // The owner can drain the inbox from here on. A sleep that arrived
            // while `on_ready` ran would have held logind's inhibitor with no
            // turn to deliver it, so the sources start only now.
            #[cfg(target_os = "linux")]
            self.start_host_notices();
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        // A held move is delivered, after the presence its reading reveals,
        // before a device arrives or leaves; raw motion keeps it held.
        if matches!(
            event,
            winit::event::DeviceEvent::Added | winit::event::DeviceEvent::Removed
        ) {
            self.flush_native_pointer();
        }
        let inputs = self.platform.with_state(|state| {
            let mut windows: Vec<_> = state.windows.keys().copied().collect();
            windows.sort_by_key(|window| window.0);
            let inputs = state.input.device_event(device_id, &event, &windows);
            let mut batch: Vec<_> = inputs
                .into_iter()
                .filter_map(|(window, input)| {
                    state
                        .windows
                        .get(&window)
                        .cloned()
                        .map(|window| (window, NativeDispatch::Input(input)))
                })
                .collect();
            // A removal reports absence after the contacts it cancelled.
            if let Some(present) = state.input.presence_change() {
                batch.extend(presence_dispatch(state, present));
            }
            batch
        });
        dispatch_native_inputs(inputs);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WinitWindowId,
        event: WinitWindowEvent,
    ) {
        let (platform_id, window) = self.platform.with_state(|state| {
            let pid = state.get_platform_window_id(window_id);
            let win = pid.and_then(|id| state.windows.get(&id).cloned());
            (pid, win)
        });

        let Some(platform_id) = platform_id else {
            tracing::warn!("Received event for unknown window");
            return;
        };

        let native_pointer = matches!(
            event,
            WinitWindowEvent::CursorMoved { .. }
                | WinitWindowEvent::MouseInput { .. }
                | WinitWindowEvent::Touch(_)
                | WinitWindowEvent::MouseWheel { .. }
                | WinitWindowEvent::PinchGesture { .. }
                | WinitWindowEvent::RotationGesture { .. }
                | WinitWindowEvent::PanGesture { .. }
                | WinitWindowEvent::TouchpadPressure { .. }
                | WinitWindowEvent::CursorEntered { .. }
                | WinitWindowEvent::CursorLeft { .. }
        );
        #[cfg(windows)]
        if native_pointer && self.deliver_preferences() {
            self.request_exit(event_loop);
            return;
        }
        if native_pointer {
            let Some(window) = window else {
                return;
            };
            if let WinitWindowEvent::MouseWheel { delta, .. } = &event {
                tracing::debug!(?delta, "MouseWheel");
            }
            let scale = window.inner().scale_factor();
            let mut batch = self.platform.with_state(|state| {
                if let WinitWindowEvent::CursorMoved { position, .. } = &event {
                    state.cursor_positions.insert(platform_id, *position);
                }
                let inputs = state
                    .input
                    .window_event(platform_id, &event, scale, state.current_modifiers)
                    .unwrap_or_default();
                native_batch(state, inputs)
            });
            match event {
                WinitWindowEvent::CursorEntered { .. } => {
                    batch.push((Arc::clone(&window), NativeDispatch::Hover(true)));
                }
                WinitWindowEvent::CursorLeft { .. } => {
                    batch.push((Arc::clone(&window), NativeDispatch::Hover(false)));
                }
                _ => {}
            }
            dispatch_native_inputs(batch);
            return;
        }
        // A held pointer move is delivered before anything that follows it.
        self.flush_native_pointer();
        if matches!(event, WinitWindowEvent::Focused(false)) {
            let inputs = self
                .platform
                .with_state(|state| state.input.focus_lost(platform_id));
            if let Some(window) = window.as_ref() {
                dispatch_native_inputs(
                    inputs
                        .into_iter()
                        .map(|input| (Arc::clone(window), NativeDispatch::Input(input)))
                        .collect(),
                );
            }
        }

        match event {
            WinitWindowEvent::CloseRequested => {
                tracing::info!(?platform_id, "Window close requested");

                // Ask the window's handler; asked behind a running method, the
                // window is kept for now and closed by command if granted.
                if window.as_ref().is_some_and(|win| {
                    let later = Arc::downgrade(win);
                    win.delivery().dispatch_close_request(move || {
                        if let Some(window) = later.upgrade() {
                            crate::owner::window_delivery::close_by_command(&*window);
                        }
                    }) == flui_platform::CloseDecision::KeepOpen
                }) {
                    return; // Vetoed, or asked later
                }

                self.complete_window_close(event_loop, platform_id, window.as_ref());
            }
            WinitWindowEvent::Resized(physical_size) => {
                tracing::debug!(?platform_id, ?physical_size, "Window resized");

                if let Some(ref win) = window {
                    win.refresh_state();
                    self.sync_window_display(event_loop, win, false);
                }
            }
            WinitWindowEvent::RedrawRequested => {
                tracing::trace!(?platform_id, "Redraw requested");

                // Dispatch per-window frame request callback
                if let Some(ref win) = window {
                    win.delivery().dispatch_request_frame();
                }
            }
            WinitWindowEvent::Focused(focused) => {
                tracing::debug!(?platform_id, ?focused, "Window focus changed");

                // Update WinitWindow focus state
                if let Some(ref win) = window {
                    win.set_focused(focused);
                    win.refresh_state();
                }

                self.platform.with_state(|state| {
                    if focused {
                        state.active_window = Some(platform_id);
                    } else if state.active_window == Some(platform_id) {
                        state.active_window = None;
                    }
                });
            }
            WinitWindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::debug!(?platform_id, ?scale_factor, "Scale factor changed");

                if let Some(ref win) = window {
                    self.sync_window_display(event_loop, win, true);
                }
                // The new metrics are published from the `Resized` that
                // follows: winit applies the OS-suggested inner size right
                // after this event ("By default, the window is resized to the
                // value suggested by the OS" — winit's
                // `WindowEvent::ScaleFactorChanged`).
            }
            WinitWindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // winit synthesizes press events for every key held when a
                // window gains focus, and release events for every key held
                // when it loses focus (X11 and Windows only — the
                // `is_synthetic` doc in winit 0.30 `src/event.rs`). Both are
                // keyboard-state synchronization, not user keystrokes, but
                // they get opposite treatment:
                //
                // - Synthetic PRESSES are dropped. Every framework key
                //   consumer — `EditableText` typing, `Shortcuts`, focus
                //   traversal — actuates on `KeyState::Down`, so dispatching
                //   one would re-type and re-trigger every held key each
                //   time the window regains focus; and no consumer needs
                //   the press for state (modifier state rides
                //   `ModifiersChanged`, which winit emits on focus change
                //   in its own right).
                // - Synthetic RELEASES are dispatched. When the physical
                //   release happens while the window is unfocused, this
                //   synthetic release is the ONLY key-up the window will
                //   ever receive — an app-level `Focus::on_key_event`
                //   handler latching a key on Down (held-key movement, a
                //   push-to-talk toggle) would otherwise stay stuck after
                //   an Alt-Tab. A stray Up is inert to every Down-actuated
                //   consumer.
                //
                // A state-sync event ought to update key state without being
                // treated as the user's direct action. FLUI's
                // `KeyboardEvent` carries no such flag, so the press half —
                // which WOULD actuate — cannot be forwarded safely and is
                // dropped instead.
                if is_synthetic && event.state == winit::event::ElementState::Pressed {
                    tracing::trace!(
                        ?event,
                        "dropping synthetic key press (focus-change state sync)"
                    );
                    return;
                }
                if let Some(ref win) = window {
                    // Raw winit modifiers state, unconverted: the translation
                    // converts it as part of assembling the whole key event.
                    let input = self
                        .platform
                        .with_state(|state| state.input.key(event, state.current_modifiers));
                    if let Some(input) = input {
                        win.delivery().dispatch_input(input);
                    }
                }
            }
            WinitWindowEvent::Ime(event) => {
                let input = self.platform.with_state(|state| state.input.ime(&event));
                if let Some(ref win) = window
                    && let Some(input) = input
                {
                    win.delivery().dispatch_input(input);
                }
            }
            WinitWindowEvent::ModifiersChanged(new_modifiers) => {
                self.platform.with_state(|state| {
                    state.current_modifiers = new_modifiers.state();
                });
            }
            WinitWindowEvent::Moved(_) => {
                if let Some(ref win) = window {
                    win.refresh_state();
                    self.sync_window_display(event_loop, win, false);
                }
            }
            WinitWindowEvent::ThemeChanged(_) => {
                if let Some(ref win) = window {
                    win.refresh_state();
                }
            }
            WinitWindowEvent::Occluded(occluded) => {
                tracing::debug!(?platform_id, ?occluded, "Window occlusion changed");

                // `occluded == true` means fully covered: the snapshot's
                // visibility is the negation. Wayland delivery rides the
                // xdg-shell v6 `suspended` state (a compositor-conditional
                // extension); on a compositor that never sends it, this arm
                // never fires and the window stays visible. X11 sends none
                // for a window the window manager minimizes, which reads as
                // hidden from its mode instead.
                if let Some(ref win) = window {
                    win.set_occluded(occluded);
                    win.refresh_state();
                }
            }
            WinitWindowEvent::HoveredFile(path) => {
                self.handle_drag_drop_path(platform_id, window.as_ref(), path, DragPath::Hovered);
            }
            WinitWindowEvent::DroppedFile(path) => {
                self.handle_drag_drop_path(platform_id, window.as_ref(), path, DragPath::Dropped);
            }
            WinitWindowEvent::HoveredFileCancelled => {
                let transfer = self
                    .platform
                    .with_state(|state| Arc::clone(&state.data_transfer));
                let event = transfer.note_hover_cancelled(platform_id);
                // Lock discipline (ADR-0038 §5): both the platform-state and
                // transfer locks are released before dispatch, mirroring the
                // CursorMoved arm.
                if let (Some(event), Some(win)) = (event, window.as_ref()) {
                    win.delivery()
                        .dispatch_input(PlatformInput::DragDrop(event));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(windows)]
        if self.deliver_preferences() {
            self.request_exit(event_loop);
            return;
        }
        self.flush_native_pointer();
        // Drop-burst boundary (ADR-0038): winit delivers one `DroppedFile`
        // per file with no end-of-burst marker, so the first `about_to_wait`
        // after at least one `DroppedFile` freezes the accumulated list as
        // the offer's payload and emits `Dropped`. A straggler after this
        // point mints a new defensive session — two complete drops, never
        // one truncated one (documented winit approximation ceiling).
        self.freeze_completed_drops();
        #[cfg(target_os = "linux")]
        self.sync_unannounced_displays(event_loop);

        // Wall-clock wake: consult the registered
        // wake-deadline hook (issue #556 — `flui-app` computes the earliest
        // instant any hosted ui_runtime's pending gesture/timer deadline needs
        // the loop to wake at, across every ui_runtime on this thread) fresh on
        // EVERY iteration, not just once — a deadline that fires, a new one
        // getting armed, or every deadline clearing must all be reflected
        // immediately, not stale from whenever the hook was installed.
        // `Some(deadline)` -> `ControlFlow::WaitUntil`; `None` (no hook
        // installed, or the hook itself has nothing pending) falls back to
        // the previously-unconditional `Wait` — re-asserted every iteration
        // so an upstream winit default change can't silently turn the
        // wake-driven frame loop into a busy poll.
        //
        // Lock discipline (ADR-0038 §5, the same rule this module's
        // `CursorMoved`/`HoveredFileCancelled` arms already follow): the
        // hook itself re-enters `flui-app`, walking every hosted ui_runtime and
        // taking gesture-arena locks — it must never run while this
        // platform's own state mutex is held, so it runs through the call
        // point.
        let wake_deadline = self.platform.wake_deadline();

        // Harness self-close (see the `self_close_deadline` field doc): fire
        // once the deadline passes and a window exists, then let its own
        // deadline participate in the wait below so an idle loop still wakes
        // to fire it (`new_events`' `ResumeTimeReached` actuation pumps the
        // loop back here).
        self.fire_self_close_if_due(event_loop);

        // An exit requested anywhere in this callback (the self-close above
        // is one) must not be followed by a blocking wait: winit's Windows
        // runner blocks on the control flow set here right after
        // `about_to_wait` returns and checks the exit flag only once the
        // wait ends. With the owner lane already shut down nothing wakes it,
        // so the loop stayed parked until an unrelated message arrived (six
        // minutes in the test harness). `Poll` makes that wait return
        // immediately, and the runner exits on the check that follows.
        if event_loop.exiting() {
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        #[cfg(windows)]
        let wake_deadline = earliest_deadline(
            wake_deadline,
            self.preference_source
                .as_ref()
                .and_then(|source| source.retry_deadline()),
        );
        let control_flow = match earliest_deadline(wake_deadline, self.self_close_deadline) {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        };
        tracing::trace!(
            wait_until_ms = control_flow_wait_ms(control_flow),
            "about to wait"
        );
        event_loop.set_control_flow(control_flow);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        // A held pointer move reaches its window before a control request
        // can close it.
        self.flush_native_pointer();
        self.process_control(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        // A close posted after the last drain but before admission closed
        // (from inside the exit-policy hook that ended the loop, say) still
        // gets its teardown — the loop is exiting, the window is not.
        self.complete_pending_closes(event_loop);
        self.finish_shutdown();
    }
}

/// Which winit file-drag arm produced a path.
#[derive(Debug, Clone, Copy)]
enum DragPath {
    /// `WindowEvent::HoveredFile` — the drag is still in flight.
    Hovered,
    /// `WindowEvent::DroppedFile` — the user released; arm the burst freeze.
    Dropped,
}

/// Whether a Linux window can move to another display without winit saying
/// so: any Wayland window (see `WinitApp::sync_unannounced_displays`), and an
/// X11 window only before its first display is known.
#[cfg(target_os = "linux")]
fn moves_unannounced(wayland: bool, awaiting_display: bool) -> bool {
    wayland || awaiting_display
}

// Helper methods for WinitApp
impl WinitApp {
    /// Look again at the display of every window winit may have moved without
    /// an event.
    ///
    /// A Wayland surface enters an output with no winit event: none for its
    /// first one, and only a `ScaleFactorChanged` for a later one whose scale
    /// differs, so a window dragged to an output of another refresh rate at
    /// the same scale would keep the old display. The loop goes idle once the
    /// compositor's `enter` has been handled, so every Wayland window is
    /// looked at there, at one monitor lookup each. An X11 window reports a
    /// move with `Moved`, and only one with no display yet is looked at.
    #[cfg(target_os = "linux")]
    fn sync_unannounced_displays(&self, event_loop: &ActiveEventLoop) {
        let wayland = self.platform.displays.is_wayland();
        let unannounced: Vec<_> = self.platform.with_state(|state| {
            state
                .windows
                .values()
                .filter(|window| moves_unannounced(wayland, window.awaiting_display()))
                .cloned()
                .collect()
        });
        for window in unannounced {
            self.sync_window_display(event_loop, &window, false);
        }
    }

    /// Publish the window's snapshot when it may be on another display.
    ///
    /// `forced` is a scale-factor change, which alters the display's snapshot
    /// entry whether or not the window moved. Otherwise the window is read
    /// again only when its display really differs from the last one reported,
    /// so a drag-resize does not resample on every `Resized`. A window on a
    /// monitor the last sample lacks (a monitor plugged in since) marks the
    /// source stale itself.
    pub(crate) fn sync_window_display(
        &self,
        event_loop: &ActiveEventLoop,
        window: &Arc<dyn LoopWindow>,
        forced: bool,
    ) {
        if forced {
            self.platform.resample_displays(event_loop);
        }
        if window.take_display_change() || forced {
            // The snapshot names the display: a move no other event reported
            // still reaches `WindowHandler::state_changed`.
            window.refresh_state();
        }
    }

    /// Start the displays' observation of what changes them, once.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn observe_displays(&mut self) {
        if self.display_watch.is_some() {
            return;
        }
        let signal = self.platform.owner_signal.lock().clone();
        if let Some(signal) = signal {
            self.display_watch = Some(Arc::clone(&self.platform.displays).observe(&signal));
        }
    }

    /// Shared body of the `HoveredFile`/`DroppedFile` arms: feed the path
    /// into the drag-session state machine and dispatch any resulting event.
    ///
    /// Lock discipline (ADR-0038 §5): platform state is only locked to clone
    /// out the transfer source and the tracked cursor position; both that
    /// lock and the source's internal lock are released before
    /// `dispatch_input` runs, mirroring the CursorMoved arm.
    fn handle_drag_drop_path(
        &self,
        platform_id: WindowId,
        window: Option<&Arc<dyn LoopWindow>>,
        path: PathBuf,
        kind: DragPath,
    ) {
        let (transfer, cursor) = self.platform.with_state(|state| {
            (
                Arc::clone(&state.data_transfer),
                state.cursor_positions.get(&platform_id).copied(),
            )
        });
        // Last tracked cursor position, if any — possibly stale on Wayland,
        // where an external drag grabs the cursor (documented limitation).
        let position = window.and_then(|win| {
            cursor.map(|cursor| logical_cursor_point(cursor, win.inner().scale_factor()))
        });
        let event = match kind {
            DragPath::Hovered => transfer.note_hovered_file(platform_id, path, position),
            DragPath::Dropped => transfer.note_dropped_file(platform_id, path, position),
        };
        if let (Some(event), Some(win)) = (event, window) {
            win.delivery()
                .dispatch_input(PlatformInput::DragDrop(event));
        }
    }

    /// Deliver the pointer move the translation holds to see whether the next
    /// event replaces it: the loop turn ended, or something else follows.
    fn flush_native_pointer(&self) {
        let batch = self.platform.with_state(|state| {
            let inputs = state.input.flush();
            native_batch(state, inputs)
        });
        dispatch_native_inputs(batch);
    }

    /// Freeze completed drop bursts and dispatch their `Dropped` events.
    /// Cheap in the steady state: one uncontended lock to observe that no
    /// session is awaiting the freeze.
    fn freeze_completed_drops(&self) {
        let transfer = self
            .platform
            .with_state(|state| Arc::clone(&state.data_transfer));
        let windows = transfer.windows_awaiting_freeze();
        if windows.is_empty() {
            return;
        }

        // Snapshot window handles and cursor positions first; the platform
        // lock is released before scale-factor reads and dispatch.
        let mut snapshots: Vec<(WindowId, Arc<dyn LoopWindow>, Option<_>)> = Vec::new();
        self.platform.with_state(|state| {
            for window_id in &windows {
                if let Some(win) = state.windows.get(window_id) {
                    snapshots.push((
                        *window_id,
                        Arc::clone(win),
                        state.cursor_positions.get(window_id).copied(),
                    ));
                }
            }
        });

        let mut positions = HashMap::new();
        let mut handles: HashMap<WindowId, Arc<dyn LoopWindow>> = HashMap::new();
        for (window_id, win, cursor) in snapshots {
            if let Some(cursor) = cursor {
                positions.insert(
                    window_id,
                    logical_cursor_point(cursor, win.inner().scale_factor()),
                );
            }
            handles.insert(window_id, win);
        }

        for (window_id, event) in transfer.freeze_completed(&positions) {
            if let Some(win) = handles.get(&window_id) {
                win.delivery()
                    .dispatch_input(PlatformInput::DragDrop(event));
            }
        }
    }

    fn process_control(&mut self, event_loop: &ActiveEventLoop) {
        let signal = self.platform.owner_signal.lock().clone();
        if signal.as_ref().is_some_and(|signal| signal.quitting()) {
            self.request_exit(event_loop);
            return;
        }
        // Begun before the reads below (host notices, the portal's
        // appearance, X11 window properties, Windows preferences), so a wake
        // that lands while they run posts the next turn instead of being
        // settled by this one's drive with its change unread.
        let turn = signal
            .as_deref()
            .map(crate::owner::signal::OwnerSignal::begin_turn);
        if self.control.take_quit_requested() {
            self.complete_pending_closes(event_loop);
            self.request_exit(event_loop);
            return;
        }

        let drain_budget = self.control.begin_drain();
        for _ in 0..drain_budget {
            // A sleep or shutdown holds logind only for a few seconds, and a
            // queue of window opens can take longer than that: the notices
            // are heard between the opens, not after them all.
            #[cfg(target_os = "linux")]
            self.deliver_host_notices();
            if self.control.take_quit_requested() {
                self.complete_pending_closes(event_loop);
                self.request_exit(event_loop);
                return;
            }

            let Some(command) = self.control.try_recv() else {
                break;
            };
            match command {
                ControlCommand::OpenWindow { request, reply } => {
                    // Claim-slot protocol (ADR-0039 §3): a request already
                    // abandoned before the owner ever looked at it skips
                    // creation entirely — never build a window nobody
                    // wants.
                    if reply.is_abandoned() {
                        tracing::debug!("window requester abandoned before creation; skipping");
                        continue;
                    }

                    let guard = OpenWindowReplyGuard::new(reply);
                    tracing::debug!("Processing window creation request");
                    let opened = request.build_on_owner().and_then(|(options, handler)| {
                        self.platform
                            .create_window_now(event_loop, options, handler)
                    });
                    match opened {
                        Ok(window_id) => {
                            let window = self
                                .platform
                                .window_by_id(window_id)
                                .expect("BUG: just-created window must be registered");
                            match guard.complete(Ok(window)) {
                                Some(reply) => self.in_flight_replies.push((window_id, reply)),
                                None => {
                                    // The requester abandoned in the race
                                    // between the `is_abandoned` check above
                                    // and this delivery — unwind instead of
                                    // leaking (ADR-0039 §3/§4).
                                    self.unwind_orphan_window(window_id);
                                }
                            }
                        }
                        Err(backend_error) => {
                            let _ = guard.complete(Err(backend_error));
                        }
                    }
                }
            }
        }

        // An accepted reading is owed to this turn's drive, not a wake: a
        // wake now would post a turn of its own with nothing left to read.
        #[cfg(windows)]
        if self.refresh_preferences()
            && let Some(turn) = &turn
        {
            turn.owe();
        }
        #[cfg(target_os = "linux")]
        self.deliver_portal_appearance();
        #[cfg(target_os = "linux")]
        self.deliver_host_notices();
        #[cfg(target_os = "linux")]
        self.platform.refresh_unreported();
        #[cfg(test)]
        self.platform.run_turn_read_hook_for_test();
        if let Some(turn) = turn
            && turn.drive()
        {
            self.request_exit(event_loop);
            return;
        }
        self.complete_pending_closes(event_loop);

        // Sweep entries whose requester claimed delivery and then dropped
        // its handle without ever reading it (late abandonment) — reclaim
        // and unwind those, and drop fully-settled entries (claimed, or
        // abandoned-and-reclaimed) from the registry. Runs at this
        // `user_event` drain anchor every turn, per the claim-slot module
        // docs' sweep contract.
        self.sweep_settled_replies();

        if self.control.take_quit_requested() {
            self.request_exit(event_loop);
            return;
        }

        // Exit-policy re-evaluation (issue #558): a keep-alive application
        // service completed on a worker thread AFTER the last window's
        // close was vetoed — with no window left to produce events,
        // this coalesced request is the only path that ever re-asks the
        // hook, so a stale veto cannot hold the process open forever. Same
        // gate shape as the `CloseRequested` arm: only when this backend's
        // own window map is empty, hook leased and consulted OUTSIDE the
        // state lock (ADR-0039). Checked AFTER the drain above so a window
        // opened by a command in this same wake (a service asking for a
        // notification window while another service completes) is already
        // in the map and vetoes the re-check by count alone.
        if self.control.take_exit_reevaluation_requested() {
            let windows_empty = self.platform.with_state(|state| state.windows.is_empty());
            if windows_empty && self.platform.exit_policy_allows_exit() {
                self.request_exit(event_loop);
            }
        }
    }

    /// Reclaims and unwinds any `in_flight_replies` entry whose requester
    /// claimed delivery, then dropped its `PendingWindow` without reading
    /// it — and drops fully-settled entries (claimed, or already
    /// reclaimed) from the registry.
    fn sweep_settled_replies(&mut self) {
        // Take ownership of the whole registry first: `unwind_orphan_window`
        // needs `&self`, which would otherwise conflict with an active
        // `self.in_flight_replies.drain(..)` borrow for the loop's duration.
        let drained = std::mem::take(&mut self.in_flight_replies);
        let mut still_pending = Vec::with_capacity(drained.len());
        for (window_id, reply) in drained {
            if reply.take_abandoned().is_some() {
                self.unwind_orphan_window(window_id);
            }
            if !reply.is_settled() {
                still_pending.push((window_id, reply));
            }
        }
        self.in_flight_replies = still_pending;
    }

    /// Unwinds a window the requester never claimed: hides it (a visible
    /// flicker instead of a silent leak — the ADR's own framing), removes
    /// it from platform state, and retires any in-flight drag session
    /// addressed at it. No per-window user callbacks fire: the window was
    /// never delivered to any caller, so nothing outside this module ever
    /// registered one.
    fn unwind_orphan_window(&self, window_id: WindowId) {
        tracing::warn!(
            ?window_id,
            "orphaned window request (requester abandoned); unwinding"
        );
        // The drag session is retired before the window leaves `window_id_map`:
        // the map entry is what observers read as "the window is gone", so
        // removing it last keeps that observation from racing the cleanup.
        // `forget_window` wakes parked deliveries, so it runs outside the
        // state lock.
        let data_transfer = self
            .platform
            .with_state(|state| Arc::clone(&state.data_transfer));
        data_transfer.forget_window(window_id);
        let window = self.platform.with_state(|state| {
            let window = state.windows.remove(&window_id);
            state.cursor_positions.remove(&window_id);
            state.input.focus_lost(window_id);
            state.window_id_map.retain(|_, v| *v != window_id);
            window
        });
        if let Some(window) = window {
            // Directly, not through `WindowCommand::Close`: that route posts
            // a close request for the owner's next turn, and this window is
            // already out of the map it would look in.
            window.inner().set_visible(false);
        }
    }

    /// Fires the harness self-close (see [`WinitApp::self_close_deadline`])
    /// when its deadline has passed and at least one window is tracked. On
    /// the default [`SelfCloseRoute::Compositor`] it synthesizes
    /// `WindowEvent::CloseRequested` through the ordinary
    /// [`ApplicationHandler::window_event`] entry so the whole close arm —
    /// should-close veto, per-window close callbacks, global handler, map
    /// removal, exit policy — runs exactly as a compositor-delivered close
    /// would; on [`SelfCloseRoute::Programmatic`] it calls
    /// [`WindowCommand::Close`](flui_platform::WindowCommand::Close) on
    /// that window instead, exactly as an application would. One-shot: the deadline is cleared on fire. If no window exists
    /// yet at the deadline, it stays armed until one does — re-paced a
    /// short interval forward on each check, never left in the past, so the
    /// loop keeps waiting instead of busy-waking on an expired
    /// `ControlFlow::WaitUntil`.
    fn fire_self_close_if_due(&mut self, event_loop: &ActiveEventLoop) {
        let due = self
            .self_close_deadline
            .is_some_and(|deadline| Instant::now() >= deadline);
        if !due {
            return;
        }
        let winit_id = self
            .platform
            .with_state(|state| state.window_id_map.keys().next().copied());
        let Some(winit_id) = winit_id else {
            // Due with no window tracked yet: re-pace instead of leaving the
            // past deadline armed. `about_to_wait` folds this deadline into
            // `ControlFlow::WaitUntil`, and an instant already behind
            // `Instant::now()` would re-fire winit's `ResumeTimeReached`
            // every iteration — a busy wake loop until a window appears.
            // FLUI's own bootstrap opens its window inside `on_ready`,
            // before the first `about_to_wait`, so this arm exists for an
            // embedder that opens its first window later (e.g. through the
            // deferred owner lane), which `Platform::run` permits.
            self.self_close_deadline = Some(Instant::now() + Duration::from_millis(50));
            return;
        };
        self.self_close_deadline = None;
        match self.self_close_route {
            SelfCloseRoute::Compositor => {
                tracing::info!(
                    route = "compositor",
                    "harness self-close deadline reached; synthesizing CloseRequested"
                );
                self.window_event(event_loop, winit_id, WinitWindowEvent::CloseRequested);
            }
            SelfCloseRoute::Programmatic => {
                // Through the public trait method, exactly as an application
                // would: it posts the close on the owner lane and wakes this
                // loop, whose next `user_event` drain runs the teardown.
                let window = self.platform.with_state(|state| {
                    state
                        .get_platform_window_id(winit_id)
                        .and_then(|id| state.windows.get(&id).cloned())
                });
                let Some(window) = window else {
                    return;
                };
                tracing::info!(
                    route = "programmatic",
                    "harness self-close deadline reached; applying WindowCommand::Close"
                );
                // A winit window always takes a close; the one failure left
                // is a window that closed since the lookup, which is the goal.
                if let Err(refusal) = flui_platform::WindowCommand::Close.admit_and_apply(&*window)
                {
                    tracing::debug!(?refusal, "the harness close was not carried out");
                }
            }
        }
    }

    /// The teardown every closing window takes once the decision to close
    /// is final — the ONE body behind both routes, because issue #919 was
    /// exactly the two routes being written separately: the compositor's
    /// `CloseRequested` arm (after its should-close veto) did all of this,
    /// while the programmatic
    /// [`WindowCommand::Close`](flui_platform::WindowCommand::Close) hid
    /// the window and fired its callback but never left the tracking map, so the exit
    /// policy — consulted only against that map — never saw the last
    /// window go, and the process lingered with nothing on screen.
    ///
    /// Always an owner-turn body, never run synchronously inside `close()`,
    /// for a reason stronger than "ending the loop needs the live
    /// `ActiveEventLoop`": a `close()` issued from inside one of this
    /// window's own callbacks runs while that callback is out of its slot
    /// for its call, and the call point restores a callback into a
    /// slot it finds empty — so a synchronous `callbacks().clear()` would be
    /// undone the moment the callback returned, leaving the renderer-owning
    /// frame callback alive in a window that is out of the map (issue #713's
    /// class from the other direction). Deferring to the next owner turn is
    /// what makes step 4 stick.
    ///
    /// Order, load-bearing throughout:
    /// 1. hide the native window where the backend can (winit's
    ///    `set_visible` is a no-op on Wayland, Android and Web — there the
    ///    window disappears when its last handle drops) and report it not
    ///    visible; the visible effect should not wait for handle drops an
    ///    embedder may defer;
    /// 2. the per-window `WindowHandler::closed` callback (an embedder tears down what it
    ///    hung on this window — `flui-app` closes the window's presentation
    ///    here, BEFORE the exit policy below reads its UI runtime registry);
    /// 3. removal from the window/cursor maps (and from `active_window`,
    ///    which otherwise keeps naming a window that no longer exists), and
    ///    retirement of any drag session addressed at this window (ADR-0038
    ///    offer lifecycle);
    /// 4. the callback clear — NOW, while the native window (`window` is
    ///    still live) and the event loop are both alive: the frame callback
    ///    owns the embedder's GPU renderer, whose `wgpu::Surface` must die
    ///    strictly before the winit window's Wayland objects (a swapchain
    ///    destroyed after its `wl_surface` marshals on a freed `wl_proxy`
    ///    and segfaults post-quit — issue #713). `WinitWindow::drop` repeats
    ///    the clear as a last resort for refs that die elsewhere;
    /// 5. the exit-policy consult, only when this backend's own map is now
    ///    empty. That map is not the only ledger: an embedder hosting more
    ///    than one top-level window (issue #555's `WindowPolicy`) tracks
    ///    ui_runtimes/presentations this layer cannot see, and a queued "open
    ///    another window" request (a splash's close handler asking for the
    ///    main window) must not be raced by an unconditional exit — so the
    ///    hook, when installed, gets the final word; unset, this is the
    ///    exact pre-#555 unconditional behavior.
    ///
    /// Lock discipline (ADR-0039): the exit-policy hook is lease-taken and
    /// invoked OUTSIDE the platform state lock. The
    /// hook's own body drops removed UI runtime state, whose destructors may call
    /// back into this platform (a dispose hook opening another window);
    /// consulting it under this non-reentrant lock would deadlock the
    /// instant such a callback re-entered `with_state`.
    fn complete_window_close(
        &mut self,
        event_loop: &ActiveEventLoop,
        platform_id: WindowId,
        window: Option<&Arc<dyn LoopWindow>>,
    ) {
        if let Some(win) = window {
            win.mark_closed();
            win.inner().set_visible(false);
            win.set_visible(false);
            win.delivery().dispatch_close();
        }

        let (windows_empty, transfer) = self.platform.with_state(|state| {
            state.window_id_map.retain(|_, v| *v != platform_id);
            state.windows.remove(&platform_id);
            state.cursor_positions.remove(&platform_id);
            state.input.focus_lost(platform_id);
            if state.active_window == Some(platform_id) {
                state.active_window = None;
            }

            (state.windows.is_empty(), Arc::clone(&state.data_transfer))
        });
        transfer.forget_window(platform_id);

        if let Some(win) = window {
            win.delivery().clear();
        }

        let should_exit = windows_empty && self.platform.exit_policy_allows_exit();
        if should_exit {
            self.request_exit(event_loop);
        }
    }

    /// Runs the close teardown for every window whose programmatic
    /// `close()` is waiting on the owner (issue #919). Called at the
    /// `user_event` drain anchor AFTER the command drain, so a window opened
    /// by a command in the same wake (a splash's close handler asking for
    /// the main window) is already in the map and vetoes exit by count
    /// alone; and called BEFORE any quit exits the loop — a close posted
    /// before the quit is a decision already made whose `WindowHandler::closed` the
    /// embedder is owed, and a quit that overtook it would strand that
    /// window hidden-but-tracked, the exact shape of the original defect. A
    /// window the map no longer holds (closed by the compositor in between,
    /// or an orphan already unwound) is skipped: its teardown already ran.
    fn complete_pending_closes(&mut self, event_loop: &ActiveEventLoop) {
        for window_id in self.control.take_close_requests() {
            let window = self
                .platform
                .with_state(|state| state.windows.get(&window_id).cloned());
            let Some(window) = window else {
                tracing::debug!(
                    ?window_id,
                    "programmatic close for a window no longer tracked"
                );
                continue;
            };
            // `route` is a harness's oracle for this path:
            // matched on the field, not the message.
            tracing::info!(
                ?window_id,
                route = "programmatic",
                "Window closed programmatically"
            );
            self.complete_window_close(event_loop, window_id, Some(&window));
        }
    }

    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
        tracing::info!("Quitting event loop");
        event_loop.exit();
        self.finish_shutdown();
    }

    fn finish_shutdown(&mut self) {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        let preference_source = self.preference_source.take();
        #[cfg(target_os = "macos")]
        let preference_observers = self.preference_observers.take();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let display_watch = self.display_watch.take();
        #[cfg(target_os = "linux")]
        let host_notices = self.host_notices.take();
        let signal = self.platform.owner_signal.lock().clone();
        if let Some(signal) = signal {
            signal.close();
        }
        // Admission is closed, so a change a display source still reports
        // wakes nobody; now its observation can end.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        drop(display_watch);
        #[cfg(target_os = "macos")]
        drop(preference_observers);
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        drop(preference_source);
        // After the source, which closes its admission as it drops: a broadcast
        // the receiver still hears while it is destroyed wakes nobody.
        #[cfg(windows)]
        drop(self.broadcasts.take());
        #[cfg(target_os = "linux")]
        drop(host_notices);
        self.release_open_window_callbacks();
        self.close_owner_lane();
        self.notify_quit_once();
    }

    /// Clears every still-tracked window's callback slots on the way out.
    ///
    /// `complete_pending_closes` only tears down windows whose `close()` was
    /// actually requested — a window left open when the loop is asked to
    /// quit (`owner.quit()`, or `on_ready` failing after step 6) never runs
    /// [`Self::complete_window_close`] at all, so its `WindowHandler::frame_requested`
    /// callback — where production code
    /// (`install_pre_present_hook`, `crates/flui-app/src/app/runner/frame_pacing.rs`)
    /// stores its own `Arc::clone` of the window inside the renderer — was
    /// never released: window -> handler -> renderer -> `Arc<window>` is a
    /// strong cycle, and only letting go of the handler breaks it (see
    /// [`crate::owner::window_delivery::WindowDelivery::clear`]'s own doc).
    /// This does not run the rest of `complete_window_close`'s teardown
    /// (visibility, `WindowHandler::closed`, the global `Closed` event, the exit-policy
    /// re-consult) — that body takes a live `&ActiveEventLoop`, and
    /// [`Self::finish_shutdown`] (this method's caller) also runs once more
    /// from `run_event_loop`, after `event_loop.run_app` has returned and
    /// consumed the `EventLoop` itself, by which point no `ActiveEventLoop`
    /// can exist to pass it. Clearing callbacks is the minimum that breaks
    /// the cycle and is safe to run unconditionally: `clear()` is
    /// idempotent-terminal, so a window already closed through the normal
    /// path (its slots already empty) is a no-op here.
    fn release_open_window_callbacks(&mut self) {
        let windows = self
            .platform
            .with_state(|state| state.windows.values().cloned().collect::<Vec<_>>());
        for window in windows {
            window.mark_closed();
            window.delivery().clear();
        }
    }

    fn close_owner_lane(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let signal = self.platform.owner_signal.lock().clone();
            if let Some(signal) = signal {
                signal.fence();
            }
            // Also runs during unwind: native observation ends before the
            // owner releases its source, without retiring user callback captures.
            #[cfg(target_os = "macos")]
            drop(self.preference_observers.take());
            let source = self.preference_source.take();
            drop(source);
            #[cfg(windows)]
            drop(self.broadcasts.take());
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            drop(self.display_watch.take());
        }
        self.control.stop_accepting();
        self.reject_pending_commands();
        self.platform.mark_stopped();
    }

    fn reject_pending_commands(&self) {
        // Admission is closed and the lane is bounded to CONTROL_CAPACITY, so
        // draining to empty is finite and includes every accepted command.
        while let Some(command) = self.control.try_recv() {
            match command {
                ControlCommand::OpenWindow { reply, .. } => {
                    let _ = OpenWindowReplyGuard::new(reply)
                        .complete(Err(OpenWindowError::OwnerGone { rejected: None }));
                }
            }
        }
    }

    fn notify_quit_once(&mut self) {
        if self.quit_notified {
            return;
        }
        self.quit_notified = true;

        self.platform.app.quit_requested().or_resume(());
    }
}

impl Drop for WinitApp {
    fn drop(&mut self) {
        // Drop may run while user code is unwinding. Close only framework
        // resources here; user callbacks run exclusively from explicit finish.
        self.close_owner_lane();
    }
}

impl host::Loop for WinitLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        tracing::info!("Starting winit event loop via Platform::run()");
        // Generic wording, not "Winit event loop error": the propagated
        // error may be a stashed `on_ready` bootstrap failure (window
        // creation, GPU init, root-widget attach), a genuine winit-level
        // error from `event_loop.run_app`, or both combined -- see
        // `run_event_loop`'s `combine_shutdown_result`. This is the sole
        // `error`-level log for whichever of those it turns out to be.
        self.run_event_loop(platform, handler).inspect_err(|error| {
            tracing::error!("winit Platform::run exited with an error: {:?}", error);
        })
    }

    fn quit(&self) {
        let signal = self.owner_signal.lock().clone();
        if let Some(signal) = signal {
            signal.fence();
        }

        tracing::info!("Quit requested");

        let control = self.with_state(|state| match &mut state.run_state {
            WinitRunState::New { quit_requested } => {
                *quit_requested = true;
                None
            }
            WinitRunState::Starting { control, .. } | WinitRunState::Running { control, .. } => {
                Some(control.clone())
            }
            WinitRunState::Stopped => None,
        });
        if let Some(control) = control {
            control.request_quit();
        }
    }

    fn request_exit_policy_reevaluation(&self) {
        // Same run-state walk as `quit` above, minus the `New` arm: before
        // the loop starts there is no hook installed and no service
        // running, so a pre-run request has nothing to re-evaluate and is
        // deliberately dropped rather than parked.
        if let Some(control) = self.control_sender() {
            control.request_exit_reevaluation();
        }
    }
}

impl host::Services for WinitLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        std::env::current_exe().map_err(|error| PlatformError::AppPath {
            message: error.to_string(),
        })
    }

    fn reveal_path(&self, path: &Path) {
        // The extension, not the path. This is a document the user picked, so
        // its name is their data; what a trace needs is that the call happened
        // and roughly on what.
        tracing::info!(
            extension = ?path.extension(),
            "Revealing path"
        );

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg("/select,")
                .arg(path)
                .spawn();
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn();
        }

        #[cfg(target_os = "linux")]
        {
            // Try xdg-open with parent directory
            if let Some(parent) = path.parent() {
                let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
            }
        }
    }

    fn open_path(&self, path: &Path) {
        // See `reveal_path`: user-chosen document, extension only.
        tracing::info!(
            extension = ?path.extension(),
            "Opening path"
        );

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .arg("/C")
                .arg("start")
                .arg(path)
                .spawn();
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(path).spawn();
        }

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        }
    }

    // The winit host opens no URL, shows no file prompt and reports no
    // compositor: those services move to the system capability crate.
    fn open_url(&self, url: &str) {
        let _ = url;
    }

    fn prompt_for_paths(
        &self,
        options: crate::PathPromptOptions,
    ) -> crate::Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        let _ = options;
        crate::Task::ready(Ok(None))
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> crate::Task<Result<Option<PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        crate::Task::ready(Ok(None))
    }

    fn compositor_name(&self) -> &'static str {
        ""
    }
}

impl<H: Host<Loop = WinitLoop>> HostPlatform<H> {
    /// Create and run the event loop
    ///
    /// This is the main entry point for the platform. It creates the event
    /// loop, initializes the platform, and runs until quit is requested.
    ///
    /// # Errors
    /// Same contract as [`Platform::run`]: [`PlatformError::Bootstrap`] for
    /// an `on_ready` failure, [`PlatformError::EventLoop`] for a loop-level
    /// failure (including a second `run` on the same platform).
    pub fn run_event_loop(
        self: Arc<Self>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        self.shared_owner().run_event_loop(self, handler)
    }
}

/// [`OwnerHooks`] for the winit backend (ADR-0039 §1). Inside `on_ready`,
/// `ACTIVE_EVENT_LOOP` is published, so creation is direct and synchronous;
/// afterwards, this defers through the owner lane instead of blocking or
/// re-entering backend state — the load-bearing design choice that lets a
/// call from arbitrary owner-thread callback context (a handler invoked by
/// [`WinitApp::window_event`], now hoisted outside the state lock) neither
/// deadlock nor mutate the platform window map mid-frame.
struct WinitOwnerHooks {
    platform: Arc<WinitLoop>,
    owner_thread: ThreadId,
}

impl OwnerHooks for WinitOwnerHooks {
    fn on_wake(
        &self,
        callback: Box<dyn FnMut() + Send>,
    ) -> Result<(), crate::WakeRegistrationError> {
        let signal = self
            .platform
            .owner_signal
            .lock()
            .clone()
            .ok_or(crate::WakeRegistrationError::OwnerGone)?;
        signal.register(callback)
    }

    fn open_owner_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<WindowOpen, OpenWindowError> {
        if self
            .platform
            .owner_signal
            .lock()
            .as_ref()
            .is_some_and(|signal| !signal.accepting())
        {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }

        if let Some(event_loop_ptr) = ACTIVE_EVENT_LOOP.with(Cell::get) {
            // SAFETY: identical justification to `Platform::open_window`'s
            // same-thread fast path (the winit window position's `open`) — this
            // call is reached only from inside the same nested `on_ready`
            // invocation that publishes `ACTIVE_EVENT_LOOP`.
            let event_loop = unsafe { event_loop_ptr.as_ref() };
            let window_id = self
                .platform
                .create_window_now(event_loop, options, handler)?;
            let window = self.platform.window_by_id(window_id)?;
            return Ok(WindowOpen::Ready(window));
        }

        // Post-bootstrap: defer through the owner lane instead of creating
        // directly (ADR-0039 §1) — a deferred `Pending` resolves at the
        // next `user_event` drain anchor rather than risking re-entrance
        // into backend state from arbitrary callback context.
        let control = self.platform.with_state(|state| match &state.run_state {
            WinitRunState::Running { control, .. } => Some(control.clone()),
            _ => None,
        });
        let Some(control) = control else {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        };
        // The lane crosses threads; the handler does not. It waits in an
        // owner-thread slot, and the request carries what takes it back out
        // where the lane is drained: on this same owner thread.
        let kept = crate::owner::slot::OwnerSlot::new();
        if let Err(handler) = kept.replace(handler) {
            crate::owner::call_point::retire(handler);
            return Err(OpenWindowError::Unavailable {
                message: "the owner thread cannot keep another window handler".to_owned(),
            });
        }
        let request = crate::WindowRequest::new(options, move || {
            kept.take()
                .expect("BUG: the owner lane is drained on the owner thread that queued it")
        });
        match control.request_open_window(request) {
            Ok(handle) => Ok(WindowOpen::Pending(PendingWindow::new(
                handle,
                self.owner_thread,
            ))),
            Err(ControlSendError::Full { capacity, rejected }) => Err(OpenWindowError::LaneFull {
                capacity,
                rejected: Box::new(rejected.options),
            }),
            Err(ControlSendError::OwnerGone { rejected }) => Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(rejected.options)),
            }),
        }
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        Arc::new(WinitProxyTransport {
            signal: self
                .platform
                .owner_signal
                .lock()
                .as_ref()
                .map(Arc::downgrade)
                .unwrap_or_default(),
            platform: Arc::clone(&self.platform),
            owner_thread: self.owner_thread,
        })
    }
}

/// [`ProxyTransport`] for the winit backend: worker threads reach the owner
/// lane exactly as the pre-ADR-0039 cross-thread `Platform::open_window`
/// path already did — enqueue-and-wake, claim-slot reply.
struct WinitProxyTransport {
    signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
    platform: Arc<WinitLoop>,
    owner_thread: ThreadId,
}

impl WinitProxyTransport {
    fn control(&self) -> Option<ControlSender> {
        self.platform.with_state(|state| match &state.run_state {
            WinitRunState::Starting { control, .. } | WinitRunState::Running { control, .. } => {
                Some(control.clone())
            }
            WinitRunState::New { .. } | WinitRunState::Stopped => None,
        })
    }
}

impl ProxyTransport for WinitProxyTransport {
    fn wake(&self) -> Result<(), ProxySendError<()>> {
        let signal = self
            .signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?;
        signal.wake()
    }

    fn open_window(
        &self,
        request: crate::WindowRequest,
    ) -> Result<PendingWindow, ProxySendError<crate::WindowRequest>> {
        if self
            .signal
            .upgrade()
            .is_none_or(|signal| !signal.accepting())
        {
            return Err(ProxySendError::OwnerGone { rejected: request });
        }
        let Some(control) = self.control() else {
            return Err(ProxySendError::OwnerGone { rejected: request });
        };
        control
            .request_open_window(request)
            .map(|handle| PendingWindow::new(handle, self.owner_thread))
            .map_err(|error| match error {
                ControlSendError::Full { capacity, rejected } => ProxySendError::Full {
                    capacity,
                    rejected: *rejected,
                },
                ControlSendError::OwnerGone { rejected } => ProxySendError::OwnerGone {
                    rejected: *rejected,
                },
            })
    }

    fn request_quit(&self) -> Result<(), ProxySendError<()>> {
        let signal = self
            .signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?;
        signal.request_quit()
    }

    fn owner_thread(&self) -> ThreadId {
        self.owner_thread
    }
}

// The loop's test on a real winit event loop, which runs `event_loop.run_app`
// on an ordinary test thread with no window; the real-loop tests that open
// windows are the window feature's. A sibling module rather than a nested one,
// so its test sits beside `tests` rather than two segments below it.
#[cfg(test)]
mod real_loop_tests;
// The real loop the tests here and the window's tests run on, and the quiet
// host and table they run over.
#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::testing::quiet_platform;
    use super::{SelfCloseRoute, WinitApp, WinitRunState};
    use crate::owner::winit::control::control_lane;
    use flui_platform::WindowOptions;

    /// The application's exit policy is asked with the state lock released,
    /// its answer decides, and its panic reaches the loop.
    #[test]
    fn winit_exit_policy_runs_outside_the_state_lock() {
        use std::cell::Cell;
        use std::rc::Rc;

        use crate::owner::app_delivery::testing::{AppCall, RecordingApp, bind};

        let host = Arc::new(quiet_platform());
        let platform = host.shared_owner();
        let app = RecordingApp::new();
        let _run = bind(Arc::clone(&platform.app), app.clone());
        let unlocked = Rc::new(Cell::new(false));
        let (seen, probe) = (Rc::clone(&unlocked), Arc::clone(&platform));
        app.react(move |call, _| {
            if call == AppCall::ExitPolicy {
                seen.set(probe.state.try_lock().is_some());
            }
        });
        assert!(platform.exit_policy_allows_exit());
        assert!(unlocked.get(), "the handler was asked under the state lock");
        app.answer_exit(false);
        assert!(!platform.exit_policy_allows_exit(), "the handler decides");

        app.react(|call, _| {
            assert!(call != AppCall::ExitPolicy, "first failure");
        });
        let failure = catch_unwind(AssertUnwindSafe(|| platform.exit_policy_allows_exit()))
            .expect_err("the handler's panic reaches the loop");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first failure")
        );
    }

    // ========================================================================
    // combine_shutdown_result
    // ========================================================================
    //
    // Fast, deterministic, no event loop needed -- these pin the exact
    // combining logic `run_event_loop` calls, independent of whether a real
    // winit loop can also be driven to produce both error shapes at once
    // (which it cannot, deterministically, in a test).

    #[test]
    fn winit_panicking_quit_callback_runs_after_idempotent_owner_close() {
        let host = Arc::new(quiet_platform());
        let platform = host.shared_owner();
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        platform.with_state(|state| {
            state.run_state = WinitRunState::Running {
                owner_thread: std::thread::current().id(),
                control: sender.clone(),
            };
        });
        let mut replies: Vec<_> = (0..3)
            .map(|index| {
                sender
                    .request_open_window(crate::WindowRequest::new(
                        WindowOptions::default().with_title(format!("quit-panic-{index}")),
                        || -> std::rc::Rc<dyn flui_platform::WindowHandler> {
                            flui_testing::window_handler::RecordingWindowHandler::new()
                        },
                    ))
                    .expect("request is admitted before quit")
            })
            .collect();
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_for_handler = Arc::clone(&callback_count);
        let platform_for_handler = Arc::clone(&platform);
        let app = crate::owner::app_delivery::testing::RecordingApp::new();
        let _run =
            crate::owner::app_delivery::testing::bind(Arc::clone(&platform.app), app.clone());
        app.react(move |call, _| {
            if call != crate::owner::app_delivery::testing::AppCall::Quit {
                return;
            }
            callback_count_for_handler.fetch_add(1, Ordering::Relaxed);
            assert!(
                platform_for_handler
                    .with_state(|state| matches!(&state.run_state, WinitRunState::Stopped))
            );
            panic!("exercise panicking quit callback cleanup");
        });
        let mut app = WinitApp {
            platform: Arc::clone(&platform),
            owner_platform: host,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            preference_source: None,
            #[cfg(windows)]
            broadcasts: None,
            #[cfg(target_os = "macos")]
            preference_observers: None,
            #[cfg(target_os = "linux")]
            host_notices: None,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            display_watch: None,
            handler: None,
            owner_hooks: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        };

        let first_finish = catch_unwind(AssertUnwindSafe(|| app.finish_shutdown()));
        assert!(first_finish.is_err(), "user callback panic must propagate");
        for reply in &mut replies {
            assert!(
                reply
                    .try_take()
                    .expect("owner closes queued request before invoking user code")
                    .is_err()
            );
        }
        assert!(platform.with_state(|state| matches!(&state.run_state, WinitRunState::Stopped)));
        assert_eq!(callback_count.load(Ordering::Relaxed), 1);

        let second_finish = catch_unwind(AssertUnwindSafe(|| app.finish_shutdown()));
        assert!(second_finish.is_ok(), "owner close is idempotent");
        assert_eq!(callback_count.load(Ordering::Relaxed), 1);
    }

    /// A preference source that cannot open costs the preferences, not the
    /// displays: the receiver stands with the displays' subscriber alone, a
    /// display change reaches them, and the failure is still reported.
    #[cfg(windows)]
    #[test]
    fn a_display_change_reaches_the_displays_when_the_preferences_cannot_open() {
        use std::rc::Rc;

        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE;

        use super::HostDisplays;
        use super::testing::{QuietHost, quiet_platform_over};
        use crate::owner::notices::StaleOnWake;
        use crate::owner::signal::OwnerSignal;
        use crate::owner::windows::broadcast::Subscriber;

        /// Displays whose part of the system's broadcasts wakes the owner on
        /// a display change, as the Win32 source's does.
        #[derive(Default)]
        struct ChangingDisplays;

        impl StaleOnWake for ChangingDisplays {
            fn mark_stale(&self) {}
        }

        impl HostDisplays for ChangingDisplays {
            fn snapshot(&self) -> Result<flui_platform::DisplaySnapshot, crate::PlatformError> {
                Ok(flui_platform::DisplaySnapshot::empty())
            }

            fn id_of_window(&self, _: &winit::window::Window) -> Option<flui_platform::DisplayId> {
                None
            }

            fn resample(
                &self,
                _: &winit::event_loop::ActiveEventLoop,
                _: Option<&Arc<OwnerSignal>>,
            ) -> bool {
                false
            }

            fn broadcasts(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Rc<dyn Subscriber> {
                Rc::new(DisplayChanges(Arc::clone(signal)))
            }
        }

        /// The displays' subscriber: a display change wakes the owner.
        struct DisplayChanges(Arc<OwnerSignal>);

        impl Subscriber for DisplayChanges {
            fn message(&self, _: HWND, message: u32, _: WPARAM, _: LPARAM) -> Option<LRESULT> {
                if message == WM_DISPLAYCHANGE {
                    let _ = self.0.wake();
                }
                None
            }
        }

        let host = Arc::new(quiet_platform_over::<QuietHost<ChangingDisplays>>());
        let platform = host.shared_owner();
        let wakes = Arc::new(AtomicUsize::new(0));
        let signal = crate::owner::signal::OwnerSignal::new({
            let wakes = Arc::clone(&wakes);
            Arc::new(move || {
                wakes.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        signal.start().expect("the owner signal starts");
        *platform.owner_signal.lock() = Some(Arc::clone(&signal));
        let (_sender, receiver) = control_lane(Arc::new(|| {}));
        let mut app = WinitApp {
            platform: Arc::clone(&platform),
            owner_platform: host,
            preference_source: None,
            broadcasts: None,
            handler: None,
            owner_hooks: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        };

        let opened = app.initialize_preferences_with(|_| {
            Err(crate::PlatformError::Preferences {
                message: "the preference source could not open".into(),
            })
        });
        assert!(opened.is_err(), "the preference failure was not reported");
        assert!(app.preference_source.is_none());
        let broadcasts = app
            .broadcasts
            .as_ref()
            .expect("the displays' receiver stands without the preferences");
        broadcasts.send_with_for_test(WM_DISPLAYCHANGE, 32, 0);
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            1,
            "the display change did not reach the displays"
        );
    }

    /// A Wayland window dragged to an output of the same scale raises no winit
    /// event, so it is looked at again whether or not it has a display; an X11
    /// window reports moves and is looked at only until it has one.
    #[cfg(target_os = "linux")]
    #[test]
    fn idle_display_check_covers_every_wayland_window() {
        for (wayland, awaiting, looked_at) in [
            (true, true, true),
            (true, false, true),
            (false, true, true),
            (false, false, false),
        ] {
            assert_eq!(
                super::moves_unannounced(wayland, awaiting),
                looked_at,
                "wayland {wayland}, awaiting a display {awaiting}"
            );
        }
    }
}
