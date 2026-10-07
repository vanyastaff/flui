//! Opaque future destructors must not replace an already authoritative failure.
//! Abort-capable cases run in child processes so every table row still executes.

use flui_scheduler::{OwnerFrame, TaskToken, UpdateScheduler};
use std::cell::RefCell;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

struct DropBomb(Arc<AtomicUsize>);

impl Drop for DropBomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("future destructor");
    }
}

enum Outcome {
    Panic,
    Ready,
    Cancel(Rc<RefCell<Option<TaskToken>>>),
    Pending,
}

struct ProbeFuture {
    outcome: Outcome,
    observed: Arc<Mutex<Option<Waker>>>,
    // Both fields have user destruction obligations. In the nested case the
    // first field panics before the nested token starts unwinding.
    _bombs: Vec<DropBomb>,
    _nested: Option<TaskToken>,
}

impl Future for ProbeFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        *self.observed.lock().expect("observed waker") = Some(cx.waker().clone());
        match &self.outcome {
            Outcome::Panic => {
                cx.waker().wake_by_ref();
                panic!("poll failure");
            }
            Outcome::Ready => Poll::Ready(()),
            Outcome::Cancel(token) => {
                let token = token.borrow_mut().take();
                token.expect("live token").cancel();
                Poll::Pending
            }
            Outcome::Pending => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

fn probe(
    outcome: Outcome,
    bombs: usize,
    drops: &Arc<AtomicUsize>,
    observed: &Arc<Mutex<Option<Waker>>>,
    nested: Option<TaskToken>,
) -> Pin<Box<ProbeFuture>> {
    Box::pin(ProbeFuture {
        outcome,
        observed: Arc::clone(observed),
        _bombs: (0..bombs).map(|_| DropBomb(Arc::clone(drops))).collect(),
        _nested: nested,
    })
}

fn assert_panic(result: std::thread::Result<()>, message: &str) {
    let payload = result.expect_err("the original failure must propagate");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some(message)
    );
}

fn assert_next_task(frame: &OwnerFrame) {
    let driver = frame.async_driver();
    let token = driver.spawn_local(Box::pin(async {}));
    assert_eq!(frame.poll_ready(), 1);
    assert_eq!(driver.pending_task_count(), 0);
    drop(token);
}

fn poll_failure(eager: bool, bombs: usize) {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(None));
    let future = probe(Outcome::Panic, bombs, &drops, &observed, None);
    if eager {
        assert_panic(
            catch_unwind(AssertUnwindSafe(|| {
                let _token = driver.spawn_local_eager(future);
            })),
            "poll failure",
        );
    } else {
        let token = driver.spawn_local(future);
        let sibling_ran = Arc::new(AtomicUsize::new(0));
        let ran = Arc::clone(&sibling_ran);
        let sibling = driver.spawn_local(Box::pin(async move {
            ran.fetch_add(1, Ordering::Relaxed);
        }));
        assert_panic(
            catch_unwind(AssertUnwindSafe(|| {
                frame.poll_ready();
            })),
            "poll failure",
        );
        assert_eq!(driver.pending_task_count(), 1);
        assert_eq!(frame.ready_task_count(), 1);
        assert_eq!(frame.poll_ready(), 1);
        assert_eq!(sibling_ran.load(Ordering::Relaxed), 1);
        drop((token, sibling));
    }
    assert_eq!(
        drops.load(Ordering::Relaxed),
        0,
        "no opaque Drop after poll failure"
    );
    let stale = observed
        .lock()
        .expect("observed waker")
        .take()
        .expect("poll waker");
    stale.wake_by_ref();
    assert_eq!(frame.ready_task_count(), 0);
    assert_eq!(frame.poll_ready(), 0);
    assert_next_task(&frame);
}

fn lazy_poll() {
    poll_failure(false, 0);
}
fn lazy_poll_one_drop() {
    poll_failure(false, 1);
}
fn lazy_poll_two_drops() {
    poll_failure(false, 2);
}
fn eager_poll() {
    poll_failure(true, 0);
}
fn eager_poll_one_drop() {
    poll_failure(true, 1);
}
fn eager_poll_two_drops() {
    poll_failure(true, 2);
}

