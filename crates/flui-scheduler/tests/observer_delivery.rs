//! Observer delivery survives containment and preserves committed transition order.
use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{AppLifecycleState, Instant, OwnerFrame, UpdateScheduler};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn lifecycle_failure_delivers_healthy_tail() {
    let scheduler = UpdateScheduler::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let failed = scheduler.add_lifecycle_state_listener(Rc::new(|_| panic!("first listener")));
    let competing = scheduler.add_lifecycle_state_listener(Rc::new(|_| panic!("second listener")));
    let output = Rc::clone(&seen);
    scheduler.add_lifecycle_state_listener(Rc::new(move |state| output.borrow_mut().push(state)));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    }))
    .expect_err("listener failures propagate after delivery");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"first listener"));
    scheduler.remove_lifecycle_state_listener(failed);
    scheduler.remove_lifecycle_state_listener(competing);
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
    assert_eq!(
        &*seen.borrow(),
        &[AppLifecycleState::Hidden, AppLifecycleState::Resumed]
    );
}

fn lifecycle_reentry_preserves_order() {
    let scheduler = UpdateScheduler::new();
    let weak = scheduler.downgrade();
    scheduler.add_lifecycle_state_listener(Rc::new(move |state| {
        if state == AppLifecycleState::Hidden {
            weak.upgrade()
                .expect("live owner")
                .handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
        }
    }));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = Rc::clone(&seen);
    scheduler.add_lifecycle_state_listener(Rc::new(move |state| output.borrow_mut().push(state)));
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    assert_eq!(
        &*seen.borrow(),
        &[AppLifecycleState::Hidden, AppLifecycleState::Resumed]
    );
}

fn lifecycle_removal_before_delivery_skips_retired_listener() {
    let scheduler = UpdateScheduler::new();
    let removed = Rc::new(std::cell::Cell::new(None));
    let target = Rc::clone(&removed);
    let weak = scheduler.downgrade();
    scheduler.add_lifecycle_state_listener(Rc::new(move |_| {
        if let Some(id) = target.take() {
            assert!(
                weak.upgrade()
                    .expect("live scheduler")
                    .remove_lifecycle_state_listener(id)
            );
        }
    }));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let retired_output = Rc::clone(&seen);
    removed.set(Some(scheduler.add_lifecycle_state_listener(Rc::new(
        move |state| {
            retired_output.borrow_mut().push(("retired", state));
        },
    ))));
    let healthy_output = Rc::clone(&seen);
    scheduler.add_lifecycle_state_listener(Rc::new(move |state| {
        healthy_output.borrow_mut().push(("healthy", state));
    }));
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
    assert_eq!(
        &*seen.borrow(),
        &[
            ("healthy", AppLifecycleState::Hidden),
            ("healthy", AppLifecycleState::Resumed)
        ],
        "removal suppresses the pending invocation without skipping the healthy tail"
    );
}

fn observer_failures_protect_nested_retirement() {
    struct Capture(Rc<std::cell::Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for mode in ["standalone", "active", "nested"] {
        let scheduler = UpdateScheduler::new();
        let owner = Rc::new(OwnerFrame::try_new(&scheduler).expect("fresh owner"));
        let drops = Rc::new(std::cell::Cell::new(0));
        let pending = Rc::new(std::cell::Cell::new(None));
        let pending_task = Rc::new(RefCell::new(None::<flui_scheduler::TaskToken>));
        let cancel = {
            let weak = scheduler.downgrade();
            let pending = Rc::clone(&pending);
            let pending_task = Rc::clone(&pending_task);
            let owner = Rc::downgrade(&owner);
            move || {
                let id = pending.take().expect("accepted listener");
                let scheduler = weak.upgrade().expect("live scheduler");
                if mode == "nested" {
                    owner
                        .upgrade()
                        .expect("live owner")
                        .pump_background(|| {
                            assert!(scheduler.remove_lifecycle_state_listener(id));
                            pending_task
                                .borrow_mut()
                                .take()
                                .expect("accepted task")
                                .cancel();
                        })
                        .expect("nested owner turn");
                } else {
                    assert!(scheduler.remove_lifecycle_state_listener(id));
                    pending_task
                        .borrow_mut()
                        .take()
                        .expect("accepted task")
                        .cancel();
                }
            }
        };
        scheduler.add_lifecycle_state_listener(Rc::new(|_| panic!("observer failure")));
        scheduler.add_lifecycle_state_listener(Rc::new(move |_| cancel()));
        let deliver = || {
            let capture = Capture(Rc::clone(&drops));
            pending.set(Some(scheduler.add_lifecycle_state_listener(Rc::new(
                move |_| {
                    let _ = &capture;
                },
            ))));
            let capture = Capture(Rc::clone(&drops));
            *pending_task.borrow_mut() =
                Some(owner.async_driver().spawn_local(Box::pin(async move {
                    std::future::pending::<()>().await;
                    drop(capture);
                })));
            scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
        };
        let failure = catch_unwind(AssertUnwindSafe(|| {
            if mode == "active" {
                let now = Instant::now();
                owner
                    .drive_frame(now, || {}, deliver)
                    .expect("owner turn admitted");
            } else {
                deliver();
            }
        }))
        .expect_err("first observer failure survives cleanup");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"observer failure"));
        assert_eq!(
            drops.get(),
            2,
            "{mode}: nested cleanup destroys both opaque captures behind the first failure"
        );
        let capture = Capture(Rc::clone(&drops));
        let id = scheduler.add_lifecycle_state_listener(Rc::new(move |_| {
            let _ = &capture;
        }));
        assert!(scheduler.remove_lifecycle_state_listener(id));
        assert_eq!(drops.get(), 3, "healthy next operation retires normally");
    }
}

