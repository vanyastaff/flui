//! Integration coverage for panic containment: window callback dispatch and the
//! background executor.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use flui_platform::{WindowCallbacks, executor::BackgroundExecutor};

fn frame_callback_is_restored_after_real_dispatch_panics() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_callback = Arc::clone(&calls);
    *callbacks.on_request_frame.lock() = Some(Box::new(move || {
        let call = calls_in_callback.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            weak_callbacks
                .upgrade()
                .expect("callbacks alive")
                .dispatch_request_frame();
        }
        assert_ne!(call, 0, "first dispatch panics");
    }));

    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        callbacks.dispatch_request_frame();
    }));
    assert!(first.is_err());

    callbacks.dispatch_request_frame();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "nested work from the aborted dispatch must not leak into the next call"
    );
}

fn nested_should_close_is_conservative_veto_without_recursion() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    *callbacks.on_should_close.lock() = Some(Box::new(move || {
        callback_calls.fetch_add(1, Ordering::SeqCst);
        let nested = weak_callbacks
            .upgrade()
            .expect("callbacks alive")
            .dispatch_should_close();
        assert!(!nested, "nested close query must conservatively veto");
        true
    }));

    assert!(callbacks.dispatch_should_close());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

fn terminal_retirement_survives_an_admitted_callback_failure() {
    struct Capture {
        owner: std::sync::Weak<WindowCallbacks>,
        panics: bool,
        reenter: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            if self.reenter
                && let Some(owner) = self.owner.upgrade()
            {
                owner.dispatch_request_frame();
                owner.clear();
            }
            assert!(!self.panics, "capture retirement failure");
        }
    }
    for (callback_panics, close_panics, capture_panics, retirement_panics, reenter) in [
        (true, false, false, false, false),
        (false, true, false, false, false),
        (false, false, true, false, false),
        (false, false, false, true, false),
        (true, true, false, false, false),
        (true, false, true, false, false),
        (true, false, false, true, false),
        (false, true, true, true, true),
        (true, true, true, true, true),
    ] {
        let callbacks = Arc::new(WindowCallbacks::new());
        let retired = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let close_observation = Arc::clone(&closed);
        *callbacks.on_close.lock() = Some(Box::new(move || {
            close_observation.store(true, Ordering::SeqCst);
            assert!(!close_panics, "terminal close failure");
        }));
        let moves = Arc::new(AtomicUsize::new(0));
        let move_observation = Arc::clone(&moves);
        let capture = Arc::new(());
        let witness = Arc::downgrade(&capture);
        let hostile = Capture {
            owner: Arc::downgrade(&callbacks),
            panics: capture_panics,
            reenter,
        };
        *callbacks.on_state_changed.lock() = Some(Box::new(move || {
            let _ = (&capture, &hostile);
            move_observation.fetch_add(1, Ordering::SeqCst);
        }));
        let nested = Arc::downgrade(&callbacks);
        let notice = Arc::clone(&retired);
        *callbacks.on_request_frame.lock() = Some(Box::new(move || {
            let callbacks = nested.upgrade().expect("owner");
            callbacks.dispatch_state_changed();
            callbacks.dispatch_close();
            let notice = Arc::clone(&notice);
            callbacks.clear_then(move || {
                notice.store(true, Ordering::SeqCst);
                assert!(!retirement_panics, "retirement failure");
            });
            assert!(!callback_panics, "admitted callback failure");
        }));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callbacks.dispatch_request_frame();
        }));
        if callback_panics || close_panics {
            let failure = failure.expect_err("admitted callback failed");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some(if callback_panics {
                    "admitted callback failure"
                } else {
                    "terminal close failure"
                })
            );
        } else {
            assert!(
                failure.is_ok(),
                "retirement failures are independently contained"
            );
        }
        assert!(
            witness.upgrade().is_none(),
            "terminal callback captures must retire during unwind"
        );
        assert!(
            retired.load(Ordering::SeqCst),
            "retirement notice must complete despite the aborted prefix"
        );
        assert!(
            closed.load(Ordering::SeqCst),
            "terminal close must reach the application's cancellation hook even when its causal callback aborts"
        );
        assert_eq!(
            moves.load(Ordering::SeqCst),
            usize::from(!callback_panics),
            "ordinary queued events from an aborted transaction remain discarded"
        );
        callbacks.dispatch_request_frame();
        callbacks.clear();
        let healthy = WindowCallbacks::new();
        let delivered = Arc::new(AtomicBool::new(false));
        let observation = Arc::clone(&delivered);
        *healthy.on_request_frame.lock() = Some(Box::new(move || {
            observation.store(true, Ordering::SeqCst);
        }));
        healthy.dispatch_request_frame();
        assert!(delivered.load(Ordering::SeqCst));
    }
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

