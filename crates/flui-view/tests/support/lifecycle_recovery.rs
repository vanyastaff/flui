//! Public lifecycle ownership failures are isolated from the parent test process.
use AppLifecycleState::{Detached, Hidden, Inactive, Resumed};
use flui_view::{
    __runtime::LifecycleSource, AppLifecycleState, LifecycleClosed, LifecycleSubscription,
};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Bomb {
    drops: Arc<AtomicUsize>,
    message: &'static str,
}
impl Drop for Bomb {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        panic!("{}", self.message);
    }
}
fn bomb(drops: &Arc<AtomicUsize>, message: &'static str) -> Bomb {
    Bomb {
        drops: Arc::clone(drops),
        message,
    }
}
fn aggregate(drops: &Arc<AtomicUsize>) -> (Bomb, Bomb) {
    (bomb(drops, "first capture"), bomb(drops, "second capture"))
}
fn assert_first(payload: Box<dyn std::any::Any + Send>, expected: &str) {
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some(expected)
    );
    flui_foundation::panic::retain_opaque_payload(payload);
}

fn cancelled_failure(opaque_payload: bool, terminal: bool) {
    let source = Rc::new(LifecycleSource::new());
    let weak = Rc::downgrade(&source);
    let handle = source.handle();
    let token = Rc::new(RefCell::new(None::<LifecycleSubscription>));
    let own_token = Rc::clone(&token);
    let capture_drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::new(AtomicUsize::new(0));
    let payload_drops_in_callback = Arc::clone(&payload_drops);
    let captures = aggregate(&capture_drops);
    let (_, first) = handle
        .subscribe(move |_| {
            let _keep = &captures;
            let owner = weak.upgrade().expect("source");
            if terminal {
                owner.begin_close();
                owner.commit_terminal(Detached).expect("terminal event");
                owner.finish_close();
            } else {
                drop(own_token.borrow_mut().take());
                owner.commit(Hidden).expect("queued next event");
            }
            if opaque_payload {
                std::panic::panic_any(aggregate(&payload_drops_in_callback));
            }
            panic!("lifecycle body first");
        })
        .expect("first subscription");
    *token.borrow_mut() = Some(first);
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&events);
    let (_, healthy) = handle
        .subscribe(move |state| observed.borrow_mut().push(state))
        .expect("healthy");
    source.commit(Inactive).expect("event");
    let payload = catch_unwind(AssertUnwindSafe(|| source.drain()))
        .expect_err("body failure propagates after delivery");
    if opaque_payload {
        flui_foundation::panic::retain_opaque_payload(payload);
    } else {
        assert_first(payload, "lifecycle body first");
    }
    assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
    assert_eq!(payload_drops.load(Ordering::SeqCst), 0);
    if terminal {
        assert_eq!(*events.borrow(), [Inactive, Detached]);
        assert_eq!(handle.snapshot(), Err(LifecycleClosed));
        assert_eq!(source.commit(Resumed), Err(LifecycleClosed));
    } else {
        assert_eq!(
            *events.borrow(),
            [Inactive, Hidden],
            "queued FIFO event and healthy sibling both complete before panic resumes"
        );
        source.commit(Resumed).expect("next operation");
        source.drain();
        assert_eq!(*events.borrow(), [Inactive, Hidden, Resumed]);
        assert_eq!(handle.snapshot(), Ok(Some(Resumed)));
    }
    drop(healthy);
    drop(token);
    drop(source);
    assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
}

fn release_retirement_failure() {
    let source = LifecycleSource::new();
    let handle = source.handle();
    let first_drops = Arc::new(AtomicUsize::new(0));
    let later_drops = Arc::new(AtomicUsize::new(0));
    let first_capture = bomb(&first_drops, "retirement first");
    let (_, first) = handle
        .subscribe(move |_| {
            let _keep = &first_capture;
        })
        .expect("first");
    let later_captures = aggregate(&later_drops);
    let (_, later) = handle
        .subscribe(move |_| {
            let _keep = &later_captures;
        })
        .expect("later");
    let payload = catch_unwind(AssertUnwindSafe(|| source.finish_close()))
        .expect_err("ordinary first retirement failure");
    assert_first(payload, "retirement first");
    assert_eq!(first_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        later_drops.load(Ordering::SeqCst),
        0,
        "later aggregate never retires after first failure"
    );
    assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    source.finish_close();
    drop((first, later));
    assert_eq!(later_drops.load(Ordering::SeqCst), 0);
}

