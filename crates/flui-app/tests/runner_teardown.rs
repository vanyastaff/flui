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
/// the loop is torn down after it as after any other end.
pub(crate) fn a_session_end_with_nothing_to_save_proceeds() {
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app(Rc::clone(&handle)));
    assert_eq!(
        run.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Proceed
    );
    run.end_session(SessionEndPhase::Ending);

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
