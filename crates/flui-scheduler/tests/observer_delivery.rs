//! Observer delivery survives containment and preserves committed transition order.
use flui_scheduler::{AppLifecycleState, IdleDeadline, Instant, OwnerFrame, UpdateScheduler};
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

fn timings_without_subscribers_retain_nothing() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("fresh owner");
    for _ in 0..10_000 {
        let now = Instant::now();
        owner
            .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
            .expect("frame completes");
    }
    assert_eq!(scheduler.report_timings(), 0);
}

fn timings_automatically_flush_and_allow_reentry() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("fresh owner");
    let received = Rc::new(RefCell::new(0));
    let seen = Rc::clone(&received);
    let weak = scheduler.downgrade();
    scheduler.add_timings_callback(Rc::new(move |batch| {
        *seen.borrow_mut() += batch.len();
        assert_eq!(weak.upgrade().expect("live owner").report_timings(), 0);
    }));
    for _ in 0..256 {
        let now = Instant::now();
        owner
            .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
            .expect("frame completes");
    }
    assert!(
        *received.borrow() > 0,
        "production frame completion must deliver timings without manual polling"
    );
    scheduler.report_timings();
    assert_eq!(*received.borrow(), 256);
}

fn timings_failure_delivers_healthy_tail_and_recovers() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::try_new(&scheduler).expect("fresh owner");
    let failed: flui_scheduler::TimingsCallback = Rc::new(|_| panic!("timings listener"));
    scheduler.add_timings_callback(Rc::clone(&failed));
    let competing: flui_scheduler::TimingsCallback = Rc::new(|_| panic!("second timings listener"));
    scheduler.add_timings_callback(Rc::clone(&competing));
    let received = Rc::new(RefCell::new(0));
    let seen = Rc::clone(&received);
    scheduler.add_timings_callback(Rc::new(move |batch| *seen.borrow_mut() += batch.len()));
    let now = Instant::now();
    owner
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
        .expect("partial batch frame");
    let failure = catch_unwind(AssertUnwindSafe(|| scheduler.report_timings()))
        .expect_err("timing failures propagate after delivery");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"timings listener"));
    scheduler.remove_timings_callback(&failed);
    scheduler.remove_timings_callback(&competing);
    let now = Instant::now();
    owner
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
        .expect("recovery frame");
    scheduler.report_timings();
    assert_eq!(*received.borrow(), 2);
}

fn observer_failures_protect_nested_retirement() {
    struct Capture(Rc<std::cell::Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for timings in [false, true] {
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
                    let id = pending.take().expect("accepted transient callback");
                    let scheduler = weak.upgrade().expect("live scheduler");
                    if mode == "nested" {
                        owner
                            .upgrade()
                            .expect("live owner")
                            .pump_background(|| {
                                assert!(scheduler.cancel_frame_callback(id));
                                pending_task
                                    .borrow_mut()
                                    .take()
                                    .expect("accepted task")
                                    .cancel();
                            })
                            .expect("nested owner turn");
                    } else {
                        assert!(scheduler.cancel_frame_callback(id));
                        pending_task
                            .borrow_mut()
                            .take()
                            .expect("accepted task")
                            .cancel();
                    }
                }
            };
            if timings {
                scheduler.add_timings_callback(Rc::new(|_| panic!("observer failure")));
                scheduler.add_timings_callback(Rc::new(move |_| cancel()));
                let now = Instant::now();
                owner
                    .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
                    .expect("partial timing batch");
            } else {
                scheduler.add_lifecycle_state_listener(Rc::new(|_| panic!("observer failure")));
                scheduler.add_lifecycle_state_listener(Rc::new(move |_| cancel()));
            }
            let deliver = || {
                let capture = Capture(Rc::clone(&drops));
                pending.set(Some(
                    scheduler.schedule_frame_callback(Box::new(move |_| drop(capture))),
                ));
                let capture = Capture(Rc::clone(&drops));
                *pending_task.borrow_mut() =
                    Some(owner.async_driver().spawn_local(Box::pin(async move {
                        std::future::pending::<()>().await;
                        drop(capture);
                    })));
                if timings {
                    scheduler.report_timings();
                } else {
                    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
                }
            };
            let failure = catch_unwind(AssertUnwindSafe(|| {
                if mode == "active" {
                    let now = Instant::now();
                    owner
                        .drive_frame(now, IdleDeadline::far_future(now), || {}, deliver)
                        .expect("owner turn admitted");
                } else {
                    deliver();
                }
            }))
            .expect_err("first observer failure survives cleanup");
            assert_eq!(failure.downcast_ref::<&str>(), Some(&"observer failure"));
            assert_eq!(
                drops.get(),
                0,
                "{timings}/{mode}: nested cleanup must retain opaque captures after containment"
            );
            let capture = Capture(Rc::clone(&drops));
            let id = scheduler.schedule_frame_callback(Box::new(move |_| drop(capture)));
            assert!(scheduler.cancel_frame_callback(id));
            assert_eq!(drops.get(), 1, "healthy next operation retires normally");
        }
    }
}

fn observer_failure_custody_follows_a_replacement_owner() {
    struct Capture(Rc<std::cell::Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for timings in [false, true] {
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
        if timings {
            scheduler.add_timings_callback(Rc::new(|_| panic!("observer failure")));
            scheduler.add_timings_callback(Rc::new(move |_| replace()));
            let now = Instant::now();
            owner
                .borrow()
                .as_ref()
                .expect("initial owner")
                .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
                .expect("partial timing batch");
        } else {
            scheduler.add_lifecycle_state_listener(Rc::new(|_| panic!("observer failure")));
            scheduler.add_lifecycle_state_listener(Rc::new(move |_| replace()));
        }
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
            if timings {
                scheduler.report_timings();
            } else {
                scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Hidden);
            }
        }))
        .expect_err("observer failure propagates after owner replacement");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"observer failure"));
        assert_eq!(
            drops.get(),
            0,
            "{timings}: outgoing and replacement task captures inherit custody"
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
            1,
            "the replacement's next healthy cancellation retires normally"
        );
    }
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
            (
                "timings_without_subscribers_retain_nothing",
                timings_without_subscribers_retain_nothing,
            ),
            (
                "timings_automatically_flush_and_allow_reentry",
                timings_automatically_flush_and_allow_reentry,
            ),
            (
                "timings_failure_delivers_healthy_tail_and_recovers",
                timings_failure_delivers_healthy_tail_and_recovers,
            ),
        ],
    );
}