fn observer_failure_custody_follows_a_replacement_owner() {
    struct Capture(Rc<std::cell::Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let scheduler = UpdateScheduler::new();
    let owner = Rc::new(RefCell::new(Some(
        OwnerFrame::try_new(&scheduler).expect("fresh owner"),
    )));
    let drops = Rc::new(std::cell::Cell::new(0));
    let replace = {
        let weak = scheduler.downgrade();
        let owner = Rc::clone(&owner);
        let drops = Rc::clone(&drops);
        move || {
            let outgoing = owner.borrow_mut().take().expect("outgoing owner");
            drop(outgoing);
            let scheduler = weak.upgrade().expect("live scheduler");
            let replacement = OwnerFrame::try_new(&scheduler).expect("owner slot released");
            let capture = Capture(Rc::clone(&drops));
            let task = replacement.async_driver().spawn_local(Box::pin(async move {
                std::future::pending::<()>().await;
                drop(capture);
            }));
            task.cancel();
            *owner.borrow_mut() = Some(replacement);
        }
    };
    scheduler.add_lifecycle_state_listener(Rc::new(|_| panic!("observer failure")));
    scheduler.add_lifecycle_state_listener(Rc::new(move |_| replace()));
    let capture = Capture(Rc::clone(&drops));
    let _outgoing_task = owner
        .borrow()
        .as_ref()
        .expect("initial owner")
        .async_driver()
        .spawn_local(Box::pin(async move {
            std::future::pending::<()>().await;
            drop(capture);
        }));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
    }))
    .expect_err("observer failure propagates after owner replacement");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"observer failure"));
    assert_eq!(
        drops.get(),
        2,
        "outgoing and replacement task captures are destroyed behind the first failure"
    );
    let capture = Capture(Rc::clone(&drops));
    let healthy = owner
        .borrow()
        .as_ref()
        .expect("replacement owner")
        .async_driver()
        .spawn_local(Box::pin(async move {
            std::future::pending::<()>().await;
            drop(capture);
        }));
    healthy.cancel();
    assert_eq!(
        drops.get(),
        3,
        "the replacement's next healthy cancellation retires normally"
    );
}

#[test]
fn observer_delivery_contract() {
    crate::run_table(
        "observer delivery",
        &[
            (
                "observer_failure_custody_follows_a_replacement_owner",
                observer_failure_custody_follows_a_replacement_owner,
            ),
            (
                "observer_failures_protect_nested_retirement",
                observer_failures_protect_nested_retirement,
            ),
            (
                "lifecycle_failure_delivers_healthy_tail",
                lifecycle_failure_delivers_healthy_tail,
            ),
            (
                "lifecycle_reentry_preserves_order",
                lifecycle_reentry_preserves_order,
            ),
            (
                "lifecycle_removal_before_delivery_skips_retired_listener",
                lifecycle_removal_before_delivery_skips_retired_listener,
            ),
        ],
    );
}