fn token_during_unwind() {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(None));
    assert_panic(
        catch_unwind(AssertUnwindSafe(|| {
            let _token = driver.spawn_local(probe(Outcome::Pending, 2, &drops, &observed, None));
            panic!("outer failure");
        })),
        "outer failure",
    );
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert_eq!(driver.pending_task_count(), 0);
    assert_next_task(&frame);
}

fn retirement(cancel: bool, nested: bool) {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let drops = Arc::new(AtomicUsize::new(0));
    let nested_drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(None));
    let cancellation = Rc::new(RefCell::new(None));
    let nested = nested
        .then(|| driver.spawn_local(probe(Outcome::Pending, 2, &nested_drops, &observed, None)));
    let outcome = if cancel {
        Outcome::Cancel(Rc::clone(&cancellation))
    } else {
        Outcome::Ready
    };
    let token = driver.spawn_local(probe(outcome, 1, &drops, &observed, nested));
    *cancellation.borrow_mut() = Some(token);
    let sibling = driver.spawn_local(Box::pin(async {}));
    assert_panic(
        catch_unwind(AssertUnwindSafe(|| {
            frame.poll_ready();
        })),
        "future destructor",
    );
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(nested_drops.load(Ordering::Relaxed), 0);
    assert_eq!(driver.pending_task_count(), 1);
    assert_eq!(frame.ready_task_count(), 1);
    assert_eq!(frame.poll_ready(), 1);
    drop(sibling);
    assert_next_task(&frame);
}

fn ready_retirement() {
    retirement(false, false);
}
fn cancelled_retirement() {
    retirement(true, false);
}
fn nested_retirement() {
    retirement(false, true);
}

fn spawn_hook_failure(eager: bool) {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    driver.set_request_frame(|| panic!("spawn hook"));
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(None));
    assert_panic(
        catch_unwind(AssertUnwindSafe(|| {
            let future = probe(Outcome::Pending, 2, &drops, &observed, None);
            if eager {
                let _token = driver.spawn_local_eager(future);
            } else {
                let _token = driver.spawn_local(future);
            }
        })),
        "spawn hook",
    );
    assert_eq!(
        driver.pending_task_count(),
        0,
        "failed spawn must not orphan a task"
    );
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    driver.set_request_frame(|| {});
    assert_next_task(&frame);
}

fn lazy_spawn_hook() {
    spawn_hook_failure(false);
}
fn eager_spawn_hook() {
    spawn_hook_failure(true);
}

