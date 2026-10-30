//! The runner's close of a window with content: what the close releases,
//! when it runs and what it leaves behind. Each row runs a whole application
//! with its main window on the headless platform through `flui_app::testing`,
//! where the window renders with the kit's stand-in renderer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flui_app::app::{
    AppConfig, AppHandle, Application, DeviceRecovery, ExitPolicy, FrameFailureHandler,
    FrameFailureKind, request_presentation_close,
};
use flui_app::testing::{HeadlessRun, HeadlessRunHandle, RunExit, StandInRenderer, TeardownStep};
use flui_foundation::PresentationAddress;
use flui_foundation::geometry::Size;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_testing::{DropLedger, DropSentinel};
use flui_view::element::FutureBuilder;
use flui_view::prelude::*;

/// The order every loop's teardown runs in.
const TEARDOWN_ORDER: [TeardownStep; 6] = [
    TeardownStep::MainWindow,
    TeardownStep::UiRuntimes,
    TeardownStep::Flush,
    TeardownStep::Services,
    TeardownStep::Execution,
    TeardownStep::NativeResources,
];

/// The exit's budget (ADR-0258 §7) and a second of slack.
const EXIT_BUDGET_AND_SLACK: Duration = Duration::from_secs(7);

/// What the observed tree plants and reports.
#[derive(Clone, Default)]
struct Observed {
    /// A sentinel in the root's state.
    states: DropLedger,
    /// A sentinel in each future the tree's load starts.
    futures: DropLedger,
    /// How many times the root's state was disposed.
    disposed: Rc<Cell<u32>>,
    /// The root's rebuild, once it is mounted.
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
    /// While set, the leaf under the load fails its paint: the frame is
    /// dropped.
    fail_paint: Arc<AtomicBool>,
    /// The presentation the root's next build closes, and what the close
    /// answered.
    close_from_build: Rc<Cell<Option<PresentationAddress>>>,
    closed_from_build: Rc<RefCell<Vec<bool>>>,
    /// Opens the load's gate: the future completes once this is set.
    gate: Rc<Gate>,
    /// The device rebuild each frame dropped on a lost device reported.
    device_reports: Arc<Mutex<Vec<DeviceRecovery>>>,
}

impl Observed {
    fn schedule_rebuild(&self) {
        self.rebuild
            .borrow()
            .as_ref()
            .expect("the root mounted and kept its rebuild")
            .schedule(RebuildReason::StateChange);
    }
}

/// A load that completes when the test opens it.
#[derive(Default)]
struct Gate {
    open: Cell<bool>,
    waker: RefCell<Option<std::task::Waker>>,
}

impl Gate {
    fn open(&self) {
        self.open.set(true);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

/// Waits for its [`Gate`], keeping a sentinel while it lives.
struct GatedLoad {
    gate: Rc<Gate>,
    _sentinel: DropSentinel,
}

impl std::future::Future for GatedLoad {
    type Output = Result<(), ()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.gate.open.get() {
            return std::task::Poll::Ready(Ok(()));
        }
        self.gate.waker.replace(Some(cx.waker().clone()));
        std::task::Poll::Pending
    }
}

/// A leaf whose paint fails while `armed` is set: the pipeline drops the
/// frame for it (`RenderError::Poisoned`).
#[derive(Clone)]
struct PaintTrip {
    armed: Arc<AtomicBool>,
}

#[derive(Debug)]
struct PaintTripBox {
    armed: Arc<AtomicBool>,
}

impl flui_foundation::Diagnosticable for PaintTripBox {}

impl RenderBox for PaintTripBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(40.0, 24.0))
    }

    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {
        assert!(
            !self.armed.load(Ordering::SeqCst),
            "the leaf's paint failed"
        );
    }
}

impl RenderView for PaintTrip {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = PaintTripBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> PaintTripBox {
        PaintTripBox {
            armed: Arc::clone(&self.armed),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut PaintTripBox,
    ) -> flui_rendering::RenderUpdateImpact {
        // A rebuild repaints, so arming the trip and rebuilding fails a frame.
        flui_rendering::RenderUpdateImpact::PAINT
    }
}

impl View for PaintTrip {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// The root of the observed tree: a state that keeps a sentinel, and under it a
/// load that waits for the test.
#[derive(Clone, StatefulView)]
struct ObservedRoot {
    observed: Observed,
}

struct ObservedRootState {
    observed: Observed,
    _sentinel: DropSentinel,
}

impl StatefulView for ObservedRoot {
    type State = ObservedRootState;

