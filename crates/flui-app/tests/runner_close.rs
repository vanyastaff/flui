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
    AppConfig, AppHandle, AppWindowError, Application, CloseReason, CloseRequestHandler,
    CloseResponse, DeviceRecovery, ExitPolicy, FrameFailureHandler, FrameFailureHandlerFactory,
    FrameFailureKind, WindowPolicy, open_window, request_presentation_close,
};
use flui_app::testing::{HeadlessRun, HeadlessRunHandle, RunExit, StandInRenderer, TeardownStep};
use flui_foundation::PresentationAddress;
use flui_foundation::geometry::Size;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintContext, RenderBox};
use flui_testing::{DropLedger, DropSentinel};
use flui_view::element::FutureBuilder;
use flui_view::prelude::*;
use flui_view::{CloseGuard, PendingClose};

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
    /// How many times the leaf under the load painted.
    paints: Arc<std::sync::atomic::AtomicU32>,
    /// The presentation the root's next build closes, and what the close
    /// answered.
    close_from_build: Rc<Cell<Option<PresentationAddress>>>,
    closed_from_build: Rc<RefCell<Vec<bool>>>,
    /// Opens the load's gate: the future completes once this is set.
    gate: Rc<Gate>,
    /// The presentation's close guard, once the root is mounted.
    guard: Rc<RefCell<Option<CloseGuard>>>,
    /// The device rebuild each frame dropped on a lost device reported.
    device_reports: Arc<Mutex<Vec<DeviceRecovery>>>,
    /// Frames a panic dropped, as the frame-failure handler was told.
    frame_panics: Arc<std::sync::atomic::AtomicU32>,
}

impl Observed {
    fn guard(&self) -> CloseGuard {
        self.guard
            .borrow()
            .clone()
            .expect("the root mounted with its window's close guard")
    }

    /// The close recorded on the window's guard and waiting, if one is.
    fn pending(&self) -> Option<PendingClose> {
        self.guard().pending()
    }

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
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.gate.open.get() {
            return std::task::Poll::Ready(Ok(()));
        }
        self.gate.waker.replace(Some(ctx.waker().clone()));
        std::task::Poll::Pending
    }
}

/// A leaf whose paint fails while `armed` is set: the pipeline drops the
/// frame for it (`RenderError::Poisoned`).
#[derive(Clone)]
struct PaintTrip {
    armed: Arc<AtomicBool>,
    paints: Arc<std::sync::atomic::AtomicU32>,
}

#[derive(Debug)]
struct PaintTripBox {
    armed: Arc<AtomicBool>,
    paints: Arc<std::sync::atomic::AtomicU32>,
}

impl flui_foundation::inspect::Inspect for PaintTripBox {
    fn kind(&self) -> &'static str {
        "PaintTripBox"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for PaintTripBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(40.0, 24.0))
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        self.paints.fetch_add(1, Ordering::SeqCst);
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
            paints: Arc::clone(&self.paints),
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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ObservedRootState {
            observed: self.observed.clone(),
            _sentinel: self.observed.states.plant(),
        }
    }
}

