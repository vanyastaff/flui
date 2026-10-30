//! The runner's teardown: how an application exits after a last window
//! closes, a callback panics or the session ends. Each row runs a whole
//! application on the headless platform through `flui_app::testing`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use flui_app::app::{
    AppConfig, AppHandle, Application, ExitPolicy, ServiceDefinition, ServiceLifetime,
    StartupWindow, WindowPolicy, open_secondary_window,
};
use flui_app::testing::{HeadlessRun, RunExit, TeardownStep};
use flui_platform::{SessionEndAnswer, SessionEndPhase};

/// The order every loop's teardown runs in.
const TEARDOWN_ORDER: [TeardownStep; 6] = [
    TeardownStep::MainWindow,
    TeardownStep::UiRuntimes,
    TeardownStep::Flush,
    TeardownStep::Services,
    TeardownStep::Execution,
    TeardownStep::NativeResources,
];

/// An application whose one window, opened when it is ready, holds a UI
/// runtime and needs no GPU, and which exits when its last window closes.
/// It hands its control to `ready`.
fn one_window_app(
    ready: Rc<RefCell<Option<AppHandle>>>,
) -> Application<flui_widgets::Text, impl FnMut(&AppHandle) -> flui_widgets::Text> {
    one_window_app_with(AppConfig::new(), ready)
}

/// [`one_window_app`] with `config`'s services.
fn one_window_app_with(
    config: AppConfig,
    ready: Rc<RefCell<Option<AppHandle>>>,
) -> Application<flui_widgets::Text, impl FnMut(&AppHandle) -> flui_widgets::Text> {
    Application::new(|_| -> flui_widgets::Text { panic!("no main window is opened") })
        .with_startup_window(StartupWindow::None)
        .with_config(config.with_exit_policy(ExitPolicy::OnLastWindowClosed))
        .on_ready(move |handle| {
            open_secondary_window(AppConfig::new(), WindowPolicy::Isolated)
                .expect("an isolated window opens on the headless platform");
            ready.replace(Some(handle.clone()));
        })
}

/// Closing the last window, as the user does, ends the loop: the run returns
/// `Ok`, exits with code 0 and tears down every step in order.
pub(crate) fn closing_the_last_window_returns_ok() {
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app(Rc::clone(&handle)));
    let windows = run.open_windows();
    assert_eq!(windows.len(), 1, "the ready callback opened one window");
    assert!(windows[0].simulate_close(), "the application lets it close");
    run.run_owner_turn();
    assert!(
        !run.is_running(),
        "the last window's close quit the platform"
    );

    let report = run.join().expect("an ordinary close returns Ok");
    assert_eq!(report.exit, RunExit::Returned);
    assert_eq!(report.code(), 0);
    assert_eq!(report.teardown, TEARDOWN_ORDER);
}

/// A quit the program asks for, which no window refuses, ends the loop as a
/// closed last window does.
pub(crate) fn a_program_quit_returns_ok() {
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app(Rc::clone(&handle)));
    let handle = handle.take().expect("on_ready ran during start");
    handle.request_quit().expect("the loop accepts a quit");
    // The application's turn asks the windows, which refuse nothing; the
    // platform quits on the turn after.
    run.run_owner_turn();
    run.run_owner_turn();
    assert!(!run.is_running(), "the quit reached the platform");

    let report = run.join().expect("a program quit returns Ok");
    assert_eq!(report.code(), 0);
    assert_eq!(report.teardown, TEARDOWN_ORDER);
}

/// A run joined while its loop still runs is torn down all the same.
pub(crate) fn joining_a_running_loop_tears_it_down() {
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app(Rc::clone(&handle)));
    assert!(run.is_running());

    let report = run.join().expect("teardown of a running loop returns Ok");
    assert_eq!(report.code(), 0);
    assert_eq!(report.teardown, TEARDOWN_ORDER);
}

/// A session end the application has no reason to hold up proceeds, and
/// the loop is asked to quit (ADR-0260 §6) and torn down as after any other
/// end.
pub(crate) fn a_session_end_with_nothing_to_save_proceeds() {
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app(Rc::clone(&handle)));
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Proceed
    );
    run.end_session(SessionEndPhase::Ending);
    run.run_owner_turn();
    assert!(!run.is_running(), "the session's end quit the loop");

    let report = run.join().expect("teardown after a session end returns Ok");
    assert_eq!(report.teardown, TEARDOWN_ORDER);
}

/// How long a job blocks that never looks at its cancellation.
const HUNG: Duration = Duration::from_secs(30);