/// Counts its drops without panicking.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// The realm's frame hook is user code too: retirement drops its captures
/// under the same catch as the tasks', keeps the first panic, and refuses a
/// hook installed afterwards instead of keeping it past the realm.
fn hook_retirement() {
    let scheduler = UpdateScheduler::new();
    let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    let driver = frame.async_driver();
    let drops = Arc::new(AtomicUsize::new(0));
    let bomb = DropBomb(Arc::clone(&drops));
    driver.set_request_frame(move || {
        let _capture = &bomb;
    });
    let first = frame.retire().expect("the hook's capture panicked");
    assert_eq!(
        flui_foundation::panic::payload_text(&*first),
        Some("future destructor")
    );
    assert_eq!(drops.load(Ordering::Relaxed), 1, "dropped once, by retire");

    let late = Arc::new(AtomicUsize::new(0));
    let capture = Counted(Arc::clone(&late));
    driver.set_request_frame(move || {
        let _capture = &capture;
    });
    assert_eq!(
        late.load(Ordering::Relaxed),
        1,
        "a hook offered to a retired store is refused at once"
    );
    drop(frame);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

/// During an existing unwind the hook's captures are retained, not dropped:
/// a capture panicking in `Drop` then would abort the process.
fn hook_retirement_during_unwind() {
    let drops = Arc::new(AtomicUsize::new(0));
    assert_panic(
        catch_unwind(AssertUnwindSafe(|| {
            let scheduler = UpdateScheduler::new();
            let frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
            let bomb = DropBomb(Arc::clone(&drops));
            frame.async_driver().set_request_frame(move || {
                let _capture = &bomb;
            });
            let _frame = frame;
            panic!("outer failure");
        })),
        "outer failure",
    );
    assert_eq!(drops.load(Ordering::Relaxed), 0);
}

/// A subscriber whose every event panics.
struct PanickingSubscriber;

impl tracing::Subscriber for PanickingSubscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        panic!("subscriber");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Spawns its future through a dead driver from its own `Drop`.
struct SpawnOnDrop {
    driver: flui_scheduler::AsyncDriver,
    future: Option<Pin<Box<ProbeFuture>>>,
}

impl Drop for SpawnOnDrop {
    fn drop(&mut self) {
        let future = self.future.take().expect("spawned once");
        let token = self.driver.spawn_local(future);
        assert!(token.is_cancelled());
    }
}

/// A spawn refused because the realm is gone makes the rejected future safe
/// before any diagnostic runs: retained during an unwind, dropped once
/// otherwise, and a panicking subscriber changes neither.
fn refused_spawn() {
    let dead = {
        let scheduler = UpdateScheduler::new();
        OwnerFrame::new(&scheduler)
            .expect("the scheduler has no live owner frame")
            .async_driver()
    };
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(Mutex::new(None));
    tracing::subscriber::with_default(PanickingSubscriber, || {
        assert_panic(
            catch_unwind(AssertUnwindSafe(|| {
                let _spawn = SpawnOnDrop {
                    driver: dead.clone(),
                    future: Some(probe(Outcome::Pending, 2, &drops, &observed, None)),
                };
                panic!("outer failure");
            })),
            "outer failure",
        );
        assert_eq!(
            drops.load(Ordering::Relaxed),
            0,
            "retained during the unwind"
        );

        let late = Arc::new(AtomicUsize::new(0));
        let capture = Counted(Arc::clone(&late));
        let token = dead.spawn_local(Box::pin(async move {
            let _capture = capture;
        }));
        assert!(token.is_cancelled());
        assert_eq!(late.load(Ordering::Relaxed), 1, "dropped once, at once");
    });
}

#[test]
fn async_driver_unwind_matrix() {
    let cases: &[(&str, fn())] = &[
        ("lazy_poll", lazy_poll),
        ("lazy_poll_one_drop", lazy_poll_one_drop),
        ("lazy_poll_two_drops", lazy_poll_two_drops),
        ("eager_poll", eager_poll),
        ("eager_poll_one_drop", eager_poll_one_drop),
        ("eager_poll_two_drops", eager_poll_two_drops),
        ("token_during_unwind", token_during_unwind),
        ("ready_retirement", ready_retirement),
        ("cancelled_retirement", cancelled_retirement),
        ("nested_retirement", nested_retirement),
        ("lazy_spawn_hook", lazy_spawn_hook),
        ("eager_spawn_hook", eager_spawn_hook),
        ("hook_retirement", hook_retirement),
        (
            "hook_retirement_during_unwind",
            hook_retirement_during_unwind,
        ),
        ("refused_spawn", refused_spawn),
    ];
    if let Ok(selected) = std::env::var("FLUI_ASYNC_UNWIND_CASE") {
        let (_, case) = cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case");
        case();
        return;
    }
    let mut failures = Vec::new();
    for (name, _) in cases {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "async_driver_unwind::async_driver_unwind_matrix",
                "--exact",
                "--nocapture",
            ])
            .env("FLUI_ASYNC_UNWIND_CASE", name)
            .output()
            .expect("child test process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || !stdout.contains("running 1 test")
            || !stdout.contains("test result: ok. 1 passed; 0 failed;")
        {
            failures.push(format!(
                "{name}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