impl ViewState<ObservedRoot> for ObservedRootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.observed.rebuild.replace(Some(ctx.rebuild_handle()));
        self.observed.guard.replace(ctx.close_guard());
    }

    fn build(&self, _view: &ObservedRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        if let Some(address) = self.observed.close_from_build.take() {
            let closed = request_presentation_close(address).is_ok();
            self.observed.closed_from_build.borrow_mut().push(closed);
        }
        let observed = self.observed.clone();
        let trip = Arc::clone(&self.observed.fail_paint);
        let paints = Arc::clone(&self.observed.paints);
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
                    paints: Arc::clone(&paints),
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
    handler: Option<CloseRequestHandler>,
) -> (Application, ObservedRoot) {
    let device_reports = Arc::clone(&observed.device_reports);
    let frame_panics = Arc::clone(&observed.frame_panics);
    let mut config = AppConfig::new()
        .exit_policy(ExitPolicy::OnLastWindowClosed)
        .frame_failure_handler(FrameFailureHandlerFactory::new(move || {
            let reports = Arc::clone(&device_reports);
            let panics = Arc::clone(&frame_panics);
            FrameFailureHandler::new(move |report| match report.kind {
                FrameFailureKind::DeviceLost { recovery, .. } => reports
                    .lock()
                    .expect("BUG: the frame-failure handler does not panic")
                    .push(recovery),
                FrameFailureKind::CallbackPanic { .. } => {
                    panics.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            })
        }));
    if let Some(handler) = handler {
        config = config.close_request_handler(handler);
    }
    let root = ObservedRoot { observed };
    let application = Application::new().config(config).on_ready(move |handle| {
        ready.replace(Some(handle.clone()));
    });
    (application, root)
}

/// A started run of [`observed_app`], its main window up and presented.
struct Started {
    run: HeadlessRunHandle,
    handle: AppHandle,
    window: flui_native::HeadlessHostWindow,
    renderer: StandInRenderer,
}

fn start(observed: &Observed) -> Started {
    start_with(observed, None)
}

/// [`start`], with `handler` answering the windows' close requests.
fn start_with(observed: &Observed, handler: Option<CloseRequestHandler>) -> Started {
    let ready = Rc::new(RefCell::new(None));
    let (application, root) = observed_app(observed.clone(), Rc::clone(&ready), handler);
    let run = HeadlessRun::start(application, root);
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

/// How the application refuses a close in a veto row.
#[derive(Clone, Copy, Debug)]
enum Veto {
    /// The close-request handler answers `KeepOpen`.
    Handler,
    /// A widget holds the window's close guard.
    Hold,
}

/// A handler that keeps the window open for `refused`, and lets every other
/// close through.
fn keep_open_for(refused: CloseReason) -> CloseRequestHandler {
    CloseRequestHandler::new(move |request| {
        if request.reason() == refused {
            CloseResponse::KeepOpen
        } else {
            CloseResponse::Close
        }
    })
}

/// Run owner turns until the loop ends, at most `turns` of them.
fn turn_until_stopped(run: &HeadlessRunHandle, turns: usize) {
    for _ in 0..turns {
        if !run.is_running() {
            return;
        }
        run.run_owner_turn();
    }
}

/// Everything the window's tree holds is still alive and nothing was
/// disposed.
fn assert_nothing_disposed(observed: &Observed, started: &Started, case: &str) {
    assert!(started.run.is_running(), "{case}: the loop still runs");
    assert_eq!(observed.disposed.get(), 0, "{case}: nothing was disposed");
    assert_eq!(observed.states.live(), 1, "{case}: the state is alive");
    assert_eq!(observed.futures.live(), 1, "{case}: the load is alive");
    assert!(
        !started.renderer.is_released(),
        "{case}: the renderer is still the window's"
    );
}

/// R23: a close the application refuses, by its handler or by a hold,
/// disposes nothing and is recorded for the user's reason; carrying the
/// record out closes the window and ends the loop.
pub(crate) fn a_vetoed_close_disposes_nothing() {
    for veto in [Veto::Handler, Veto::Hold] {
        let observed = Observed::default();
        let handler = matches!(veto, Veto::Handler).then(|| keep_open_for(CloseReason::User));
        let started = start_with(&observed, handler);
        let hold = matches!(veto, Veto::Hold).then(|| observed.guard().hold());

        let _ = started.window.simulate_close();
        started.run.run_owner_turn();
        assert_nothing_disposed(&observed, &started, &format!("{veto:?}"));
        let pending = observed.pending().expect("the refused close is recorded");
        assert_eq!(pending.reason(), CloseReason::User, "{veto:?}");

        match hold {
            Some(hold) => drop(hold),
            None => pending
                .discard_and_close()
                .expect("the record is the current one"),
        }
        turn_until_stopped(&started.run, 4);
        assert!(
            !started.run.is_running(),
            "{veto:?}: the recorded close ran and ended the loop"
        );
        assert_eq!(observed.disposed.get(), 1, "{veto:?}: disposed once");
        join_ok(started.run);
    }
}

/// A quit a hold or the handler refuses is deferred, disposes nothing and is
/// recorded as the program's; once the record is carried out the quit goes
/// ahead without asking again.
pub(crate) fn a_refused_quit_waits_for_the_record_then_exits() {
    for veto in [Veto::Handler, Veto::Hold] {
        let observed = Observed::default();
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&asked);
        let handler = matches!(veto, Veto::Handler).then(|| {
            CloseRequestHandler::new(move |request| {
                if request.reason() == CloseReason::Program {
                    counted.fetch_add(1, Ordering::SeqCst);
                    CloseResponse::KeepOpen
                } else {
                    CloseResponse::Close
                }
            })
        });
        let started = start_with(&observed, handler);
        let hold = matches!(veto, Veto::Hold).then(|| observed.guard().hold());

        started.handle.request_quit().expect("the quit is accepted");
        started.run.run_owner_turn();
        started.run.run_owner_turn();
        assert_nothing_disposed(&observed, &started, &format!("{veto:?}"));
        let pending = observed.pending().expect("the refused quit is recorded");
        assert_eq!(pending.reason(), CloseReason::Program, "{veto:?}");

        match hold {
            Some(hold) => drop(hold),
            None => pending
                .discard_and_close()
                .expect("the record is the current one"),
        }
        turn_until_stopped(&started.run, 4);
        assert!(
            !started.run.is_running(),
            "{veto:?}: the deferred quit went ahead"
        );
        if matches!(veto, Veto::Handler) {
            assert_eq!(
                asked.load(Ordering::SeqCst),
                1,
                "the handler was not asked again"
            );
        }
        join_ok(started.run);
    }
}

/// A quit is cancelled when the window stays open: by its recorded close
/// resolved with `stay_open`, or by a handler that panicked and so recorded
/// nothing. The application keeps running and accepts work.
pub(crate) fn a_quit_the_window_stays_open_for_is_cancelled() {
    for panics in [false, true] {
        let observed = Observed::default();
        let handler = CloseRequestHandler::new(move |request| {
            if request.reason() != CloseReason::Program {
                return CloseResponse::Close;
            }
            assert!(!panics, "deliberate handler panic under test");
            CloseResponse::KeepOpen
        });
        let started = start_with(&observed, Some(handler));

        started.handle.request_quit().expect("the quit is accepted");
        started.run.run_owner_turn();
        if let Some(pending) = observed.pending() {
            assert!(!panics, "a panic recorded a close");
            pending.stay_open().expect("the record is the current one");
        } else {
            assert!(panics, "a kept-open quit was not recorded");
        }
        for _ in 0..3 {
            started.run.run_owner_turn();
        }
        assert_nothing_disposed(&observed, &started, &format!("panics={panics}"));
        // The cancelled quit fenced nothing: a show request is still served.
        main_window_address(&started);

        close(&started, CloseRoute::User);
        assert!(!started.run.is_running(), "panics={panics}");
        join_ok(started.run);
    }
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
/// Every window of a run draws on the run's one device (ADR-0305): a lost
/// device fails the frames of both windows, one rebuild brings it back for
/// both, and both draw again.
pub(crate) fn two_windows_draw_on_one_device() {
    let main = Observed::default();
    let secondary = Observed::default();
    let ready = Rc::new(RefCell::new(None));
    let ready_slot = Rc::clone(&ready);
    let opened = secondary.clone();
    let (application, root) = observed_app(main.clone(), Rc::clone(&ready), None);
    let application = application.on_ready(move |handle| {
        ready_slot.replace(Some(handle.clone()));
        drop(
            open_window(
                AppConfig::new(),
                WindowPolicy::Isolated,
                ObservedRoot { observed: opened },
            )
            .expect("an isolated window opens on the headless platform"),
        );
    });
    let run = HeadlessRun::start(application, root);
    run.run_owner_turn();
    run.run_owner_turn();
    let windows = run.open_windows();
    assert_eq!(windows.len(), 2, "both windows are open");
    let renderers: Vec<StandInRenderer> = windows
        .iter()
        .map(|window| run.renderer(window).expect("each window renders"))
        .collect();
    let presented = |renderers: &[StandInRenderer]| -> Vec<u64> {
        renderers
            .iter()
            .map(StandInRenderer::frames_presented)
            .collect()
    };
    assert!(
        presented(&renderers).iter().all(|frames| *frames > 0),
        "both windows presented"
    );

    // One window's device is the other's: losing it fails both windows' frames.
    renderers[0].fail_recoveries(u32::MAX);
    renderers[0].lose_device();
    let before = presented(&renderers);
    main.schedule_rebuild();
    secondary.schedule_rebuild();
    for window in &windows {
        flui_platform::PlatformWindow::request_redraw(window);
    }
    run.run_owner_turn();
    assert_eq!(
        presented(&renderers),
        before,
        "neither window presents on the lost device"
    );
    let attempts = renderers[1].recovery_attempts();
    assert!(attempts > 0, "the second window's frame saw the loss");
    assert_eq!(
        renderers[0].recovery_attempts(),
        attempts,
        "both windows count the same rebuilds"
    );

    // The loop as a desktop runs it: each turn sleeps until the deadline the
    // previous one asked to wake at. The next rebuild, at its deadline,
    // brings the device back for both windows, and both draw again.
    renderers[1].fail_recoveries(0);
    let drawn = |renderers: &[StandInRenderer]| {
        presented(renderers)
            .iter()
            .zip(&before)
            .all(|(now, before)| now > before)
    };
    let mut turns = 0;
    while !drawn(&renderers) {
        turns += 1;
        assert!(
            turns <= 16,
            "the windows never drew again: {before:?} -> {:?}",
            presented(&renderers)
        );
        let deadline = run
            .wake_deadline()
            .expect("the loop wakes for the rebuild and the dropped frames");
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        for window in &windows {
            flui_platform::PlatformWindow::request_redraw(window);
        }
        run.run_owner_turn();
    }
    assert_eq!(
        renderers[0].recovery_attempts(),
        attempts + 1,
        "one rebuild served both windows"
    );

    for window in &windows {
        assert!(window.simulate_close(), "the window closes");
        run.run_owner_turn();
    }
    assert!(!run.is_running(), "closing both windows ended the loop");
    join_ok(run);
}

/// A panic in the renderer reaches the application's frame-failure handler
/// as the frame's failure, and the window draws again after it.
pub(crate) fn a_panicking_frame_is_reported_and_the_window_draws_again() {
    let observed = Observed::default();
    let started = start(&observed);
    let presented = started.renderer.frames_presented();
    // A renderer that panics on this frame and on the next ones: a retry
    // would panic again.
    started.renderer.panic_in_next_frames(8);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    assert_eq!(
        observed.frame_panics.load(Ordering::SeqCst),
        1,
        "the renderer's panic was reported to the frame-failure handler after its frame"
    );
    let paints = observed.paints.load(Ordering::SeqCst);
    // The loop's own deadlines pass with nothing asked of the window: the
    // panicked frame is not retried into the same panic.
    for _ in 0..3 {
        let Some(deadline) = started.run.wake_deadline() else {
            break;
        };
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        started.run.run_owner_turn();
    }
    assert_eq!(
        observed.frame_panics.load(Ordering::SeqCst),
        1,
        "the panic was reported once, and its frame not retried into the same panic"
    );
    assert_eq!(
        observed.paints.load(Ordering::SeqCst),
        paints,
        "a panicked frame repaints nothing on its own"
    );
    started.renderer.panic_in_next_frames(0);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    assert!(
        started.renderer.frames_presented() > presented,
        "the window's next change drew again after the panic"
    );
    join_ok_after_close(started);
}

/// A window whose surface the run's GPU cannot draw to fails to open with
/// a renderer error, instead of opening blank.
pub(crate) fn a_window_the_gpu_refuses_does_not_open() {
    let observed = Observed::default();
    let started = start(&observed);
    started.renderer.refuse_next_window();
    let mut request = open_window(
        AppConfig::new(),
        WindowPolicy::Isolated,
        ObservedRoot {
            observed: Observed::default(),
        },
    )
    .expect("the window is admitted");
    for _ in 0..4 {
        started.run.run_owner_turn();
    }
    let answer = request
        .try_result()
        .expect("the window request is answered");
    assert!(
        matches!(answer, Err(AppWindowError::Renderer { .. })),
        "{answer:?}"
    );
    assert_eq!(
        started.run.open_windows().len(),
        1,
        "only the main window is open"
    );
    join_ok_after_close(started);
}

/// A window resized past what the device can draw shows nothing at that
/// size without retrying it every frame, and draws again at a size the
/// device takes.
pub(crate) fn a_window_past_the_device_limit_waits_for_a_size_it_takes() {
    let observed = Observed::default();
    let started = start(&observed);
    let presented = started.renderer.frames_presented();
    started.renderer.limit_size(4096);
    started.window.simulate_resize(8192.0, 600.0);
    started.run.run_owner_turn();
    assert_eq!(
        started.run.wake_deadline(),
        None,
        "no retry of the refused size is armed"
    );
    assert_eq!(
        started.renderer.frames_presented(),
        presented,
        "nothing is drawn at the refused size"
    );
    started.window.simulate_resize(800.0, 600.0);
    started.run.run_owner_turn();
    assert!(
        started.renderer.frames_presented() > presented,
        "a size the device takes draws again"
    );
    join_ok_after_close(started);
}

/// A size a lost device refused is tried again once the device is rebuilt,
/// since the new device may take it, without waiting for a resize.
pub(crate) fn a_rebuilt_device_tries_the_refused_size_again() {
    let observed = Observed::default();
    let started = start(&observed);
    started.renderer.limit_size(4096);
    started.window.simulate_resize(8192.0, 600.0);
    started.run.run_owner_turn();
    let presented = started.renderer.frames_presented();
    // The rebuilt device takes any size; its first rebuild fails, so the
    // window sees the device lost before it sees it back.
    started.renderer.limit_size(i32::MAX);
    started.renderer.lose_device();
    started.renderer.fail_recoveries(1);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    for _ in 0..8 {
        if started.renderer.frames_presented() > presented {
            break;
        }
        let Some(deadline) = started.run.wake_deadline() else {
            break;
        };
        wake_at(&started, deadline);
    }
    assert_eq!(
        started.renderer.recovery_attempts(),
        2,
        "the device was rebuilt"
    );
    assert!(
        started.renderer.frames_presented() > presented,
        "the rebuilt device drew at the size the lost one refused"
    );
    join_ok_after_close(started);
}

/// A panic of the renderer while it resizes a window is reported, and the
/// window still reaches its new size: the frame that met the panic is
/// retried, and the retry applies the size. An earlier frame's panic of
/// another kind does not take that retry away.
pub(crate) fn a_panicking_resize_is_reported_and_the_window_reaches_its_size() {
    let observed = Observed::default();
    let started = start(&observed);
    // A render that panicked: its frame fails, unretried.
    started.renderer.panic_in_next_frames(1);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    let presented = started.renderer.frames_presented();
    started.renderer.panic_in_next_resizes(1);
    started.window.simulate_resize(700.0, 500.0);
    started.run.run_owner_turn();
    assert_eq!(
        observed.frame_panics.load(Ordering::SeqCst),
        2,
        "the render's and the resize's panics were reported to the frame-failure handler"
    );
    for _ in 0..8 {
        if started.renderer.frames_presented() > presented {
            break;
        }
        let Some(deadline) = started.run.wake_deadline() else {
            break;
        };
        wake_at(&started, deadline);
    }
    assert!(
        started.renderer.frames_presented() > presented,
        "the window drew at its new size after the panic"
    );
    join_ok_after_close(started);
}

/// A resize the renderer panics on every time is retried once, not on every
/// frame after it.
pub(crate) fn a_resize_that_keeps_panicking_is_not_retried_forever() {
    let observed = Observed::default();
    let started = start(&observed);
    started.renderer.panic_in_next_resizes(8);
    started.window.simulate_resize(700.0, 500.0);
    started.run.run_owner_turn();
    for _ in 0..4 {
        let Some(deadline) = started.run.wake_deadline() else {
            break;
        };
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        started.run.run_owner_turn();
    }
    assert_eq!(
        observed.frame_panics.load(Ordering::SeqCst),
        2,
        "the resize's panic and its one retry's were reported, and no more"
    );
    assert_eq!(
        started.run.wake_deadline(),
        None,
        "no retry of the panicking resize is armed"
    );
    let presented = started.renderer.frames_presented();
    started.renderer.panic_in_next_resizes(0);
    observed.schedule_rebuild();
    started.run.run_owner_turn();
    assert!(
        started.renderer.frames_presented() > presented,
        "the window's next change draws at its size"
    );
    join_ok_after_close(started);
}

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
                "a_vetoed_close_disposes_nothing",
                a_vetoed_close_disposes_nothing,
            ),
            (
                "a_refused_quit_waits_for_the_record_then_exits",
                a_refused_quit_waits_for_the_record_then_exits,
            ),
            (
                "a_quit_the_window_stays_open_for_is_cancelled",
                a_quit_the_window_stays_open_for_is_cancelled,
            ),
            (
                "a_lost_device_waits_out_its_rebuild_backoff",
                a_lost_device_waits_out_its_rebuild_backoff,
            ),
            (
                "a_given_up_device_renders_again_when_the_application_asks",
                a_given_up_device_renders_again_when_the_application_asks,
            ),
            (
                "two_windows_draw_on_one_device",
                two_windows_draw_on_one_device,
            ),
            (
                "a_panicking_frame_is_reported_and_the_window_draws_again",
                a_panicking_frame_is_reported_and_the_window_draws_again,
            ),
            (
                "a_window_the_gpu_refuses_does_not_open",
                a_window_the_gpu_refuses_does_not_open,
            ),
            (
                "a_window_past_the_device_limit_waits_for_a_size_it_takes",
                a_window_past_the_device_limit_waits_for_a_size_it_takes,
            ),
            (
                "a_rebuilt_device_tries_the_refused_size_again",
                a_rebuilt_device_tries_the_refused_size_again,
            ),
            (
                "a_panicking_resize_is_reported_and_the_window_reaches_its_size",
                a_panicking_resize_is_reported_and_the_window_reaches_its_size,
            ),
            (
                "a_resize_that_keeps_panicking_is_not_retried_forever",
                a_resize_that_keeps_panicking_is_not_retried_forever,
            ),
        ],
    );
}
