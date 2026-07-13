//! `HeadlessBinding::pump_frame` runs the shared async-driver step.
//!
//! `flui-app` carries the mirror-image test for `AppBinding::draw_frame`. Both
//! call `Scheduler::drive_async_tasks`; if either stopped, exactly one of the two
//! would fail — which is the headless↔production divergence this pair exists to
//! catch.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Poll, Waker};
use std::time::Duration;

use flui_binding::HeadlessBinding;
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
            *self.waker.lock() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// The headless helper: complete a future *between* frames and prove the next
/// frame observes it.
#[test]
fn headless_pump_frame_polls_a_ready_future() {
    let mut binding = HeadlessBinding::new();
    let ran = Arc::new(AtomicBool::new(false));
    let ran_for_task = Arc::clone(&ran);

    let _token = binding.spawn_local(Box::pin(async move {
        ran_for_task.store(true, Ordering::Release);
    }));

    assert!(!ran.load(Ordering::Acquire), "spawn must not poll inline");
    assert_eq!(binding.scheduler().pending_task_count(), 1);

    binding.pump_frame(Duration::from_millis(16));

    assert!(
        ran.load(Ordering::Acquire),
        "pump_frame must run the shared async-driver step"
    );
    assert_eq!(binding.scheduler().pending_task_count(), 0);
}

/// A completion signalled between frames is observed by the next frame, and only
/// by that frame — polling never happens outside the driver step.
#[test]
fn headless_completion_between_frames_is_observed_by_the_next_frame() {
    let mut binding = HeadlessBinding::new();
    let done = Arc::new(AtomicBool::new(false));
    let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
    let polls = Arc::new(AtomicUsize::new(0));

    let _token = binding.spawn_local(Box::pin(Signal {
        done: Arc::clone(&done),
        waker: Arc::clone(&waker),
        polls: Arc::clone(&polls),
    }));

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 1, "first frame polls once");
    assert_eq!(binding.scheduler().pending_task_count(), 1, "still pending");

    // A frame with no wake must not re-poll.
    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 1);

    // Complete from outside a frame, as an async completion would.
    done.store(true, Ordering::Release);
    waker.lock().as_ref().expect("waker stored").wake_by_ref();
    assert_eq!(
        polls.load(Ordering::Relaxed),
        1,
        "waking must not poll; only the frame's driver step polls"
    );

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 2);
    assert_eq!(binding.scheduler().pending_task_count(), 0, "completed");
}

/// A wake from a worker thread is picked up by the next frame, on the frame
/// thread.
#[test]
fn headless_wake_from_another_thread_is_polled_on_the_frame_thread() {
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

/// Dropping the token cancels: the task is never polled again by any frame.
#[test]
fn headless_dropping_the_token_cancels_the_task() {
    let mut binding = HeadlessBinding::new();
    let polls = Arc::new(AtomicUsize::new(0));
    let token = binding.spawn_local(Box::pin(Signal {
        done: Arc::new(AtomicBool::new(false)),
        waker: Arc::new(Mutex::new(None)),
        polls: Arc::clone(&polls),
    }));

    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 1);

    drop(token);
    assert_eq!(binding.scheduler().pending_task_count(), 0);

    binding.pump_frame(Duration::from_millis(16));
    binding.pump_frame(Duration::from_millis(16));
    assert_eq!(polls.load(Ordering::Relaxed), 1, "never polled again");
}
