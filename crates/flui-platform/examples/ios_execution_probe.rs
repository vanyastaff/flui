//! UIKit delegate protocol probe, not an OS background-transition test.
//! A bounded external runner reads `tmp/flui-execution-result.txt` in the app
//! data container, then terminates the owned probe (UIApplicationMain does not return).
#[cfg(target_os = "ios")]
mod native {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::{UIApplication, UIApplicationDelegate};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::Duration;

    fn later(callback: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            dispatch2::DispatchQueue::main().exec_async(callback);
        });
    }

    fn report(message: &str) {
        let path = std::env::temp_dir().join("flui-execution-result.txt");
        if let Err(error) = std::fs::write(&path, message) {
            eprintln!("probe result write failed: {error}");
        }
        println!("FLUI_EXECUTION_RESULT {message}");
    }

    #[expect(
        unsafe_code,
        reason = "probe invokes the live UIKit-owned delegate on main"
    )]
    #[expect(
        deprecated,
        reason = "regression probe exercises the existing legacy delegate before scene migration"
    )]
    fn observe(event: &str) {
        let marker = MainThreadMarker::new().expect("main queue");
        let application = UIApplication::sharedApplication(marker);
        // SAFETY: UIKit retains the current delegate throughout this main-thread
        // protocol call. The platform installed FluiAppDelegate before bootstrap.
        let delegate = unsafe { application.delegate() }.expect("FLUI delegate");
        match event {
            "inactive" => delegate.applicationWillResignActive(&application),
            "active" => delegate.applicationDidBecomeActive(&application),
            "background" => delegate.applicationDidEnterBackground(&application),
            "foreground" => delegate.applicationWillEnterForeground(&application),
            _ => unreachable!("probe event"),
        }
    }

    pub(super) fn run() {
        let case = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "transient".into());
        let frames = Arc::new(AtomicUsize::new(0));
        let surfaces = Arc::new(Mutex::new(Vec::new()));
        let executions = Arc::new(Mutex::new(Vec::new()));
        let panic_calls = Arc::new(AtomicUsize::new(0));
        let nested_frames = Arc::new(AtomicUsize::new(usize::MAX));
        let focus_mismatch = Arc::new(AtomicBool::new(false));
        flui_platform::current_platform().expect("platform").run(Box::new(move |owner| {
            let window = owner.open_window(flui_platform::WindowOptions::default())?.try_ready()?;
            let focus_window = Arc::clone(&window);
            let mismatched = Arc::clone(&focus_mismatch);
            window.on_active_status_change(Box::new(move |focused| {
                if focus_window.is_focused() != focused { mismatched.store(true, Ordering::SeqCst); }
            }));
            let execution_history = Arc::clone(&executions);
            window.on_execution_state_change(Box::new(move |state| execution_history.lock().expect("execution history").push(state)));
            let counter = Arc::clone(&frames);
            window.on_request_frame(Box::new(move || { counter.fetch_add(1, Ordering::SeqCst); }));
            let history = Arc::clone(&surfaces);
            window.on_surface_status_change(Box::new(move |available| {
                history.lock().expect("history").push(available);
            }));
            later(move || {
                let before = frames.load(Ordering::SeqCst);
                if before == 0 {
                    report("FAIL no initial CADisplayLink frames");
                    return;
                }
                surfaces.lock().expect("history").clear();
                if matches!(case.as_str(), "background-nested-active" | "background-nested-inactive" | "nested-foreground-then-panic") {
                    let history = Arc::clone(&executions);
                    let once = AtomicBool::new(false);
                    let invoked = Arc::clone(&panic_calls);
                    let mode = case.clone();
                    window.on_execution_state_change(Box::new(move |state| {
                        history.lock().expect("execution history").push(state);
                        if state == flui_platform::WindowExecutionState::Suspended && !once.swap(true, Ordering::SeqCst) {
                            if mode == "nested-foreground-then-panic" {
                                observe("foreground");
                                invoked.fetch_add(1, Ordering::SeqCst);
                                panic!("nested foreground observer panic");
                            }
                            observe(if mode.ends_with("inactive") { "inactive" } else { "active" });
                        }
                    }));
                    observe("background");
                } else if case == "background-nested-run-loop" {
                    let frames = Arc::clone(&frames);
                    let witnessed = Arc::clone(&nested_frames);
                    let once = AtomicBool::new(false);
                    window.on_active_status_change(Box::new(move |focused| {
                        if !focused && !once.swap(true, Ordering::SeqCst) {
                            observe("background");
                            let before = frames.load(Ordering::SeqCst);
                            let deadline = objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.15);
                            objc2_foundation::NSRunLoop::currentRunLoop().runUntilDate(&deadline);
                            witnessed.store(frames.load(Ordering::SeqCst) - before, Ordering::SeqCst);
                        }
                    }));
                    observe("inactive");
                } else if case == "superseded-foreground" {
                    let history = Arc::clone(&executions);
                    let once = AtomicBool::new(false);
                    window.on_execution_state_change(Box::new(move |state| {
                        history.lock().expect("execution history").push(state);
                        if state == flui_platform::WindowExecutionState::Suspended && !once.swap(true, Ordering::SeqCst) {
                            observe("foreground");
                            observe("background");
                        }
                    }));
                    observe("background");
                } else if case.starts_with("foreground-nested-") {
                    observe("background");
                    surfaces.lock().expect("history").clear();
                    let history = Arc::clone(&surfaces);
                    let once = AtomicBool::new(false);
                    let mode = case.clone();
                    window.on_surface_status_change(Box::new(move |available| {
                        history.lock().expect("history").push(available);
                        if available && !once.swap(true, Ordering::SeqCst) {
                            observe(if mode.ends_with("inactive") { "inactive" } else { "active" });
                        }
                    }));
                    observe("foreground");
                } else if case == "duplicate-foreground" {
                    observe("active");
                    observe("foreground");
                    observe("foreground");
                } else if case == "foreground-callback-panic" {
                    observe("background");
                    surfaces.lock().expect("history").clear();
                    let invoked = Arc::clone(&panic_calls);
                    window.on_surface_status_change(Box::new(move |available| {
                        invoked.fetch_add(1, Ordering::SeqCst);
                        assert!(!available, "probe surface observer panic");
                    }));
                    observe("foreground");
                } else if case == "foreground-reentered-background" {
                    observe("background");
                    surfaces.lock().expect("history").clear();
                    let history = Arc::clone(&surfaces);
                    let once = AtomicBool::new(false);
                    window.on_surface_status_change(Box::new(move |available| {
                        history.lock().expect("history").push(available);
                        if available && !once.swap(true, Ordering::SeqCst) { observe("background"); }
                    }));
                    observe("foreground");
                } else if case == "background-reentered-foreground" {
                    let once = AtomicBool::new(false);
                    window.on_execution_state_change(Box::new(move |state| {
                        if state == flui_platform::WindowExecutionState::Suspended && !once.swap(true, Ordering::SeqCst) { observe("foreground"); }
                    }));
                    observe("background");
                } else {
                    observe("inactive");
                    observe("active");
                }
                let after_callbacks = frames.load(Ordering::SeqCst);
                later(move || {
                    let after = frames.load(Ordering::SeqCst);
                    let history = surfaces.lock().expect("history").clone();
                    let preserved = !history.contains(&false);
                    let progressed = after > after_callbacks;
                    let execution = window.execution_state();
                    let delivered = executions.lock().expect("execution history").clone();
                    let passed = if case.starts_with("background-nested-") || case == "superseded-foreground" {
                        execution == flui_platform::WindowExecutionState::Suspended && !progressed && history == [false]
                            && delivered.last() == Some(&execution) && !window.is_focused() && !window.is_visible()
                    } else if case.starts_with("foreground-nested-") {
                        execution == flui_platform::WindowExecutionState::Running && progressed && history == [true]
                            && delivered.last() == Some(&execution) && window.is_focused() == case.ends_with("-active") && window.is_visible()
                    } else if case == "nested-foreground-then-panic" {
                        execution == flui_platform::WindowExecutionState::Running && progressed && history == [true]
                            && delivered.last() == Some(&execution) && panic_calls.load(Ordering::SeqCst) == 1 && window.is_visible()
                    } else if case == "foreground-reentered-background" {
                        execution == flui_platform::WindowExecutionState::Suspended && !progressed && history == [true, false]
                    } else if case == "foreground-callback-panic" {
                        execution == flui_platform::WindowExecutionState::Running && progressed && panic_calls.load(Ordering::SeqCst) == 1
                    } else if case == "duplicate-foreground" {
                        execution == flui_platform::WindowExecutionState::Running && window.is_focused() && preserved && progressed
                    } else {
                        execution == flui_platform::WindowExecutionState::Running && preserved && progressed
                    };
                    let passed = passed && !focus_mismatch.load(Ordering::SeqCst)
                        && (case != "background-nested-run-loop" || nested_frames.load(Ordering::SeqCst) == 0);
                    report(&format!("{} case={case} baseline={before} after_callbacks={after_callbacks} after={after} preserved={preserved} progressed={progressed} execution={execution:?} delivered={delivered:?} surfaces={history:?}", if passed { "PASS" } else { "FAIL" }));
                    drop(window);
                });
            });
            Ok(())
        })).expect("UIKit run");
    }
}
fn main() {
    #[cfg(target_os = "ios")]
    native::run();
    #[cfg(not(target_os = "ios"))]
    eprintln!("This protocol probe requires an iOS simulator.");
}