fn independent_unwind(drop_source: bool) {
    let source = LifecycleSource::new();
    let handle = source.handle();
    let drops = Arc::new(AtomicUsize::new(0));
    let captures = aggregate(&drops);
    let (_, token) = handle
        .subscribe(move |_| {
            let _keep = &captures;
        })
        .expect("subscription");
    let payload = if drop_source {
        catch_unwind(AssertUnwindSafe(move || {
            let _owner = source;
            panic!("independent first");
        }))
        .expect_err("independent unwind")
    } else {
        let payload = catch_unwind(AssertUnwindSafe(move || {
            let _token = token;
            panic!("independent first");
        }))
        .expect_err("independent unwind");
        source
            .commit(Resumed)
            .expect("cancelled source remains usable");
        source.drain();
        assert_eq!(handle.snapshot(), Ok(Some(Resumed)));
        return assert_independent(payload, &drops);
    };
    assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    drop(token);
    assert_independent(payload, &drops);
}
fn assert_independent(payload: Box<dyn std::any::Any + Send>, drops: &AtomicUsize) {
    assert_first(payload, "independent first");
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

pub(crate) fn dispatch_child(kind: &str) {
    match kind {
        "cancel" => cancelled_failure(false, false),
        "cancel_opaque" => cancelled_failure(true, false),
        "terminal" => cancelled_failure(false, true),
        "retirement" => release_retirement_failure(),
        "token_unwind" => independent_unwind(false),
        "source_unwind" => independent_unwind(true),
        _ => panic!("unknown lifecycle child"),
    }
}
fn child(kind: &str) {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "lifecycle_panic_containment_matrix",
            "--nocapture",
        ])
        .env("FLUI_LIFECYCLE_RECOVERY_CHILD", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lifecycle child");
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().expect("status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill child");
            let output = child.wait_with_output().expect("reap child");
            panic!("lifecycle recovery blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("test result: ok. 1 passed; 0 failed;"),
        "lifecycle recovery failed: {output:?}"
    );
}
pub(crate) fn self_cancelled_callback_failure_retains_its_capture_aggregate() {
    child("cancel");
}
pub(crate) fn self_cancellation_retains_competing_payload_and_capture_aggregates() {
    child("cancel_opaque");
}
pub(crate) fn terminal_release_keeps_the_earlier_callback_failure() {
    child("terminal");
}
pub(crate) fn release_retains_later_envelopes_after_first_retirement_failure() {
    child("retirement");
}
pub(crate) fn subscription_retirement_during_unwind_retains_its_envelope() {
    child("token_unwind");
}
pub(crate) fn source_retirement_during_unwind_retains_its_envelopes() {
    child("source_unwind");
}

pub(crate) fn successful_lifecycle_cancellation_retires_captures_and_keeps_fifo() {
    struct CountDrop(Arc<AtomicUsize>);
    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let source = LifecycleSource::new();
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = CountDrop(Arc::clone(&drops));
    let (_, cancelled) = source
        .handle()
        .subscribe(move |_| {
            let _keep = &capture;
        })
        .expect("cancelled subscription");
    drop(cancelled);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&events);
    let (_, healthy) = source
        .handle()
        .subscribe(move |state| log.borrow_mut().push(state))
        .expect("healthy subscription");
    source.commit(Inactive).expect("first event");
    source.commit(Hidden).expect("second event");
    source.drain();
    assert_eq!(*events.borrow(), [Inactive, Hidden]);
    source.finish_close();
    drop(healthy);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