    fn create_state(&self) -> Self::State {
        ObservedRootState {
            observed: self.observed.clone(),
            _sentinel: self.observed.states.plant(),
        }
    }
}

impl ViewState<ObservedRoot> for ObservedRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.observed.rebuild.replace(Some(ctx.rebuild_handle()));
    }

    fn build(&self, _view: &ObservedRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        if let Some(address) = self.observed.close_from_build.take() {
            let closed = request_presentation_close(address).is_ok();
            self.observed.closed_from_build.borrow_mut().push(closed);
        }
        let observed = self.observed.clone();
        let trip = Arc::clone(&self.observed.fail_paint);
        FutureBuilder::keyed(
            Some(0_u32),
            Rc::new(move || {
                Box::pin(GatedLoad {
                    gate: Rc::clone(&observed.gate),
                    _sentinel: observed.futures.plant(),
                }) as flui_view::element::BoxedResultFuture<(), ()>
            }),
            Rc::new(move |_, _| {
                PaintTrip {
                    armed: Arc::clone(&trip),
                }
                .boxed()
            }),
        )
    }

    fn dispose(&mut self) {
        self.observed.disposed.set(self.observed.disposed.get() + 1);
    }
}

/// An application whose main window holds the observed tree and which exits
/// when its last window closes. It hands its control to `ready`.
fn observed_app(
    observed: Observed,
    ready: Rc<RefCell<Option<AppHandle>>>,
) -> Application<ObservedRoot, impl FnMut(&AppHandle) -> ObservedRoot> {
    let device_reports = Arc::clone(&observed.device_reports);
    let failures = FrameFailureHandler::new(move |report| {
        if let FrameFailureKind::DeviceLost { recovery, .. } = report.kind {
            device_reports
                .lock()
                .expect("BUG: the frame-failure handler does not panic")
                .push(recovery);
        }
    });
    Application::new(move |_| ObservedRoot {
        observed: observed.clone(),
    })
    .with_config(
        AppConfig::new()
            .with_exit_policy(ExitPolicy::OnLastWindowClosed)
            .with_frame_failure_handler(failures),
    )
    .on_ready(move |handle| {
        ready.replace(Some(handle.clone()));
    })
}

/// A started run of [`probe_app`], its main window up and presented.
struct Started {
    run: HeadlessRunHandle,
    handle: AppHandle,
    window: flui_native::MockWindow,
    renderer: StandInRenderer,
}

fn start(observed: &Observed) -> Started {
    let ready = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(observed_app(observed.clone(), Rc::clone(&ready)));
    run.run_owner_turn();
    let handle = ready.take().expect("on_ready ran during start");
    let windows = run.open_windows();
    assert_eq!(windows.len(), 1, "the main window opened");
    let window = windows.into_iter().next().expect("one window");
    let renderer = run
        .renderer(&window)
        .expect("the main window renders with a stand-in");
    assert!(renderer.frames_presented() > 0, "the main window presented");
    assert_eq!(observed.states.live(), 1, "the root's state is mounted");
    assert_eq!(observed.futures.live(), 1, "the root's load is pending");
    Started {
        run,
        handle,
        window,
        renderer,
    }
}

/// The main window's address, as the application reaches it.
fn main_window_address(started: &Started) -> PresentationAddress {
    let mut request = started
        .handle
        .request_show_main_window()
        .expect("the loop accepts a show request");
    started.run.run_owner_turn();
    request
        .try_result()
        .expect("the show of an open window completes on the next turn")
        .expect("the main window is installed")
}

/// Join `run`, which must return `Ok` within the exit's budget after every
/// teardown step.
fn join_ok(run: HeadlessRunHandle) {
    let started = Instant::now();
    let report = run.join().expect("the run returns Ok");
    let took = started.elapsed();
    assert_eq!(report.exit, RunExit::Returned);
    assert_eq!(report.code(), 0);
    assert_eq!(report.teardown, TEARDOWN_ORDER);
    assert!(
        took < EXIT_BUDGET_AND_SLACK,
        "the exit took {took:?}, past its budget"
    );
}

