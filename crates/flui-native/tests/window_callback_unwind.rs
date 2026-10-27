//! Panic containment of a window's handler and of the background executor:
//! a handler method that panics never wedges what runs next, and what was
//! queued behind it is still delivered (ADR-0254 §4, ADR-0207 §7).
//!
//! The windows are the headless host's, driven through the window and its
//! simulated operating system, so the delivery under test is the one every
//! host uses.

use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use flui_native::{
    HeadlessPlatform, HostWindow, MockWindow, Platform as _, executor::BackgroundExecutor,
};
use flui_platform::{CloseDecision, WindowCommand, WindowOptions};
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

/// A headless window delivering to `handler`, and its platform.
fn open(handler: &Rc<RecordingWindowHandler>) -> (HeadlessPlatform, Arc<dyn HostWindow>) {
    let platform = HeadlessPlatform::new();
    let window = platform
        .open_window(WindowOptions::default(), handler.clone())
        .expect("a headless window");
    handler.take_calls();
    (platform, window)
}

fn mock(window: &Weak<dyn HostWindow>) -> Arc<dyn HostWindow> {
    window.upgrade().expect("the window is open")
}

/// A frame whose method panics after it asked for another: the nested
/// request queued behind it is still delivered, the panic resumes to the
/// caller, and the next request runs as usual.
fn a_request_queued_behind_a_failed_frame_is_delivered() {
    let handler = RecordingWindowHandler::new();
    let (_platform, window) = open(&handler);
    let calls = Rc::new(Cell::new(0_u32));
    let weak = Arc::downgrade(&window);
    let counted = Rc::clone(&calls);
    handler.react_to_frame(move |_| {
        let call = counted.get();
        counted.set(call + 1);
        if call == 0 {
            mock(&weak).request_redraw();
            panic!("the first frame fails");
        }
    });

    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        window.request_redraw();
    }));
    assert!(first.is_err(), "the frame's panic resumes to its caller");
    assert_eq!(
        calls.get(),
        2,
        "the request queued behind the failure was lost"
    );
    window.request_redraw();
    assert_eq!(calls.get(), 3);
}

/// A close asked again while the handler decides the first is kept open
/// without asking the handler a second time.
fn a_nested_close_request_keeps_the_window() {
    let handler = RecordingWindowHandler::new();
    let (_platform, window) = open(&handler);
    let nested = Rc::new(Cell::new(None));
    let weak = Arc::downgrade(&window);
    let heard = Rc::clone(&nested);
    handler.react_to_close_request(move || {
        let window = mock(&weak);
        let window = window
            .as_any()
            .downcast_ref::<MockWindow>()
            .expect("a headless window");
        heard.set(Some(window.simulate_close()));
        CloseDecision::Close
    });
    let window = window
        .as_any()
        .downcast_ref::<MockWindow>()
        .expect("a headless window");
    assert!(
        window.simulate_close(),
        "the outer request closes the window"
    );
    assert_eq!(nested.get(), Some(false), "the nested request is kept open");
    assert_eq!(handler.count(&HandlerCall::CloseRequested), 1);
    assert_eq!(handler.count(&HandlerCall::Closed), 1);
}

/// A frame closes its own window and then panics: the state change and the
/// close queued behind it still reach the handler, the window lets go of the
/// handler, and the frame's panic stays the one that resumes.
fn a_close_from_a_failed_method_still_finishes() {
    let handler = RecordingWindowHandler::new();
    let (_platform, window) = open(&handler);
    let weak = Arc::downgrade(&window);
    handler.react_to_frame(move |_| {
        flui_testing::window_commands::AdmitAndApply::close_for_cleanup(&*mock(&weak));
        panic!("the frame fails after closing");
    });
    let held = Rc::downgrade(&handler);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        window.request_redraw();
    }))
    .expect_err("the frame's panic resumes");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("the frame fails after closing")
    );
    let calls = handler.take_calls();
    assert!(
        matches!(
            calls.as_slice(),
            [
                HandlerCall::FrameRequested(_),
                HandlerCall::StateChanged,
                HandlerCall::Closed
            ]
        ),
        "the deliveries behind the failed frame: {calls:?}"
    );
    drop(handler);
    assert_eq!(held.strong_count(), 0, "the closed window kept its handler");
    assert!(
        matches!(
            window.apply(
                WindowCommand::Close
                    .admit(&window.state().capabilities)
                    .expect("closing is admitted")
            ),
            Err(flui_platform::ApplyError::Closed)
        ),
        "the window is closed"
    );
}