/// A service that ignores its cancellation and a compute job that never
/// returns cost the exit no more than its budget: the services and the
/// pools share one deadline, six seconds from the flush (ADR-0258 §7),
/// instead of five seconds each.
pub(crate) fn a_hung_service_and_compute_job_end_within_the_exit_budget() {
    let hung = ServiceDefinition::new("hung", ServiceLifetime::StopsWithLastWindow, |context| {
        let job = context
            .spawner()
            .spawn_compute("hung compute", |_| std::thread::sleep(HUNG))
            .expect("the compute lane takes the job");
        Box::pin(async move {
            // Blocks its IO worker without an await point, so neither the
            // service's cancellation nor the pool's reaches it.
            std::thread::sleep(HUNG);
            drop(job);
        })
    });
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app_with(
        AppConfig::new().with_service(hung),
        Rc::clone(&handle),
    ));
    // The service and its job reach their workers before the exit starts.
    std::thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    let report = run.join().expect("an exit past hung work returns Ok");
    let took = started.elapsed();
    assert_eq!(report.teardown, TEARDOWN_ORDER);
    // Six seconds of budget and one of slack; the old per-step graces took
    // ten or more.
    assert!(
        took < Duration::from_secs(7),
        "the exit took {took:?}, past the six-second budget"
    );
}
#[test]
fn runner_teardown() {
    crate::run_table(
        "runner_teardown",
        &[
            (
                "closing_the_last_window_returns_ok",
                closing_the_last_window_returns_ok,
            ),
            ("a_program_quit_returns_ok", a_program_quit_returns_ok),
            (
                "joining_a_running_loop_tears_it_down",
                joining_a_running_loop_tears_it_down,
            ),
            (
                "a_session_end_with_nothing_to_save_proceeds",
                a_session_end_with_nothing_to_save_proceeds,
            ),
            (
                "a_hung_service_and_compute_job_end_within_the_exit_budget",
                a_hung_service_and_compute_job_end_within_the_exit_budget,
            ),
            (
                "teardown_panic_flushes_the_registry_before_resuming",
                teardown_panic_flushes_the_registry_before_resuming,
            ),
            (
                "closing_during_composition_commits_before_detached",
                closing_during_composition_commits_before_detached,
            ),
            (
                "a_session_end_query_asks_each_window_guard",
                a_session_end_query_asks_each_window_guard,
            ),
            (
                "a_cancelled_logoff_withdraws_only_the_session_entry",
                a_cancelled_logoff_withdraws_only_the_session_entry,
            ),
            (
                "a_session_end_with_a_checked_out_runtime_calls_no_user_code",
                a_session_end_with_a_checked_out_runtime_calls_no_user_code,
            ),
            (
                "a_session_end_asks_the_handler_before_the_guard",
                a_session_end_asks_the_handler_before_the_guard,
            ),
            (
                "a_refused_session_end_is_released_when_its_window_closes",
                a_refused_session_end_is_released_when_its_window_closes,
            ),
            (
                "a_refused_session_end_is_released_when_an_earlier_close_carries_out",
                a_refused_session_end_is_released_when_an_earlier_close_carries_out,
            ),
            (
                "session_end_commits_composition_before_the_flush",
                session_end_commits_composition_before_the_flush,
            ),
            (
                "a_notify_only_session_end_leaves_the_application_running",
                a_notify_only_session_end_leaves_the_application_running,
            ),
            (
                "session_end_skips_detached_once_the_notice_spent_the_margin",
                session_end_skips_detached_once_the_notice_spent_the_margin,
            ),
            (
                "a_repeated_session_end_query_does_not_release_the_session",
                a_repeated_session_end_query_does_not_release_the_session,
            ),
            (
                "a_slow_detached_observer_stops_the_session_end_delivery",
                a_slow_detached_observer_stops_the_session_end_delivery,
            ),
            (
                "a_session_end_handler_panic_with_a_panicking_payload_refuses",
                a_session_end_handler_panic_with_a_panicking_payload_refuses,
            ),
        ],
    );
}

/// Panics when dropped, as an application value whose destructor fails.
struct PanicsOnDrop(&'static str);

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        panic!("{}", self.0);
    }
}

/// A destructor that panics at the exit is terminal: the exit still writes
/// the flush registry, stops the services and pools, and only then resumes
/// the panic, which ends the process with 101 and the panic's text
/// (ADR-0258 §2, §3).
pub(crate) fn teardown_panic_flushes_the_registry_before_resuming() {
    let capture = PanicsOnDrop("the factory's capture failed to drop");
    let application = Application::new(move |_| -> flui_widgets::Text {
        let _held = &capture;
        panic!("no main window is opened")
    })
    .with_startup_window(StartupWindow::None);
    let run = HeadlessRun::start(application);

    let report = run.join().expect("a terminal panic is no run error");
    assert_eq!(
        report.exit,
        RunExit::Panicked {
            message: "the factory's capture failed to drop".to_owned()
        }
    );
    assert_eq!(report.code(), flui_app::testing::EXIT_CODE_PANIC);
    assert_eq!(
        report.teardown, TEARDOWN_ORDER,
        "every exit step, the flush first among the framework's, ran before the panic resumed"
    );
}

