//! The runner's teardown: how an application exits after a last window
//! closes, a callback panics or the session ends. Each row runs a whole
//! application on the headless platform through `flui_app::testing`.

use std::cell::RefCell;
use std::rc::Rc;

use flui_app::app::{
    AppConfig, AppHandle, Application, ExitPolicy, StartupWindow, WindowPolicy,
    open_secondary_window,
};
use flui_app::testing::{HeadlessRun, RunExit, TeardownStep};
use flui_platform::{SessionEndAnswer, SessionEndPhase};

/// The order every loop's teardown runs in.
const TEARDOWN_ORDER: [TeardownStep; 5] = [
    TeardownStep::MainWindow,
    TeardownStep::UiRuntimes,
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
    Application::new(|_| -> flui_widgets::Text { panic!("no main window is opened") })
        .with_startup_window(StartupWindow::None)
        .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed))
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

/// A quit the program asks for ends the loop as a closed last window does.
pub(crate) fn a_program_quit_returns_ok() {
    let handle = Rc::new(RefCell::new(None));
    let run = HeadlessRun::start(one_window_app(Rc::clone(&handle)));
    let handle = handle.take().expect("on_ready ran during start");
    handle.request_quit().expect("the loop accepts a quit");
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
        ],
    );
}
