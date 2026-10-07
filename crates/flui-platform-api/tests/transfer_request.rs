//! Public transport delivery, cancellation and executor recovery contracts.
use flui_platform_api::data_transfer::{
    TransferCompleter, TransferError, TransferPayload, TransferRequest,
};
use std::future::Future;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Wake, Waker};
use std::time::{Duration, Instant};

fn poll(
    request: &mut TransferRequest,
    waker: &Waker,
) -> Poll<Result<TransferPayload, TransferError>> {
    Pin::new(request).poll(&mut Context::from_waker(waker))
}
fn assert_text(request: &mut TransferRequest, expected: &str) {
    let Poll::Ready(Ok(TransferPayload::Text(text))) = poll(request, Waker::noop()) else {
        panic!("expected a delivered text payload");
    };
    assert_eq!(text, expected);
}
fn retain_failure(result: std::thread::Result<()>, expected: &str) {
    let payload = result.expect_err("the first failure escapes");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some(expected)
    );
    flui_foundation::panic::retain_opaque_payload(payload);
}
fn next_request() {
    let (mut request, completer) = TransferRequest::channel();
    assert!(poll(&mut request, Waker::noop()).is_pending());
    completer.complete(Ok(TransferPayload::Text("next".into())));
    assert_text(&mut request, "next");
}
// Arc-backed raw callbacks provide the one executor behavior safe Wake cannot
// customize: clone. Each RawWaker owns one strong reference. Borrowed callbacks
// use its live reference; consuming callbacks reconstruct and release it once.
struct CloneExecutor(Box<dyn Fn() + Send + Sync>);
#[expect(
    unsafe_code,
    reason = "exercise arbitrary executor clone through the public Future API"
)]
fn cloning_waker(action: impl Fn() + Send + Sync + 'static) -> Waker {
    unsafe fn clone(data: *const ()) -> RawWaker {
        // SAFETY: the input RawWaker owns a live Arc strong reference throughout
        // this borrowed invocation. Acquire the new reference only after the
        // hook succeeds, so a hook panic cannot strand an unpublished clone.
        let executor = unsafe { &*data.cast::<CloneExecutor>() };
        (executor.0)();
        unsafe { Arc::increment_strong_count(data.cast::<CloneExecutor>()) };
        RawWaker::new(data, &VTABLE)
    }
    unsafe fn wake(data: *const ()) {
        // SAFETY: consuming wake receives exactly the one reference this raw
        // waker owns; it has not been reconstructed by another consuming call.
        drop(unsafe { Arc::from_raw(data.cast::<CloneExecutor>()) });
    }
    unsafe fn wake_by_ref(_: *const ()) {}
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, wake);
    let data = Arc::into_raw(Arc::new(CloneExecutor(Box::new(action)))).cast();
    // SAFETY: data holds an Arc allocation with one strong reference transferred
    // to the waker, and the vtable preserves thread-safe Arc ownership above.
    unsafe { Waker::from_raw(RawWaker::new(data, &VTABLE)) }
}

