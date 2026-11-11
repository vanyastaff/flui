//! Consumer-visible continuation ownership and terminal waiter recovery.
use super::AnimationRunFuture;
use flui_foundation::testing::CountingWaker;
use std::future::Future;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("capture retirement");
    }
}
struct Counted(Arc<AtomicUsize>);
impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
fn register(future: &mut AnimationRunFuture, waker: &Waker) {
    assert!(
        Pin::new(future)
            .poll(&mut Context::from_waker(waker))
            .is_pending()
    );
}
fn assert_failure(result: std::thread::Result<()>, expected: &str) {
    let payload = result.expect_err("delivery failed");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some(expected)
    );
    flui_foundation::panic::discard_payload(payload);
}
fn assert_next_run() {
    let (completer, mut future) = AnimationRunFuture::pending();
    let calls = CountingWaker::new().shared();
    let wake = calls.waker();
    register(&mut future, &wake);
    completer.complete().deliver();
    assert_eq!(calls.count(), 1);
    assert_eq!(
        Pin::new(&mut future).poll(&mut Context::from_waker(&wake)),
        Poll::Ready(Ok(()))
    );
}

fn competing_payloads(mode: &str) {
    let (completer, mut future) = AnimationRunFuture::pending();
    let tail = Arc::new(AtomicUsize::new(0));
    let tail_calls = Arc::clone(&tail);
    future.when_complete_or_cancel(|_| panic!("primary continuation"));
    future.when_complete_or_cancel(move |_| {
        panic!("secondary continuation");
    });
    future.when_complete_or_cancel(move |_| {
        tail_calls.fetch_add(1, Ordering::Relaxed);
    });
    let wakes = CountingWaker::new().shared();
    let wake = wakes.waker();
    register(&mut future, &wake);
    let result = catch_unwind(AssertUnwindSafe(|| match mode {
        "complete" => completer.complete().deliver(),
        "cancel" => completer.cancel().deliver(),
        "drop delivery" => drop(completer.complete()),
        "drop completer" => drop(completer),
        "unwind" => {
            let _completer = completer;
            panic!("outer failure");
        }
        _ => panic!("unknown mode"),
    }));
    assert_failure(
        result,
        if mode == "unwind" {
            "outer failure"
        } else {
            "primary continuation"
        },
    );
    assert_eq!(tail.load(Ordering::Relaxed), 1);
    assert_eq!(wakes.count(), 1);
    let terminal = Pin::new(&mut future).poll(&mut Context::from_waker(&wake));
    if matches!(mode, "complete" | "drop delivery") {
        assert_eq!(terminal, Poll::Ready(Ok(())));
    } else {
        assert!(matches!(terminal, Poll::Ready(Err(_))));
    }
    assert_next_run();
}

fn captured_callback_failure(resolved: bool) {
    let drops = Arc::new(AtomicUsize::new(0));
    let captures = (Counted(Arc::clone(&drops)), Counted(Arc::clone(&drops)));
    if resolved {
        let (completer, future) = AnimationRunFuture::pending();
        completer.cancel().deliver();
        assert_failure(
            catch_unwind(AssertUnwindSafe(|| {
                future.when_complete_or_cancel(move |outcome| {
                    let _keep = &captures;
                    assert!(outcome.is_err());
                    panic!("callback body");
                });
            })),
            "callback body",
        );
    } else {
        let (completer, future) = AnimationRunFuture::pending();
        future.when_complete_or_cancel(move |_| {
            let _keep = &captures;
            panic!("callback body");
        });
        assert_failure(
            catch_unwind(AssertUnwindSafe(|| completer.complete().deliver())),
            "callback body",
        );
    }
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_next_run();
}

fn ordinary_capture_retirement_keeps_the_tail_live() {
    let (completer, future) = AnimationRunFuture::pending();
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Bomb(Arc::clone(&drops));
    future.when_complete_or_cancel(move |_| {
        let _keep = &capture;
    });
    let tail = Arc::new(AtomicUsize::new(0));
    let tail_calls = Arc::clone(&tail);
    future.when_complete_or_cancel(move |_| {
        tail_calls.fetch_add(1, Ordering::Relaxed);
    });
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| completer.cancel().deliver())),
        "capture retirement",
    );
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(tail.load(Ordering::Relaxed), 1);
    assert_next_run();
}