/// How a test closes the last window.
#[derive(Clone, Copy, Debug)]
enum CloseRoute {
    /// The user closes it, as the window's close button does.
    User,
    /// The application closes it by its address.
    Program,
}

fn close(started: &Started, route: CloseRoute) {
    match route {
        CloseRoute::User => assert!(started.window.simulate_close(), "the window closes"),
        CloseRoute::Program => {
            let address = main_window_address(started);
            request_presentation_close(address).expect("the program's close is accepted");
        }
    }
    started.run.run_owner_turn();
}

/// R4: the last window's close, the user's or the program's, ends the loop,
/// and the run returns `Ok` within the exit's budget after every teardown
/// step.
pub(crate) fn last_window_close_returns_ok_on_both_routes() {
    for route in [CloseRoute::User, CloseRoute::Program] {
        let observed = Observed::default();
        let started = start(&observed);
        close(&started, route);
        assert!(
            !started.run.is_running(),
            "{route:?}: the last window's close quit the platform"
        );
        join_ok(started.run);
    }
}

/// R1: the runner's close of the last window releases what its UI runtime
/// holds, before the exit's own teardown runs: the state is disposed and
/// dropped, its load is dropped and the window's renderer is released.
pub(crate) fn last_window_close_releases_the_runtime_through_the_runner() {
    let observed = Observed::default();
    let started = start(&observed);
    close(&started, CloseRoute::User);

    assert_eq!(observed.disposed.get(), 1, "the root's state was disposed");
    assert_eq!(observed.states.live(), 0, "the root's state was dropped");
    assert_eq!(observed.futures.live(), 0, "the pending load was dropped");
    assert!(
        started.renderer.is_released(),
        "the window's renderer was released"
    );
    join_ok(started.run);
}

/// R2: a load still pending when its window closes is dropped once, and
/// its completion never comes.
pub(crate) fn closing_with_a_pending_load_retires_the_future_once() {
    let observed = Observed::default();
    let started = start(&observed);
    close(&started, CloseRoute::User);

    assert_eq!(observed.futures.planted(), 1, "the load started once");
    assert_eq!(observed.futures.live(), 0, "the load was dropped");
    join_ok(started.run);
    assert_eq!(
        observed.futures.planted(),
        1,
        "no load started after the close"
    );
}

/// R16: a load that completes after its window closed asks for no frame:
/// nothing renders and the run still ends as an ordinary close does.
pub(crate) fn a_late_completion_after_close_requests_no_frame() {
    let observed = Observed::default();
    let started = start(&observed);
    let frames = started.renderer.frames_presented();
    close(&started, CloseRoute::User);

    observed.gate.open();
    started.run.run_owner_turn();
    assert_eq!(
        started.renderer.frames_presented(),
        frames,
        "the late completion rendered a frame"
    );
    join_ok(started.run);
}

/// R18: a second close of a window already closing tears it down once.
pub(crate) fn a_second_close_request_tears_down_once() {
    let observed = Observed::default();
    let started = start(&observed);
    let address = main_window_address(&started);
    request_presentation_close(address).expect("the first close is accepted");
    let second = request_presentation_close(address);
    assert!(
        second.is_err(),
        "the second close found the presentation closed: {second:?}"
    );
    // The user closes it too, after the program did.
    let _ = started.window.simulate_close();
    started.run.run_owner_turn();

    assert_eq!(
        observed.disposed.get(),
        1,
        "the root's state was disposed once"
    );
    assert_eq!(observed.states.live(), 0);
    join_ok(started.run);
}

/// R15: a close the application asks for while its window builds a frame
/// runs once, after the frame, and ends the loop on the turn it ran in.
pub(crate) fn close_requested_inside_a_frame_runs_once_on_the_next_turn() {
    let observed = Observed::default();
    let started = start(&observed);
    let address = main_window_address(&started);
    observed.close_from_build.set(Some(address));
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    started.run.run_owner_turn();

    assert_eq!(
        *observed.closed_from_build.borrow(),
        [true],
        "the build asked for the close once, and it was accepted"
    );
    assert_eq!(
        observed.disposed.get(),
        1,
        "the root's state was disposed once"
    );
    assert!(!started.run.is_running(), "the close ended the loop");
    join_ok(started.run);
}

