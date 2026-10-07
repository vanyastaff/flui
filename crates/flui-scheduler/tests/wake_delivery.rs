//! Frame demand survives failed delivery and overlapping hook invocations.
use flui_scheduler::{AsyncDriver, OwnerFrame, UpdateScheduler};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Poll, Waker};
use std::time::Duration;

fn repeated_scheduler_request_retries_failed_delivery() {
    let scheduler = UpdateScheduler::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        assert_ne!(hook_calls.fetch_add(1, Ordering::Relaxed), 0, "wake failed");
    })));
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scheduler.request_frame()));
    assert!(failure.is_err());
    scheduler.request_frame();
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "retry once, then coalesce successful delivery"
    );
    scheduler.finish_async_pump();
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "the next frame retains demand"
    );
}

fn hookless_requests_preserve_debt_until_a_hook_is_installed() {
    let scheduler = UpdateScheduler::new();
    scheduler.request_frame();
    scheduler.request_frame();
    scheduler.set_on_frame_scheduled(None);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::Relaxed);
    })));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "hook installation pays previously unpaid demand"
    );
    scheduler.request_frame();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

fn older_success_cannot_acknowledge_a_newer_failed_request() {
    let scheduler = UpdateScheduler::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        match hook_calls.fetch_add(1, Ordering::Relaxed) {
            0 => {
                entered_tx.send(()).expect("entered receiver");
                release_rx
                    .lock()
                    .expect("release receiver")
                    .recv_timeout(Duration::from_secs(10))
                    .expect("release old wake");
            }
            1 => panic!("newer wake failed"),
            _ => {}
        }
    })));
    let older = scheduler.clone();
    let worker = std::thread::spawn(move || older.request_frame());
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("old hook entered");
    scheduler.finish_async_pump();
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scheduler.request_frame()));
    release_tx.send(()).expect("release old hook");
    worker.join().expect("old successful hook");
    assert!(failure.is_err());
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "old success must not erase the newer failure"
    );
}

fn reentrant_fresh_request_is_delivered_without_recursing() {
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    let entered = Arc::new(AtomicBool::new(false));
    let hook_entered = Arc::clone(&entered);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        assert!(
            !hook_entered.swap(true, Ordering::AcqRel),
            "hook must not recursively invoke itself"
        );
        let scheduler = weak.upgrade().expect("live scheduler");
        if hook_calls.fetch_add(1, Ordering::Relaxed) == 0 {
            scheduler.finish_async_pump();
            scheduler.request_frame();
        } else {
            scheduler.request_frame();
        }
        hook_entered.store(false, Ordering::Release);
    })));
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "outer call pays the reentrant fresh demand"
    );
    scheduler.request_frame();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

fn repeated_cloned_task_wake_retries_a_panicking_hook() {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let observed = Arc::new(Mutex::new(None::<Waker>));
    let task_observed = Arc::clone(&observed);
    let token = driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
        *task_observed.lock().expect("observed waker") = Some(cx.waker().clone());
        Poll::<()>::Pending
    })));
    assert_eq!(frame.poll_ready(), 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    driver.set_request_frame(move || {
        assert_ne!(
            hook_calls.fetch_add(1, Ordering::Relaxed),
            0,
            "task wake failed"
        );
    });
    let waker = observed
        .lock()
        .expect("observed waker")
        .clone()
        .expect("polled waker");
    let cloned = waker.clone();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref()));
    assert!(failure.is_err());
    cloned.wake_by_ref();
    waker.wake_by_ref();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        frame.poll_ready(),
        1,
        "failed delivery must not lose the indexed task"
    );
    cloned.wake_by_ref();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    token.cancel();
    cloned.wake_by_ref();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

fn scheduled_async_wake_retries_both_delivery_layers() {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let observed = Arc::new(Mutex::new(None::<Waker>));
    let task_observed = Arc::clone(&observed);
    let token = driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
        *task_observed.lock().expect("observed waker") = Some(cx.waker().clone());
        Poll::<()>::Pending
    })));
    scheduler.finish_async_pump();
    assert_eq!(frame.poll_ready(), 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        assert_ne!(
            hook_calls.fetch_add(1, Ordering::Relaxed),
            0,
            "platform wake failed"
        );
    })));
    let waker = observed
        .lock()
        .expect("observed waker")
        .clone()
        .expect("polled waker");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake_by_ref()));
    assert!(failure.is_err());
    waker.clone().wake_by_ref();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "both the task and frame latch must permit retry"
    );
    scheduler.finish_async_pump();
    assert_eq!(frame.poll_ready(), 1);
    waker.wake_by_ref();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    drop(token);
}