/// Names the row a child process of this test binary runs.
const CHILD_CASE: &str = "FLUI_RUNNER_CHILD_CASE";

/// Blocks forever when dropped, as an application destructor that waits on
/// something that never comes.
struct BlocksForever;

impl Drop for BlocksForever {
    fn drop(&mut self) {
        loop {
            std::thread::park();
        }
    }
}

/// An application destructor that never returns cannot hold the process: the
/// exit watchdog ends it with `EXIT_CODE_HUNG` (ADR-0258 §7). Runs in the
/// child; the parent reads the code.
fn a_blocking_destructor_ends_the_process_with_the_hung_code() {
    let blocker = BlocksForever;
    let ready = Rc::new(RefCell::new(None));
    let installed = Rc::clone(&ready);
    let application = Application::new(move |_| -> flui_widgets::Text {
        let _held = &blocker;
        panic!("no main window is opened")
    })
    .with_startup_window(StartupWindow::None)
    .on_ready(move |handle| {
        installed.replace(Some(handle.clone()));
    });
    let run = HeadlessRun::start(application);
    // The main window's shutdown drops the factory, and with it the blocker.
    let _ = run.join();
    panic!("the exit returned past a destructor that never returns");
}

/// Rows whose outcome is the process's exit code, each run in a child
/// process of this test binary.
#[test]
fn runner_exit_codes() {
    if let Ok(case) = std::env::var(CHILD_CASE) {
        match case.as_str() {
            "a_blocking_destructor_ends_the_process_with_the_hung_code" => {
                a_blocking_destructor_ends_the_process_with_the_hung_code();
            }
            other => panic!("no child row named {other}"),
        }
        return;
    }
    let started = Instant::now();
    let mut child = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "runner_teardown::runner_exit_codes",
            "--nocapture",
        ])
        .env(
            CHILD_CASE,
            "a_blocking_destructor_ends_the_process_with_the_hung_code",
        )
        .env("RUST_BACKTRACE", "0")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the child test process starts");
    // Past this the child is taken as hung: the old exit never ended.
    let limit = Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().expect("the child's status") {
            break status;
        }
        if started.elapsed() > limit {
            child.kill().expect("the hung child is killed");
            child.wait().expect("the killed child is reaped");
            panic!("the child's exit still ran after {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let took = started.elapsed();
    assert_eq!(
        status.code(),
        Some(flui_app::EXIT_CODE_HUNG),
        "the child ended {status:?} after {took:?}"
    );
    // Fifteen seconds of watchdog, one of slack and the child's start.
    assert!(
        took < Duration::from_secs(18),
        "the watchdog ended the exit after {took:?}"
    );
}

/// What a [`ComposingField`] saw of its store when its window's Detached
/// arrived: whether it was still composing, and its text.
type SeenAtDetached = Rc<RefCell<Option<(bool, String)>>>;

/// A field that attaches `store` to its window's text input and records the
/// store as its window's Detached observer sees it.
#[derive(Clone)]
struct ComposingField {
    store: Rc<flui_interaction::testing::InMemoryTextStore>,
    seen: SeenAtDetached,
    /// The loop of the run, when the test asks whether it still ran at
    /// Detached; what it answered then.
    loop_at_detached: Option<(
        flui_native::HeadlessOwnerTurns,
        Rc<std::cell::Cell<Option<bool>>>,
    )>,
    /// How long the field's session-end notice subscriber blocks.
    notice_delay: Duration,
}

struct ComposingFieldState {
    field: ComposingField,
    token: Option<flui_interaction::ClientToken>,
    subscription: Option<flui_view::LifecycleSubscription>,
    notices: Option<flui_view::HostNoticeSubscription>,
}

impl flui_view::StatefulView for ComposingField {
    type State = ComposingFieldState;

    fn create_state(&self) -> Self::State {
        ComposingFieldState {
            field: self.clone(),
            token: None,
            subscription: None,
            notices: None,
        }
    }
}

impl flui_view::ViewState<ComposingField> for ComposingFieldState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let store: Rc<dyn flui_platform::TextStore> = self.field.store.clone();
        self.token = Some(
            ctx.text_input_handle()
                .expect("the window takes text input")
                .attach(flui_interaction::TextInputClient::new(store))
                .expect("the field attaches"),
        );
        let (store, seen) = (Rc::clone(&self.field.store), Rc::clone(&self.field.seen));
        let at_detached = self.field.loop_at_detached.clone();
        let (_, subscription) = ctx
            .lifecycle_handle()
            .expect("a bound lifecycle")
            .subscribe(move |state| {
                if state == flui_view::AppLifecycleState::Detached {
                    seen.replace(Some((store.composition().is_some(), store.text())));
                    if let Some((turns, running)) = &at_detached {
                        running.set(Some(turns.is_running()));
                    }
                }
            })
            .expect("the lifecycle is open");
        self.subscription = Some(subscription);
        let delay = self.field.notice_delay;
        self.notices = Some(
            ctx.lifecycle_handle()
                .expect("a bound lifecycle")
                .subscribe_notices(move |notice| {
                    if matches!(notice, flui_platform::HostNotice::SessionEnded) {
                        std::thread::sleep(delay);
                    }
                })
                .expect("the lifecycle is open"),
        );
    }

    fn build(
        &self,
        _view: &ComposingField,
        _ctx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(10.0)
    }
}