/// A panicking callback or task never wedges what runs next: the frame
/// callback is restored after a dispatch panic, a nested close query is a
/// conservative veto, and the background executor keeps running tasks.
#[test]
fn panics_in_callbacks_and_tasks_leave_the_dispatcher_usable() {
    frame_callback_is_restored_after_real_dispatch_panics();
    nested_should_close_is_conservative_veto_without_recursion();
    terminal_retirement_survives_an_admitted_callback_failure();
    test_background_executor_panic_handling();
}

fn ready_task_returns_owner_local_data() {
    let expected = std::rc::Rc::new(String::from("owner"));
    let mut task = flui_platform::Task::ready(std::rc::Rc::clone(&expected));
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
    let mut task = flui_platform::Task::ready(ResultValue {
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
    let mut task = flui_platform::Task::ready(value.as_str());
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

#[test]
fn a_retirement_notice_contains_its_capture_failure() {
    const CHILD: &str = "FLUI_DISCARDED_NOTICE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        struct Hostile;
        impl Drop for Hostile {
            fn drop(&mut self) {
                panic!("notice capture failure");
            }
        }
        let callbacks = Arc::new(WindowCallbacks::new());
        let nested = Arc::downgrade(&callbacks);
        *callbacks.on_request_frame.lock() = Some(Box::new(move || {
            let hostile = Hostile;
            nested
                .upgrade()
                .expect("owner")
                .clear_then(move || drop(hostile));
            panic!("first failure");
        }));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callbacks.dispatch_request_frame();
        }))
        .expect_err("original failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first failure")
        );
        callbacks.clear();
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
            panic!("notice retirement timed out");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_replaced_callback_capture_cannot_replace_its_invocation_failure() {
    const CHILD: &str = "FLUI_REPLACED_CAPTURE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        struct Hostile;
        impl Drop for Hostile {
            fn drop(&mut self) {
                panic!("replaced capture failure");
            }
        }
        let callbacks = Arc::new(WindowCallbacks::new());
        let nested = Arc::downgrade(&callbacks);
        let hostile = Hostile;
        *callbacks.on_request_frame.lock() = Some(Box::new(move || {
            let _ = &hostile;
            let owner = nested.upgrade().expect("owner");
            *owner.on_request_frame.lock() = Some(Box::new(|| {}));
            owner.clear();
            panic!("first invocation failure");
        }));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callbacks.dispatch_request_frame();
        }))
        .expect_err("invocation failed");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first invocation failure")
        );
        callbacks.dispatch_request_frame();
        std::process::exit(86);
    }
    let mut child = std::process::Command::new(std::env::current_exe().expect("binary"))
        .args(["--exact", "window_callback_unwind::a_replaced_callback_capture_cannot_replace_its_invocation_failure", "--nocapture"])
        .env(CHILD, "1").env("RUST_BACKTRACE", "0").spawn().expect("child");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("wait") {
            assert_eq!(status.code(), Some(86));
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill timed-out child");
            panic!("capture retirement timed out");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
