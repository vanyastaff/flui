//! Owner callbacks may capture UI state; only wake capabilities cross threads.

use std::cell::RefCell;
use std::rc::Rc;

use flui_scheduler::{OwnerFrame, Priority, UpdateScheduler};

fn recursive_turns_preserve_the_admitted_frame() {
    use flui_scheduler::{ExecutionError, IdleDeadline, Instant, SchedulerPhase};
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let delivered = Rc::new(std::cell::Cell::new(false));
    let observed = delivered.clone();
    let now = Instant::now();
    owner
        .post_frame_handle()
        .schedule(move |_| observed.set(true))
        .expect("live owner");
    owner
        .drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {},
            || {
                let phase = scheduler.phase();
                assert_eq!(
                    owner.drive_frame(
                        now,
                        IdleDeadline::far_future(now),
                        || {},
                        || panic!("nested pipeline ran")
                    ),
                    Err(ExecutionError::AlreadyExecuting)
                );
                assert_eq!(
                    owner.pump_background(|| panic!("nested preparation ran")),
                    Err(ExecutionError::AlreadyExecuting)
                );
                assert_eq!(scheduler.phase(), phase);
                assert!(!delivered.get());
            },
        )
        .expect("outer frame completes");
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    assert!(delivered.get());
    assert_eq!(owner.pump_background(|| {}), Ok(0));
}

fn retired_owner_refuses_without_consuming_demand() {
    use flui_scheduler::{ExecutionError, IdleDeadline, Instant};
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    assert!(owner.retire().is_none());
    scheduler.request_frame();
    let now = Instant::now();
    assert_eq!(
        owner.drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {},
            || panic!("retired pipeline ran")
        ),
        Err(ExecutionError::Retired)
    );
    assert_eq!(
        owner.pump_background(|| panic!("retired preparation ran")),
        Err(ExecutionError::Retired)
    );
    assert!(scheduler.is_frame_scheduled());
}

fn produced_result_is_retained_after_completion_failure() {
    use flui_scheduler::{IdleDeadline, Instant, SchedulerPhase};
    struct ResultCapture(Rc<std::cell::Cell<usize>>);
    impl Drop for ResultCapture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = Rc::new(std::cell::Cell::new(0));
    owner
        .post_frame_handle()
        .schedule(|_| panic!("post-frame primary"))
        .expect("live owner");
    let now = Instant::now();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {},
            || ResultCapture(drops.clone()),
        )
    }))
    .err()
    .expect("callback failure escapes");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("post-frame primary")
    );
    assert_eq!(
        drops.get(),
        0,
        "produced output must not retire during failure custody"
    );
    assert_eq!(scheduler.phase(), SchedulerPhase::Idle);
    owner
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || ())
        .expect("next frame recovers");
}

thread_local! {
    static OWNER_SCHEDULER: RefCell<Option<flui_scheduler::WeakUpdateScheduler>> = const { RefCell::new(None) };
    static EXECUTING_OWNER: RefCell<Option<Rc<OwnerFrame>>> = const { RefCell::new(None) };
}

fn handled_nested_failure_retains_completion_envelope() {
    use flui_scheduler::{IdleDeadline, Instant};
    use std::future::Future;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Wake, Waker};
    struct RefusedCapture;
    impl Drop for RefusedCapture {
        fn drop(&mut self) {
            panic!("refused capture failure");
        }
    }
    struct CompletionProbe(Arc<AtomicUsize>);
    impl Drop for CompletionProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl Wake for CompletionProbe {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            let owner = EXECUTING_OWNER
                .with(|slot| slot.borrow().as_ref().expect("installed owner").clone());
            let capture = RefusedCapture;
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                owner.pump_background(move || {
                    let _ = &capture;
                    panic!("nested body must not run");
                })
            }))
            .expect_err("refused envelope destructor failed");
            assert_eq!(
                flui_foundation::panic::payload_text(&*failure),
                Some("refused capture failure")
            );
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
    EXECUTING_OWNER.with(|slot| *slot.borrow_mut() = Some(owner.clone()));
    let drops = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(CompletionProbe(drops.clone())));
    let mut completion = scheduler.end_of_frame();
    assert!(
        std::pin::Pin::new(&mut completion)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    drop(waker);
    let now = Instant::now();
    let result = owner.drive_frame(now, IdleDeadline::far_future(now), || {}, || ());
    EXECUTING_OWNER.with(|slot| {
        slot.borrow_mut().take();
    });
    result.expect("caller handled the nested destructor failure");
    assert_eq!(
        drops.load(Ordering::Relaxed),
        0,
        "live failure custody retains the successful completion envelope"
    );
    assert_eq!(owner.pump_background(|| {}), Ok(0));
}

