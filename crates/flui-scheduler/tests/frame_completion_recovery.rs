//! Consumer failure boundaries for frame-completion executor envelopes.
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use flui_foundation::testing::CountingWaker;
use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{FrameCompletionFuture, FrameOutcome, IdleDeadline, UpdateScheduler};

/// Counts its drop and, when armed, panics from it. A waker holds at most one
/// armed capture: a second destructor panicking while the first unwinds
/// aborts, as in std (ADR-0281).
struct Bomb {
    drops: Arc<AtomicUsize>,
    armed: bool,
}
impl Drop for Bomb {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
        assert!(!self.armed, "executor retirement");
    }
}
/// `count` captures counted into `drops`, the first of them armed.
fn bombs(count: usize, drops: &Arc<AtomicUsize>) -> Vec<Bomb> {
    (0..count)
        .map(|index| Bomb {
            drops: Arc::clone(drops),
            armed: index == 0,
        })
        .collect()
}
/// A waker that counts into `calls`, runs `action`, and keeps `captures` until it is
/// dropped.
fn probe(
    calls: &Arc<CountingWaker>,
    action: impl Fn() + Send + Sync + 'static,
    captures: Vec<Bomb>,
) -> Waker {
    calls.retiring_waker(action, captures)
}
fn register(future: &mut FrameCompletionFuture, waker: &Waker) {
    assert!(
        Pin::new(future)
            .poll(&mut Context::from_waker(waker))
            .is_pending()
    );
}
fn assert_failure(result: Result<(), Box<dyn std::any::Any + Send>>, expected: &str) {
    let payload = result.expect_err("the first failure must escape");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some(expected)
    );
}
fn assert_next_frame(scheduler: &UpdateScheduler) {
    let mut next = scheduler.end_of_frame();
    flui_scheduler::OwnerFrame::try_new(scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");
    assert!(matches!(
        Pin::new(&mut next).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(FrameOutcome::Completed { .. }))
    ));
}
struct ErrorSubscriber;
impl tracing::Subscriber for ErrorSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() == tracing::Level::ERROR
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        panic!("telemetry secondary");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn delivery(case: &str) {
    let scheduler = UpdateScheduler::new();
    let mut first = scheduler.end_of_frame();
    let mut later = scheduler.end_of_frame();
    let mut last = scheduler.end_of_frame();
    let drops = Arc::new(AtomicUsize::new(0));
    let first_calls = CountingWaker::new().shared();
    let tail_calls = CountingWaker::new().shared();
    let retirement = case == "ordinary retirement";
    let preserve = case == "pipeline preserves successful envelope"
        || case == "post-frame preserves successful envelope";
    let hostile = !case.starts_with("telemetry")
        && case != "competing wake failure"
        && case != "later successful envelope";
    let first_captures = if retirement {
        1
    } else if hostile {
        2
    } else {
        0
    };
    let first_waker = probe(
        &first_calls,
        move || {
            assert!(retirement || preserve, "wake primary");
        },
        bombs(first_captures, &drops),
    );
    register(&mut first, &first_waker);
    drop(first_waker);
    let secondary_drops = Arc::new(AtomicUsize::new(0));
    let competition = case == "competing wake failure";
    let tail_waker = probe(
        &tail_calls,
        move || {
            assert!(!competition, "competing wake");
        },
        if case == "later successful envelope" {
            bombs(2, &secondary_drops)
        } else {
            Vec::new()
        },
    );
    register(&mut later, &tail_waker);
    drop(tail_waker);
    let last_calls = CountingWaker::new().shared();
    let last_waker = probe(&last_calls, || {}, Vec::new());
    register(&mut last, &last_waker);
    drop(last_waker);
    if case == "post-frame preserves successful envelope" {
        scheduler.add_post_frame_callback(Box::new(|_| panic!("post-frame primary")));
    }
    let teardown = case.contains("teardown");
    let unwind = case.contains("unwinding teardown");
    let pipeline = case.starts_with("pipeline") || case == "telemetry during abort";
    let mut live = Some(scheduler);
    let trigger = || {
        let scheduler = live.as_ref().expect("live scheduler");
        if teardown {
            let owned = live.take();
            if unwind {
                let _owned = owned;
                panic!("outer primary");
            }
            drop(owned);
        } else if pipeline {
            flui_scheduler::OwnerFrame::try_new(scheduler)
                .expect("the scheduler has no live owner frame")
                .drive_frame(
                    Instant::now(),
                    IdleDeadline::far_future(Instant::now()),
                    || {},
                    || panic!("pipeline primary"),
                )
                .expect("live owner frame");
        } else {
            flui_scheduler::OwnerFrame::try_new(scheduler)
                .expect("the scheduler has no live owner frame")
                .drive_frame(
                    flui_scheduler::Instant::now(),
                    flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                    || {},
                    || {},
                )
                .expect("live owner frame");
        }
    };
    let result = if case.starts_with("telemetry") {
        tracing::subscriber::with_default(ErrorSubscriber, || {
            catch_unwind(AssertUnwindSafe(trigger))
        })
    } else {
        catch_unwind(AssertUnwindSafe(trigger))
    };
    if teardown && !unwind {
        assert!(result.is_ok(), "teardown must retain caught failures");
    } else {
        assert_failure(
            result,
            if unwind {
                "outer primary"
            } else if pipeline {
                "pipeline primary"
            } else if preserve {
                "post-frame primary"
            } else if retirement {
                "executor retirement"
            } else {
                "wake primary"
            },
        );
    }
    assert_eq!(first_calls.count(), 1);
    assert_eq!(
        tail_calls.count(),
        1,
        "a failed wake must not starve the tail"
    );
    assert_eq!(
        last_calls.count(),
        1,
        "competing failures must not starve the final waiter"
    );
    assert_eq!(
        drops.load(Ordering::Relaxed),
        first_captures,
        "a woken envelope is destroyed, a failure included"
    );
    assert_eq!(
        secondary_drops.load(Ordering::Relaxed),
        if case == "later successful envelope" {
            2
        } else {
            0
        },
        "a later envelope is destroyed behind the first failure"
    );
    assert!(
        Pin::new(&mut later)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready()
    );
    let terminal = Pin::new(&mut last).poll(&mut Context::from_waker(Waker::noop()));
    if teardown {
        assert!(matches!(terminal, Poll::Ready(Err(_))));
    } else if pipeline {
        assert!(matches!(
            terminal,
            Poll::Ready(Ok(FrameOutcome::Aborted { .. }))
        ));
    } else {
        assert!(matches!(
            terminal,
            Poll::Ready(Ok(FrameOutcome::Completed { .. }))
        ));
    }
    if let Some(scheduler) = live.as_ref() {
        assert_next_frame(scheduler);
    } else {
        assert_next_frame(&UpdateScheduler::new());
    }
}

