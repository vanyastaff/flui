//! Opaque retirement observes failure caught during an admitted owner turn.

use std::cell::{Cell, RefCell};
use std::future::{Future, poll_fn};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

use flui_scheduler::{
    ExecutionError, IdleDeadline, Instant, OwnerFrame, TaskToken, UpdateScheduler,
};

struct RejectedCapture;
impl Drop for RejectedCapture {
    fn drop(&mut self) {
        panic!("nested refused capture failure");
    }
}

fn catch_nested_refusal_failure(owner: &OwnerFrame) {
    let capture = RejectedCapture;
    let failure = catch_unwind(AssertUnwindSafe(|| {
        owner.pump_background(move || {
            let _ = &capture;
            panic!("refused preparation ran");
        })
    }))
    .expect_err("refused capture retirement reports its failure");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("nested refused capture failure")
    );
}

struct FinishedFuture {
    owner: Weak<OwnerFrame>,
    drops: Rc<Cell<usize>>,
    cancel: Option<Rc<RefCell<Option<TaskToken>>>>,
}
impl Future for FinishedFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        catch_nested_refusal_failure(&self.owner.upgrade().expect("live owner"));
        if let Some(cancel) = &self.cancel {
            drop(cancel.borrow_mut().take());
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}
impl Drop for FinishedFuture {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

fn finished_and_cancelled_futures_preserve_caught_failure_custody() {
    for cancelled in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
        let drops = Rc::new(Cell::new(0));
        let cancel = Rc::new(RefCell::new(None));
        let token = owner.async_driver().spawn_local(Box::pin(FinishedFuture {
            owner: Rc::downgrade(&owner),
            drops: Rc::clone(&drops),
            cancel: cancelled.then(|| Rc::clone(&cancel)),
        }));
        *cancel.borrow_mut() = Some(token);
        let sibling_polls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&sibling_polls);
        let sibling = owner
            .async_driver()
            .spawn_local(Box::pin(poll_fn(move |cx| {
                observed.set(observed.get() + 1);
                if observed.get() == 1 {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })));
        assert_eq!(owner.pump_background(|| {}), Ok(2));
        assert_eq!(
            drops.get(),
            0,
            "finished opaque future is retained after the caught failure"
        );
        assert_eq!(sibling_polls.get(), 1);
        assert_eq!(owner.pump_background(|| {}), Ok(1));
        assert_eq!(
            sibling_polls.get(),
            2,
            "accepted sibling progresses on the healthy next turn"
        );
        assert_eq!(owner.async_driver().pending_task_count(), 0);
        assert!(!sibling.is_cancelled());
    }
}

fn cancelling_transient_work_preserves_caught_failure_custody() {
    struct Capture(Rc<Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = Rc::new(Cell::new(0));
    let capture = Capture(Rc::clone(&drops));
    let cancelled = scheduler.schedule_frame_callback(Box::new(move |_| {
        let _ = &capture;
        panic!("cancelled transient ran");
    }));
    let sibling_calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&sibling_calls);
    scheduler.schedule_frame_callback(Box::new(move |_| observed.set(observed.get() + 1)));
    let now = Instant::now();
    owner
        .drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {
                catch_nested_refusal_failure(&owner);
                assert!(scheduler.cancel_frame_callback(cancelled));
            },
            || {},
        )
        .expect("caught nested failure does not abort the outer frame");
    assert_eq!(
        drops.get(),
        0,
        "cancelled opaque capture inherits active failure custody"
    );
    assert_eq!(sibling_calls.get(), 1);
    assert_eq!(owner.pump_background(|| {}), Ok(0));
}

fn callbacks_and_task_polls_refuse_nested_owner_turns() {
    fn refuse(owner: &OwnerFrame, calls: &Cell<usize>) {
        let now = Instant::now();
        assert_eq!(
            owner.drive_frame(
                now,
                IdleDeadline::far_future(now),
                || panic!("nested frame prepared"),
                || ()
            ),
            Err(ExecutionError::AlreadyExecuting)
        );
        assert_eq!(
            owner.pump_background(|| panic!("nested background prepared")),
            Err(ExecutionError::AlreadyExecuting)
        );
        calls.set(calls.get() + 1);
    }
    let scheduler = UpdateScheduler::new();
    let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
    let calls = Rc::new(Cell::new(0));
    let weak = Rc::downgrade(&owner);
    let observed = Rc::clone(&calls);
    scheduler.schedule_frame_callback(Box::new(move |_| {
        refuse(&weak.upgrade().expect("transient owner"), &observed)
    }));
    let weak = Rc::downgrade(&owner);
    let observed = Rc::clone(&calls);
    let task = owner.async_driver().spawn_local(Box::pin(async move {
        refuse(&weak.upgrade().expect("task owner"), &observed);
    }));
    let weak = Rc::downgrade(&owner);
    let observed = Rc::clone(&calls);
    owner
        .post_frame_handle()
        .schedule(move |_| refuse(&weak.upgrade().expect("post-frame owner"), &observed))
        .expect("live lane");
    let now = Instant::now();
    owner
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
        .expect("outer frame");
    assert_eq!(
        calls.get(),
        3,
        "transient, task and post-frame slots all preserve admission"
    );
    assert_eq!(scheduler.frame_count(), 1);
    assert_eq!(owner.pump_background(|| {}), Ok(0));
    assert!(!task.is_cancelled());
}