fn driver_older_success_cannot_erase_newer_failed_wake() {
    fn pending(frame: &OwnerFrame) -> (flui_scheduler::TaskToken, Waker) {
        let driver = frame.async_driver();
        let observed = Arc::new(Mutex::new(None::<Waker>));
        let task_observed = Arc::clone(&observed);
        let token = driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            *task_observed.lock().expect("observed waker") = Some(cx.waker().clone());
            Poll::<()>::Pending
        })));
        assert_eq!(frame.poll_ready(), 1);
        let waker = observed
            .lock()
            .expect("observed waker")
            .clone()
            .expect("polled waker");
        (token, waker)
    }
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let (_first_token, first) = pending(&frame);
    let (_second_token, second) = pending(&frame);
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    driver.set_request_frame(move || match hook_calls.fetch_add(1, Ordering::Relaxed) {
        0 => {
            entered_tx.send(()).expect("entered receiver");
            release_rx
                .lock()
                .expect("release receiver")
                .recv_timeout(Duration::from_secs(10))
                .expect("release old wake");
        }
        1 => panic!("newer task wake failed"),
        _ => {}
    });
    let worker = std::thread::spawn(move || first.wake_by_ref());
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("old hook entered");
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| second.wake_by_ref()));
    release_tx.send(()).expect("release old hook");
    worker.join().expect("old successful hook");
    assert!(failure.is_err());
    second.wake_by_ref();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert_eq!(frame.poll_ready(), 2);
}

fn competing_reentrant_failures_preserve_the_first_panic() {
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        match hook_calls.fetch_add(1, Ordering::Relaxed) {
            0 => {
                let scheduler = weak.upgrade().expect("live scheduler");
                scheduler.finish_async_pump();
                scheduler.request_frame();
                panic!("first wake failure");
            }
            1 => panic!("compensating wake failure"),
            _ => {}
        }
    })));
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scheduler.request_frame()))
            .expect_err("first wake failed");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first wake failure"));
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "compensation must be bounded"
    );
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "next request must progress after both failures"
    );
}

fn a_panicking_self_uninstalled_hook_retains_its_captures() {
    struct Capture(Arc<AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("hook capture destructor");
        }
    }
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Capture(Arc::clone(&drops));
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        let _capture = &capture;
        weak.upgrade()
            .expect("live scheduler")
            .set_on_frame_scheduled(None);
        panic!("self-uninstalled hook failure");
    })));
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scheduler.request_frame()))
            .expect_err("hook failed");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"self-uninstalled hook failure")
    );
    assert_eq!(
        drops.load(Ordering::Relaxed),
        0,
        "opaque hook captures must not drop during the original unwind"
    );
    let delivered = Arc::new(AtomicUsize::new(0));
    let hook_delivered = Arc::clone(&delivered);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        hook_delivered.fetch_add(1, Ordering::Relaxed);
    })));
    assert_eq!(
        delivered.load(Ordering::Relaxed),
        1,
        "a replacement hook must recover retained demand"
    );
}

fn secondary_payload_retirement(aggregate: bool) {
    struct Payload(Arc<AtomicUsize>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("secondary payload destructor");
        }
    }
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    let payload_drops = Arc::new(AtomicUsize::new(0));
    let hook_drops = Arc::clone(&payload_drops);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        match hook_calls.fetch_add(1, Ordering::Relaxed) {
            0 => {
                let scheduler = weak.upgrade().expect("live scheduler");
                scheduler.finish_async_pump();
                scheduler.request_frame();
                panic!("primary failure");
            }
            1 => {
                if aggregate {
                    std::panic::panic_any((
                        Payload(Arc::clone(&hook_drops)),
                        Payload(Arc::clone(&hook_drops)),
                    ));
                }
                std::panic::panic_any(Payload(Arc::clone(&hook_drops)));
            }
            _ => {}
        }
    })));
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scheduler.request_frame()))
            .expect_err("hook failed");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"primary failure"));
    assert_eq!(
        payload_drops.load(Ordering::Relaxed),
        0,
        "neither opaque payload nor aggregate field destructors may execute"
    );
    scheduler.request_frame();
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

