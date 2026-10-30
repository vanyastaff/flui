//! A real winit loop for tests: the event loop a test thread may build, the
//! owner signal and the control lane a hand-built [`WinitApp`] needs
//! ([`install_test_owner_signal`], [`install_control_lane`]), the application
//! a test runs the loop with ([`WinitApp::for_test`]), what a test reads of
//! the loop's state, and the waits for the loop to run and to track its
//! windows.
//!
//! The loop's own tests run over a host whose loop hooks report nothing
//! ([`QuietHost`]) and a platform over it whose table opens no window
//! ([`QuietTable`], [`quiet_platform`]), so they name no feature. The tests of
//! the windows the loop drives open a window through the loop's control lane
//! and run a scenario against it on a worker thread while the loop runs on the
//! test's own (`run_window_scenario`).

use std::{
    collections::HashMap,
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use flui_foundation::DataTransferId;
use flui_foundation::geometry::{Point, Size};
use flui_platform::data_transfer::{
    DropFeedback, RepresentationIndex, TransferLimits, TransferRequest,
};
use flui_platform::input_capabilities::{InputCapabilities, PointerKindSet};
use flui_platform::{
    DataTransferOffer, DataTransferSource, DisplayId, DisplaySnapshot, DragDropEvent,
    NullDataTransferSource, PlatformInput, SystemPreferences, WindowCapabilities, WindowId,
    WindowOptions,
};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId as WinitWindowId;

#[cfg(target_os = "linux")]
pub(crate) use window_scenario::{ScenarioWindow, run_window_scenario};

use super::control::{ControlReceiver, ControlSender, control_lane};
use super::{
    FileDrops, HostDisplays, LoopInput, LoopWindow, LoopWindows, PreferenceStart, SelfCloseRoute,
    WindowSeed, WinitApp, WinitHost, WinitLoop, WinitLoopState, WinitRunState,
};
use crate::error::PlatformError;
use crate::host::{self, Absent, Host, position};
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::notices::testing::NoNotices;
use crate::owner::notices::{NoticeChannel, StaleOnWake};
use crate::owner::signal::OwnerSignal;
use crate::owner::sources::SystemPreferenceSource;
use crate::platform::{AppHandler, HostPlatform, Platform};

/// Builds a real `EventLoop` for a test running on an ordinary test
/// thread, not the process's actual main thread. winit refuses this by
/// default (a real cross-platform hazard for production code); Linux
/// and Windows offer an explicit opt-out for exactly this situation
/// (test harnesses, embedding). AppKit has no such opt-out — main-thread
/// affinity there is enforced by the OS, not just winit's own guard —
/// so the tests that build one are `#[ignore]`d on macOS.
///
/// **Only one `EventLoop` may ever exist per process** (a separate,
/// permanent winit limitation, not the main-thread one above): a second
/// `build()` call anywhere in the same process fails with
/// `RecreationAttempt`, even sequentially, even after the first loop
/// exited. Every test that calls this must run in its own process —
/// `cargo nextest run` gives each test one by default; plain
/// `cargo test` runs the whole binary in one process and WILL fail
/// whichever of these tests happens to run second. CI runs this crate's
/// suite with nextest under `xvfb-run` (see `docs/testing.md`),
/// which is what gives these tests their X11 connection there.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn build_test_event_loop() -> EventLoop<()> {
    #[cfg(target_os = "linux")]
    {
        // `EventLoopBuilderExtWayland` adds a same-named, same-effect
        // `with_any_thread` to this same shared `EventLoopBuilder` (the
        // backend, X11 or Wayland, is a runtime choice, not this
        // builder's) -- importing both traits makes the call
        // ambiguous, so this sets the flag through X11's alone; it
        // applies regardless of which backend is actually selected at
        // runtime.
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = EventLoop::builder();
        EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        builder
            .build()
            .expect("build a real winit event loop off the main thread")
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("build a real winit event loop off the main thread")
    }
}

