//! Complete background turns preserve accepted work without producing a frame.

use std::cell::{Cell, RefCell};
use std::future::poll_fn;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;

use flui_scheduler::{ExecutionError, IdleDeadline, Instant, OwnerFrame, UpdateScheduler};

fn background_batches_are_bounded_with_frames_disabled() {
    let mut scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    scheduler.set_frames_enabled(false);
    let visual_calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&visual_calls);
    scheduler.add_persistent_frame_callback(Rc::new(move |_| observed.set(observed.get() + 1)));
    let trace = Rc::new(RefCell::new(Vec::new()));
    let task_trace = Rc::clone(&trace);
    let polls = Rc::new(Cell::new(0));
    let task_polls = Rc::clone(&polls);
    let token = owner
        .async_driver()
        .spawn_local(Box::pin(poll_fn(move |cx| {
            task_trace.borrow_mut().push("poll");
            task_polls.set(task_polls.get() + 1);
            if task_polls.get() == 1 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })));
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    })));
    let before = wakes.load(Ordering::SeqCst);
    assert_eq!(
        owner.pump_background(|| {
            assert!(
                !scheduler.is_frame_scheduled(),
                "old issuance is consumed before preparation"
            );
            trace.borrow_mut().push("prepare");
        }),
        Ok(1)
    );
    assert_eq!(trace.borrow().as_slice(), ["prepare", "poll"]);
    assert_eq!(polls.get(), 1, "self-wake cannot re-enter this ready batch");
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        before + 1,
        "self-wake requests a real next opportunity"
    );
    assert!(
        scheduler.is_frame_scheduled(),
        "new demand survives the completed background turn"
    );
    assert_eq!(
        owner.pump_background(|| trace.borrow_mut().push("prepare")),
        Ok(1)
    );
    assert_eq!(
        trace.borrow().as_slice(),
        ["prepare", "poll", "prepare", "poll"]
    );
    assert_eq!(
        owner.pump_background(|| {}),
        Ok(0),
        "completed task is no longer polled"
    );
    assert_eq!(scheduler.frame_count(), 0);
    assert_eq!(visual_calls.get(), 0);
    assert!(!token.is_cancelled());
}

fn failed_preparation_restores_hook_delivery_and_preserves_ready_work() {
    for hook_present in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
        let polls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&polls);
        let token = owner.async_driver().spawn_local(Box::pin(async move {
            observed.set(observed.get() + 1);
        }));
        let wakes = Arc::new(AtomicUsize::new(0));
        if hook_present {
            let observed = Arc::clone(&wakes);
            scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            })));
        }
        let before = wakes.load(Ordering::SeqCst);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner.pump_background(|| panic!("background preparation failure"))
        }))
        .expect_err("preparation propagates its failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("background preparation failure")
        );
        assert_eq!(polls.get(), 0, "failed preparation cannot poll ready work");
        assert_eq!(owner.async_driver().pending_task_count(), 1);
        if hook_present {
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                before + 1,
                "failure explicitly re-delivers accepted work"
            );
        } else {
            let observed = Arc::clone(&wakes);
            scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            })));
            assert_eq!(
                wakes.load(Ordering::SeqCst),
                1,
                "installing a hook pays retained debt"
            );
        }
        assert_eq!(owner.pump_background(|| {}), Ok(1));
        assert_eq!(polls.get(), 1);
        assert_eq!(scheduler.frame_count(), 0);
        assert!(!token.is_cancelled());
    }
}

fn preparation_generated_demand_survives_the_ready_poll() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
    let task_ran = Rc::new(Cell::new(false));
    let observed = Rc::clone(&task_ran);
    let token = owner.async_driver().spawn_local(Box::pin(async move {
        observed.set(true);
    }));
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    })));
    let before = wakes.load(Ordering::SeqCst);
    assert_eq!(
        owner.pump_background(|| {
            assert!(!task_ran.get(), "preparation precedes task effects");
            scheduler.request_frame();
        }),
        Ok(1)
    );
    assert!(task_ran.get());
    assert_eq!(wakes.load(Ordering::SeqCst), before + 1);
    assert!(
        scheduler.is_frame_scheduled(),
        "preparation's new demand is not an old receipt"
    );
    assert_eq!(owner.pump_background(|| {}), Ok(0));
    assert!(!token.is_cancelled());
}

thread_local! {
    static RECOVERY_OWNER: RefCell<Option<(Rc<OwnerFrame>, flui_scheduler::WeakUpdateScheduler)>> = const { RefCell::new(None) };
}

struct OwnerScope;
impl Drop for OwnerScope {
    fn drop(&mut self) {
        RECOVERY_OWNER.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

struct HostileCapture(Arc<AtomicUsize>);
impl Drop for HostileCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("rejected recovery capture was destroyed");
    }
}

