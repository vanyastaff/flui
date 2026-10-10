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
        ],
    );
}