/// See the Linux and Windows builder: on macOS the loop is the main thread's.
#[cfg(target_os = "macos")]
pub(crate) fn build_test_event_loop() -> EventLoop<()> {
    EventLoop::builder()
        .build()
        .expect("build a real winit event loop (main-thread only on macOS)")
}

/// The owner signal `run_event_loop` installs, for a hand-built `WinitApp`:
/// a wake posts a user event to `event_loop`.
pub(crate) fn install_test_owner_signal(platform: &WinitLoop, event_loop: &EventLoop<()>) {
    let proxy = event_loop.create_proxy();
    *platform.owner_signal.lock() = Some(OwnerSignal::new(Arc::new(move || {
        proxy
            .send_event(())
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            })
    })));
}

/// The receiving end of the control lane a test installed
/// ([`install_control_lane`]), which the application it builds drains
/// ([`WinitApp::for_test`]).
pub(crate) struct LaneReceiver(ControlReceiver);

/// Installs on `platform` the control lane `run_event_loop` installs for the
/// thread that runs `event_loop`, the calling one: a request posts a user event
/// that wakes the loop. Returns the sending end, through which a test opens
/// windows and quits, and the receiving end for the application.
pub(crate) fn install_control_lane(
    platform: &WinitLoop,
    event_loop: &EventLoop<()>,
) -> (ControlSender, LaneReceiver) {
    let proxy = event_loop.create_proxy();
    let (control, receiver) = control_lane(Arc::new(move || {
        let _ = proxy.send_event(());
    }));
    platform
        .install_control_lane(thread::current().id(), control.clone())
        .expect("a loop that has not started takes its lane");
    (control, LaneReceiver(receiver))
}

impl ControlSender {
    /// Asks the loop to quit, as its owner's quit does.
    pub(crate) fn request_quit_for_test(&self) {
        self.request_quit();
    }
}

impl WinitLoopState {
    /// The windows the loop tracks, by their winit ids.
    pub(crate) fn window_ids_for_test(&self) -> &HashMap<WinitWindowId, WindowId> {
        &self.window_id_map
    }

    /// The cursor position the loop keeps of each window, which a test may
    /// seed to see the loop forget it.
    pub(crate) fn cursor_positions_for_test(
        &mut self,
    ) -> &mut HashMap<WindowId, winit::dpi::PhysicalPosition<f64>> {
        &mut self.cursor_positions
    }
}

