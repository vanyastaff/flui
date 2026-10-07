//! Real async builders publish before retiring caller-owned values.
use flui_foundation::{AsyncSnapshot, ConnectionState};
use flui_scheduler::{OwnerFrame, UpdateScheduler};
use flui_view::{
    BuildOwner, ElementTree, ErrorView, RebuildReason, ViewExt,
    element::{FutureBuilder, FutureFactory, SnapshotBuilder, StreamBuilder, StreamFactory},
};
use futures_core::Stream;
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

struct Value {
    number: i32,
    fail_drop: bool,
    drops: Arc<AtomicUsize>,
}
impl Drop for Value {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        assert!(!self.fail_drop, "snapshot retirement first");
    }
}
#[derive(Default)]
struct Mailbox {
    events: VecDeque<Result<Value, ()>>,
    waker: Option<Waker>,
}
struct Producer(Arc<Mutex<Mailbox>>);
impl Future for Producer {
    type Output = Result<Value, ()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut mailbox = self.0.lock();
        if let Some(value) = mailbox.events.pop_front() {
            Poll::Ready(value)
        } else {
            mailbox.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Stream for Producer {
    type Item = Result<Value, ()>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut mailbox = self.0.lock();
        if let Some(value) = mailbox.events.pop_front() {
            Poll::Ready(Some(value))
        } else {
            mailbox.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
fn future_factory(mailbox: &Arc<Mutex<Mailbox>>) -> FutureFactory<Value, ()> {
    let mailbox = Arc::clone(mailbox);
    Rc::new(move || Box::pin(Producer(Arc::clone(&mailbox))))
}
fn stream_factory(mailbox: &Arc<Mutex<Mailbox>>) -> StreamFactory<Value, ()> {
    let mailbox = Arc::clone(mailbox);
    Rc::new(move || Box::pin(Producer(Arc::clone(&mailbox))))
}
fn send(mailbox: &Arc<Mutex<Mailbox>>, value: Value) {
    let waker = {
        let mut mailbox = mailbox.lock();
        mailbox.events.push_back(Ok(value));
        mailbox.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}
fn builder(log: &Arc<Mutex<Vec<(ConnectionState, i32)>>>) -> SnapshotBuilder<Value, ()> {
    let log = Arc::clone(log);
    Rc::new(move |_, snapshot: &AsyncSnapshot<Value, ()>| {
        log.lock().push((
            snapshot.connection_state(),
            snapshot.data().map_or(-1, |value| value.number),
        ));
        ErrorView::new("snapshot leaf").boxed()
    })
}
fn run_child(stream: bool, competing_value: bool, competing_wake: bool) {
    let scheduler = UpdateScheduler::new();
    let mut owner = BuildOwner::new();
    let owner_frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    owner.set_async_driver(owner_frame.async_driver());
    let fail_wake = Arc::new(AtomicBool::new(false));
    let wake_failures = Arc::new(AtomicUsize::new(0));
    let fail = Arc::clone(&fail_wake);
    let failures = Arc::clone(&wake_failures);
    owner.set_on_build_scheduled(move || {
        if fail.swap(false, Ordering::SeqCst) {
            failures.fetch_add(1, Ordering::SeqCst);
            panic!("snapshot wake secondary");
        }
    });
    let old_drops = Arc::new(AtomicUsize::new(0));
    let incoming_drops = Arc::new(AtomicUsize::new(0));
    let next_drops = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(Mutex::new(Vec::new()));
    let mailbox = Arc::new(Mutex::new(Mailbox::default()));
    let initial_drops = Arc::clone(&old_drops);
    let initial = Rc::new(move || Value {
        number: 0,
        fail_drop: true,
        drops: Arc::clone(&initial_drops),
    });
    let view = if stream {
        StreamBuilder::keyed(Some(1u32), stream_factory(&mailbox), builder(&log))
            .with_initial_data(initial)
            .boxed()
    } else {
        FutureBuilder::keyed(Some(1u32), future_factory(&mailbox), builder(&log))
            .with_initial_data(initial)
            .boxed()
    };
    let mut tree = ElementTree::new();
    let root = tree.mount_root(&view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(
        log.lock().last().copied(),
        Some((ConnectionState::Waiting, 0))
    );
    if stream {
        owner_frame.poll_ready();
    }
    send(
        &mailbox,
        Value {
            number: 1,
            fail_drop: competing_value,
            drops: Arc::clone(&incoming_drops),
        },
    );
    fail_wake.store(competing_wake, Ordering::SeqCst);
    let payload = catch_unwind(AssertUnwindSafe(|| owner_frame.poll_ready()))
        .expect_err("old snapshot retirement panics");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("snapshot retirement first")
    );
    flui_foundation::panic::retain_opaque_payload(payload);
    assert_eq!(old_drops.load(Ordering::SeqCst), 1);
    assert_eq!(
        incoming_drops.load(Ordering::SeqCst),
        0,
        "incoming snapshot survives the old value's unwind"
    );
    assert_eq!(
        wake_failures.load(Ordering::SeqCst),
        usize::from(competing_wake),
        "the rebuild wake is attempted despite retirement failure"
    );
    let builds_before = log.lock().len();
    owner.build_scope(&mut tree);
    assert!(
        log.lock().len() > builds_before,
        "publication queued a real build, without test dirtiness"
    );
    let published = if stream {
        ConnectionState::Active
    } else {
        ConnectionState::Done
    };
    assert_eq!(log.lock().last().copied(), Some((published, 1)));

    // A panicking task is not continued. A new key creates a new producer.
    let next_mailbox = Arc::new(Mutex::new(Mailbox::default()));
    let next_view = if stream {
        StreamBuilder::keyed(Some(2u32), stream_factory(&next_mailbox), builder(&log)).boxed()
    } else {
        FutureBuilder::keyed(Some(2u32), future_factory(&next_mailbox), builder(&log)).boxed()
    };
    tree.update(root, &next_view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::StateChange);
    owner.build_scope(&mut tree);
    if stream {
        owner_frame.poll_ready();
    }
    send(
        &next_mailbox,
        Value {
            number: 2,
            fail_drop: false,
            drops: Arc::clone(&next_drops),
        },
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| owner_frame.poll_ready()));
    if competing_value {
        let payload = outcome.expect_err("incoming value has its own ordinary retirement failure");
        assert_eq!(
            flui_foundation::panic::payload_text(&*payload),
            Some("snapshot retirement first")
        );
        flui_foundation::panic::retain_opaque_payload(payload);
    } else {
        outcome.expect("healthy incoming retirement");
    }
    owner.build_scope(&mut tree);
    assert_eq!(
        log.lock().last().copied(),
        Some((published, 2)),
        "fresh keyed producer still publishes and rebuilds"
    );
    assert_eq!(incoming_drops.load(Ordering::SeqCst), 1);
    assert_eq!(next_drops.load(Ordering::SeqCst), 0);
}

fn eager_disposal() {
    struct Field(Arc<AtomicUsize>);
    impl Drop for Field {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("eager incoming field dropped");
        }
    }
    struct EagerValue {
        initial: bool,
        drops: Arc<AtomicUsize>,
        _fields: Option<(Field, Field)>,
    }
    impl Drop for EagerValue {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            assert!(!self.initial, "eager old retirement first");
        }
    }
    let scheduler = UpdateScheduler::new();
    let mut owner = BuildOwner::new();
    let owner_frame = OwnerFrame::new(&scheduler).expect("the scheduler has no live owner frame");
    owner.set_async_driver(owner_frame.async_driver());
    let old_drops = Arc::new(AtomicUsize::new(0));
    let incoming_drops = Arc::new(AtomicUsize::new(0));
    let field_drops = Arc::new(AtomicUsize::new(0));
    let initial_drops = Arc::clone(&old_drops);
    let new_drops = Arc::clone(&incoming_drops);
    let new_fields = Arc::clone(&field_drops);
    let view = FutureBuilder::<u32, EagerValue, ()>::keyed(
        Some(1),
        Rc::new(move || {
            Box::pin(std::future::ready(Ok(EagerValue {
                initial: false,
                drops: Arc::clone(&new_drops),
                _fields: Some((
                    Field(Arc::clone(&new_fields)),
                    Field(Arc::clone(&new_fields)),
                )),
            })))
        }),
        Rc::new(|_, _| ErrorView::new("eager leaf").boxed()),
    )
    .with_initial_data(Rc::new(move || EagerValue {
        initial: true,
        drops: Arc::clone(&initial_drops),
        _fields: None,
    }));
    let mut tree = ElementTree::new();
    let root = tree.mount_root(&view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    let payload = catch_unwind(AssertUnwindSafe(|| owner.build_scope(&mut tree)))
        .expect_err("root eager init resumes its old retirement failure");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("eager old retirement first")
    );
    flui_foundation::panic::retain_opaque_payload(payload);
    tree.remove(root, &mut owner.element_owner_mut());
    drop(view);
    assert_eq!(old_drops.load(Ordering::SeqCst), 1);
    assert_eq!(incoming_drops.load(Ordering::SeqCst), 0);
    assert_eq!(
        field_drops.load(Ordering::SeqCst),
        0,
        "eager state disposal cannot retire incoming aggregate"
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&seen);
    let healthy = FutureBuilder::<u32, i32, ()>::keyed(
        Some(2),
        Rc::new(|| Box::pin(std::future::ready(Ok(4)))),
        Rc::new(move |_, snapshot| {
            observed.lock().push(snapshot.data().copied());
            ErrorView::new("healthy eager leaf").boxed()
        }),
    );
    let root = tree.mount_root(&healthy, &mut owner.element_owner_mut());
    owner.schedule_build_for(root, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(seen.lock().last().copied(), Some(Some(4)));
    drop((tree, owner, owner_frame, scheduler));
    assert_eq!(incoming_drops.load(Ordering::SeqCst), 0);
    assert_eq!(field_drops.load(Ordering::SeqCst), 0);
}

pub(crate) fn dispatch_child(kind: &str) {
    match kind {
        "eager" => eager_disposal(),
        "future" => run_child(false, false, false),
        "future_values" => run_child(false, true, false),
        "future_wake" => run_child(false, false, true),
        "stream" => run_child(true, false, false),
        "stream_values" => run_child(true, true, false),
        "stream_wake" => run_child(true, false, true),
        _ => panic!("unknown snapshot child"),
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
        .env("FLUI_ASYNC_SNAPSHOT_CHILD", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("snapshot child");
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().expect("status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill child");
            let output = child.wait_with_output().expect("reap child");
            panic!("snapshot recovery blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("output");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("test result: ok. 1 passed; 0 failed;"),
        "snapshot recovery failed: {output:?}"
    );
}
pub(crate) fn future_publication_survives_old_value_retirement() {
    child("future");
}
pub(crate) fn future_publication_keeps_competing_incoming_value_alive() {
    child("future_values");
}
pub(crate) fn future_retirement_panic_keeps_priority_over_rebuild_wake() {
    child("future_wake");
}
pub(crate) fn stream_publication_survives_old_value_retirement() {
    child("stream");
}
pub(crate) fn stream_publication_keeps_competing_incoming_value_alive() {
    child("stream_values");
}
pub(crate) fn stream_retirement_panic_keeps_priority_over_rebuild_wake() {
    child("stream_wake");
}

pub(crate) fn eager_failure_keeps_incoming_aggregate_owned_after_state_disposal() {
    child("eager");
}
