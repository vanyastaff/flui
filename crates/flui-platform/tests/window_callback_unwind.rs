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