/// R10: a dropped frame is retried at its retry deadline, not at once and
/// not never: the runner asks its loop to wake at that instant, after now,
/// and a turn before it retries nothing.
pub(crate) fn the_runner_sleeps_until_the_failure_retry_deadline() {
    let observed = Observed::default();
    let started = start(&observed);
    assert_eq!(
        started.run.wake_deadline(),
        None,
        "an idle loop sleeps until an event"
    );
    observed.fail_paint.store(true, Ordering::SeqCst);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    let failed_at = Instant::now();
    let presented = started.renderer.frames_presented();

    let deadline = started
        .run
        .wake_deadline()
        .expect("a dropped frame asks the loop to wake for its retry");
    assert!(
        deadline > failed_at,
        "the retry is due at once, so the loop would spin: {:?} before the failure",
        failed_at.saturating_duration_since(deadline)
    );
    observed.fail_paint.store(false, Ordering::SeqCst);
    started.run.run_owner_turn();
    assert_eq!(
        started.renderer.frames_presented(),
        presented,
        "a turn before the retry deadline retried the frame"
    );
    join_ok(started.run);
}

/// R20: a device lost on the frame the window closes on does not hold up
/// the exit, even when its rebuild keeps failing: the close the frame asked
/// for runs after it, and the loop ends.
pub(crate) fn device_loss_on_the_closing_frame_still_exits_ok() {
    let observed = Observed::default();
    let started = start(&observed);
    let address = main_window_address(&started);
    started.renderer.lose_device();
    started.renderer.fail_recoveries(u32::MAX);
    observed.close_from_build.set(Some(address));
    observed.schedule_rebuild();
    started.run.run_owner_turn();

    assert_eq!(*observed.closed_from_build.borrow(), [true]);
    assert!(!started.run.is_running(), "the close ended the loop");
    assert!(started.renderer.is_released());
    join_ok(started.run);
}

/// Sleep until `deadline` and run the turn a native loop runs when its wake
/// deadline is due: it asks every window for a frame.
fn wake_at(started: &Started, deadline: Instant) {
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    flui_platform::PlatformWindow::request_redraw(&started.window);
    started.run.run_owner_turn();
}

/// The device rebuilds the dropped frames of `observed` reported, in order.
fn device_reports(observed: &Observed) -> Vec<DeviceRecovery> {
    observed
        .device_reports
        .lock()
        .expect("BUG: the frame-failure handler does not panic")
        .clone()
}

/// R21: a lost device whose rebuild keeps failing is rebuilt on its backoff,
/// not at once: the turn that lost it tries one rebuild, reports the failed
/// rebuild with the dropped frame, and asks its loop to wake later instead of
/// running frames until the device comes back.
pub(crate) fn a_lost_device_waits_out_its_rebuild_backoff() {
    let observed = Observed::default();
    let started = start(&observed);
    started.renderer.lose_device();
    started.renderer.fail_recoveries(u32::MAX);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    let lost_at = Instant::now();

    assert_eq!(started.renderer.recovery_attempts(), 1, "one rebuild ran");
    assert_eq!(
        device_reports(&observed).last(),
        Some(&DeviceRecovery::Failed { attempts: 1 }),
        "the dropped frame carries the failed rebuild"
    );
    let deadline = started
        .run
        .wake_deadline()
        .expect("the loop wakes for the next rebuild");
    assert!(deadline > lost_at, "the next rebuild is due at once");
    started.run.run_owner_turn();
    assert_eq!(
        started.renderer.recovery_attempts(),
        1,
        "a turn before the deadline rebuilt the device again"
    );
    // The loop's deadlines, the rebuild's among them, lead to the next one.
    let mut deadline = deadline;
    for _ in 0..8 {
        wake_at(&started, deadline);
        if started.renderer.recovery_attempts() > 1 {
            break;
        }
        deadline = started
            .run
            .wake_deadline()
            .expect("the loop still wakes for the next rebuild");
    }
    assert_eq!(
        started.renderer.recovery_attempts(),
        2,
        "the next rebuild ran at its deadline"
    );
    join_ok_after_close(started);
}