fn a_secondary_panic_payload_destructor_does_not_displace_the_first_failure() {
    secondary_payload_retirement(false);
}

fn a_secondary_aggregate_with_two_panicking_fields_is_retained() {
    secondary_payload_retirement(true);
}

fn perpetual_reentrant_demand_is_bounded_and_retained() {
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    let calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::Relaxed);
        let scheduler = weak.upgrade().expect("live scheduler");
        scheduler.finish_async_pump();
        scheduler.request_frame();
    })));
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "a hook cannot make one request recurse forever"
    );
    scheduler.request_frame();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        4,
        "capped reentrant debt must remain retryable"
    );
}

fn reentrant_hook_replacement_delivers_the_current_hook_after_failure() {
    fn scheduler() {
        assert_reentrant_hook_replacement_after_failure(false);
    }
    fn driver() {
        assert_reentrant_hook_replacement_after_failure(true);
    }
    crate::run_table(
        "reentrant_hook_replacement",
        &[("scheduler", scheduler as fn()), ("driver", driver as fn())],
    );
}

thread_local! {
    /// The driver a hook replaces itself through, on the owner thread.
    static OWNER_DRIVER: std::cell::RefCell<Option<AsyncDriver>> =
        const { std::cell::RefCell::new(None) };
}

fn assert_reentrant_hook_replacement_after_failure(use_driver: bool) {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let observed = Arc::new(Mutex::new(None::<Waker>));
    let task_observed = Arc::clone(&observed);
    let token = use_driver.then(|| {
        driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
            *task_observed.lock().expect("observed waker") = Some(cx.waker().clone());
            Poll::<()>::Pending
        })))
    });
    if use_driver {
        assert_eq!(frame.poll_ready(), 1);
    }
    scheduler.finish_async_pump();
    let initial_calls = Arc::new(AtomicUsize::new(0));
    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let hook_initial_calls = Arc::clone(&initial_calls);
    let hook_replacement_calls = Arc::clone(&replacement_calls);
    let weak = scheduler.downgrade();
    let initial = move || {
        hook_initial_calls.fetch_add(1, Ordering::Relaxed);
        let scheduler = weak.upgrade().expect("live scheduler");
        let replacement_calls = Arc::clone(&hook_replacement_calls);
        let replacement = move || {
            replacement_calls.fetch_add(1, Ordering::Relaxed);
        };
        if use_driver {
            // The driver is owner-local, so a `Send` hook reaches it through
            // the owner thread it runs on here.
            OWNER_DRIVER.with(|owner| {
                owner
                    .borrow()
                    .as_ref()
                    .expect("owner driver installed")
                    .set_request_frame(replacement);
            });
        } else {
            scheduler.set_on_frame_scheduled(Some(Arc::new(replacement)));
        }
        panic!("initial hook failure");
    };
    if use_driver {
        OWNER_DRIVER.with(|owner| *owner.borrow_mut() = Some(driver.clone()));
        driver.set_request_frame(initial);
    } else {
        scheduler.set_on_frame_scheduled(Some(Arc::new(initial)));
    }
    let waker = observed.lock().expect("observed waker").clone();
    let demand = || {
        if let Some(waker) = &waker {
            waker.wake_by_ref();
        } else {
            scheduler.request_frame();
        }
    };
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(demand))
        .expect_err("initial hook failed");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"initial hook failure")
    );
    assert_eq!(initial_calls.load(Ordering::Relaxed), 1);
    assert_eq!(replacement_calls.load(Ordering::Relaxed), 1);
    demand();
    assert_eq!(replacement_calls.load(Ordering::Relaxed), 1);
    if let Some(token) = token {
        token.cancel();
    }
    OWNER_DRIVER.with(|owner| owner.borrow_mut().take());
}

