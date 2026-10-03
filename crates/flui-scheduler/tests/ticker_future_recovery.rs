//! Consumer-visible continuation ownership and terminal waiter recovery.
use flui_scheduler::ticker::TickerFuture;
use std::future::Future;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("capture retirement");
    }
}
struct ProbeWake {
    calls: Arc<AtomicUsize>,
    action: Box<dyn Fn() + Send + Sync>,
    _retirement: Option<Bomb>,
}
impl Wake for ProbeWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        (self.action)();
    }
}
fn waker(calls: &Arc<AtomicUsize>, action: impl Fn() + Send + Sync + 'static) -> Waker {
    Waker::from(Arc::new(ProbeWake {
        calls: Arc::clone(calls),
        action: Box::new(action),
        _retirement: None,
    }))
}
fn register(future: &mut TickerFuture, waker: &Waker) {
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
    flui_foundation::panic::retain_opaque_payload(payload);
}
fn assert_next_run() {
    let (completer, mut future) = TickerFuture::pending();
    let calls = Arc::new(AtomicUsize::new(0));
    let wake = waker(&calls, || {});
    register(&mut future, &wake);
    completer.complete().deliver();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        Pin::new(&mut future).poll(&mut Context::from_waker(&wake)),
        Poll::Ready(Ok(()))
    );
}

fn competing_payloads(mode: &str) {
    let (completer, mut future) = TickerFuture::pending();
    let drops = Arc::new(AtomicUsize::new(0));
    let secondary_drops = Arc::clone(&drops);
    let tail = Arc::new(AtomicUsize::new(0));
    let tail_calls = Arc::clone(&tail);
    future.when_complete_or_cancel(|_| panic!("primary continuation"));
    future.when_complete_or_cancel(move |_| {
        std::panic::panic_any((
            Bomb(Arc::clone(&secondary_drops)),
            Bomb(Arc::clone(&secondary_drops)),
        ));
    });
    future.when_complete_or_cancel(move |_| {
        tail_calls.fetch_add(1, Ordering::Relaxed);
    });
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake = waker(&wakes, || {});
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
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert_eq!(tail.load(Ordering::Relaxed), 1);
    assert_eq!(wakes.load(Ordering::Relaxed), 1);
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
    let captures = (Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops)));
    if resolved {
        let future = TickerFuture::canceled();
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
        let (completer, future) = TickerFuture::pending();
        future.when_complete_or_cancel(move |_| {
            let _keep = &captures;
            panic!("callback body");
        });
        assert_failure(
            catch_unwind(AssertUnwindSafe(|| completer.complete().deliver())),
            "callback body",
        );
    }
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert_next_run();
}

fn ordinary_capture_retirement_keeps_the_tail_live() {
    let (completer, future) = TickerFuture::pending();
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
    let (completer, mut first) = TickerFuture::pending();
    let mut later = first.clone();
    if primary {
        first.when_complete_or_cancel(|_| panic!("primary continuation"));
    }
    let first_calls = Arc::new(AtomicUsize::new(0));
    let later_calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let first_waker = Waker::from(Arc::new(ProbeWake {
        calls: Arc::clone(&first_calls),
        action: Box::new(move || {
            assert!(retirement, "waker failure");
        }),
        _retirement: retirement.then(|| Bomb(Arc::clone(&drops))),
    }));
    let later_waker = waker(&later_calls, || {});
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
    assert_eq!(first_calls.load(Ordering::Relaxed), 1);
    assert_eq!(later_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        drops.load(Ordering::Relaxed),
        usize::from(retirement && !primary)
    );
    assert_eq!(
        Pin::new(&mut later).poll(&mut Context::from_waker(&later_waker)),
        Poll::Ready(Ok(()))
    );
    assert_next_run();
}

fn hostile_waker_captures_are_retained(unwinding: bool) {
    struct HostileWake {
        _captures: (Bomb, Bomb),
    }
    impl Wake for HostileWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            panic!("hostile waker failure");
        }
    }
    let (completer, mut first) = TickerFuture::pending();
    let mut later = first.clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let hostile = Waker::from(Arc::new(HostileWake {
        _captures: (Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))),
    }));
    register(&mut first, &hostile);
    drop(hostile);
    let calls = Arc::new(AtomicUsize::new(0));
    let wake = waker(&calls, || {});
    register(&mut later, &wake);
    let result = catch_unwind(AssertUnwindSafe(|| {
        if unwinding {
            let _completer = completer;
            panic!("outer failure");
        }
        completer.cancel().deliver();
    }));
    assert_failure(
        result,
        if unwinding {
            "outer failure"
        } else {
            "hostile waker failure"
        },
    );
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(first.is_canceled());
    assert!(matches!(
        Pin::new(&mut later).poll(&mut Context::from_waker(&wake)),
        Poll::Ready(Err(_))
    ));
    assert_next_run();
}