impl flui_view::View for ComposingField {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// R21: a window closed while its focused field composes commits the
/// composition, keeping its text, before the window's Detached observers
/// run, so what they save holds the preedit as text (ADR-0142 item 4).
///
/// Red-check: drop the commit from the owner's close — the observer sees the
/// store still composing.
pub(crate) fn closing_during_composition_commits_before_detached() {
    use flui_platform::text_store::{
        Composition, LockGrant, LockOutcome, LockTiming, TextStore, Utf16Offset, Utf16Range,
    };

    let store = flui_interaction::testing::InMemoryTextStore::new("ab");
    let seen: SeenAtDetached = Rc::new(RefCell::new(None));
    let field = ComposingField {
        store: Rc::clone(&store),
        seen: Rc::clone(&seen),
        loop_at_detached: None,
        notice_delay: Duration::ZERO,
    };
    let application = Application::new(move |_| field.clone())
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed));
    let platform = flui_native::HeadlessPlatform::new().with_text_store_host(|| {
        flui_testing::RecordingTextStoreHost::new()
            as Rc<dyn flui_platform::text_store::TextStoreHost>
    });
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();
    let composing = Composition {
        range: Utf16Range::try_new(Utf16Offset::new(2), Utf16Offset::new(4)).expect("ordered"),
        hides_caret: false,
    };
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(move |session| {
                session.insert_at_selection("東京").expect("in range");
                session.set_composition(Some(composing)).expect("in range");
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(store.composition(), Some(composing), "precondition");

    let windows = run.open_windows();
    assert_eq!(windows.len(), 1);
    assert!(windows[0].simulate_close(), "the application lets it close");
    run.run_owner_turn();

    assert_eq!(
        *seen.borrow(),
        Some((false, "ab東京".to_owned())),
        "Detached saw the composition committed, its text kept"
    );
    run.join().expect("the close returns Ok");
}

/// Which hold a [`GuardedView`] takes on its window's close.
#[derive(Clone, Copy, Debug)]
enum HoldOn {
    Nothing,
    /// [`flui_view::CloseGuard::hold`]: work that finishes by itself.
    Work,
    /// [`flui_view::CloseGuard::require_decision`]: a state only the user
    /// resolves.
    Decision,
}

/// What a [`GuardedView`] shares with its test.
#[derive(Default)]
struct Guarded {
    guard: Option<flui_view::CloseGuard>,
    hold: Option<flui_view::CloseHold>,
    rebuild: Option<flui_view::RebuildHandle>,
    /// Run once from the next `build`, while the window's UI runtime is
    /// checked out to its frame.
    during_build: Option<Box<dyn FnOnce()>>,
}

/// A window root that takes its close guard and the hold `hold_on` names.
#[derive(Clone)]
struct GuardedView {
    hold_on: HoldOn,
    shared: Rc<RefCell<Guarded>>,
}

struct GuardedViewState(GuardedView);

impl flui_view::StatefulView for GuardedView {
    type State = GuardedViewState;

    fn create_state(&self) -> Self::State {
        GuardedViewState(self.clone())
    }
}

impl flui_view::ViewState<GuardedView> for GuardedViewState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let guard = ctx
            .close_guard()
            .expect("a runner window has a close guard");
        let hold = match self.0.hold_on {
            HoldOn::Nothing => None,
            HoldOn::Work => Some(guard.hold()),
            HoldOn::Decision => Some(guard.require_decision()),
        };
        let mut shared = self.0.shared.borrow_mut();
        shared.guard = Some(guard);
        shared.hold = hold;
        shared.rebuild = Some(ctx.rebuild_handle());
    }

    fn build(
        &self,
        _view: &GuardedView,
        _ctx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        let during = self.0.shared.borrow_mut().during_build.take();
        if let Some(during) = during {
            during();
        }
        flui_widgets::SizedBox::square(10.0)
    }
}

