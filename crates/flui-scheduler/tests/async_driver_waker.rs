//! Task wakers retain identity across polls and become inert after retirement.

use flui_scheduler::{OwnerFrame, UpdateScheduler};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

fn retained_waker_lifecycle(eager: bool, complete: bool) {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let requests = Arc::new(AtomicUsize::new(0));
    let hook_requests = Arc::clone(&requests);
    driver.set_request_frame(move || {
        hook_requests.fetch_add(1, Ordering::Relaxed);
    });
    let finish = Arc::new(AtomicBool::new(false));
    let task_finish = Arc::clone(&finish);
    let observed = Arc::new(Mutex::new(Vec::<Waker>::new()));
    let task_observed = Arc::clone(&observed);
    let future = Box::pin(std::future::poll_fn(move |cx| {
        task_observed
            .lock()
            .expect("waker observations")
            .push(cx.waker().clone());
        if task_finish.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }));
    let token = if eager {
        driver
            .spawn_local_eager(future)
            .expect("pending eager task")
    } else {
        let token = driver.spawn_local(future);
        assert_eq!(frame.poll_ready(), 1);
        token
    };
    let first = observed.lock().expect("waker observations")[0].clone();
    let clone = first.clone();
    let before = requests.load(Ordering::Relaxed);
    first.wake_by_ref();
    clone.wake_by_ref();
    assert_eq!(requests.load(Ordering::Relaxed), before + 1);
    assert_eq!(frame.poll_ready(), 1);
    let second = observed.lock().expect("waker observations")[1].clone();
    assert!(
        first.will_wake(&second),
        "every poll must reuse the task's waker identity"
    );

    if complete {
        finish.store(true, Ordering::Release);
        second.wake_by_ref();
        assert_eq!(frame.poll_ready(), 1);
    } else {
        token.cancel();
    }
    assert_eq!(driver.pending_task_count(), 0);
    let before = requests.load(Ordering::Relaxed);
    first.wake_by_ref();
    second.wake_by_ref();
    clone.wake_by_ref();
    assert_eq!(
        requests.load(Ordering::Relaxed),
        before,
        "retired task must not request frames"
    );
    assert_eq!(frame.ready_task_count(), 0);
    assert_eq!(frame.poll_ready(), 0);

    // Retirement must not poison the next task or let old handles target it.
    let next = driver.spawn_local(Box::pin(async {}));
    first.wake_by_ref();
    assert_eq!(frame.poll_ready(), 1);
    assert_eq!(driver.pending_task_count(), 0);
    drop(next);
    drop(token);
    drop(driver);
    drop(frame);
    let before = requests.load(Ordering::Relaxed);
    second.wake_by_ref();
    assert_eq!(
        requests.load(Ordering::Relaxed),
        before,
        "external wakers must not retain the driver"
    );
}

fn lazy_completion() {
    retained_waker_lifecycle(false, true);
}
fn lazy_cancellation() {
    retained_waker_lifecycle(false, false);
}
fn eager_completion() {
    retained_waker_lifecycle(true, true);
}
fn eager_cancellation() {
    retained_waker_lifecycle(true, false);
}

fn pending_waker_does_not_retain_driver() {
    struct Retired(Arc<AtomicBool>);
    impl Drop for Retired {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let retired = Arc::new(AtomicBool::new(false));
    let guard = Retired(Arc::clone(&retired));
    let observed = Arc::new(Mutex::new(None::<Waker>));
    let task_observed = Arc::clone(&observed);
    let token = driver.spawn_local(Box::pin(std::future::poll_fn(move |cx| {
        let _guard = &guard;
        *task_observed.lock().expect("waker observation") = Some(cx.waker().clone());
        Poll::<()>::Pending
    })));
    assert_eq!(frame.poll_ready(), 1);
    let waker = observed
        .lock()
        .expect("waker observation")
        .clone()
        .expect("polled waker");
    drop(driver);
    drop(frame);
    assert!(
        retired.load(Ordering::Acquire),
        "a pending task must drop with its driver despite external wakers"
    );
    waker.wake_by_ref();
    drop(token);
}

#[test]
fn task_waker_lifecycle() {
    crate::run_table(
        "task_waker_lifecycle",
        &[
            ("lazy_completion", lazy_completion as fn()),
            ("lazy_cancellation", lazy_cancellation as fn()),
            ("eager_completion", eager_completion as fn()),
            ("eager_cancellation", eager_cancellation as fn()),
            (
                "pending_waker_does_not_retain_driver",
                pending_waker_does_not_retain_driver as fn(),
            ),
        ],
    );
}
