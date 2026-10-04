//! Executor and owner failures around the public request/reply contract.
use flui_foundation::{ClaimHandle, ClaimOutcome, claim_slot};
use std::future::Future;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Wake, Waker};
use std::time::{Duration, Instant};

struct Bomb(Arc<AtomicUsize>);
impl Drop for Bomb {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("envelope retirement");
    }
}
struct Executor {
    calls: Arc<AtomicUsize>,
    action: Box<dyn Fn() + Send + Sync>,
    _captures: Vec<Bomb>,
}
impl Wake for Executor {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.action)();
    }
}
fn executor(
    calls: &Arc<AtomicUsize>,
    action: impl Fn() + Send + Sync + 'static,
    captures: Vec<Bomb>,
) -> Waker {
    Waker::from(Arc::new(Executor {
        calls: Arc::clone(calls),
        action: Box::new(action),
        _captures: captures,
    }))
}
fn register<T>(handle: &mut ClaimHandle<T>, waker: &Waker) {
    assert!(
        Pin::new(handle)
            .poll(&mut Context::from_waker(waker))
            .is_pending()
    );
}
fn assert_failure(result: std::thread::Result<()>, expected: &str) {
    let payload = result.expect_err("the first failure escapes");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some(expected)
    );
    flui_foundation::panic::retain_opaque_payload(payload);
}
fn next_request() {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    register(&mut handle, Waker::noop());
    slot.deliver(37).expect("pending request");
    assert_eq!(handle.try_take(), Some(37));
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

fn clone_reenters_delivery() {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    let owner = Arc::new(Mutex::new(Some(slot)));
    let clone_owner = Arc::clone(&owner);
    let waker = cloning_waker(move || {
        clone_owner
            .lock()
            .expect("owner")
            .as_ref()
            .expect("live owner")
            .deliver(41)
            .expect("pending during clone");
    });
    assert_eq!(
        Pin::new(&mut handle).poll(&mut Context::from_waker(&waker)),
        Poll::Ready(ClaimOutcome::Delivered(41))
    );
    drop(owner.lock().expect("owner").take());
    next_request();
}

fn clone_failure_keeps_the_previous_registration() {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    let calls = Arc::new(AtomicUsize::new(0));
    let previous = executor(&calls, || {}, Vec::new());
    register(&mut handle, &previous);
    let failing = cloning_waker(|| panic!("clone primary"));
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = Pin::new(&mut handle).poll(&mut Context::from_waker(&failing));
        })),
        "clone primary",
    );
    slot.deliver(43).expect("pending after clone failure");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(handle.try_take(), Some(43));
    next_request();
}

fn replacement_retirement_keeps_the_new_registration() {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    let drops = Arc::new(AtomicUsize::new(0));
    let previous = executor(
        &Arc::new(AtomicUsize::new(0)),
        || {},
        vec![Bomb(Arc::clone(&drops))],
    );
    register(&mut handle, &previous);
    drop(previous);
    let calls = Arc::new(AtomicUsize::new(0));
    let replacement = executor(&calls, || {}, Vec::new());
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = Pin::new(&mut handle).poll(&mut Context::from_waker(&replacement));
        })),
        "envelope retirement",
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    slot.deliver(47).expect("pending after retirement failure");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(handle.try_take(), Some(47));
    next_request();
}

fn delivered_wake_failure(opaque: bool) {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    let drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&drops);
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = executor(
        &calls,
        move || {
            if opaque {
                std::panic::panic_any((
                    Bomb(Arc::clone(&payload_drops)),
                    Bomb(Arc::clone(&payload_drops)),
                ));
            }
            panic!("wake primary");
        },
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    register(&mut handle, &waker);
    drop(waker);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        slot.deliver(53).expect("pending");
    }));
    if opaque {
        flui_foundation::panic::retain_opaque_payload(failure.expect_err("opaque wake failure"));
    } else {
        assert_failure(failure, "wake primary");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        handle.try_take(),
        Some(53),
        "delivery committed before waking"
    );
    next_request();
}

fn successful_wake_retirement_preserves_the_delivery() {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let waker = executor(&calls, || {}, vec![Bomb(Arc::clone(&drops))]);
    register(&mut handle, &waker);
    drop(waker);
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            slot.deliver(61).expect("pending");
        })),
        "envelope retirement",
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.try_take(), Some(61));
    next_request();
}

fn owner_disconnect(unwinding: bool, wake_fails: bool) {
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(|| {}));
    let drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&drops);
    let calls = Arc::new(AtomicUsize::new(0));
    let waker = executor(
        &calls,
        move || {
            if wake_fails {
                if unwinding {
                    std::panic::panic_any((
                        Bomb(Arc::clone(&payload_drops)),
                        Bomb(Arc::clone(&payload_drops)),
                    ));
                }
                panic!("disconnect wake");
            }
        },
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    register(&mut handle, &waker);
    drop(waker);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let _owner = slot;
        assert!(!unwinding, "owner primary");
    }));
    assert_failure(
        failure,
        if unwinding {
            "owner primary"
        } else {
            "disconnect wake"
        },
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        Pin::new(&mut handle).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(ClaimOutcome::OwnerGone)
    );
    next_request();
}