impl flui_view::View for GuardedView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// A run whose main window is a [`GuardedView`] holding `hold_on`, mounted,
/// with the platform's session-end reports.
fn guarded_run(
    hold_on: HoldOn,
) -> (
    flui_app::testing::HeadlessRunHandle,
    flui_native::HeadlessHostEvents,
    Rc<RefCell<Guarded>>,
) {
    guarded_run_with(hold_on, AppConfig::new())
}

/// [`guarded_run`] with `config`'s close-request handler.
fn guarded_run_with(
    hold_on: HoldOn,
    config: AppConfig,
) -> (
    flui_app::testing::HeadlessRunHandle,
    flui_native::HeadlessHostEvents,
    Rc<RefCell<Guarded>>,
) {
    let shared = Rc::new(RefCell::new(Guarded::default()));
    let view = GuardedView {
        hold_on,
        shared: Rc::clone(&shared),
    };
    let application = Application::new(move |_| view.clone())
        .with_config(config.with_exit_policy(ExitPolicy::OnLastWindowClosed));
    let platform = flui_native::HeadlessPlatform::new();
    let events = platform.host_events();
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();
    assert!(shared.borrow().guard.is_some(), "the window is mounted");
    (run, events, shared)
}

/// The reason of the close the window's guard has recorded first, if any.
fn pending_reason(shared: &RefCell<Guarded>) -> Option<flui_view::CloseReason> {
    shared
        .borrow()
        .guard
        .as_ref()
        .expect("mounted")
        .pending()
        .map(|pending| pending.reason())
}

/// A session-end query asks every window's guard with
/// `CloseReason::SessionEnd` (ADR-0260 §1): a hold for work that finishes by
/// itself lets the session end, since the flush writes what it protects; a
/// hold that waits for the user's decision blocks it and records the close.
///
/// Red-check: answer the query from the notice alone — the decision hold's
/// row proceeds and records nothing.
pub(crate) fn a_session_end_query_asks_each_window_guard() {
    for (hold_on, answer, recorded) in [
        (HoldOn::Nothing, SessionEndAnswer::Proceed, None),
        (HoldOn::Work, SessionEndAnswer::Proceed, None),
        (
            HoldOn::Decision,
            SessionEndAnswer::Block,
            Some(flui_view::CloseReason::SessionEnd),
        ),
    ] {
        let (run, _events, shared) = guarded_run(hold_on);
        assert_eq!(
            run.end_session(SessionEndPhase::Query),
            answer,
            "{hold_on:?}"
        );
        assert_eq!(pending_reason(&shared), recorded, "{hold_on:?}");
        run.end_session(SessionEndPhase::Cancelled);
        let hold = shared.borrow_mut().hold.take();
        drop(hold);
        run.join().expect("the run returns Ok");
    }
}

/// A cancelled log-off withdraws the session-end close the query recorded,
/// and nothing else: a close the user asked for earlier stays pending
/// (ADR-0260 §2).
pub(crate) fn a_cancelled_logoff_withdraws_only_the_session_entry() {
    let (run, _events, shared) = guarded_run(HoldOn::Decision);
    let windows = run.open_windows();
    assert!(!windows[0].simulate_close(), "the decision hold keeps it");
    run.run_owner_turn();
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block
    );
    run.end_session(SessionEndPhase::Cancelled);
    run.run_owner_turn();

    let pending = shared
        .borrow()
        .guard
        .as_ref()
        .expect("mounted")
        .pending()
        .expect("the user's close stays pending");
    assert_eq!(pending.reason(), flui_view::CloseReason::User);
    pending.stay_open().expect("the user's close is current");
    assert_eq!(
        pending_reason(&shared),
        None,
        "the session end's close was withdrawn"
    );
    let hold = shared.borrow_mut().hold.take();
    drop(hold);
    run.join().expect("the run returns Ok");
}