fn initial_hook_retirement_retains_the_compensating_envelope_on_failure() {
    struct Capture {
        drops: Arc<AtomicUsize>,
        failure: &'static str,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
            panic!("{}", self.failure);
        }
    }
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    let initial_drops = Arc::new(AtomicUsize::new(0));
    let replacement_drops = Arc::new(AtomicUsize::new(0));
    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let initial_capture = Capture {
        drops: Arc::clone(&initial_drops),
        failure: "initial envelope retirement",
    };
    let hook_replacement_drops = Arc::clone(&replacement_drops);
    let hook_replacement_calls = Arc::clone(&replacement_calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        let _keep_capture = &initial_capture;
        let scheduler = weak.upgrade().expect("live scheduler");
        scheduler.finish_async_pump();
        let replacement_capture = Capture {
            drops: Arc::clone(&hook_replacement_drops),
            failure: "replacement envelope retirement",
        };
        let replacement_calls = Arc::clone(&hook_replacement_calls);
        let replacement_weak = scheduler.downgrade();
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            let _keep_capture = &replacement_capture;
            replacement_calls.fetch_add(1, Ordering::Relaxed);
            replacement_weak
                .upgrade()
                .expect("live scheduler")
                .set_on_frame_scheduled(None);
        })));
        scheduler.request_frame();
    })));
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scheduler.request_frame()));
    // Retire any still-installed replacement before asserting. This also makes
    // a counterfactual using the old callback selection fail without unwinding
    // through a panicking capture destructor at the end of this table row.
    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scheduler.set_on_frame_scheduled(None);
    }));
    assert!(cleanup.is_ok(), "the compensation uninstalled its hook");
    let failure = failure.expect_err("initial envelope retirement failed");
    assert_eq!(
        failure.downcast_ref::<String>().map(String::as_str),
        Some("initial envelope retirement")
    );
    assert_eq!(initial_drops.load(Ordering::Relaxed), 1);
    assert_eq!(replacement_drops.load(Ordering::Relaxed), 0);
    assert_eq!(replacement_calls.load(Ordering::Relaxed), 1);
    let next_calls = Arc::new(AtomicUsize::new(0));
    let hook_next_calls = Arc::clone(&next_calls);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        hook_next_calls.fetch_add(1, Ordering::Relaxed);
    })));
    scheduler.finish_async_pump();
    scheduler.request_frame();
    assert_eq!(next_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn coalesced_wake_delivery_recovery() {
    crate::run_table(
        "coalesced_wake_delivery_recovery",
        &[
            (
                "initial_hook_retirement_retains_the_compensating_envelope_on_failure",
                initial_hook_retirement_retains_the_compensating_envelope_on_failure as fn(),
            ),
            (
                "reentrant_hook_replacement_delivers_the_current_hook_after_failure",
                reentrant_hook_replacement_delivers_the_current_hook_after_failure as fn(),
            ),
            (
                "a_secondary_aggregate_with_two_panicking_fields_is_retained",
                a_secondary_aggregate_with_two_panicking_fields_is_retained as fn(),
            ),
            (
                "perpetual_reentrant_demand_is_bounded_and_retained",
                perpetual_reentrant_demand_is_bounded_and_retained as fn(),
            ),
            (
                "a_panicking_self_uninstalled_hook_retains_its_captures",
                a_panicking_self_uninstalled_hook_retains_its_captures as fn(),
            ),
            (
                "a_secondary_panic_payload_destructor_does_not_displace_the_first_failure",
                a_secondary_panic_payload_destructor_does_not_displace_the_first_failure as fn(),
            ),
            (
                "scheduled_async_wake_retries_both_delivery_layers",
                scheduled_async_wake_retries_both_delivery_layers as fn(),
            ),
            (
                "driver_older_success_cannot_erase_newer_failed_wake",
                driver_older_success_cannot_erase_newer_failed_wake as fn(),
            ),
            (
                "competing_reentrant_failures_preserve_the_first_panic",
                competing_reentrant_failures_preserve_the_first_panic as fn(),
            ),
            (
                "repeated_scheduler_request_retries_failed_delivery",
                repeated_scheduler_request_retries_failed_delivery as fn(),
            ),
            (
                "hookless_requests_preserve_debt_until_a_hook_is_installed",
                hookless_requests_preserve_debt_until_a_hook_is_installed as fn(),
            ),
            (
                "older_success_cannot_acknowledge_a_newer_failed_request",
                older_success_cannot_acknowledge_a_newer_failed_request as fn(),
            ),
            (
                "reentrant_fresh_request_is_delivered_without_recursing",
                reentrant_fresh_request_is_delivered_without_recursing as fn(),
            ),
            (
                "repeated_cloned_task_wake_retries_a_panicking_hook",
                repeated_cloned_task_wake_retries_a_panicking_hook as fn(),
            ),
        ],
    );
}
