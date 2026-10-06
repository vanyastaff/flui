//! Consumer failure boundaries for frame-completion executor envelopes.
use std::future::Future;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use flui_scheduler::{FrameCompletionFuture, FrameOutcome, IdleDeadline, UpdateScheduler};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("executor retirement");
    }
}
struct Probe {
    calls: Arc<AtomicUsize>,
    action: Box<dyn Fn() + Send + Sync>,
    _captures: Vec<Bomb>,
}
impl Wake for Probe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        (self.action)();
    }
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
    scheduler.execute_frame(&flui_scheduler::OwnerFrame::new(scheduler));
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
    let first_calls = Arc::new(AtomicUsize::new(0));
    let tail_calls = Arc::new(AtomicUsize::new(0));
    let retirement = case == "ordinary retirement";
    let preserve = case == "pipeline preserves successful envelope"
        || case == "post-frame preserves successful envelope";
    let hostile = !case.starts_with("telemetry")
        && case != "payload competition"
        && case != "later successful envelope";
    let first_waker = Waker::from(Arc::new(Probe {
        calls: Arc::clone(&first_calls),
        action: Box::new(move || {
            assert!(retirement || preserve, "wake primary");
        }),
        _captures: (0..if retirement {
            1
        } else if hostile {
            2
        } else {
            0
        })
            .map(|_| Bomb(Arc::clone(&drops)))
            .collect(),
    }));
    register(&mut first, &first_waker);
    drop(first_waker);
    let secondary_drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&secondary_drops);
    let competition = case == "payload competition";
    let tail_waker = Waker::from(Arc::new(Probe {
        calls: Arc::clone(&tail_calls),
        action: Box::new(move || {
            if competition {
                std::panic::panic_any((
                    Bomb(Arc::clone(&payload_drops)),
                    Bomb(Arc::clone(&payload_drops)),
                ));
            }
        }),
        _captures: if case == "later successful envelope" {
            vec![
                Bomb(Arc::clone(&secondary_drops)),
                Bomb(Arc::clone(&secondary_drops)),
            ]
        } else {
            Vec::new()
        },
    }));
    register(&mut later, &tail_waker);
    drop(tail_waker);
    let last_calls = Arc::new(AtomicUsize::new(0));
    let last_waker = Waker::from(Arc::new(Probe {
        calls: Arc::clone(&last_calls),
        action: Box::new(|| {}),
        _captures: Vec::new(),
    }));
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
            scheduler.drive_frame(
                &flui_scheduler::OwnerFrame::new(scheduler),
                Instant::now(),
                IdleDeadline::far_future(Instant::now()),
                || panic!("pipeline primary"),
            );
        } else if case == "direct abort" {
            scheduler
                .handle_begin_frame(Instant::now(), &flui_scheduler::OwnerFrame::new(scheduler));
            scheduler.abort_frame();
        } else {
            scheduler.execute_frame(&flui_scheduler::OwnerFrame::new(scheduler));
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
    assert_eq!(first_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        tail_calls.load(Ordering::Relaxed),
        1,
        "a failed wake must not starve the tail"
    );
    assert_eq!(
        last_calls.load(Ordering::Relaxed),
        1,
        "competing failures must not starve the final waiter"
    );
    assert_eq!(drops.load(Ordering::Relaxed), usize::from(retirement));
    assert_eq!(secondary_drops.load(Ordering::Relaxed), 0);
    assert!(
        Pin::new(&mut later)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready()
    );
    let terminal = Pin::new(&mut last).poll(&mut Context::from_waker(Waker::noop()));
    if teardown {
        assert!(matches!(terminal, Poll::Ready(Err(_))));
    } else if pipeline || case == "direct abort" {
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

fn pending_cancellation_during_unwind() {
    let scheduler = UpdateScheduler::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut future = scheduler.end_of_frame();
    let waker = Waker::from(Arc::new(Probe {
        calls: Arc::new(AtomicUsize::new(0)),
        action: Box::new(|| {}),
        _captures: vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    }));
    register(&mut future, &waker);
    drop(waker);
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _future = future;
            panic!("outer primary");
        })),
        "outer primary",
    );
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert_next_frame(&scheduler);
}

const CASES: &[&str] = &[
    "normal completion",
    "direct abort",
    "pipeline failure",
    "teardown",
    "unwinding teardown",
    "ordinary retirement",
    "payload competition",
    "later successful envelope",
    "telemetry competition",
    "telemetry during abort",
    "telemetry teardown",
    "telemetry unwinding teardown",
    "pipeline preserves successful envelope",
    "post-frame preserves successful envelope",
    "pending cancellation unwind",
];
pub(crate) fn selected_child() -> bool {
    let Ok(case) = std::env::var("FLUI_FRAME_COMPLETION_CASE") else {
        return false;
    };
    assert!(CASES.contains(&case.as_str()), "known child case");
    if case == "pending cancellation unwind" {
        pending_cancellation_during_unwind();
    } else {
        delivery(&case);
    }
    true
}
pub(crate) fn completion_wake_ownership_and_recovery() {
    let mut failures = Vec::new();
    for case in CASES {
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "end_of_frame_lifecycle::end_of_frame_demand_matrix",
                    "--nocapture",
                ])
                .env("FLUI_FRAME_COMPLETION_CASE", case)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("completion recovery child");
        let mut stdout = child.stdout.take().expect("stdout pipe");
        let mut stderr = child.stderr.take().expect("stderr pipe");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("child stdout");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("child stderr");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill hung child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success()
            || !stdout.contains("running 1 test")
            || !stdout.contains("test result: ok. 1 passed; 0 failed;")
        {
            failures.push(format!("{case}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
