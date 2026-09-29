//! `HeadlessBinding::pump_frame` runs the shared async-driver step.
//!
//! `flui-app` carries the mirror-image test for `UiRealm::draw_frame`. Both
//! call `UpdateScheduler::drive_async_tasks`; if either stopped, exactly one of the two
//! would fail — which is the headless↔production divergence this pair exists to
//! catch.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_testing::HeadlessBinding;
use parking_lot::Mutex;

/// A future the test can complete from outside, exposing its waker.
struct Signal {
    done: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Waker>>>,
    polls: Arc<AtomicUsize>,
}

impl std::future::Future for Signal {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        self.polls.fetch_add(1, Ordering::Relaxed);
        if self.done.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            let _prev = self.waker.lock().replace(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// A wake from a worker thread is picked up by the next frame, on the frame
/// thread.
pub(crate) fn headless_wake_from_another_thread_is_polled_on_the_frame_thread() {
    let mut binding = HeadlessBinding::new();
    let done = Arc::new(AtomicBool::new(false));
    let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
    let polls = Arc::new(AtomicUsize::new(0));
    let polled_on = Arc::new(Mutex::new(Vec::new()));
    let polled_on_for_task = Arc::clone(&polled_on);

    let signal = Signal {
        done: Arc::clone(&done),
        waker: Arc::clone(&waker),
        polls: Arc::clone(&polls),
    };
    let _token = binding.spawn_local(Box::pin(async move {
        polled_on_for_task.lock().push(std::thread::current().id());
        signal.await;
    }));

    binding.pump_frame(Duration::from_millis(16));
    let waker = waker.lock().clone().expect("waker stored");
    done.store(true, Ordering::Release);

    let worker_id = std::thread::spawn(move || {
        waker.wake_by_ref();
        std::thread::current().id()
    })
    .join()
    .expect("worker");

    assert_eq!(polls.load(Ordering::Relaxed), 1, "no poll off-thread");

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 2);

    let main_id = std::thread::current().id();
    let threads = polled_on.lock().clone();
    assert!(threads.iter().all(|id| *id == main_id));
    assert_ne!(worker_id, main_id);
}