/// A session end that arrives while a window's UI runtime is checked out (a
/// nested native loop inside its frame) asks none of its code: the runtime
/// counts as letting the session end, and its guard records nothing
/// (ADR-0260 §1). Asked again once the frame is over, the same hold blocks.
///
/// Red-check: ask a checked-out runtime as well — the query from inside the
/// frame blocks and records the session end.
pub(crate) fn a_session_end_with_a_checked_out_runtime_calls_no_user_code() {
    let (run, events, shared) = guarded_run(HoldOn::Decision);
    let inside = Rc::new(RefCell::new(None));
    let answered = Rc::clone(&inside);
    let state = Rc::clone(&shared);
    shared.borrow_mut().during_build = Some(Box::new(move || {
        let answer = events.session_end(SessionEndPhase::Query);
        answered.replace(Some((answer, pending_reason(&state))));
    }));
    let rebuild = shared.borrow().rebuild.clone().expect("mounted");
    rebuild.schedule(flui_view::RebuildReason::StateChange);
    run.run_owner_turn();
    assert_eq!(
        inside.take(),
        Some((SessionEndAnswer::Proceed, None)),
        "the checked-out runtime was not asked"
    );

    run.end_session(SessionEndPhase::Cancelled);
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block,
        "back in its slot, the runtime is asked"
    );
    run.end_session(SessionEndPhase::Cancelled);
    let hold = shared.borrow_mut().hold.take();
    drop(hold);
    run.join().expect("the run returns Ok");
}

/// A session-end query asks the window's close handler before its guard
/// (ADR-0260 §1), so a handler can start the save or put up the decision even
/// while a decision hold refuses.
///
/// Red-check: consult the guard first — a decision hold refuses before the
/// handler is asked, and the handler hears nothing.
pub(crate) fn a_session_end_asks_the_handler_before_the_guard() {
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&asked);
    let handler = flui_app::app::CloseRequestHandler::new(move |request| {
        assert_eq!(request.reason(), flui_view::CloseReason::SessionEnd);
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        flui_app::app::CloseResponse::Close
    });
    let (run, _events, shared) = guarded_run_with(
        HoldOn::Decision,
        AppConfig::new().with_close_request_handler(handler),
    );
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block,
        "the decision hold still refuses"
    );
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    run.end_session(SessionEndPhase::Cancelled);
    let hold = shared.borrow_mut().hold.take();
    drop(hold);
    run.join().expect("the run returns Ok");
}

/// A window that refused a session end holds it until the window carries the
/// close out: then, with nothing else holding it, the platform is released
/// (ADR-0260 §1, §6).
///
/// Red-check: answer the refusal without a block on the session end's
/// request — the platform is never released.
pub(crate) fn a_refused_session_end_is_released_when_its_window_closes() {
    let (run, events, shared) = guarded_run(HoldOn::Decision);
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block
    );
    run.run_owner_turn();
    assert_eq!(events.session_end_releases(), 0, "the window still refuses");

    let pending = shared
        .borrow()
        .guard
        .as_ref()
        .expect("mounted")
        .pending()
        .expect("the session end's close is pending");
    assert_eq!(pending.reason(), flui_view::CloseReason::SessionEnd);
    pending
        .discard_and_close()
        .expect("the session end's close is current");
    run.run_owner_turn();
    assert_eq!(
        events.session_end_releases(),
        1,
        "the carried-out close released the session end"
    );
    run.end_session(SessionEndPhase::Ending);
    let hold = shared.borrow_mut().hold.take();
    drop(hold);
    run.join().expect("the run returns Ok");
}

/// A window whose user close was already pending when it refused the
/// session end releases the session when its decision carries that earlier
/// close out: the window is gone, whichever record the guard reports
/// (ADR-0260 §1).
///
/// Red-check: release a refusal only for a close reported as `SessionEnd` —
/// the platform stays held after the window closed.
pub(crate) fn a_refused_session_end_is_released_when_an_earlier_close_carries_out() {
    let (run, events, shared) = guarded_run(HoldOn::Decision);
    let windows = run.open_windows();
    assert!(
        !windows[0].simulate_close(),
        "the decision keeps the window"
    );
    assert_eq!(pending_reason(&shared), Some(flui_view::CloseReason::User));
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block
    );
    run.run_owner_turn();
    assert_eq!(events.session_end_releases(), 0, "the window still refuses");

    let hold = shared.borrow_mut().hold.take();
    drop(hold);
    run.run_owner_turn();
    assert_eq!(
        events.session_end_releases(),
        1,
        "the user's close carried out released the session end"
    );
    run.join().expect("the run returns Ok");
}