/// A future dropped during an unwind destroys its waker with it. Its captures
/// are unarmed: a destructor panicking during the unwind aborts, as in std
/// (ADR-0281).
fn pending_cancellation_during_unwind() {
    let scheduler = UpdateScheduler::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut future = scheduler.end_of_frame();
    let waker = CountingWaker::new().shared().retiring_waker(
        || {},
        (0..2)
            .map(|_| Bomb {
                drops: Arc::clone(&drops),
                armed: false,
            })
            .collect::<Vec<_>>(),
    );
    register(&mut future, &waker);
    drop(waker);
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _future = future;
            panic!("outer primary");
        })),
        "outer primary",
    );
    assert_eq!(
        drops.load(Ordering::Relaxed),
        2,
        "the cancelled waker is destroyed during the unwind"
    );
    assert_next_frame(&scheduler);
}

const CASES: &[&str] = &[
    "normal completion",
    "pipeline failure",
    "teardown",
    "unwinding teardown",
    "ordinary retirement",
    "competing wake failure",
    "later successful envelope",
    "telemetry competition",
    "telemetry during abort",
    "telemetry teardown",
    "telemetry unwinding teardown",
    "pipeline preserves successful envelope",
    "post-frame preserves successful envelope",
    "pending cancellation unwind",
];
/// Runs every case in this process: no case aborts, since each envelope
/// holds at most one armed capture (ADR-0281).
pub(crate) fn completion_wake_ownership_and_recovery() {
    let failures: Vec<&str> = CASES
        .iter()
        .copied()
        .filter(|case| {
            catch_unwind(|| {
                if *case == "pending cancellation unwind" {
                    pending_cancellation_during_unwind();
                } else {
                    delivery(case);
                }
            })
            .map_err(flui_foundation::panic::discard_payload)
            .is_err()
        })
        .collect();
    assert!(failures.is_empty(), "failing cases: {failures:?}");
}