fn independent_replaced_and_dropped_waiters() {
    let (completer, mut first) = TickerFuture::pending();
    let mut second = first.clone();
    let mut dropped = first.clone();
    let old_calls = Arc::new(AtomicUsize::new(0));
    let new_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let dropped_calls = Arc::new(AtomicUsize::new(0));
    let old = waker(&old_calls, || {});
    let replacement = waker(&new_calls, || {});
    let sibling = waker(&second_calls, || {});
    struct ReleasedResource(Arc<AtomicUsize>);
    impl Drop for ReleasedResource {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let released = Arc::new(AtomicUsize::new(0));
    let resource = ReleasedResource(Arc::clone(&released));
    let canceled = waker(&dropped_calls, move || {
        let _keep = &resource;
    });
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
    assert_eq!(old_calls.load(Ordering::Relaxed), 0);
    assert_eq!(new_calls.load(Ordering::Relaxed), 1);
    assert_eq!(second_calls.load(Ordering::Relaxed), 1);
    assert_eq!(dropped_calls.load(Ordering::Relaxed), 0);
    assert_next_run();
}

fn inline_wake_can_poll_and_drop_its_future() {
    let (completer, future) = TickerFuture::pending();
    let holder = Arc::new(Mutex::new(Some(future)));
    let target = Arc::clone(&holder);
    let calls = Arc::new(AtomicUsize::new(0));
    let wake = waker(&calls, move || {
        let mut future = target
            .lock()
            .expect("future holder")
            .take()
            .expect("live future");
        assert_eq!(
            Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(()))
        );
        drop(future);
    });
    register(
        holder
            .lock()
            .expect("future holder")
            .as_mut()
            .expect("live future"),
        &wake,
    );
    completer.complete().deliver();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(holder.lock().expect("future holder").is_none());
    assert_next_run();
}

fn replaced_waker_retirement_can_reenter_registration() {
    struct RegisterOnDrop {
        future: Arc<Mutex<Option<TickerFuture>>>,
        calls: Arc<AtomicUsize>,
    }
    impl Drop for RegisterOnDrop {
        fn drop(&mut self) {
            let mut future = self
                .future
                .lock()
                .expect("nested future")
                .take()
                .expect("live nested future");
            register(&mut future, Waker::noop());
            drop(future);
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }
    let (completer, mut future) = TickerFuture::pending();
    let holder = Arc::new(Mutex::new(Some(future.clone())));
    let retired = Arc::new(AtomicUsize::new(0));
    let capture = RegisterOnDrop {
        future: Arc::clone(&holder),
        calls: Arc::clone(&retired),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let old = waker(&calls, move || {
        let _keep = &capture;
    });
    register(&mut future, &old);
    drop(old);
    register(&mut future, Waker::noop());
    assert_eq!(retired.load(Ordering::Relaxed), 1);
    assert!(holder.lock().expect("nested future").is_none());
    completer.complete().deliver();
    assert_next_run();
}

fn publication_and_registration_race() {
    for _ in 0..32 {
        let (completer, mut future) = TickerFuture::pending();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            completer.cancel().deliver();
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let wake = waker(&calls, || {});
        barrier.wait();
        let initial = Pin::new(&mut future).poll(&mut Context::from_waker(&wake));
        worker.join().expect("resolver");
        if initial.is_pending() {
            assert_eq!(calls.load(Ordering::Relaxed), 1);
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
        let (completer, future) = TickerFuture::pending();
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
fn ticker_future_delivery_recovery() {
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
        ("hostile waker captured aggregate", || {
            hostile_waker_captures_are_retained(false);
        }),
        ("unwinding hostile waker captured aggregate", || {
            hostile_waker_captures_are_retained(true);
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
            "publication registration race",
            publication_and_registration_race,
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
                    "ticker_future_recovery::ticker_future_delivery_recovery",
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