/// Test that background executor handles panic in tasks gracefully
fn test_background_executor_panic_handling() {
    let executor = BackgroundExecutor::new();
    let post_panic_executed = Arc::new(AtomicBool::new(false));
    let post_panic_clone = Arc::clone(&post_panic_executed);

    // Spawn task that panics
    executor
        .spawn(async {
            panic!("Intentional panic for testing");
        })
        .detach();

    thread::sleep(Duration::from_millis(50));

    // Spawn another task after panic
    executor
        .spawn(async move {
            post_panic_clone.store(true, Ordering::SeqCst);
        })
        .detach();

    thread::sleep(Duration::from_millis(50));

    // Verify executor still works after panic
    assert!(
        post_panic_executed.load(Ordering::SeqCst),
        "Executor should continue working after task panic"
    );
}

/// A panicking handler method or task never wedges what runs next: a request
/// queued behind a failed frame is delivered, a nested close request keeps
/// the window, a close made from a failed method still finishes, and the
/// background executor keeps running tasks.
#[test]
fn panics_in_callbacks_and_tasks_leave_the_dispatcher_usable() {
    crate::run_table(
        "panics_in_callbacks_and_tasks_leave_the_dispatcher_usable",
        &[
            (
                "a_request_queued_behind_a_failed_frame_is_delivered",
                a_request_queued_behind_a_failed_frame_is_delivered,
            ),
            (
                "a_nested_close_request_keeps_the_window",
                a_nested_close_request_keeps_the_window,
            ),
            (
                "a_close_from_a_failed_method_still_finishes",
                a_close_from_a_failed_method_still_finishes,
            ),
            (
                "test_background_executor_panic_handling",
                test_background_executor_panic_handling,
            ),
        ],
    );
}

fn ready_task_returns_owner_local_data() {
    let expected = std::rc::Rc::new(String::from("owner"));
    let mut task = flui_native::Task::ready(std::rc::Rc::clone(&expected));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(actual) =
        std::future::Future::poll(std::pin::Pin::new(&mut task), &mut context)
    else {
        panic!("a ready task must return its result on the first poll");
    };
    assert!(std::rc::Rc::ptr_eq(&expected, &actual));
}

fn ready_task_returns_a_non_unpin_result() {
    struct ResultValue {
        value: String,
        _pin: std::marker::PhantomPinned,
    }
    let mut task = flui_native::Task::ready(ResultValue {
        value: String::from("completed"),
        _pin: std::marker::PhantomPinned,
    });
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(actual) =
        std::future::Future::poll(std::pin::Pin::new(&mut task), &mut context)
    else {
        panic!("a ready task must return its result on the first poll");
    };
    assert_eq!(actual.value, "completed");
}

fn ready_task_returns_borrowed_data() {
    let value = String::from("borrowed");
    let mut task = flui_native::Task::ready(value.as_str());
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert_eq!(
        std::future::Future::poll(std::pin::Pin::new(&mut task), &mut context),
        std::task::Poll::Ready("borrowed"),
    );
}

#[test]
fn ready_tasks_return_results_without_executor_bounds() {
    let cases: &[(&str, fn())] = &[
        ("owner_local", ready_task_returns_owner_local_data),
        ("non_unpin", ready_task_returns_a_non_unpin_result),
        ("borrowed", ready_task_returns_borrowed_data),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed cases: {failures:?}");
}

/// A handler whose captures panic when the window lets go of it, after a
/// method that already failed: the capture's panic is contained, and the
/// process neither aborts nor loses the method's failure.
#[test]
fn a_retirement_notice_contains_its_capture_failure() {
    const CHILD: &str = "FLUI_DISCARDED_NOTICE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        struct Hostile;
        impl Drop for Hostile {
            fn drop(&mut self) {
                panic!("handler capture failure");
            }
        }
        let handler = RecordingWindowHandler::new();
        let (_platform, window) = open(&handler);
        let weak = Arc::downgrade(&window);
        let hostile = Hostile;
        handler.react_to_frame(move |_| {
            let _ = &hostile;
            flui_testing::window_commands::AdmitAndApply::close_for_cleanup(&*mock(&weak));
            panic!("first failure");
        });
        drop(handler);
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            window.request_redraw();
        }))
        .expect_err("original failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first failure")
        );
        std::process::exit(86);
    }
    let mut child = std::process::Command::new(std::env::current_exe().expect("binary"))
        .args([
            "--exact",
            "window_callback_unwind::a_retirement_notice_contains_its_capture_failure",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("RUST_BACKTRACE", "0")
        .spawn()
        .expect("child");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            assert_eq!(status.code(), Some(86));
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill timed-out child");
            panic!("handler retirement timed out");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