fn abandonment_competition(owner_fails: bool, task_fails: bool, unwinding: bool) {
    let drops = Arc::new(AtomicUsize::new(0));
    let owner_calls = Arc::new(AtomicUsize::new(0));
    let owner_count = Arc::clone(&owner_calls);
    let captures = (Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops)));
    let (slot, mut handle) = claim_slot::<u32>(Arc::new(move || {
        let _captures = &captures;
        owner_count.fetch_add(1, Ordering::SeqCst);
        assert!(!owner_fails, "abandonment primary");
    }));
    let calls = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&drops);
    let waker = executor(
        &calls,
        move || {
            if task_fails {
                if owner_fails || unwinding {
                    std::panic::panic_any((
                        Bomb(Arc::clone(&payload_drops)),
                        Bomb(Arc::clone(&payload_drops)),
                    ));
                }
                panic!("task primary");
            }
        },
        vec![Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))],
    );
    register(&mut handle, &waker);
    drop(waker);
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _requester = handle;
            assert!(!unwinding, "outer primary");
        })),
        if unwinding {
            "outer primary"
        } else if owner_fails {
            "abandonment primary"
        } else {
            "task primary"
        },
    );
    assert_eq!(owner_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "owner failure cannot starve the task wake"
    );
    assert_eq!(
        slot.deliver(59),
        Err(59),
        "cancelled request returns the owned result"
    );
    assert!(slot.is_settled());
    drop(slot);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    next_request();
}

fn unwinding_retains_an_unclaimed_result() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (slot, handle) = claim_slot(Arc::new(|| {}));
    assert!(
        slot.deliver((Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops))))
            .is_ok()
    );
    assert_failure(
        catch_unwind(AssertUnwindSafe(|| {
            let _requester = handle;
            panic!("result primary");
        })),
        "result primary",
    );
    assert!(slot.is_abandoned());
    let result = slot
        .take_abandoned()
        .expect("the owner can still reclaim the result");
    std::mem::forget(result);
    assert!(slot.is_settled());
    drop(slot);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    next_request();
}

fn wake_failure_retains_captures() {
    delivered_wake_failure(false);
}
fn opaque_wake_failure() {
    delivered_wake_failure(true);
}
fn owner_disconnect_wake_failure() {
    owner_disconnect(false, true);
}
fn owner_unwind_successful_wake() {
    owner_disconnect(true, false);
}
fn owner_unwind_wake_competition() {
    owner_disconnect(true, true);
}
fn abandonment_failure_healthy_task() {
    abandonment_competition(true, false, false);
}
fn abandonment_healthy_owner_task_failure() {
    abandonment_competition(false, true, false);
}
fn abandonment_owner_and_task_competition() {
    abandonment_competition(true, true, false);
}
fn abandonment_during_unwind() {
    abandonment_competition(false, true, true);
}
fn abandonment_all_failures() {
    abandonment_competition(true, true, true);
}

#[test]
fn claim_slot_executor_and_owner_recovery() {
    let cases: &[(&str, fn())] = &[
        ("clone reenters delivery", clone_reenters_delivery),
        (
            "clone failure keeps previous",
            clone_failure_keeps_the_previous_registration,
        ),
        (
            "replacement retirement keeps new",
            replacement_retirement_keeps_the_new_registration,
        ),
        (
            "wake failure retains captures",
            wake_failure_retains_captures,
        ),
        ("opaque wake failure", opaque_wake_failure),
        (
            "successful wake retirement",
            successful_wake_retirement_preserves_the_delivery,
        ),
        (
            "owner disconnect wake failure",
            owner_disconnect_wake_failure,
        ),
        ("owner unwind successful wake", owner_unwind_successful_wake),
        (
            "owner unwind wake competition",
            owner_unwind_wake_competition,
        ),
        (
            "abandonment failure healthy task",
            abandonment_failure_healthy_task,
        ),
        (
            "abandonment healthy owner task failure",
            abandonment_healthy_owner_task_failure,
        ),
        (
            "abandonment owner and task competition",
            abandonment_owner_and_task_competition,
        ),
        ("abandonment during unwind", abandonment_during_unwind),
        ("abandonment all failures", abandonment_all_failures),
        (
            "unwind retains delivered result",
            unwinding_retains_an_unclaimed_result,
        ),
    ];
    const SELECTED: &str = "FLUI_CLAIM_SLOT_RECOVERY_CASE";
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
                    "claim_slot::claim_slot_executor_and_owner_recovery",
                    "--nocapture",
                ])
                .env(SELECTED, name)
                .env("RUST_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("claim slot child");
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
