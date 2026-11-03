//! An app launched without a driver's endpoint pays nothing for the driver agent at run time
//! (ADR-0288): while it runs, no thread of the agent's exists, so no endpoint is bound and no
//! conversation is served. The agent's threads are named `flui-agent-*`.

use std::cell::RefCell;
use std::rc::Rc;

use flui_app::app::{
    AppConfig, Application, ExitPolicy, StartupWindow, WindowPolicy, open_secondary_window,
};
use flui_app::testing::HeadlessRun;
use flui_protocol::launch::ENDPOINT_ENV;

/// The names of this process's threads, as the system reports them.
fn thread_names() -> Vec<String> {
    std::fs::read_dir("/proc/self/task")
        .expect("Linux lists a process's threads")
        .filter_map(|task| std::fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .map(|name| name.trim_end().to_owned())
        .collect()
}

#[test]
fn an_app_launched_without_an_endpoint_starts_no_agent() {
    assert!(
        std::env::var_os(ENDPOINT_ENV).is_none(),
        "run this test without {ENDPOINT_ENV} set: it is the launch it checks"
    );
    let opened = Rc::new(RefCell::new(false));
    let ready = Rc::clone(&opened);
    let application =
        Application::new(|_| -> flui_widgets::Text { panic!("no main window is opened") })
            .with_startup_window(StartupWindow::None)
            .with_config(AppConfig::new().with_exit_policy(ExitPolicy::OnLastWindowClosed))
            .on_ready(move |_| {
                open_secondary_window(AppConfig::new(), WindowPolicy::Isolated)
                    .expect("an isolated window opens on the headless platform");
                ready.replace(true);
            });
    let run = HeadlessRun::start(application);
    run.run_owner_turn();
    assert!(*opened.borrow(), "the app is up with its window");

    let agents: Vec<String> = thread_names()
        .into_iter()
        .filter(|name| name.starts_with("flui-agent"))
        .collect();
    assert!(
        agents.is_empty(),
        "an app launched without an endpoint runs agent threads: {agents:?}"
    );

    for window in run.open_windows() {
        assert!(window.simulate_close(), "the window lets the user close it");
    }
    run.run_owner_turn();
    run.join().expect("an ordinary close returns Ok");
}