/// Polls (bounded, 2s) until the owner has reached `Running` — the point
/// at which `resumed()` has returned and the owner lane accepts
/// deferred, post-bootstrap requests. Real wall-clock, not simulated:
/// these tests drive an actual winit event loop.
pub(crate) fn wait_for_running(platform: &WinitLoop) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !platform.with_state(|state| matches!(state.run_state, WinitRunState::Running { .. })) {
        assert!(
            Instant::now() < deadline,
            "owner never reached Running within 2s"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Polls (bounded, 2s) until the loop tracks `expected` windows. Used
/// both to detect that a deferred request actually landed (count goes
/// up) and that an unwind completed (count goes back down) — a
/// deterministic condition, not a blind sleep, even though the exact
/// wake-up latency is real wall-clock time.
pub(crate) fn wait_for_map_len(platform: &WinitLoop, expected: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let actual = platform.with_state(|state| state.window_id_map.len());
        if actual == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: window_id_map.len() = {actual}, expected {expected}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

impl WinitApp {
    /// The application `run_event_loop` runs over `platform`, for a test that
    /// runs a real loop by hand: `owner_platform` is what `on_ready`'s owner
    /// capability wraps, `lane` the receiving end of the lane the test
    /// installed, and nothing is observed yet.
    pub(crate) fn for_test(
        platform: Arc<WinitLoop>,
        owner_platform: Arc<dyn Platform>,
        LaneReceiver(control): LaneReceiver,
        handler: Option<Rc<dyn AppHandler>>,
    ) -> Self {
        Self {
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
            platform,
            owner_platform,
            handler,
            owner_hooks: None,
            control,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        }
    }

    /// The preference source the loop started as it resumed, or the one a
    /// test put in its place before.
    #[cfg(windows)]
    pub(crate) fn preference_source_for_test(
        &mut self,
    ) -> &mut Option<std::rc::Rc<dyn crate::owner::windows::control::OwnerPreferences>> {
        &mut self.preference_source
    }

    /// The broadcast receiver the loop built as it resumed.
    #[cfg(windows)]
    pub(crate) fn broadcasts_for_test(
        &self,
    ) -> Option<&crate::owner::windows::broadcast::Receiver> {
        self.broadcasts.as_ref()
    }

    /// Reads the preferences again now, as an owner wake does.
    #[cfg(windows)]
    pub(crate) fn refresh_preferences_for_test(&self) -> bool {
        self.refresh_preferences()
    }

    /// The failure `on_ready` returned, which `run` reports once the loop
    /// unwound.
    #[cfg(windows)]
    pub(crate) fn bootstrap_error_for_test(&self) -> Option<&crate::error::BootstrapError> {
        self.bootstrap_error.as_ref()
    }
}

/// A winit host whose loop hooks report nothing: displays of `D` (none by
/// default), no notices, preferences or host notices, an in-memory clipboard,
/// no file drags and no input. The loop's tests and the tests of a window run a
/// real loop over it without the host table; a test that needs the displays to
/// answer gives its own `D`.
pub(crate) struct QuietHost<D = NoDisplays>(PhantomData<fn() -> D>);

impl<D: HostDisplays + Default + 'static> WinitHost for QuietHost<D> {
    fn displays() -> Arc<dyn HostDisplays> {
        Arc::new(D::default())
    }

    fn notices(_: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(NoNotices)
    }

    fn preferences() -> Box<dyn PreferenceStart> {
        Box::new(NoPreferences)
    }

    #[cfg(target_os = "linux")]
    fn host_notices() -> Box<dyn super::HostNoticeStart> {
        Box::new(NoHostNotices)
    }

    fn clipboard() -> Arc<dyn crate::host::SharedClipboard> {
        Arc::new(crate::host::MemoryClipboard::new())
    }

    fn drag() -> Arc<dyn FileDrops> {
        Arc::new(NoFileDrops)
    }

    fn input() -> Box<dyn LoopInput> {
        Box::new(NoInput)
    }
}

/// The host table of a loop that opens no window: every position the loop does
/// not answer itself is absent. Its tests reach the platform through it,
/// as an application does, without naming a feature.
pub(crate) struct QuietTable;

impl Host for QuietTable {
    const NAME: &'static str = "winit loop with no windows (tests)";

    type Loop = WinitLoop;
    type Window = NoWindows;
    type Displays = WinitLoop;
    type Input = Absent<position::Input>;
    type Text = Absent<position::Text>;
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = Absent<position::Clipboard>;
    type Drag = WinitLoop;
    type Haptics = Absent<position::Haptics>;
    type Preferences = WinitLoop;
    // The loop is the lifecycle position where it hears logind.
    #[cfg(target_os = "linux")]
    type Lifecycle = WinitLoop;
    #[cfg(not(target_os = "linux"))]
    type Lifecycle = Absent<position::Lifecycle>;
    type Chrome = Absent<position::Chrome>;
    type Storage = Absent<position::Storage>;
}

/// A platform of [`QuietTable`] over a loop whose hooks `H` makes.
pub(crate) fn quiet_platform_over<H: WinitHost>() -> HostPlatform<QuietTable> {
    HostPlatform::from_parts(
        |_| NoWindows,
        |windows| WinitLoop::new::<H>(Arc::clone(windows) as _),
    )
}

/// A platform of [`QuietTable`] over a loop of [`QuietHost`].
pub(crate) fn quiet_platform() -> HostPlatform<QuietTable> {
    quiet_platform_over::<QuietHost>()
}

/// The windows of a loop that opens none: a request to open one is refused.
/// The window position registers the application's window-event handler and
/// reads the active window in the loop, as every winit window position does.
pub(crate) struct NoWindows;

impl NoWindows {
    fn refusal() -> OpenWindowError {
        OpenWindowError::Backend {
            message: "a quiet winit loop opens no windows".to_owned(),
        }
    }
}

impl LoopWindows for NoWindows {
    fn build(
        &self,
        _: &ActiveEventLoop,
        _: WindowSeed<'_>,
        _: WindowOptions,
    ) -> Result<Rc<dyn LoopWindow>, OpenWindowError> {
        Err(Self::refusal())
    }

    #[cfg(target_os = "linux")]
    fn take_unreported(&self) -> Vec<u32> {
        Vec::new()
    }
}

impl host::Window<WinitLoop> for NoWindows {
    type Shared = NoWindows;
    /// No window is opened, so none is referred to.
    type Ref = ();

    fn open(
        _: &WinitLoop,
        _: &Arc<NoWindows>,
        _: WindowOptions,
        _: Rc<dyn flui_platform::WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        Err(Self::refusal())
    }

    fn active(owner: &WinitLoop, _: &NoWindows) -> Option<WindowId> {
        owner.with_state(|state| state.active_window)
    }

    fn capabilities(_: &WinitLoop, _: &NoWindows) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn frame_inset(_: &WinitLoop, _: &NoWindows, _: f64) -> Size<f64> {
        Size::new(0.0, 0.0)
    }
}

/// A translation that delivers no input and sees no device.
struct NoInput;

impl LoopInput for NoInput {
    fn opening(&mut self, _: &ActiveEventLoop) -> InputCapabilities {
        InputCapabilities::UNAVAILABLE
    }

    fn window_event(
        &mut self,
        _: WindowId,
        _: &winit::event::WindowEvent,
        _: f64,
        _: winit::keyboard::ModifiersState,
    ) -> Option<Vec<(WindowId, PlatformInput)>> {
        None
    }

    fn device_event(
        &mut self,
        _: winit::event::DeviceId,
        _: &winit::event::DeviceEvent,
        _: &[WindowId],
    ) -> Vec<(WindowId, PlatformInput)> {
        Vec::new()
    }

    fn presence_change(&mut self) -> Option<PointerKindSet> {
        None
    }

    fn flush(&mut self) -> Vec<(WindowId, PlatformInput)> {
        Vec::new()
    }

    fn focus_lost(&mut self, _: WindowId) -> Vec<PlatformInput> {
        Vec::new()
    }

    fn key(
        &self,
        _: winit::event::KeyEvent,
        _: winit::keyboard::ModifiersState,
    ) -> Option<PlatformInput> {
        None
    }

    fn ime(&self, _: &winit::event::Ime) -> Option<PlatformInput> {
        None
    }
}

/// Displays the source holds none of.
#[derive(Default)]
pub(crate) struct NoDisplays;

impl StaleOnWake for NoDisplays {
    fn mark_stale(&self) {}
}

impl HostDisplays for NoDisplays {
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        Ok(DisplaySnapshot::empty())
    }

    fn id_of_window(&self, _: &winit::window::Window) -> Option<DisplayId> {
        None
    }

    fn resample(&self, _: &ActiveEventLoop, _: Option<&Arc<OwnerSignal>>) -> bool {
        false
    }

    #[cfg(windows)]
    fn broadcasts(
        self: Arc<Self>,
        _: &Arc<OwnerSignal>,
    ) -> std::rc::Rc<dyn crate::owner::windows::broadcast::Subscriber> {
        std::rc::Rc::new(NoBroadcasts)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn observe(self: Arc<Self>, _: &Arc<OwnerSignal>) -> crate::owner::watch::Watch {
        crate::owner::watch::Watch::new(())
    }

    #[cfg(target_os = "linux")]
    fn is_wayland(&self) -> bool {
        false
    }

    #[cfg(target_os = "macos")]
    fn version(&self) -> crate::version::gate::apple::MacOSVersion {
        crate::version::gate::apple::MacOSVersion::new(14, 0, 0)
    }
}

/// A subscriber of the system's broadcasts that answers none of them.
#[cfg(windows)]
struct NoBroadcasts;

#[cfg(windows)]
impl crate::owner::windows::broadcast::Subscriber for NoBroadcasts {
    fn message(
        &self,
        _: windows::Win32::Foundation::HWND,
        _: u32,
        _: windows::Win32::Foundation::WPARAM,
        _: windows::Win32::Foundation::LPARAM,
    ) -> Option<windows::Win32::Foundation::LRESULT> {
        None
    }
}

/// Preferences that stay the defaults.
pub(crate) struct NoPreferences;

impl PreferenceStart for NoPreferences {
    #[cfg(windows)]
    fn start(
        &self,
        _: &Arc<OwnerSignal>,
    ) -> Result<std::rc::Rc<dyn crate::owner::windows::control::OwnerPreferences>, PlatformError>
    {
        Ok(std::rc::Rc::new(
            crate::owner::windows::testing::NoPreferences,
        ))
    }

    #[cfg(target_os = "macos")]
    fn start(
        &self,
        _: &Arc<OwnerSignal>,
    ) -> (
        Arc<dyn SystemPreferenceSource + Send + Sync>,
        crate::owner::watch::Watch,
    ) {
        (Arc::new(NoPreferences), crate::owner::watch::Watch::new(()))
    }

    #[cfg(target_os = "linux")]
    fn start(
        &self,
        _: &crate::executor::BackgroundExecutor,
        _: &Arc<OwnerSignal>,
    ) -> Arc<dyn super::PortalPreferences> {
        Arc::new(NoPreferences)
    }
}

impl SystemPreferenceSource for NoPreferences {
    fn sample(&self) -> Result<SystemPreferences, PlatformError> {
        Ok(SystemPreferences::default())
    }
}

#[cfg(target_os = "linux")]
impl super::PortalPreferences for NoPreferences {
    fn appearance(&self) -> flui_platform::WindowAppearance {
        flui_platform::WindowAppearance::default()
    }
}

/// Host notices that never queue anything.
#[cfg(target_os = "linux")]
struct NoHostNotices;

#[cfg(target_os = "linux")]
impl super::HostNoticeStart for NoHostNotices {
    fn start(
        &self,
        _: &crate::executor::BackgroundExecutor,
        _: &Arc<OwnerSignal>,
    ) -> Box<dyn super::HostNotices> {
        Box::new(NoHostNotices)
    }
}

#[cfg(target_os = "linux")]
impl super::HostNotices for NoHostNotices {
    fn deliver(&self, _: &WinitLoop) {}
}

/// A drag transport that hears of no files: every report ends nothing
/// and offers nothing.
struct NoFileDrops;

impl DataTransferSource for NoFileDrops {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        NullDataTransferSource.clipboard_offer()
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        NullDataTransferSource.request(id, representation, limits)
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        NullDataTransferSource.update_drop_feedback(id, feedback);
    }

    fn conclude_drop(&self, id: DataTransferId) {
        NullDataTransferSource.conclude_drop(id);
    }
}

impl FileDrops for NoFileDrops {
    fn note_hovered_file(
        &self,
        _: WindowId,
        _: std::path::PathBuf,
        _: Option<Point<f64>>,
    ) -> Option<DragDropEvent> {
        None
    }

    fn note_dropped_file(
        &self,
        _: WindowId,
        _: std::path::PathBuf,
        _: Option<Point<f64>>,
    ) -> Option<DragDropEvent> {
        None
    }

    fn note_hover_cancelled(&self, _: WindowId) -> Option<DragDropEvent> {
        None
    }

    fn windows_awaiting_freeze(&self) -> Vec<WindowId> {
        Vec::new()
    }

    fn freeze_completed(
        &self,
        _: &std::collections::HashMap<WindowId, Point<f64>>,
    ) -> Vec<(WindowId, DragDropEvent)> {
        Vec::new()
    }

    fn forget_window(&self, _: WindowId) {}
}

#[cfg(target_os = "linux")]
mod window_scenario {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
        rc::Rc,
        sync::Arc,
        thread,
        time::Duration,
    };

    use flui_foundation::ClaimOutcome;
    use flui_platform::{CommandRefusal, WindowCommand, WindowState};
    use winit::event_loop::EventLoop;

    use super::{install_control_lane, install_test_owner_signal, wait_for_running};
    use crate::host_window::HostWindow;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::winit::{WinitApp, WinitLoop};
    use crate::platform::Platform;
    use crate::{OpenedWindow, WindowRequest};

    /// The window a scenario drives from its worker thread. A window is its
    /// owner thread's (ADR-0254 §5), so each call runs there, in an owner
    /// turn, and the worker waits for its answer; a panic of the call is
    /// raised on the worker.
    pub(crate) struct ScenarioWindow {
        opened: Arc<OpenedWindow>,
        signal: Arc<OwnerSignal>,
    }

    impl ScenarioWindow {
        /// Run `call` with the window on the owner thread and hand back what
        /// it answered.
        pub(crate) fn with<R: Send + 'static>(
            &self,
            call: impl FnOnce(&Rc<dyn HostWindow>) -> R + Send + 'static,
        ) -> R {
            let (answer, answered) = std::sync::mpsc::sync_channel(1);
            let opened = Arc::clone(&self.opened);
            let call = parking_lot::Mutex::new(Some(call));
            let turn = OwnerSignal::window_wake(&self.signal, move || {
                let Some(call) = call.lock().take() else {
                    return;
                };
                let crate::OpenedWindowReach::Open(window) = opened.window() else {
                    panic!("the scenario's window is open, on its owner thread");
                };
                let _ = answer.send(catch_unwind(AssertUnwindSafe(|| call(&window))));
            });
            turn();
            match answered
                .recv_timeout(Duration::from_secs(10))
                .expect("the owner ran the scenario's call")
            {
                Ok(answer) => answer,
                Err(payload) => resume_unwind(payload),
            }
        }

        /// The window's snapshot, read on the owner thread.
        pub(crate) fn state(&self) -> WindowState {
            self.with(|window| window.state())
        }

        /// Admit `command` and apply it on the owner thread.
        pub(crate) fn admit_and_apply(&self, command: WindowCommand) -> Result<(), CommandRefusal> {
            self.with(move |window| command.admit_and_apply(&**window))
        }

        /// The first frame was presented: the window reveals itself.
        pub(crate) fn reveal_after_first_frame(&self) {
            self.with(|window| window.reveal_after_first_frame());
        }
    }

    /// Run `event_loop` for `platform`, the loop of `owner_platform`, open a
    /// window for `opening` once the loop runs, and run `scenario` against
    /// it on a worker thread; the loop quits when the scenario returns, and a
    /// panic in it fails the test.
    pub(crate) fn run_window_scenario(
        platform: Arc<WinitLoop>,
        owner_platform: Arc<dyn Platform>,
        event_loop: EventLoop<()>,
        opening: WindowRequest,
        scenario: impl FnOnce(ScenarioWindow) + Send + 'static,
    ) {
        install_test_owner_signal(&platform, &event_loop);
        let (control, lane) = install_control_lane(&platform, &event_loop);

        let platform_for_worker = Arc::clone(&platform);
        let control_for_worker = control;
        let worker = thread::spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                wait_for_running(&platform_for_worker);
                let opened = match control_for_worker
                    .request_open_window(opening)
                    .expect("lane accepts the request")
                    .wait()
                {
                    ClaimOutcome::Delivered(result) => result.expect("window creation succeeds"),
                    _ => panic!("the window request is delivered"),
                };
                let signal = platform_for_worker
                    .owner_signal
                    .lock()
                    .clone()
                    .expect("the test installed the owner signal");
                scenario(ScenarioWindow {
                    opened: Arc::new(opened),
                    signal,
                });
            }));
            control_for_worker.request_quit();
            if let Err(payload) = outcome {
                resume_unwind(payload);
            }
        });

        let mut app = WinitApp::for_test(Arc::clone(&platform), owner_platform, lane, None);
        event_loop
            .run_app(&mut app)
            .expect("event loop runs to completion");
        worker.join().expect("worker thread does not panic");
    }
}