/// The session's end commits a field's composition and tells its Detached
/// observers before the flush writes, so what they publish holds the
/// preedit as text (ADR-0260 §3).
///
/// Red-check: skip the close delivery in the ending phase — the observer
/// hears Detached only from the teardown its quit starts.
pub(crate) fn session_end_commits_composition_before_the_flush() {
    use flui_platform::text_store::{
        Composition, LockGrant, LockOutcome, LockTiming, TextStore, Utf16Offset, Utf16Range,
    };

    let store = flui_interaction::testing::InMemoryTextStore::new("ab");
    let seen: SeenAtDetached = Rc::new(RefCell::new(None));
    let platform = flui_native::HeadlessPlatform::new().with_text_store_host(|| {
        flui_testing::RecordingTextStoreHost::new()
            as Rc<dyn flui_platform::text_store::TextStoreHost>
    });
    let running = Rc::new(std::cell::Cell::new(None));
    let field = ComposingField {
        store: Rc::clone(&store),
        seen: Rc::clone(&seen),
        loop_at_detached: Some((platform.owner_turns(), Rc::clone(&running))),
        notice_delay: Duration::ZERO,
    };
    let application = Application::new(move |_| field.clone())
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed));
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();
    let composing = Composition {
        range: Utf16Range::try_new(Utf16Offset::new(2), Utf16Offset::new(4)).expect("ordered"),
        hides_caret: false,
    };
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(move |session| {
                session.insert_at_selection("東京").expect("in range");
                session.set_composition(Some(composing)).expect("in range");
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );

    run.end_session(SessionEndPhase::Ending);
    assert_eq!(
        *seen.borrow(),
        Some((false, "ab東京".to_owned())),
        "Detached saw the composition committed before the phase returned"
    );
    assert_eq!(
        running.get(),
        Some(true),
        "Detached came from the session end's delivery, before its quit"
    );
    run.join().expect("the session end returns Ok");
}

/// On a platform that only tells (an Android stop, a web pagehide), the
/// session's end writes what the documents published but leaves the
/// application up: the user may come back to the same windows, so they hear
/// no Detached and the loop does not quit (ADR-0260 §3).
///
/// Red-check: deliver the close and ask the loop to quit whatever the
/// platform's support — the observer hears Detached and the loop ends.
pub(crate) fn a_notify_only_session_end_leaves_the_application_running() {
    let seen: SeenAtDetached = Rc::new(RefCell::new(None));
    let field = ComposingField {
        store: flui_interaction::testing::InMemoryTextStore::new("ab"),
        seen: Rc::clone(&seen),
        loop_at_detached: None,
        notice_delay: Duration::ZERO,
    };
    let application = Application::new(move |_| field.clone())
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed));
    let platform = flui_native::HeadlessPlatform::new()
        .with_session_end_support(flui_platform::SessionEndSupport::NotifyOnly)
        .with_text_store_host(|| {
            flui_testing::RecordingTextStoreHost::new()
                as Rc<dyn flui_platform::text_store::TextStoreHost>
        });
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();

    assert_eq!(
        run.end_session(SessionEndPhase::Ending),
        SessionEndAnswer::Proceed
    );
    run.run_owner_turn();
    assert!(
        run.is_running(),
        "the application stays up for the user's return"
    );
    assert_eq!(*seen.borrow(), None, "no window heard Detached");

    let windows = run.open_windows();
    assert_eq!(windows.len(), 1, "the window is still open");
    assert!(windows[0].simulate_close(), "the application lets it close");
    run.run_owner_turn();
    run.join()
        .expect("closing the window afterwards returns Ok");
}

/// How long the session end's notice subscriber blocks in
/// [`session_end_skips_detached_once_the_notice_spent_the_margin`]: past the
/// moment less than one second of the budget remains.
const SPENT_MARGIN: Duration = Duration::from_millis(2_500);

/// A session-end notice subscriber that spends the margin for application code
/// keeps the close delivery from starting: the flush still has the rest of
/// the budget (ADR-0260 §3).
///
/// Red-check: check the margin only before the notice — the field's Detached
/// observer runs while the loop still runs.
pub(crate) fn session_end_skips_detached_once_the_notice_spent_the_margin() {
    let seen: SeenAtDetached = Rc::new(RefCell::new(None));
    let platform = flui_native::HeadlessPlatform::new().with_text_store_host(|| {
        flui_testing::RecordingTextStoreHost::new()
            as Rc<dyn flui_platform::text_store::TextStoreHost>
    });
    let running = Rc::new(std::cell::Cell::new(None));
    let field = ComposingField {
        store: flui_interaction::testing::InMemoryTextStore::new("ab"),
        seen: Rc::clone(&seen),
        loop_at_detached: Some((platform.owner_turns(), Rc::clone(&running))),
        notice_delay: SPENT_MARGIN,
    };
    let application = Application::new(move |_| field.clone())
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed));
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();

    run.end_session(SessionEndPhase::Ending);
    assert_eq!(
        running.get(),
        Some(false),
        "Detached came only from the teardown the quit started, not from the \
         session end's delivery"
    );
    run.join().expect("the session end returns Ok");
}