/// R22: a rebuild that keeps failing at the backoff's ceiling is given up,
/// reported once, and tried no more; the application's
/// `AppHandle::retry_render_device` starts it over, and the window renders
/// again.
pub(crate) fn a_given_up_device_renders_again_when_the_application_asks() {
    let observed = Observed::default();
    let started = start(&observed);
    started.renderer.lose_device();
    started.renderer.fail_recoveries(u32::MAX);
    observed.schedule_rebuild();
    started.run.run_owner_turn();

    // The loop as a desktop runs it: each turn sleeps until the deadline the
    // previous one asked to wake at.
    let given_up_by = Instant::now() + Duration::from_secs(30);
    let given_up = |observed: &Observed| {
        device_reports(observed)
            .iter()
            .any(|recovery| recovery.is_abandoned())
    };
    while !given_up(&observed) {
        assert!(
            Instant::now() < given_up_by,
            "the rebuild was never given up"
        );
        let deadline = started
            .run
            .wake_deadline()
            .expect("a rebuild not given up asks the loop to wake for the next");
        wake_at(&started, deadline);
    }
    let attempts = started.renderer.recovery_attempts();
    assert_eq!(
        device_reports(&observed).last(),
        Some(&DeviceRecovery::Abandoned { attempts }),
        "giving up is reported with every failed rebuild counted"
    );

    // Past the ceiling's interval, the given-up device is left alone.
    wake_at(&started, Instant::now() + Duration::from_millis(1_100));
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    assert_eq!(started.renderer.recovery_attempts(), attempts);
    assert_eq!(
        device_reports(&observed)
            .iter()
            .filter(|recovery| recovery.is_abandoned())
            .count(),
        1,
        "giving up is reported once"
    );

    started.renderer.fail_recoveries(0);
    let presented = started.renderer.frames_presented();
    started
        .handle
        .retry_render_device()
        .expect("the loop accepts the request");
    started.run.run_owner_turn();
    assert_eq!(started.renderer.recovery_attempts(), attempts + 1);
    assert!(
        started.renderer.frames_presented() > presented,
        "the rebuilt device shows the window again"
    );
    join_ok_after_close(started);
}

/// Close the last window and join the run.
fn join_ok_after_close(started: Started) {
    close(&started, CloseRoute::User);
    assert!(!started.run.is_running(), "the close ended the loop");
    join_ok(started.run);
}

#[test]
fn runner_close() {
    crate::run_table(
        "runner_close",
        &[
            (
                "last_window_close_returns_ok_on_both_routes",
                last_window_close_returns_ok_on_both_routes,
            ),
            (
                "last_window_close_releases_the_runtime_through_the_runner",
                last_window_close_releases_the_runtime_through_the_runner,
            ),
            (
                "closing_with_a_pending_load_retires_the_future_once",
                closing_with_a_pending_load_retires_the_future_once,
            ),
            (
                "a_late_completion_after_close_requests_no_frame",
                a_late_completion_after_close_requests_no_frame,
            ),
            (
                "a_second_close_request_tears_down_once",
                a_second_close_request_tears_down_once,
            ),
            (
                "close_requested_inside_a_frame_runs_once_on_the_next_turn",
                close_requested_inside_a_frame_runs_once_on_the_next_turn,
            ),
            (
                "the_runner_sleeps_until_the_failure_retry_deadline",
                the_runner_sleeps_until_the_failure_retry_deadline,
            ),
            (
                "device_loss_on_the_closing_frame_still_exits_ok",
                device_loss_on_the_closing_frame_still_exits_ok,
            ),
            (
                "a_lost_device_waits_out_its_rebuild_backoff",
                a_lost_device_waits_out_its_rebuild_backoff,
            ),
            (
                "a_given_up_device_renders_again_when_the_application_asks",
                a_given_up_device_renders_again_when_the_application_asks,
            ),
        ],
    );
}