fn waiter_failure_keeps_later_waiters_live(primary: bool, retirement: bool) {
    let (completer, mut first) = AnimationRunFuture::pending();
    let mut later = first.clone();
    if primary {
        first.when_complete_or_cancel(|_| panic!("primary continuation"));
    }
    // The test reads the count after the waker's retirement ran, so the counter is a second
    // waker the probe forwards to: the probe itself is held by nothing but its `Waker`.
    let first_calls = CountingWaker::new().shared();
    let later_calls = CountingWaker::new().shared();
    let drops = Arc::new(AtomicUsize::new(0));
    let first_count = first_calls.waker();
    let probe = CountingWaker::calling(move || {
        first_count.wake_by_ref();
        assert!(retirement, "waker failure");
    });
    let first_waker = if retirement {
        probe.holding(Bomb(Arc::clone(&drops)))
    } else {
        probe
    }
    .shared()
    .waker();
    let later_waker = later_calls.waker();
    register(&mut first, &first_waker);
    register(&mut later, &later_waker);
    drop(first_waker);
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| completer.complete().deliver())),
        if primary {
            "primary continuation"
        } else if retirement {
            "capture retirement"
        } else {
            "waker failure"
        },
    );
    assert_eq!(first_calls.count(), 1);
    assert_eq!(later_calls.count(), 1);
    assert_eq!(drops.load(Ordering::Relaxed), usize::from(retirement));
    assert_eq!(
        Pin::new(&mut later).poll(&mut Context::from_waker(&later_waker)),
        Poll::Ready(Ok(()))
    );
    assert_next_run();
}

fn independent_replaced_and_dropped_waiters() {
    let (completer, mut first) = AnimationRunFuture::pending();
    let mut second = first.clone();
    let mut dropped = first.clone();
    let old_calls = CountingWaker::new().shared();
    let new_calls = CountingWaker::new().shared();
    let second_calls = CountingWaker::new().shared();
    let dropped_calls = CountingWaker::new().shared();
    let old = old_calls.waker();
    let replacement = new_calls.waker();
    let sibling = second_calls.waker();
    struct ReleasedResource(Arc<AtomicUsize>);
    impl Drop for ReleasedResource {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let released = Arc::new(AtomicUsize::new(0));
    let resource = ReleasedResource(Arc::clone(&released));
    // The resource goes with the waker the future drops; the counter is a separate waker it
    // forwards to, so the test can read it after that waker is gone.
    let dropped_count = dropped_calls.waker();
    let canceled = CountingWaker::calling(move || dropped_count.wake_by_ref())
        .holding(resource)
        .shared()
        .waker();
    register(&mut first, &old);
    register(&mut first, &replacement);
    register(&mut first, &replacement);
    register(&mut second, &sibling);
    register(&mut dropped, &canceled);
    drop(canceled);
    drop(dropped);
    assert_eq!(
        released.load(Ordering::Relaxed),
        1,
        "a dropped waiter releases its registration immediately"
    );
    completer.complete().deliver();
    assert_eq!(old_calls.count(), 0);
    assert_eq!(new_calls.count(), 1);
    assert_eq!(second_calls.count(), 1);
    assert_eq!(dropped_calls.count(), 0);
    assert_next_run();
}

fn inline_wake_can_poll_and_drop_its_future() {
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static FUTURE: std::cell::RefCell<Option<AnimationRunFuture>> = const { std::cell::RefCell::new(None) };
    }
    let (completer, future) = AnimationRunFuture::pending();
    FUTURE.with(|slot| *slot.borrow_mut() = Some(future));
    let owner_thread = std::thread::current().id();
    let calls = CountingWaker::calling(move || {
        assert_eq!(owner_thread, std::thread::current().id());
        let mut future = FUTURE
            .with(|slot| slot.borrow_mut().take())
            .expect("live future");
        assert_eq!(
            Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(()))
        );
        drop(future);
    })
    .shared();
    let wake = calls.waker();
    FUTURE.with(|slot| register(slot.borrow_mut().as_mut().expect("live future"), &wake));
    completer.complete().deliver();
    assert_eq!(calls.count(), 1);
    FUTURE.with(|slot| assert!(slot.borrow().is_none()));
    assert_next_run();
}