struct HookCapture(Arc<AtomicUsize>);
impl Drop for HookCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn recovery_keeps_first_failure_through_nested_refusal_and_hook_retirement() {
    for hook_fails in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
        RECOVERY_OWNER
            .with(|slot| *slot.borrow_mut() = Some((Rc::clone(&owner), scheduler.downgrade())));
        let _scope = OwnerScope;
        let armed = Arc::new(AtomicBool::new(false));
        let hook_armed = Arc::clone(&armed);
        let calls = Arc::new(AtomicUsize::new(0));
        let hook_calls = Arc::clone(&calls);
        let rejected_drops = Arc::new(AtomicUsize::new(0));
        let hook_rejected_drops = Arc::clone(&rejected_drops);
        let hook_drops = Arc::new(AtomicUsize::new(0));
        let hook_capture = HookCapture(Arc::clone(&hook_drops));
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            let _ = &hook_capture;
            hook_calls.fetch_add(1, Ordering::SeqCst);
            if !hook_armed.load(Ordering::SeqCst) {
                return;
            }
            let (owner, weak) =
                RECOVERY_OWNER.with(|slot| slot.borrow().as_ref().expect("owner scope").clone());
            let capture = HostileCapture(Arc::clone(&hook_rejected_drops));
            assert_eq!(
                owner.pump_background(move || {
                    let _ = &capture;
                    panic!("nested preparation ran");
                }),
                Err(ExecutionError::AlreadyExecuting)
            );
            let capture = HostileCapture(Arc::clone(&hook_rejected_drops));
            let now = Instant::now();
            assert_eq!(
                owner.drive_frame(
                    now,
                    IdleDeadline::far_future(now),
                    || {},
                    move || -> () {
                        let _ = &capture;
                        panic!("nested pipeline ran");
                    }
                ),
                Err(ExecutionError::AlreadyExecuting)
            );
            weak.upgrade()
                .expect("scheduler still owned")
                .set_on_frame_scheduled(None);
            assert!(!hook_fails, "recovery hook failure");
        })));
        let polls = Rc::new(Cell::new(0));
        let observed = Rc::clone(&polls);
        let token = owner.async_driver().spawn_local(Box::pin(async move {
            observed.set(observed.get() + 1);
        }));
        let before = calls.load(Ordering::SeqCst);
        armed.store(true, Ordering::SeqCst);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner.pump_background(|| panic!("first preparation failure"))
        }))
        .expect_err("first failure propagates");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("first preparation failure")
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            before + 1,
            "bounded recovery does not spin"
        );
        assert_eq!(
            rejected_drops.load(Ordering::SeqCst),
            0,
            "caught failure custody protects refused envelopes"
        );
        assert_eq!(
            hook_drops.load(Ordering::SeqCst),
            0,
            "successful self-uninstallation still retains the hook envelope"
        );
        assert_eq!(polls.get(), 0);
        let replacement_calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&replacement_calls);
        scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        })));
        assert_eq!(
            replacement_calls.load(Ordering::SeqCst),
            usize::from(hook_fails),
            "only failed recovery still owes delivery"
        );
        assert_eq!(owner.pump_background(|| {}), Ok(1));
        assert_eq!(
            polls.get(),
            1,
            "healthy next turn delivers the retained future"
        );
        assert_eq!(scheduler.frame_count(), 0);
        assert!(!token.is_cancelled());
    }
}

fn healthy_wake_retirement_observes_newly_caught_failure() {
    let scheduler = UpdateScheduler::new();
    let owner = Rc::new(OwnerFrame::new(&scheduler).expect("fresh owner"));
    RECOVERY_OWNER.with(|slot| {
        *slot.borrow_mut() = Some((Rc::clone(&owner), scheduler.downgrade()));
    });
    let _scope = OwnerScope;
    let hook_drops = Arc::new(AtomicUsize::new(0));
    let hook_capture = HookCapture(Arc::clone(&hook_drops));
    let rejected_drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&rejected_drops);
    scheduler.set_on_frame_scheduled(Some(Arc::new(move || {
        let _ = &hook_capture;
        let (owner, weak) =
            RECOVERY_OWNER.with(|slot| slot.borrow().as_ref().expect("owner scope").clone());
        weak.upgrade().expect("scheduler live").set_on_frame_scheduled(None);
        let capture = HostileCapture(Arc::clone(&observed));
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner.pump_background(move || { let _ = &capture; })
        })).expect_err("nested refused envelope fails");
        assert_eq!(flui_foundation::panic::payload_text(failure.as_ref()),
            Some("rejected recovery capture was destroyed"));
    })));
    assert_eq!(owner.pump_background(|| scheduler.request_frame()), Ok(0));
    assert_eq!(rejected_drops.load(Ordering::SeqCst), 1);
    assert_eq!(hook_drops.load(Ordering::SeqCst), 0,
        "wake begun healthy retains its envelope after newly caught failure");
    assert_eq!(owner.pump_background(|| {}), Ok(0));
}

#[test]
fn owner_background_turn_contract() {
    crate::run_table(
        "owner_background_turn_contract",
        &[
            (
                "bounded_disabled_frames",
                background_batches_are_bounded_with_frames_disabled as fn(),
            ),
            (
                "preparation_failure_delivery",
                failed_preparation_restores_hook_delivery_and_preserves_ready_work,
            ),
            (
                "preparation_generated_demand",
                preparation_generated_demand_survives_the_ready_poll,
            ),
            (
                "recovery_failure_custody",
                recovery_keeps_first_failure_through_nested_refusal_and_hook_retirement,
            ),
            (
                "new_failure_during_healthy_wake",
                healthy_wake_retirement_observes_newly_caught_failure,
            ),
        ],
    );
}