struct RemovedCapture(Rc<Cell<usize>>);
impl Drop for RemovedCapture {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

struct RemovedFuture {
    capture: RemovedCapture,
    ready: bool,
}
impl Future for RemovedFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        let _ = &self.capture;
        if self.ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

fn lifecycle_removal_preserves_caught_failure() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = Rc::new(Cell::new(0));
    let capture = RemovedCapture(Rc::clone(&drops));
    let id = scheduler.add_lifecycle_state_listener(Rc::new(move |_| {
        let _ = &capture;
    }));
    let now = Instant::now();
    owner
        .drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {
                catch_nested_refusal_failure(&owner);
                assert!(scheduler.remove_lifecycle_state_listener(id));
            },
            || {},
        )
        .expect("outer frame");
    assert_eq!(
        drops.get(),
        0,
        "removed lifecycle capture inherits active custody"
    );
}

fn hook_replacement_preserves_caught_failure() {
    struct Capture(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture = Capture(std::sync::Arc::clone(&drops));
    scheduler.set_on_frame_scheduled(Some(std::sync::Arc::new(move || {
        let _ = &capture;
    })));
    let now = Instant::now();
    owner
        .drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {
                catch_nested_refusal_failure(&owner);
                scheduler.set_on_frame_scheduled(None);
            },
            || {},
        )
        .expect("outer frame");
    assert_eq!(
        drops.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "displaced final hook inherits active custody"
    );
}

fn explicit_task_cancellation_preserves_caught_failure() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = Rc::new(Cell::new(0));
    let token = owner.async_driver().spawn_local(Box::pin(RemovedFuture {
        capture: RemovedCapture(Rc::clone(&drops)),
        ready: false,
    }));
    let now = Instant::now();
    owner
        .drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {
                catch_nested_refusal_failure(&owner);
                token.cancel();
            },
            || {},
        )
        .expect("outer frame");
    assert!(token.is_cancelled());
    assert_eq!(owner.async_driver().pending_task_count(), 0);
    assert_eq!(drops.get(), 0, "removed future inherits active custody");
}

fn eager_completion_preserves_caught_failure() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = Rc::new(Cell::new(0));
    let now = Instant::now();
    owner
        .drive_frame(
            now,
            IdleDeadline::far_future(now),
            || {
                catch_nested_refusal_failure(&owner);
                assert!(
                    owner
                        .async_driver()
                        .spawn_local_eager(Box::pin(RemovedFuture {
                            capture: RemovedCapture(Rc::clone(&drops)),
                            ready: true,
                        }))
                        .is_none()
                );
            },
            || {},
        )
        .expect("outer frame");
    assert_eq!(
        drops.get(),
        0,
        "eagerly completed future inherits active custody"
    );
}