fn replaced_waker_retirement_can_reenter_registration() {
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static FUTURE: std::cell::RefCell<Option<AnimationRunFuture>> = const { std::cell::RefCell::new(None) };
    }
    struct RegisterOnDrop {
        owner_thread: std::thread::ThreadId,
        calls: Arc<AtomicUsize>,
    }
    impl Drop for RegisterOnDrop {
        fn drop(&mut self) {
            assert_eq!(self.owner_thread, std::thread::current().id());
            let mut future = FUTURE
                .with(|slot| slot.borrow_mut().take())
                .expect("live nested future");
            register(&mut future, Waker::noop());
            drop(future);
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }
    let (completer, mut future) = AnimationRunFuture::pending();
    FUTURE.with(|slot| *slot.borrow_mut() = Some(future.clone()));
    let retired = Arc::new(AtomicUsize::new(0));
    let capture = RegisterOnDrop {
        owner_thread: std::thread::current().id(),
        calls: Arc::clone(&retired),
    };
    let old = CountingWaker::new().holding(capture).shared().waker();
    register(&mut future, &old);
    drop(old);
    register(&mut future, Waker::noop());
    assert_eq!(retired.load(Ordering::Relaxed), 1);
    FUTURE.with(|slot| assert!(slot.borrow().is_none()));
    completer.complete().deliver();
    assert_next_run();
}

fn publication_and_registration_orderings() {
    for publish_first in [false, true] {
        let (completer, mut future) = AnimationRunFuture::pending();
        let calls = CountingWaker::new().shared();
        let wake = calls.waker();
        let mut completer = Some(completer);
        if publish_first {
            completer
                .take()
                .expect("unpublished outcome")
                .cancel()
                .deliver();
        }
        let initial = Pin::new(&mut future).poll(&mut Context::from_waker(&wake));
        if let Some(completer) = completer {
            completer.cancel().deliver();
        }
        if initial.is_pending() {
            assert_eq!(calls.count(), 1);
        }
        assert!(matches!(
            Pin::new(&mut future).poll(&mut Context::from_waker(&wake)),
            Poll::Ready(Err(_))
        ));
    }
}

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
        panic!("telemetry failure");
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
fn telemetry_failure_stays_secondary() {
    tracing::subscriber::with_default(PanickingSubscriber, || {
        let (completer, future) = AnimationRunFuture::pending();
        let tail = Arc::new(AtomicUsize::new(0));
        let tail_calls = Arc::clone(&tail);
        future.when_complete_or_cancel(|_| panic!("primary continuation"));
        future.when_complete_or_cancel(move |_| {
            tail_calls.fetch_add(1, Ordering::Relaxed);
        });
        assert_failure(
            catch_unwind(AssertUnwindSafe(|| completer.complete().deliver())),
            "primary continuation",
        );
        assert_eq!(tail.load(Ordering::Relaxed), 1);
    });
    assert_next_run();
}

#[test]
fn run_delivery_recovery() {
    let cases: &[(&str, fn())] = &[
        ("complete payload competition", || {
            competing_payloads("complete");
        }),
        ("cancel payload competition", || {
            competing_payloads("cancel");
        }),
        ("implicit delivery payload competition", || {
            competing_payloads("drop delivery");
        }),
        ("implicit cancellation payload competition", || {
            competing_payloads("drop completer");
        }),
        ("unwinding cancellation payload competition", || {
            competing_payloads("unwind");
        }),
        ("pending callback captures", || {
            captured_callback_failure(false);
        }),
        ("resolved callback captures", || {
            captured_callback_failure(true);
        }),
        (
            "ordinary callback retirement",
            ordinary_capture_retirement_keeps_the_tail_live,
        ),
        ("panicking waker tail", || {
            waiter_failure_keeps_later_waiters_live(false, false);
        }),
        ("primary failure then waker failure", || {
            waiter_failure_keeps_later_waiters_live(true, false);
        }),
        ("ordinary waker retirement", || {
            waiter_failure_keeps_later_waiters_live(false, true);
        }),
        (
            "replacement independent clones and cancellation",
            independent_replaced_and_dropped_waiters,
        ),
        (
            "inline wake polling and dropping",
            inline_wake_can_poll_and_drop_its_future,
        ),
        (
            "replacement retirement reenters registration",
            replaced_waker_retirement_can_reenter_registration,
        ),
        (
            "publication registration orderings",
            publication_and_registration_orderings,
        ),
        ("telemetry competition", telemetry_failure_stays_secondary),
    ];
    if let Ok(selected) = std::env::var("FLUI_TICKER_RECOVERY_CASE") {
        cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("known child case")
            .1();
        return;
    }
    let mut failures = Vec::new();
    for (name, _) in cases {
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "run_future::recovery_tests::run_delivery_recovery",
                    "--nocapture",
                ])
                .env("FLUI_TICKER_RECOVERY_CASE", name)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("ticker recovery child");
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
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