/// A second session-end query while a window still refuses the first one
/// replaces it without telling the platform the session was released: the
/// displaced request's blocks drop, but the platform waits on the new one
/// (ADR-0260 §1).
///
/// Red-check: let the displaced request's release reach the platform — it
/// counts one release while the window still refuses.
pub(crate) fn a_repeated_session_end_query_does_not_release_the_session() {
    let (run, events, shared) = guarded_run(HoldOn::Decision);
    for _ in 0..2 {
        assert_eq!(
            run.end_session(SessionEndPhase::Query),
            SessionEndAnswer::Block
        );
        run.run_owner_turn();
    }
    assert_eq!(
        events.session_end_releases(),
        0,
        "the window still refuses the session end"
    );
    run.end_session(SessionEndPhase::Cancelled);
    let hold = shared.borrow_mut().hold.take();
    drop(hold);
    run.join().expect("the run returns Ok");
}

/// A window root whose Detached observer records whether the loop still ran
/// and then blocks for `delay`, as a slow save does.
#[derive(Clone)]
struct SlowDetached {
    turns: flui_native::HeadlessOwnerTurns,
    running: Rc<std::cell::Cell<Option<bool>>>,
    delay: Duration,
}

struct SlowDetachedState {
    root: SlowDetached,
    subscription: Option<flui_view::LifecycleSubscription>,
}

impl flui_view::StatefulView for SlowDetached {
    type State = SlowDetachedState;

    fn create_state(&self) -> Self::State {
        SlowDetachedState {
            root: self.clone(),
            subscription: None,
        }
    }
}

impl flui_view::ViewState<SlowDetached> for SlowDetachedState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        let root = self.root.clone();
        let (_, subscription) = ctx
            .lifecycle_handle()
            .expect("a bound lifecycle")
            .subscribe(move |state| {
                if state == flui_view::AppLifecycleState::Detached && root.running.get().is_none() {
                    root.running.set(Some(root.turns.is_running()));
                    std::thread::sleep(root.delay);
                }
            })
            .expect("the lifecycle is open");
        self.subscription = Some(subscription);
    }

    fn build(
        &self,
        _view: &SlowDetached,
        _ctx: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::SizedBox::square(10.0)
    }
}

impl flui_view::View for SlowDetached {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// A slow Detached observer in one UI runtime keeps the next runtime's
/// observers from starting once less than the margin is left: the flush
/// keeps the rest of the budget (ADR-0260 §3).
///
/// Red-check: check the margin only once before the close delivery — both
/// windows' observers run while the loop still runs.
pub(crate) fn a_slow_detached_observer_stops_the_session_end_delivery() {
    let platform = flui_native::HeadlessPlatform::new();
    let windows: [Rc<std::cell::Cell<Option<bool>>>; 2] = Default::default();
    let root = |running: &Rc<std::cell::Cell<Option<bool>>>| SlowDetached {
        turns: platform.owner_turns(),
        running: Rc::clone(running),
        delay: SPENT_MARGIN,
    };
    let (first, second) = (root(&windows[0]), root(&windows[1]));
    let application = Application::new(move |_| first.clone())
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed))
        .on_ready(move |_| {
            drop(
                flui_app::open_window(AppConfig::new(), WindowPolicy::Isolated, second)
                    .expect("an isolated window opens on the headless platform"),
            );
        });
    let run = HeadlessRun::start_on(application, platform);
    run.run_owner_turn();
    assert_eq!(run.open_windows().len(), 2, "both windows are open");

    run.end_session(SessionEndPhase::Ending);
    let delivered = windows
        .iter()
        .filter(|running| running.get() == Some(true))
        .count();
    assert_eq!(
        delivered, 1,
        "only the first window heard Detached from the session end's delivery"
    );
    run.join().expect("the session end returns Ok");
}

/// A close handler that panics on a session end refuses it, even when the
/// panic's payload panics again as it is dropped: the payload is kept, so
/// the answer stays `Block` (ADR-0260 §1).
///
/// Red-check: drop the payload where the panic is caught — the second panic
/// escapes the query and no answer is given.
pub(crate) fn a_session_end_handler_panic_with_a_panicking_payload_refuses() {
    let handler = flui_app::app::CloseRequestHandler::new(|_| {
        std::panic::panic_any(PanicsOnDrop("the handler's payload panicked on drop"))
    });
    let (run, _events, _shared) = guarded_run_with(
        HoldOn::Nothing,
        AppConfig::new().with_close_request_handler(handler),
    );
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block,
        "the panicking handler refused the session end"
    );
    run.end_session(SessionEndPhase::Cancelled);
    run.join().expect("the run returns Ok");
}