#[test]
fn owner_retirement_custody_contract() {
    crate::run_table(
        "owner retirement custody",
        &[
            (
                "finished and cancelled future custody",
                finished_and_cancelled_futures_preserve_caught_failure_custody as fn(),
            ),
            (
                "transient cancellation custody",
                cancelling_transient_work_preserves_caught_failure_custody,
            ),
            (
                "nested callback and task entry",
                callbacks_and_task_polls_refuse_nested_owner_turns,
            ),
            (
                "lifecycle removal custody",
                lifecycle_removal_preserves_caught_failure,
            ),
            (
                "wake hook replacement custody",
                hook_replacement_preserves_caught_failure,
            ),
            (
                "explicit task cancellation custody",
                explicit_task_cancellation_preserves_caught_failure,
            ),
            (
                "eager completion custody",
                eager_completion_preserves_caught_failure,
            ),
            (
                "eager pending retirement custody",
                eager_pending_retirement_preserves_caught_failure,
            ),
            (
                "retired lazy admission custody",
                retired_lazy_admission_preserves_caught_failure,
            ),
            (
                "retired eager admission custody",
                retired_eager_admission_preserves_caught_failure,
            ),
            (
                "healthy teardown after completed caught failure",
                healthy_teardown_after_completed_caught_failure,
            ),
            (
                "recursive retirement shares caught failure",
                recursive_retirement_shares_caught_failure,
            ),
            (
                "completed retirement ends caller custody",
                completed_retirement_ends_caller_failure_custody,
            ),
            ("standalone frame refusal ends custody", standalone_frame_refusal_ends_custody),
            ("standalone background refusal ends custody", standalone_background_refusal_ends_custody),
            ("standalone failed retirement ends custody", standalone_failed_retirement_ends_custody),
            ("standalone retirement publishes producer custody", standalone_retirement_publishes_producer_custody),
        ],
    );
}

fn healthy_teardown_after_completed_caught_failure() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let drops = Rc::new(Cell::new(0));
    assert_eq!(owner.pump_background(|| {
        catch_nested_refusal_failure(&owner);
        let capture = RemovedCapture(Rc::clone(&drops));
        owner.post_frame_handle().schedule(move |_| { let _ = &capture; })
            .expect("accepted callback");
    }), Ok(0));
    assert_eq!(drops.get(), 0, "callback remains accepted beyond its turn");
    assert!(owner.retire().is_none());
    assert_eq!(drops.get(), 1, "new healthy teardown retires the callback");
}

fn completed_retirement_ends_caller_failure_custody() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    assert_eq!(owner.pump_background(|| {
        catch_nested_refusal_failure(&owner);
        assert!(owner.retire().is_none());
    }), Ok(0));
    let drops = Rc::new(Cell::new(0));
    let capture = RemovedCapture(Rc::clone(&drops));
    assert_eq!(owner.pump_background(move || { let _ = &capture; }),
        Err(ExecutionError::Retired));
    assert_eq!(drops.get(), 1, "later healthy refused preparation retires normally");
    let prepare = RemovedCapture(Rc::clone(&drops));
    let pipeline = RemovedCapture(Rc::clone(&drops));
    let now = Instant::now();
    assert_eq!(owner.drive_frame(now, IdleDeadline::far_future(now),
        move || { let _ = &prepare; }, move || { let _ = &pipeline; }),
        Err(ExecutionError::Retired));
    assert_eq!(drops.get(), 3, "later healthy refused frame retires both envelopes");
}

fn recursive_retirement_shares_caught_failure() {
    struct RecursiveRetirement(Weak<OwnerFrame>);
    impl Future for RecursiveRetirement {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            Poll::Pending
        }
    }
    impl Drop for RecursiveRetirement {
        fn drop(&mut self) {
            let owner = self.0.upgrade().expect("retiring owner lives");
            catch_nested_refusal_failure(&owner);
            assert!(owner.retire().is_none());
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
    let driver = owner.async_driver();
    let first = driver.spawn_local(Box::pin(RecursiveRetirement(Rc::downgrade(&owner))));
    let drops = Rc::new(Cell::new(0));
    let second = driver.spawn_local(Box::pin(RemovedFuture {
        capture: RemovedCapture(Rc::clone(&drops)),
        ready: false,
    }));
    assert!(owner.retire().is_none());
    assert_eq!(drops.get(), 0, "recursive retirement cannot reset earlier custody");
    assert_eq!(driver.pending_task_count(), 0);
    assert!(first.is_cancelled());
    assert!(second.is_cancelled());
    assert_healthy_refusal_retires(&owner);
}

fn assert_healthy_refusal_retires(owner: &OwnerFrame) {
    let drops = Rc::new(Cell::new(0));
    let capture = RemovedCapture(Rc::clone(&drops));
    assert_eq!(owner.pump_background(move || { let _ = &capture; }),
        Err(ExecutionError::Retired));
    assert_eq!(drops.get(), 1, "fresh standalone refusal has no earlier failure custody");
}

fn standalone_frame_refusal_ends_custody() {
    standalone_refusal_ends_custody(true);
}

fn standalone_background_refusal_ends_custody() {
    standalone_refusal_ends_custody(false);
}

fn standalone_refusal_ends_custody(frame: bool) {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    assert!(owner.retire().is_none());
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let capture = RejectedCapture;
        if frame {
            let now = Instant::now();
            let _ = owner.drive_frame(now, IdleDeadline::far_future(now),
                move || { let _ = &capture; }, || {});
        } else {
            let _ = owner.pump_background(move || { let _ = &capture; });
        }
    })).expect_err("standalone refused envelope reports its destructor failure");
    assert_eq!(flui_foundation::panic::payload_text(failure.as_ref()),
        Some("nested refused capture failure"));
    assert_healthy_refusal_retires(&owner);
}