fn clone_reenters_completion() {
    let (mut request, completer) = TransferRequest::channel();
    let owner = Arc::new(Mutex::new(Some(completer)));
    let clone_owner = Arc::clone(&owner);
    let waker = cloning_waker(move || {
        let completer = clone_owner
            .lock()
            .expect("owner")
            .take()
            .expect("live owner");
        assert!(!completer.is_cancelled());
        completer.complete(Ok(TransferPayload::Text("reentered".into())));
    });
    let Poll::Ready(Ok(TransferPayload::Text(text))) = poll(&mut request, &waker) else {
        panic!("completion during clone must be visible");
    };
    assert_eq!(text, "reentered");
    next_request();
}
struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("capture retirement");
    }
}
struct Executor {
    calls: Arc<AtomicUsize>,
    fail: bool,
    _captures: Vec<Bomb>,
}
impl Wake for Executor {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(!self.fail, "wake secondary");
    }
}
fn executor(calls: &Arc<AtomicUsize>, fail: bool, captures: Vec<Bomb>) -> Waker {
    Waker::from(Arc::new(Executor {
        calls: Arc::clone(calls),
        fail,
        _captures: captures,
    }))
}
fn clone_failure_preserves_registration() {
    let (mut request, completer) = TransferRequest::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let previous = executor(&calls, false, vec![]);
    assert!(poll(&mut request, &previous).is_pending());
    let failing = cloning_waker(|| panic!("clone primary"));
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = poll(&mut request, &failing);
        })),
        "clone primary",
    );
    completer.complete(Ok(TransferPayload::Text("survived".into())));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_text(&mut request, "survived");
    next_request();
}
fn wake_failure_retains_aggregate_and_delivery() {
    let (mut request, completer) = TransferRequest::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = executor(
        &calls,
        true,
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    assert!(poll(&mut request, &waker).is_pending());
    drop(waker);
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| {
            completer.complete(Ok(TransferPayload::Text("durable".into())));
        })),
        "wake secondary",
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_text(&mut request, "durable");
    next_request();
}
fn owner_disconnect_during_unwind_preserves_primary() {
    let (mut request, completer) = TransferRequest::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = executor(
        &calls,
        true,
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    assert!(poll(&mut request, &waker).is_pending());
    drop(waker);
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _producer = completer;
            panic!("producer primary");
        })),
        "producer primary",
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(matches!(
        poll(&mut request, Waker::noop()),
        Poll::Ready(Err(TransferError::SourceGone))
    ));
    next_request();
}
fn cancellation_during_unwind_preserves_primary() {
    let (mut request, completer) = TransferRequest::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = executor(
        &calls,
        true,
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    assert!(poll(&mut request, &waker).is_pending());
    drop(waker);
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _consumer = request;
            panic!("consumer primary");
        })),
        "consumer primary",
    );
    assert!(completer.is_cancelled());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    completer.complete(Ok(TransferPayload::Text("discarded".into())));
    next_request();
}
fn cancellation_wake_failure_preserves_cancelled_state() {
    let (mut request, completer) = TransferRequest::channel();
    let drops = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = executor(
        &calls,
        true,
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    assert!(poll(&mut request, &waker).is_pending());
    drop(waker);
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| drop(request))),
        "wake secondary",
    );
    assert!(completer.is_cancelled());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    completer.complete(Ok(TransferPayload::Text("discarded".into())));
    next_request();
}
struct ReenterOnRetirement(Arc<Mutex<Option<TransferCompleter>>>);
#[expect(
    clippy::manual_noop_waker,
    reason = "executor retirement must invoke the reentrant Drop hook; Waker::noop has none"
)]
impl Wake for ReenterOnRetirement {
    fn wake(self: Arc<Self>) {}
}
impl Drop for ReenterOnRetirement {
    fn drop(&mut self) {
        assert!(
            !self
                .0
                .lock()
                .expect("owner")
                .as_ref()
                .expect("producer")
                .is_cancelled()
        );
    }
}
fn displaced_executor_retirement_reenters_producer() {
    let (mut request, completer) = TransferRequest::channel();
    let owner = Arc::new(Mutex::new(Some(completer)));
    let previous = Waker::from(Arc::new(ReenterOnRetirement(Arc::clone(&owner))));
    assert!(poll(&mut request, &previous).is_pending());
    drop(previous);
    assert!(poll(&mut request, Waker::noop()).is_pending());
    owner
        .lock()
        .expect("owner")
        .take()
        .expect("producer")
        .complete(Ok(TransferPayload::Text("replacement".into())));
    assert_text(&mut request, "replacement");
    next_request();
}
fn ready_skips_executor_clone() {
    let mut request = TransferRequest::ready(Ok(TransferPayload::Text("ready".into())));
    let waker = cloning_waker(|| panic!("ready must not clone"));
    assert!(matches!(
        poll(&mut request, &waker),
        Poll::Ready(Ok(TransferPayload::Text(_)))
    ));
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = poll(&mut request, Waker::noop());
        })),
        "BUG: TransferRequest polled again after it resolved",
    );
    next_request();
}
fn cancellation_and_disconnect_resolve() {
    fn require_send<T: Send>(_: &T) {}
    let (request, completer) = TransferRequest::channel();
    require_send(&request);
    require_send(&completer);
    drop(request);
    assert!(completer.is_cancelled());
    completer.complete(Ok(TransferPayload::Text("discarded".into())));
    let (mut request, completer) = TransferRequest::channel();
    assert!(poll(&mut request, Waker::noop()).is_pending());
    drop(completer);
    assert!(matches!(
        poll(&mut request, Waker::noop()),
        Poll::Ready(Err(TransferError::SourceGone))
    ));
    retain_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = poll(&mut request, Waker::noop());
        })),
        "BUG: TransferRequest polled again after it resolved",
    );
    next_request();
}
#[test]
fn transfer_request_recovery() {
    let cases: &[(&str, fn())] = &[
        ("clone reenters completion", clone_reenters_completion),
        (
            "clone failure preserves registration",
            clone_failure_preserves_registration,
        ),
        (
            "wake failure retains aggregate and delivery",
            wake_failure_retains_aggregate_and_delivery,
        ),
        (
            "owner disconnect during unwind",
            owner_disconnect_during_unwind_preserves_primary,
        ),
        (
            "cancellation during unwind",
            cancellation_during_unwind_preserves_primary,
        ),
        (
            "cancellation wake failure",
            cancellation_wake_failure_preserves_cancelled_state,
        ),
        (
            "displaced executor retirement reenters producer",
            displaced_executor_retirement_reenters_producer,
        ),
        ("ready skips executor clone", ready_skips_executor_clone),
        (
            "cancellation and disconnect resolve",
            cancellation_and_disconnect_resolve,
        ),
    ];
    const SELECTED: &str = "FLUI_TRANSFER_REQUEST_RECOVERY_CASE";
    if let Ok(selected) = std::env::var(SELECTED) {
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
                    "transfer_request::transfer_request_recovery",
                    "--nocapture",
                ])
                .env(SELECTED, name)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("transfer request child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
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
                child.kill().expect("kill deadlocked child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("child exit");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success() || !stdout.contains("1 passed; 0 failed") {
            failures.push(format!("{name}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