/// A Send test probe resolves UI state only on the thread that installed it.
/// Production wake capabilities never contain or resolve this state.
#[derive(Clone, Copy)]
pub(crate) struct OwnerProbe(std::thread::ThreadId);

pub(crate) fn owner_probe(scheduler: &UpdateScheduler) -> OwnerProbe {
    OWNER_SCHEDULER.with(|slot| *slot.borrow_mut() = Some(scheduler.downgrade()));
    OwnerProbe(std::thread::current().id())
}

impl OwnerProbe {
    pub(crate) fn upgrade(self) -> Option<UpdateScheduler> {
        assert_eq!(
            self.0,
            std::thread::current().id(),
            "owner probe stays on its thread"
        );
        OWNER_SCHEDULER.with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(flui_scheduler::WeakUpdateScheduler::upgrade)
        })
    }
}

fn callbacks_share_owner_state_and_allow_registration_reentry() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("one frame owner");
    let events = Rc::new(RefCell::new(Vec::new()));
    let callback_events = events.clone();
    let reentrant = scheduler.clone();
    scheduler.schedule_frame_callback(Box::new(move |_| {
        callback_events.borrow_mut().push("transient");
        let events = callback_events.clone();
        reentrant.schedule_microtask(Box::new(move || events.borrow_mut().push("microtask")));
    }));
    let callback_events = events.clone();
    scheduler.add_task(Priority::Build, move || {
        callback_events.borrow_mut().push("build");
    });
    let callback_events = events.clone();
    scheduler.add_persistent_frame_callback(Rc::new(move |_| {
        callback_events.borrow_mut().push("persistent");
    }));
    let callback_events = events.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        callback_events.borrow_mut().push("post_frame");
    }));
    owner
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");
    assert_eq!(
        events.borrow().as_slice(),
        [
            "transient",
            "microtask",
            "persistent",
            "build",
            "post_frame"
        ]
    );
}

fn post_frame_storage_belongs_to_one_owner_generation() {
    struct Capture {
        drops: Rc<std::cell::Cell<usize>>,
        owner_thread: std::thread::ThreadId,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert_eq!(self.owner_thread, std::thread::current().id());
            self.drops.set(self.drops.get() + 1);
        }
    }
    let scheduler = UpdateScheduler::new();
    let construction = flui_scheduler::PostFrameHandle::new(&scheduler);
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    construction
        .schedule(move |_| observed.borrow_mut().push("construction"))
        .expect("construction admits work before its first owner");
    let first = OwnerFrame::new(&scheduler).expect("first owner");
    first
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");
    assert_eq!(events.borrow().as_slice(), ["construction"]);

    let stale = first.post_frame_handle();
    let drops = Rc::new(std::cell::Cell::new(0));
    let capture = Capture {
        drops: drops.clone(),
        owner_thread: std::thread::current().id(),
    };
    stale
        .schedule(move |_| {
            let _ = &capture;
        })
        .expect("live owner");
    drop(first);
    assert_eq!(
        drops.get(),
        1,
        "scheduler and weak handles do not retain the retired queue"
    );
    let second = OwnerFrame::new(&scheduler).expect("replacement owner");
    assert!(
        stale
            .schedule(|_| panic!("stale admission must be refused"))
            .is_err()
    );
    assert!(
        construction
            .schedule(|_| panic!("construction handle cannot follow replacement"))
            .is_err()
    );
    let observed = events.clone();
    second
        .post_frame_handle()
        .schedule(move |_| observed.borrow_mut().push("replacement"))
        .expect("replacement admits its own work");
    second
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");
    assert_eq!(events.borrow().as_slice(), ["construction", "replacement"]);
}

#[test]
fn owner_callback_contract() {
    crate::run_table(
        "owner callbacks",
        &[
            (
                "recursive frame and background refusal",
                recursive_turns_preserve_the_admitted_frame,
            ),
            (
                "permanent owner retirement",
                retired_owner_refuses_without_consuming_demand,
            ),
            (
                "produced result custody",
                produced_result_is_retained_after_completion_failure,
            ),
            (
                "handled nested failure custody",
                handled_nested_failure_retains_completion_envelope,
            ),
            (
                "frame reentry",
                callbacks_share_owner_state_and_allow_registration_reentry,
            ),
            (
                "post-frame owner generation",
                post_frame_storage_belongs_to_one_owner_generation,
            ),
        ],
    );
}