fn standalone_failed_retirement_ends_custody() {
    struct PanickingFuture;
    impl Future for PanickingFuture {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> { Poll::Pending }
    }
    impl Drop for PanickingFuture {
        fn drop(&mut self) { panic!("standalone retired future failure"); }
    }
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let token = owner.async_driver().spawn_local(Box::pin(PanickingFuture));
    let failure = owner.retire().expect("retirement returns its first failure");
    assert_eq!(flui_foundation::panic::payload_text(failure.as_ref()),
        Some("standalone retired future failure"));
    assert!(token.is_cancelled());
    assert_healthy_refusal_retires(&owner);
}

fn standalone_retirement_publishes_producer_custody() {
    struct ProducerRemoval {
        owner: Weak<OwnerFrame>,
        scheduler: flui_scheduler::WeakUpdateScheduler,
        callback: flui_scheduler::CallbackId,
    }
    impl Drop for ProducerRemoval {
        fn drop(&mut self) {
            let owner = self.owner.upgrade().expect("retiring owner lives");
            catch_nested_refusal_failure(&owner);
            assert!(self.scheduler.upgrade().expect("scheduler lives")
                .cancel_frame_callback(self.callback));
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
    let drops = Rc::new(Cell::new(0));
    let capture = RemovedCapture(Rc::clone(&drops));
    let callback = scheduler.schedule_frame_callback(Box::new(move |_| { let _ = &capture; }));
    let capture = ProducerRemoval {
        owner: Rc::downgrade(&owner),
        scheduler: scheduler.downgrade(),
        callback,
    };
    owner.post_frame_handle().schedule(move |_| { let _ = &capture; })
        .expect("queued post-frame envelope");
    assert!(owner.retire().is_none());
    assert_eq!(drops.get(), 0, "standalone cleanup shares custody with producer removals");
    assert_healthy_refusal_retires(&owner);
}

fn eager_pending_retirement_preserves_caught_failure() {
    retired_task_admission_preserves_caught_failure(0);
}

fn retired_lazy_admission_preserves_caught_failure() {
    retired_task_admission_preserves_caught_failure(1);
}

fn retired_eager_admission_preserves_caught_failure() {
    retired_task_admission_preserves_caught_failure(2);
}

fn retired_task_admission_preserves_caught_failure(mode: u8) {
    // Eager polling requires a 'static future; use a weak owner envelope.
    struct EagerRetiringFuture {
        owner: Weak<OwnerFrame>,
        capture: RemovedCapture,
    }
    impl Future for EagerRetiringFuture {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            let _ = &self.capture;
            let owner = self.owner.upgrade().expect("live owner");
            catch_nested_refusal_failure(&owner);
            assert!(owner.retire().is_none());
            Poll::Pending
        }
    }
    {
        let scheduler = UpdateScheduler::new();
        let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
        let driver = owner.async_driver();
        let drops = Rc::new(Cell::new(0));
        assert_eq!(owner.pump_background(|| {
            if mode == 0 {
                let token = driver.spawn_local_eager(Box::pin(EagerRetiringFuture {
                    owner: Rc::downgrade(&owner),
                    capture: RemovedCapture(Rc::clone(&drops)),
                })).expect("pending poll refuses retired admission");
                assert!(token.is_cancelled());
            } else {
                catch_nested_refusal_failure(&owner);
                assert!(owner.retire().is_none());
                let future = Box::pin(RemovedFuture {
                    capture: RemovedCapture(Rc::clone(&drops)),
                    ready: false,
                });
                let token = if mode == 1 { driver.spawn_local(future) }
                    else { driver.spawn_local_eager(future).expect("refused eager token") };
                assert!(token.is_cancelled());
            }
        }), Ok(0));
        assert_eq!(drops.get(), 0, "retired admission retains opaque future, mode {mode}");
        assert_eq!(driver.pending_task_count(), 0);
        assert_eq!(owner.pump_background(|| {}), Err(ExecutionError::Retired));
        drop(owner);
        let replacement = OwnerFrame::new(&scheduler).expect("replacement owner");
        assert_eq!(replacement.pump_background(|| {}), Ok(0));
    }
}
