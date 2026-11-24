//! `InspectorCounters` installed the way an application installs a development tree observer:
//! through `AppConfig::dev_tree_observer`, with the application run on the headless platform by
//! the same runner a desktop run uses.
#![cfg(all(
    feature = "inspector",
    not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
))]

use std::sync::{Arc, Mutex};

use flui_app::testing::HeadlessRun;
use flui_app::{AppConfig, Application, DiagnosticsProfile};
use flui_devtools::InspectorCounters;
use flui_sdk::foundation::UiRuntimeId;
use flui_sdk::foundation::observe::TreeObserver;
use flui_sdk::widgets::SizedBox;
use flui_testing::log_capture::capture;

/// The runner asks the factory once for its window's runtime, and the counters it returns see
/// that runtime's mounts; the run's end retires the runtime and ends their stream.
///
/// Red-check: take the runner's `install_dev_tree_observer` call out and the factory is never
/// asked and nothing is counted.
#[test]
fn an_apps_dev_tree_observer_counts_its_runtimes_mounts() {
    let counters = Arc::new(InspectorCounters::new());
    let asked: Arc<Mutex<Vec<UiRuntimeId>>> = Arc::default();
    let config = quiet().dev_tree_observer({
        let counters = Arc::clone(&counters);
        let asked = Arc::clone(&asked);
        move |runtime| {
            asked.lock().expect("the test's record").push(runtime);
            Arc::clone(&counters) as Arc<dyn TreeObserver>
        }
    });

    let run = HeadlessRun::start(Application::new().config(config), SizedBox::new(32.0, 32.0));
    run.run_owner_turn();
    let running = counters.snapshot();
    assert_eq!(
        asked.lock().expect("the test's record").len(),
        1,
        "one window, one runtime, one call"
    );
    assert!(
        running.mounts > 0,
        "the window's tree mounted under the observer: {running:?}"
    );
    assert!(!running.is_final(), "the runtime still runs");

    run.join().expect("the run ends cleanly");
    assert!(
        counters.snapshot().is_final(),
        "the retired runtime ended the observer's stream"
    );
}

/// A factory that panics is logged, and the runtime it was asked for runs on without an
/// observer: its window still presents frames and the run ends cleanly.
///
/// Red-check: take the runner's `install_dev_tree_observer` call out and no failure is logged;
/// call the factory outside containment and the run fails.
#[test]
fn a_panicking_dev_tree_observer_factory_leaves_the_runtime_running() {
    let config = quiet().dev_tree_observer(|_| panic!("the observer factory failed"));

    let (presented, log) = capture(|| {
        let run = HeadlessRun::start(Application::new().config(config), SizedBox::new(32.0, 32.0));
        for _ in 0..3 {
            run.run_owner_turn();
        }
        let windows = run.open_windows();
        assert_eq!(windows.len(), 1, "the window opened");
        let presented = run
            .renderer(&windows[0])
            .expect("the window renders")
            .frames_presented();
        run.join().expect("the run ends cleanly");
        presented
    });
    assert!(presented > 0, "the runtime kept rendering");
    let failure = log
        .records()
        .iter()
        .find(|record| record.message.contains("tree-observer factory panicked"))
        .expect("the factory's panic is logged");
    assert_eq!(
        failure.field("panic"),
        Some("the observer factory failed"),
        "the log carries the panic's text"
    );
}

/// A configuration whose run logs only warnings and errors, so a passing test prints nothing.
fn quiet() -> AppConfig {
    AppConfig::new().diagnostics_profile(DiagnosticsProfile::Production)
}
