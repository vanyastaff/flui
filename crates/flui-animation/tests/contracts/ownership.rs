//! Listener ownership and the frame-scheduled hook: what a listener captures is
//! released once its owner is disposed or dropped, a run in flight does not
//! keep its controller alive, and the embedder's frame-scheduled hook runs
//! with the controller free to read.
//!
//! The contract rows run in the ordinary suite. Hook rows use child processes
//! to contain a possible deadlock without hanging the parent runner.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::child_process;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, AnimationSwitch, CurvedAnimation,
    ProxyAnimation, RepeatMode, ReverseAnimation, Tween, TweenAnimation, Vsync, curves,
};
use flui_foundation::Listenable;

#[test]
fn vsync_admission_contract() {
    crate::run_table(&[
        (
            "second parent cannot apply its motion policy to an existing owner",
            second_parent_cannot_apply_its_motion_policy_to_an_existing_owner,
        ),
        ("duplicate child is refused", duplicate_child_is_refused),
        (
            "cycles are refused without changing the tree",
            cycles_are_refused_without_changing_the_tree,
        ),
        (
            "a surviving child can move after parent release",
            a_surviving_child_can_move_after_parent_release,
        ),
        (
            "a failed attachment wake rolls back before recovery",
            a_failed_attachment_wake_rolls_back_before_recovery,
        ),
        (
            "moving a subtree preserves its last sampled motion",
            moving_a_subtree_preserves_its_last_sampled_motion,
        ),
        (
            "a withdrawn child waits for its new parent",
            a_withdrawn_child_waits_for_its_new_parent,
        ),
        (
            "a withdrawn ancestor stops its in-flight descendants",
            a_withdrawn_ancestor_stops_its_in_flight_descendants,
        ),
        (
            "a muted ancestor stops its in-flight descendants",
            a_muted_ancestor_stops_its_in_flight_descendants,
        ),
        (
            "direct child frames retain their parent clock authority",
            direct_child_frames_retain_their_parent_clock_authority,
        ),
        (
            "child snapshot custody survives reentry and retires outside borrows",
            child_snapshot_custody_survives_reentry_and_retires_outside_borrows,
        ),
    ]);
}

fn second_parent_cannot_apply_its_motion_policy_to_an_existing_owner() {
    use flui_animation::{AnimatedValue, ArcCurve, MotionClock, MotionOverride, MotionSpec};
    let normal = Vsync::new();
    let reduced = Vsync::new();
    let child = Vsync::new();
    normal.attach_child(&child).expect("first parent");
    let admission = reduced.attach_child(&child);
    let mut normal_clock = MotionClock::new();
    let mut reduced_clock = MotionClock::new();
    reduced_clock.set_override(MotionOverride::Reduce);
    let mut value = AnimatedValue::new(
        0.0_f64,
        MotionSpec::Curve {
            duration: Duration::from_secs(1),
            curve: ArcCurve::new(curves::EASE_IN_OUT),
        },
        Some(&child),
    )
    .expect("finite motion");
    let run = value.animate_to(1.0).expect("finite target");
    normal.tick_all(&normal_clock.frame(Duration::ZERO));
    normal.tick_all(&normal_clock.frame(Duration::from_millis(250)));
    let before = value.value();
    assert!(before > 0.0 && before < 1.0 && run.is_pending());
    reduced.tick_all(&reduced_clock.frame(Duration::from_millis(250)));
    assert_eq!(
        (
            admission == Err(flui_animation::VsyncRegistrationError::AlreadyAttached),
            value.value(),
            run.is_pending()
        ),
        (true, before, true),
        "the second presentation must neither gain ownership nor settle the first presentation's motion"
    );
}

fn child_snapshot_custody_survives_reentry_and_retires_outside_borrows() {
    use flui_animation::MotionClock;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct Capture {
        parent: Vsync,
        events: Rc<RefCell<Vec<&'static str>>>,
        name: &'static str,
        fail: bool,
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            let _ = self.parent.has_running();
            self.events.borrow_mut().push(self.name);
            assert!(!self.fail, "child capture retirement");
        }
    }

    for (mute, callback_failure, drop_failure) in [
        (false, false, false),
        (true, false, false),
        (false, false, true),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let parent = Vsync::new();
        let first = Vsync::new();
        parent.attach_child(&first).expect("first registry");
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut seats = Vec::new();
        for name in ["second", "third"] {
            let child = Vsync::new();
            let capture = Capture {
                parent: parent.clone(),
                events: events.clone(),
                name,
                fail: name == "second" && drop_failure,
            };
            child.set_frame_requester(Some(Rc::new(move || {
                let _ = &capture;
            })));
            seats.push(parent.attach_child(&child).expect("later registry"));
            // The parent's entry snapshot must be the final owner after detach.
            drop(child);
        }
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&first));
        owner.controller().forward().expect("first child run");
        let mut clock = MotionClock::new();
        parent.tick_all(&clock.frame(Duration::ZERO));
        let tick = clock.frame(Duration::from_millis(250));
        let callback_parent = parent.clone();
        let callback_events = events.clone();
        let entered = Cell::new(false);
        owner.controller().add_listener(Rc::new(move || {
            if entered.replace(true) {
                return;
            }
            for seat in &seats {
                callback_parent.detach_child(seat);
            }
            callback_parent.set_muted(mute);
            callback_parent.tick_all(&tick);
            assert!(
                callback_events.borrow().is_empty(),
                "entry custody survives reentry"
            );
            callback_events.borrow_mut().push("callback finished");
            assert!(!callback_failure, "first child callback");
        }));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parent.tick_all(&tick);
        }));
        let expected = if callback_failure {
            vec!["callback finished"]
        } else if drop_failure {
            vec!["callback finished", "second"]
        } else {
            vec!["callback finished", "second", "third"]
        };
        assert_eq!(*events.borrow(), expected);
        if callback_failure || drop_failure {
            let payload = result.expect_err("authored failure propagates after retirement");
            assert_eq!(
                payload.downcast_ref::<&str>().copied(),
                Some(if callback_failure {
                    "first child callback"
                } else {
                    "child capture retirement"
                })
            );
        } else {
            result.expect("healthy child retirement");
        }
        parent.set_muted(false);
        parent.tick_all(&clock.frame(Duration::from_millis(500)));
        assert!((owner.controller().value() - 0.5).abs() < 1e-9);
        assert_eq!(
            *events.borrow(),
            expected,
            "empty cache retains no detached captures"
        );
    }
}

fn duplicate_child_is_refused() {
    use flui_animation::MotionClock;
    use std::cell::Cell;
    use std::rc::Rc;
    let parent = Vsync::new();
    let child = Vsync::new();
    let first = parent.attach_child(&child).expect("first admission");
    let duplicate = parent.attach_child(&child);
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    let samples = Rc::new(Cell::new(0));
    let observed = Rc::clone(&samples);
    owner
        .controller()
        .add_listener(Rc::new(move || observed.set(observed.get() + 1)));
    let run = owner.controller().forward().expect("finite run");
    let mut clock = MotionClock::new();
    parent.tick_all(&clock.frame(Duration::ZERO));
    samples.set(0);
    parent.tick_all(&clock.frame(Duration::from_millis(250)));
    assert_eq!(
        (
            duplicate == Err(flui_animation::VsyncRegistrationError::AlreadyAttached),
            samples.get()
        ),
        (true, 1),
        "an admitted child is sampled once and duplicate attachment is refused"
    );
    parent.detach_child(&first);
    let replacement = parent.attach_child(&child).expect("reattach after detach");
    parent.detach_child(&first);
    parent.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(
        run.is_complete(),
        "an old seat must not detach the replacement"
    );
    parent.detach_child(&replacement);
}

fn cycles_are_refused_without_changing_the_tree() {
    use flui_animation::{MotionClock, VsyncRegistrationError};
    let root = Vsync::new();
    let middle = Vsync::new();
    let leaf = Vsync::new();
    root.attach_child(&middle).expect("middle");
    middle.attach_child(&leaf).expect("leaf");
    for (parent, child) in [(&root, &root), (&leaf, &root), (&leaf, &middle)] {
        assert_eq!(
            parent.attach_child(child),
            Err(VsyncRegistrationError::WouldCycle)
        );
    }
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&leaf));
    let run = owner.controller().forward().expect("resident run");
    let mut clock = MotionClock::new();
    root.tick_all(&clock.frame(Duration::ZERO));
    root.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(
        run.is_complete(),
        "refusals leave the existing tree deliverable"
    );
    leaf.prepare_close().finish();
    assert_eq!(
        root.attach_child(&leaf),
        Err(VsyncRegistrationError::Closed)
    );
}

fn a_surviving_child_can_move_after_parent_release() {
    use flui_animation::MotionClock;
    let child = Vsync::new();
    let old = Vsync::new();
    let old_seat = old.attach_child(&child).expect("first parent");
    drop(old);
    let new = Vsync::new();
    let new_seat = new
        .attach_child(&child)
        .expect("expired parent is not an attachment");
    new.detach_child(&old_seat);
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    let run = owner.controller().forward().expect("finite run");
    let mut clock = MotionClock::new();
    new.tick_all(&clock.frame(Duration::ZERO));
    new.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(run.is_complete());
    new.detach_child(&new_seat);
}

fn a_failed_attachment_wake_rolls_back_before_recovery() {
    use flui_animation::{MotionClock, VsyncRegistrationError};
    use std::rc::Rc;
    let parent = Vsync::new();
    let child = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    let run = owner
        .controller()
        .forward()
        .expect("resident run demands a frame");
    let observed = child.clone();
    parent.set_frame_requester(Some(Rc::new(move || {
        assert_eq!(
            Vsync::new().attach_child(&observed),
            Err(VsyncRegistrationError::AlreadyAttached),
            "the first attachment commits before its wake"
        );
        panic!("attachment wake failure");
    })));
    let failure =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parent.attach_child(&child)))
            .expect_err("wake failure propagates after rollback");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"attachment wake failure")
    );
    parent.set_frame_requester(None);
    let replacement = Vsync::new();
    replacement
        .attach_child(&child)
        .expect("failed attachment released the parent slot");
    let mut clock = MotionClock::new();
    replacement.tick_all(&clock.frame(Duration::ZERO));
    replacement.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(
        run.is_complete(),
        "resident work survives admission recovery"
    );
}

fn moving_a_subtree_preserves_its_last_sampled_motion() {
    use flui_animation::MotionClock;
    for origin in [Duration::ZERO, Duration::from_secs(2000)] {
        for nested in [false, true] {
            for release_parent in [false, true] {
                let old = Vsync::new();
                let moving = Vsync::new();
                let leaf = Vsync::new();
                let old_seat = old.attach_child(&moving).expect("old parent");
                let registry = if nested {
                    moving.attach_child(&leaf).expect("nested owner");
                    &leaf
                } else {
                    &moving
                };
                let owner =
                    AnimationController::builder(Duration::from_secs(1)).build_on(Some(registry));
                let controller = owner.controller();
                let run = controller.forward().expect("resident run");
                let mut old_clock = MotionClock::new();
                old.tick_all(&old_clock.frame(Duration::from_secs(100)));
                old.tick_all(&old_clock.frame(Duration::from_millis(100_400)));
                let before = controller.value();
                assert!((before - 0.4).abs() < 1e-10 && run.is_pending());
                if !release_parent {
                    old.detach_child(&old_seat);
                }
                drop(old);
                let new = Vsync::new();
                new.attach_child(&moving).expect("new parent");
                new.detach_child(&old_seat);
                let mut new_clock = MotionClock::new();
                new.tick_all(&new_clock.frame(origin));
                assert!(
                    (controller.value() - before).abs() < 1e-10 && run.is_pending(),
                    "transfer preserves the sample and run: origin={origin:?}, nested={nested}, release={release_parent}, value={}, pending={}",
                    controller.value(),
                    run.is_pending()
                );
                new.tick_all(&new_clock.frame(origin + Duration::from_millis(100)));
                assert!(
                    (controller.value() - 0.5).abs() < 1e-10,
                    "the new clock advances motion: origin={origin:?}, nested={nested}, release={release_parent}, value={}",
                    controller.value()
                );
                new.tick_all(&new_clock.frame(origin + Duration::from_millis(600)));
                assert_eq!(controller.value(), 1.0);
                assert!(
                    run.is_complete(),
                    "transfer preserves the original completion future"
                );
            }
        }
    }
}

fn direct_child_frames_retain_their_parent_clock_authority() {
    use flui_animation::MotionClock;
    for depth in [1, 3] {
        let root = Vsync::new();
        let mut child = root.clone();
        for _ in 0..depth {
            let next = Vsync::new();
            child
                .attach_child(&next)
                .expect("child belongs to this presentation");
            child = next;
        }
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
        let controller = owner.controller();
        let run = controller.forward().expect("resident run");
        let mut clock = MotionClock::new();
        root.tick_all(&clock.frame(Duration::from_secs(100)));
        root.tick_all(&clock.frame(Duration::from_millis(100_200)));
        child.tick_all(&clock.frame(Duration::from_millis(100_400)));
        assert!(
            (controller.value() - 0.4).abs() < 1e-10,
            "choosing a child entry point does not transfer presentation ownership: {}",
            controller.value()
        );
        root.tick_all(&clock.frame(Duration::from_millis(100_500)));
        assert!((controller.value() - 0.5).abs() < 1e-10 && run.is_pending());
        root.tick_all(&clock.frame(Duration::from_secs(101)));
        assert!(run.is_complete());
    }
}

fn a_withdrawn_child_waits_for_its_new_parent() {
    use flui_animation::MotionClock;
    use std::rc::Rc;
    let parent = Vsync::new();
    let earlier = Vsync::new();
    let child = Vsync::new();
    let replacement = Vsync::new();
    parent.attach_child(&earlier).expect("earlier child");
    let seat = parent.attach_child(&child).expect("later child");
    let trigger = AnimationController::builder(Duration::from_millis(400)).build_on(Some(&earlier));
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    let run = owner.controller().forward().expect("resident run");
    trigger.controller().forward().expect("trigger run");
    let mut clock = MotionClock::new();
    parent.tick_all(&clock.frame(Duration::from_secs(100)));
    let outgoing = parent.clone();
    let incoming = replacement.clone();
    let moving = child;
    let _subscription = trigger.controller().subscribe_status(Rc::new(move |_| {
        outgoing.detach_child(&seat);
        incoming.attach_child(&moving).expect("new parent");
    }));
    parent.tick_all(&clock.frame(Duration::from_millis(100_400)));
    assert_eq!(
        owner.controller().value(),
        0.0,
        "a stale child snapshot has no frame authority"
    );
    assert!(run.is_pending());
    let mut incoming_clock = MotionClock::new();
    replacement.tick_all(&incoming_clock.frame(Duration::ZERO));
    replacement.tick_all(&incoming_clock.frame(Duration::from_millis(100)));
    assert!((owner.controller().value() - 0.1).abs() < 1e-10);
}

fn a_withdrawn_ancestor_stops_its_in_flight_descendants() {
    ancestor_changes_stop_descendants(false);
}

fn a_muted_ancestor_stops_its_in_flight_descendants() {
    ancestor_changes_stop_descendants(true);
}

fn ancestor_changes_stop_descendants(mute: bool) {
    use flui_animation::MotionClock;
    use std::rc::Rc;
    let parent = Vsync::new();
    let middle = Vsync::new();
    let leaf = Vsync::new();
    let replacement = Vsync::new();
    let seat = parent.attach_child(&middle).expect("middle");
    middle.attach_child(&leaf).expect("leaf");
    let trigger = AnimationController::builder(Duration::from_millis(400)).build_on(Some(&leaf));
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&leaf));
    let run = owner.controller().forward().expect("resident run");
    trigger.controller().forward().expect("trigger run");
    let mut clock = MotionClock::new();
    parent.tick_all(&clock.frame(Duration::from_secs(100)));
    let outgoing = parent.clone();
    let incoming = replacement.clone();
    let moving = middle;
    let _subscription = trigger.controller().subscribe_status(Rc::new(move |_| {
        if mute {
            outgoing.set_muted(true);
        } else {
            outgoing.detach_child(&seat);
            incoming.attach_child(&moving).expect("new parent");
        }
    }));
    parent.tick_all(&clock.frame(Duration::from_millis(100_400)));
    assert_eq!(
        owner.controller().value(),
        0.0,
        "ancestor withdrawal or mute stops the remaining descendant samples: mute={mute}"
    );
    assert!(run.is_pending());
    if mute {
        parent.set_muted(false);
        parent.tick_all(&clock.frame(Duration::from_millis(100_500)));
        assert!((owner.controller().value() - 0.5).abs() < 1e-10);
    } else {
        let mut incoming_clock = MotionClock::new();
        replacement.tick_all(&incoming_clock.frame(Duration::ZERO));
        replacement.tick_all(&incoming_clock.frame(Duration::from_millis(100)));
        assert!((owner.controller().value() - 0.1).abs() < 1e-10);
    }
}

#[test]
fn reentrant_settles_leave_the_next_run_for_a_later_entry() {
    const TEST: &str = "ownership::reentrant_settles_leave_the_next_run_for_a_later_entry";
    let Some(case) = child_process::selected_case() else {
        child_process::run_rows(TEST, &["finite", "instant", "zero repeat"]);
        return;
    };
    let duration = if case == "finite" {
        Duration::from_secs(1)
    } else {
        Duration::ZERO
    };
    let owner = AnimationController::builder(duration).build_on(None);
    let controller = owner.controller().clone();
    let callback_controller = controller.clone();
    let completions = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = completions.clone();
    controller
        .subscribe_status(std::rc::Rc::new(move |status| match status {
            AnimationStatus::ForwardSettled => {
                observed.set(observed.get() + 1);
                let _next = callback_controller.reverse().expect("reentrant reverse");
            }
            AnimationStatus::ReverseSettled => {
                observed.set(observed.get() + 1);
                let _next = callback_controller.forward().expect("reentrant forward");
            }
            _ => {}
        }))
        .detach();
    let _run = if case == "zero repeat" {
        controller
            .repeat(RepeatMode::Restart)
            .expect("initial repeat")
    } else {
        controller.forward().expect("initial run")
    };
    assert!(
        completions.get() <= 2,
        "one outer entry has a bounded settle budget"
    );
    assert!(
        controller.is_animating(),
        "the listener's next run remains accepted"
    );
    drop(owner);
    child_process::pass();
}

/// A one-second `[0, 1]` controller advanced only by explicit times.
fn controller() -> AnimationController {
    AnimationController::builder(Duration::from_secs(1)).build()
}

fn dropping_a_driven_controller_unregisters_then_cancels_its_run() {
    let vsync = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let mut run = owner.controller().forward().expect("bound owner");
    let observed = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed_in_callback = observed.clone();
    let registry = vsync.clone();
    run.when_complete_or_cancel(move |result| {
        assert!(result.is_err());
        assert!(
            registry.is_empty(),
            "seat is absent before cancellation delivery"
        );
        registry.tick_all(&flui_animation::MotionClock::new().frame(Duration::ZERO));
        let peer = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        drop(peer);
        observed_in_callback.set(observed_in_callback.get() + 1);
    });
    drop(owner);
    assert!(vsync.is_empty());
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(_))
    ));
    assert_eq!(observed.get(), 1);
}

fn dispose_then_drop_is_one_retirement() {
    let vsync = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let run = owner.controller().forward().expect("bound owner");
    let outcomes = std::rc::Rc::new(std::cell::Cell::new(0));
    let sink = outcomes.clone();
    run.when_complete_or_cancel(move |_| sink.set(sink.get() + 1));
    owner.dispose();
    owner.dispose();
    drop(owner);
    assert!(vsync.is_empty());
    assert_eq!(outcomes.get(), 1);
}

fn rebinding_mid_run_keeps_the_value_continuous() {
    for new_origin in [0, 100_000] {
        let old = Vsync::new();
        let new = Vsync::new();
        let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
        let mut clock = flui_animation::MotionClock::new();
        let _run = owner.controller().forward().expect("bound owner");
        old.tick_all(&clock.frame(Duration::ZERO));
        old.tick_all(&clock.frame(Duration::from_millis(400)));
        let displayed = owner.controller().value();
        owner.rebind(Some(&old)).expect("same seat");
        assert_eq!(old.len(), 1);
        owner.rebind(Some(&new)).expect("fresh seat");
        assert!(old.is_empty());
        let mut new_clock = flui_animation::MotionClock::new();
        new.tick_all(&new_clock.frame(Duration::from_millis(new_origin)));
        assert_eq!(
            owner.controller().value(),
            displayed,
            "migration preserves the displayed sample"
        );
        new.tick_all(&new_clock.frame(Duration::from_millis(new_origin + 100)));
        assert_eq!(owner.controller().value(), 0.5);
    }
}

fn unbound_time_run_completes_once() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let run = owner
        .controller()
        .forward()
        .expect("unbound run is admitted");
    assert_eq!(owner.controller().value(), 1.0);
    assert!(run.is_complete());
    assert_eq!(owner.controller().status(), AnimationStatus::ForwardSettled);
}

fn rebinding_retires_the_old_registry_after_committing_the_new_clock() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture {
        controller: AnimationController,
        seen: Rc<Cell<f64>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.seen.set(self.controller.value());
            panic!("registry retirement");
        }
    }

    let old = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
    let seen = Rc::new(Cell::new(-1.0));
    let capture = Capture {
        controller: owner.controller().clone(),
        seen: seen.clone(),
    };
    old.set_frame_requester(Some(Rc::new(move || {
        let _ = &capture;
    })));
    let mut run = owner.controller().forward().expect("bound run");
    drop(old);

    let failure = catch_unwind(AssertUnwindSafe(|| {
        owner.rebind(None).expect("release old clock");
    }))
    .expect_err("outgoing registry capture fails");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"registry retirement"));
    assert!(!owner.is_bound());
    assert_eq!(
        owner.controller().value(),
        1.0,
        "losing the clock settles the run"
    );
    assert_eq!(
        seen.get(),
        1.0,
        "retirement observes the committed clock transition"
    );
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    ));

    let next = Vsync::new();
    owner.rebind(Some(&next)).expect("recover with a new clock");
    let reverse = owner.controller().reverse().expect("next run");
    let mut clock = flui_animation::MotionClock::new();
    next.tick_all(&clock.frame(Duration::ZERO));
    next.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!(owner.controller().value(), 0.0);
    assert!(reverse.is_complete());
}

fn rebinding_preserves_delivery_failure_before_outgoing_retirement() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture(Rc<Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("outgoing registry retirement");
        }
    }

    for binding in ["missing", "replacement"] {
        let old = Vsync::new();
        let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old));
        let retired = Rc::new(Cell::new(0));
        let capture = Capture(retired.clone());
        old.set_frame_requester(Some(Rc::new(move || {
            let _ = &capture;
        })));
        let run = owner.controller().forward().expect("initial bound run");
        drop(old);

        let replacement = Vsync::new();
        let healthy = Rc::new(Cell::new(0));
        if binding == "replacement" {
            replacement.set_frame_requester(Some(Rc::new(|| panic!("clock delivery"))));
        } else {
            owner
                .controller()
                .add_listener(Rc::new(|| panic!("clock delivery")));
            let observed = healthy.clone();
            owner
                .controller()
                .add_listener(Rc::new(move || observed.set(observed.get() + 1)));
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner
                .rebind((binding == "replacement").then_some(&replacement))
                .expect("clock transition");
        }))
        .expect_err("clock delivery fails");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"clock delivery"));
        assert_eq!(
            retired.get(),
            0,
            "opaque outgoing captures retain first-failure custody"
        );
        assert_eq!(owner.is_bound(), binding == "replacement");
        if binding == "missing" {
            assert_eq!(owner.controller().value(), 1.0);
            assert!(run.is_complete());
            assert_eq!(healthy.get(), 1, "healthy settlement delivery finishes");
            owner.controller().remove_all_listeners();
            owner.rebind(Some(&replacement)).expect("restore clock");
        } else {
            assert_eq!(replacement.len(), 1);
            assert!(!run.is_complete(), "accepted run survives the failed wake");
            replacement.set_frame_requester(None);
        }
        let next_run = owner
            .controller()
            .reverse()
            .expect("next operation after recovery");
        let mut clock = flui_animation::MotionClock::new();
        replacement.tick_all(&clock.frame(Duration::ZERO));
        replacement.tick_all(&clock.frame(Duration::from_secs(1)));
        assert_eq!(owner.controller().value(), 0.0);
        assert!(next_run.is_complete());
    }
}

fn dropping_the_last_owner_from_its_own_listener_mid_frame() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    for channel in ["status", "value", "continuation"] {
        let registry = Vsync::new();
        let first = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let peer = AnimationController::builder(Duration::from_secs(2)).build_on(Some(&registry));
        let survivor =
            AnimationController::builder(Duration::from_secs(2)).build_on(Some(&registry));
        let first_controller = first.controller().clone();
        let peer_controller = peer.controller().clone();
        let owners = Rc::new(RefCell::new(Some((first, peer))));
        let callback_owners = owners.clone();
        let retire = Rc::new(move || {
            let outgoing = callback_owners.borrow_mut().take();
            drop(outgoing);
        });
        let peer_calls = Rc::new(Cell::new(0));
        let observed = peer_calls.clone();
        peer_controller.add_listener(Rc::new(move || observed.set(observed.get() + 1)));
        if channel == "status" {
            let retire = retire.clone();
            first_controller
                .subscribe_status(Rc::new(move |status| {
                    if status == AnimationStatus::ForwardSettled {
                        retire();
                    }
                }))
                .detach();
        } else if channel == "value" {
            let retire = retire.clone();
            let controller = first_controller.clone();
            first_controller.add_listener(Rc::new(move || {
                if controller.value() > 0.0 {
                    retire();
                }
            }));
        }
        let run = first_controller.forward().expect("first run");
        if channel == "continuation" {
            run.when_complete_or_cancel(move |_| retire());
        }
        let _peer_run = peer_controller.forward().expect("peer run");
        let _survivor_run = survivor.controller().forward().expect("survivor run");
        let mut clock = flui_animation::MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        peer_calls.set(0);
        registry.tick_all(&clock.frame(Duration::from_secs(1)));
        assert!(owners.borrow().is_none(), "{channel}: owners were released");
        assert_eq!(
            peer_calls.get(),
            0,
            "{channel}: a retired peer is skipped in this snapshot"
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(survivor.controller().value(), 0.5);
        registry.tick_all(&clock.frame(Duration::from_millis(1200)));
        assert_eq!(peer_calls.get(), 0);
        assert_eq!(survivor.controller().value(), 0.6);
    }
}

fn disposal_registry_custody(failure: &str) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture {
        controller: AnimationController,
        drops: Rc<Cell<usize>>,
        refused: Rc<Cell<bool>>,
        panics: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            self.refused.set(self.controller.forward().is_err());
            assert!(!self.panics, "registry retirement");
        }
    }

    let registry = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let controller = owner.controller().clone();
    let drops = Rc::new(Cell::new(0));
    let refused = Rc::new(Cell::new(false));
    let capture = Capture {
        controller: controller.clone(),
        drops: drops.clone(),
        refused: refused.clone(),
        panics: failure != "none",
    };
    registry.set_frame_requester(Some(Rc::new(move || {
        let _ = &capture;
    })));
    let mut run = controller.forward().expect("bound run");
    let deliveries = Rc::new(Cell::new(0));
    if failure == "cancellation" {
        run.when_complete_or_cancel(|_| panic!("run cancellation"));
    }
    let healthy = deliveries.clone();
    run.when_complete_or_cancel(move |result| {
        assert!(result.is_err());
        healthy.set(healthy.get() + 1);
    });
    drop(registry);

    let result = catch_unwind(AssertUnwindSafe(|| owner.dispose()));
    if failure == "none" {
        assert!(result.is_ok());
    } else {
        let payload = result.expect_err("the first failure resumes after cleanup");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&if failure == "registry" {
                "registry retirement"
            } else {
                "run cancellation"
            })
        );
    }
    assert!(!owner.is_bound());
    assert!(controller.forward().is_err(), "the kernel stays closed");
    assert_eq!(
        deliveries.get(),
        1,
        "healthy cancellation delivery finishes"
    );
    assert!(matches!(
        Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(_))
    ));
    if failure == "cancellation" {
        assert_eq!(
            drops.get(),
            0,
            "outgoing registry retains first-failure custody"
        );
    } else {
        assert_eq!(drops.get(), 1);
        assert!(refused.get(), "retirement observes the closed kernel");
    }
    owner.dispose();
    assert_eq!(deliveries.get(), 1, "the next disposal is inert");
}

fn healthy_disposal_commits_before_registry_retirement() {
    disposal_registry_custody("none");
}

fn failing_registry_retirement_observes_the_disposed_kernel() {
    disposal_registry_custody("registry");
}

fn cancellation_failure_retains_the_outgoing_registry() {
    disposal_registry_custody("cancellation");
}

fn disposal_closes_the_kernel_before_retiring_the_registry() {
    crate::run_table(&[
        (
            "healthy retirement",
            healthy_disposal_commits_before_registry_retirement,
        ),
        (
            "registry retirement failure",
            failing_registry_retirement_observes_the_disposed_kernel,
        ),
        (
            "cancellation and retirement competition",
            cancellation_failure_retains_the_outgoing_registry,
        ),
    ]);
}

fn an_unbound_infinite_repeat_parks_then_resumes_on_a_registry() {
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let run = owner
        .controller()
        .repeat(RepeatMode::Restart)
        .expect("infinite repeat");
    assert_eq!(owner.controller().value(), 0.0);
    assert!(run.is_pending());
    let registry = Vsync::new();
    owner.rebind(Some(&registry)).expect("bind parked run");
    let mut clock = flui_animation::MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(300)));
    assert_eq!(owner.controller().value(), 0.3);
    assert!(run.is_pending());
    drop(owner);
    assert!(run.is_canceled());
}

fn unbound_finite_repeat_lands_on_its_last_leg() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let run = owner
        .controller()
        .repeat(
            flui_animation::RepeatSpec::new(RepeatMode::Alternate)
                .with_iterations(flui_animation::Iterations::Finite(4)),
        )
        .expect("finite repeat");
    assert_eq!(owner.controller().value(), 0.0);
    assert_eq!(owner.controller().status(), AnimationStatus::ReverseSettled);
    assert!(run.is_complete());
}

#[derive(Debug)]
struct NeverSettling;

impl flui_animation::Simulation for NeverSettling {
    fn x(&self, time: f64) -> f64 {
        time
    }
    fn dx(&self, _time: f64) -> f64 {
        1.0
    }
    fn is_done(&self, _time: f64) -> bool {
        false
    }
}

fn unbound_simulation_uses_the_last_finite_probe() {
    let owner = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build_on(None);
    let run = owner
        .controller()
        .animate_with(NeverSettling)
        .expect("simulation run");
    assert_eq!(owner.controller().value(), 64.0);
    assert!(run.is_complete());
}

#[test]
fn an_unbound_controller_settles_every_run_kind_at_once() {
    crate::run_table(&[
        ("finite time", unbound_time_run_completes_once),
        ("finite repeat", unbound_finite_repeat_lands_on_its_last_leg),
        ("simulation", unbound_simulation_uses_the_last_finite_probe),
        (
            "infinite repeat parks and resumes",
            an_unbound_infinite_repeat_parks_then_resumes_on_a_registry,
        ),
    ]);
}

#[test]
fn driven_controller_owns_its_seat_and_run() {
    crate::run_table(&[
        (
            "reentrant owner retirement joins registry custody",
            reentrant_owner_retirement_joins_registry_custody,
        ),
        (
            "registry closure withdraws kernels before cancellation delivery",
            closing_a_registry_withdraws_kernels_before_delivery,
        ),
        (
            "disposal closes the kernel before registry retirement",
            disposal_closes_the_kernel_before_retiring_the_registry,
        ),
        (
            "rebind preserves delivery failure before outgoing retirement",
            rebinding_preserves_delivery_failure_before_outgoing_retirement,
        ),
        (
            "rebind commits the clock before retiring the outgoing registry",
            rebinding_retires_the_old_registry_after_committing_the_new_clock,
        ),
        (
            "run restart and rate resume request samples",
            runs_and_rate_changes_request_samples,
        ),
        (
            "nested mute and rebind preserve the addressed driver",
            nested_mute_and_rebind_address_the_live_driver,
        ),
        (
            "wake failure preserves delivery and recovery",
            wake_failure_preserves_delivery_and_recovery,
        ),
        (
            "wake replacement retires captures outside borrows",
            wake_replacement_retires_captures_outside_borrows,
        ),
        (
            "last driven owner release revokes its driver after wake failure",
            last_driven_owner_release_revokes_its_driver,
        ),
        (
            "drop unregisters before cancel",
            dropping_a_driven_controller_unregisters_then_cancels_its_run,
        ),
        (
            "dispose and drop are idempotent",
            dispose_then_drop_is_one_retirement,
        ),
        (
            "migration preserves elapsed",
            rebinding_mid_run_keeps_the_value_continuous,
        ),
        (
            "last owner retires itself and a peer",
            dropping_the_last_owner_from_its_own_listener_mid_frame,
        ),
    ]);
}

fn reentrant_owner_retirement_joins_registry_custody() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct Capture(Rc<Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let mut observed = Vec::new();
    for earlier_failure in [false, true] {
        let registry = Vsync::new();
        let drops = Rc::new(Cell::new(0));
        let capture = Capture(Rc::clone(&drops));
        registry.set_frame_requester(Some(Rc::new(move || {
            let _keep = &capture;
        })));
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = owner.controller().clone();
        let run = controller.forward().unwrap();
        let storage = Rc::new(RefCell::new(Some(owner)));
        controller.add_listener(Rc::new(move || {
            assert!(!earlier_failure, "earlier driven listener failure");
        }));
        controller.add_listener({
            let storage = Rc::downgrade(&storage);
            Rc::new(move || {
                let storage = storage.upgrade().unwrap();
                let outgoing = storage.borrow_mut().take();
                drop(outgoing);
            })
        });
        drop(registry);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controller.set_value(0.25);
        }));
        assert_eq!(outcome.is_err(), earlier_failure);
        if let Err(payload) = outcome {
            assert_eq!(
                flui_foundation::panic::payload_text(&*payload),
                Some("earlier driven listener failure")
            );
        }
        assert!(storage.borrow().is_none() && run.is_canceled());
        observed.push((earlier_failure, drops.get()));
    }
    assert_eq!(
        observed,
        [(false, 1), (true, 0)],
        "registry captures retain the active controller delivery's first-failure custody"
    );
}

fn closing_a_registry_withdraws_kernels_before_delivery() {
    use flui_animation::{AnimationError, MotionClock, VsyncRegistrationError};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[derive(Clone, Copy)]
    enum Delivery {
        Finish,
        Drop,
        Incoming,
        HeldFailure,
    }

    struct HookCapture {
        registry: Vsync,
        retired: Rc<Cell<usize>>,
        failure: bool,
    }
    impl Drop for HookCapture {
        fn drop(&mut self) {
            self.registry.prepare_close().finish();
            let mut fresh = AnimationController::builder(Duration::from_secs(1)).build_on(None);
            assert_eq!(
                fresh.rebind(Some(&self.registry)),
                Err(VsyncRegistrationError::Closed)
            );
            self.retired.set(self.retired.get() + 1);
            assert!(!self.failure, "requester retirement");
        }
    }

    for delivery in [
        Delivery::Finish,
        Delivery::Drop,
        Delivery::Incoming,
        Delivery::HeldFailure,
    ] {
        for (callback_failure, capture_failure) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let root = Vsync::new();
            let child = Vsync::new();
            let independent = Vsync::new();
            root.attach_child(&child).expect("nested clock");
            independent
                .attach_child(&child)
                .expect_err("another presentation cannot own this descendant");
            let retired = Rc::new(Cell::new(0));
            let wakes = Rc::new(Cell::new(0));
            let capture = HookCapture {
                registry: root.clone(),
                retired: Rc::clone(&retired),
                failure: capture_failure,
            };
            let wake_count = Rc::clone(&wakes);
            root.set_frame_requester(Some(Rc::new(move || {
                std::hint::black_box(&capture);
                wake_count.set(wake_count.get() + 1);
            })));
            let mut first =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&root));
            let second =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
            let peer =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&independent));
            let peer_run = peer.controller().forward().expect("independent run");
            let observers = [first.controller().clone(), second.controller().clone()];
            let mut runs = [
                observers[0].forward().expect("root run"),
                observers[1].forward().expect("nested run"),
            ];
            let outcomes = Rc::new(RefCell::new(Vec::new()));
            for (index, run) in runs.iter().enumerate() {
                let observers = observers.clone();
                let root = root.clone();
                let child = child.clone();
                let outcomes = Rc::clone(&outcomes);
                run.when_complete_or_cancel(move |result| {
                    assert!(result.is_err());
                    assert!(root.is_empty() && child.is_empty());
                    for observer in &observers {
                        assert!(matches!(observer.forward(), Err(AnimationError::Disposed)));
                    }
                    root.prepare_close().finish();
                    outcomes.borrow_mut().push(index);
                    assert!(!(callback_failure && index == 0), "first cancellation");
                });
            }
            let previous_wakes = wakes.get();
            let receipt = root.prepare_close();
            assert!(outcomes.borrow().is_empty(), "withdrawal runs no callback");
            for run in &mut runs {
                assert!(
                    matches!(
                        Pin::new(run).poll(&mut Context::from_waker(Waker::noop())),
                        Poll::Ready(Err(_))
                    ),
                    "cancellation outcome commits before callback delivery"
                );
            }
            assert!(root.is_empty() && child.is_empty());
            assert!(!first.is_bound() && !second.is_bound());
            assert!(independent.has_running());
            assert_eq!(
                root.attach_child(&independent),
                Err(VsyncRegistrationError::Closed)
            );
            assert_eq!(
                independent.attach_child(&root),
                Err(VsyncRegistrationError::Closed)
            );
            let rejected_drops = Rc::new(Cell::new(0));
            let rejected = HookCapture {
                registry: root.clone(),
                retired: Rc::clone(&rejected_drops),
                failure: capture_failure,
            };
            let rejected_wakes = Rc::clone(&wakes);
            let rejection = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                root.set_frame_requester(Some(Rc::new(move || {
                    std::hint::black_box(&rejected);
                    rejected_wakes.set(rejected_wakes.get() + 1);
                })));
            }));
            assert_eq!(rejection.is_err(), capture_failure);
            if let Err(failure) = rejection {
                assert_eq!(
                    failure.downcast_ref::<&str>(),
                    Some(&"requester retirement")
                );
            }
            assert_eq!(
                rejected_drops.get(),
                1,
                "rejected hook retires outside borrows"
            );
            let mut fresh = AnimationController::builder(Duration::from_secs(1)).build_on(None);
            assert_eq!(
                fresh.rebind(Some(&root)),
                Err(VsyncRegistrationError::Closed)
            );
            assert_eq!(
                fresh.rebind(Some(&child)),
                Err(VsyncRegistrationError::Closed)
            );
            first
                .rebind(Some(&independent))
                .expect("retired owner stays retired");
            assert!(!first.is_bound());
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match delivery {
                    Delivery::Finish => receipt.finish(),
                    Delivery::Drop => drop(receipt),
                    Delivery::Incoming => {
                        let _receipt = receipt;
                        std::panic::panic_any("outer unwind");
                    }
                    Delivery::HeldFailure => {
                        let mut recovery = flui_foundation::panic::PanicRecovery::new();
                        recovery.run(|| std::panic::panic_any("held failure"));
                        receipt.publish_with(&mut recovery.scope());
                        recovery.finish();
                    }
                }));
            let expected = match delivery {
                Delivery::Incoming => Some("outer unwind"),
                Delivery::HeldFailure => Some("held failure"),
                _ if callback_failure => Some("first cancellation"),
                _ if capture_failure => Some("requester retirement"),
                _ => None,
            };
            assert_eq!(result.is_err(), expected.is_some());
            if let Err(failure) = result {
                assert_eq!(failure.downcast_ref::<&str>().copied(), expected);
            }
            assert_eq!(
                *outcomes.borrow(),
                [0, 1],
                "healthy cancellation tail remains deliverable"
            );
            let preserving =
                callback_failure || matches!(delivery, Delivery::Incoming | Delivery::HeldFailure);
            assert_eq!(retired.get(), usize::from(!preserving));
            for run in &mut runs {
                assert!(matches!(
                    Pin::new(run).poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Err(_))
                ));
            }
            root.tick_all(&MotionClock::new().frame(Duration::from_secs(2)));
            assert_eq!(wakes.get(), previous_wakes, "a closed clock cannot wake");
            assert!(peer.controller().is_animating());
            drop(peer_run);
            drop(peer);
        }
    }
}

fn counted_registry() -> (Vsync, std::rc::Rc<std::cell::Cell<usize>>) {
    let registry = Vsync::new();
    let wakes = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = wakes.clone();
    registry.set_frame_requester(Some(std::rc::Rc::new(move || {
        observed.set(observed.get() + 1);
    })));
    (registry, wakes)
}

fn wake_replacement_retires_captures_outside_borrows() {
    use std::cell::Cell;
    use std::rc::Rc;
    struct Capture {
        registry: Vsync,
        retired: Rc<Cell<usize>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert!(
                self.registry.has_running(),
                "capture can reenter the registry during retirement"
            );
            self.retired.set(self.retired.get() + 1);
        }
    }
    let registry = Vsync::new();
    let retired = Rc::new(Cell::new(0));
    let capture = Capture {
        registry: registry.clone(),
        retired: retired.clone(),
    };
    let wakes = Rc::new(Cell::new(0));
    let replacement_wakes = wakes.clone();
    registry.set_frame_requester(Some(Rc::new(move || {
        let observed = replacement_wakes.clone();
        capture
            .registry
            .set_frame_requester(Some(Rc::new(move || observed.set(observed.get() + 1))));
        let _ = &capture;
    })));
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    owner.controller().forward().unwrap();
    assert_eq!(retired.get(), 1, "the replaced hook releases its captures");
    assert_eq!(
        wakes.get(),
        1,
        "replacement delivers the accepted running demand"
    );
    owner.controller().forward().unwrap();
    assert_eq!(wakes.get(), 2);
    registry.set_frame_requester(None);
    let run = owner.controller().forward().unwrap();
    let observed = wakes.clone();
    registry.set_frame_requester(Some(Rc::new(move || observed.set(observed.get() + 1))));
    assert_eq!(
        wakes.get(),
        3,
        "missing hook keeps demand for later installation"
    );
    let mut clock = flui_animation::MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(run.is_complete());
}

fn runs_and_rate_changes_request_samples() {
    use flui_animation::{MotionClock, PlaybackRate};
    let (registry, wakes) = counted_registry();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    assert_eq!(wakes.get(), 0, "an idle registration does not wake");
    owner.controller().forward().unwrap();
    let run = owner.controller().forward().unwrap();
    owner.controller().set_playback_rate(PlaybackRate::NORMAL);
    assert_eq!(
        wakes.get(),
        2,
        "a same-status restart requests its frame; an unchanged rate adds no demand"
    );
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(500)));
    owner.controller().set_playback_rate(PlaybackRate::PAUSED);
    assert_eq!(wakes.get(), 3);
    registry.tick_all(&clock.frame(Duration::from_millis(600)));
    assert!(!registry.has_running(), "pause releases continuous demand");
    registry.tick_all(&clock.frame(Duration::from_secs(10)));
    owner.controller().set_playback_rate(PlaybackRate::NORMAL);
    assert_eq!(
        wakes.get(),
        4,
        "resume requests the rate application sample"
    );
    registry.tick_all(&clock.frame(Duration::from_secs(11)));
    assert!(
        (owner.controller().value() - 0.6).abs() < 1e-9,
        "pause time is excluded"
    );
    registry.tick_all(&clock.frame(Duration::from_millis(11_400)));
    assert!(run.is_complete());
    assert_eq!(owner.controller().value(), 1.0);
    assert_eq!(
        wakes.get(),
        4,
        "sampling does not manufacture new wake requests"
    );
}

fn nested_mute_and_rebind_address_the_live_driver() {
    let (a, a_wakes) = counted_registry();
    let (b, b_wakes) = counted_registry();
    let child = Vsync::new();
    let seat = a.attach_child(&child).expect("child admitted");
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    a.set_muted(true);
    owner.controller().forward().unwrap();
    assert_eq!(
        a_wakes.get(),
        0,
        "an unmuted descendant cannot wake a muted ancestor"
    );
    a.set_muted(false);
    assert_eq!(a_wakes.get(), 1, "unmute requests the retained run");
    child.set_muted(true);
    owner.controller().forward().unwrap();
    assert_eq!(a_wakes.get(), 1);
    a.detach_child(&seat);
    child.set_muted(false);
    assert_eq!(
        a_wakes.get(),
        1,
        "a detached child cannot wake its old parent"
    );
    owner.rebind(Some(&b)).unwrap();
    assert_eq!(b_wakes.get(), 1, "a live run requests its new driver");
    owner.controller().forward().unwrap();
    assert_eq!((a_wakes.get(), b_wakes.get()), (1, 2));
    let observer = owner.controller().clone();
    drop(owner);
    observer.set_playback_rate(flui_animation::PlaybackRate::NORMAL);
    assert_eq!(
        (a_wakes.get(), b_wakes.get()),
        (1, 2),
        "retired observer clones cannot wake either driver"
    );
}

fn last_driven_owner_release_revokes_its_driver() {
    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    for fail in [false, true] {
        let registry = Vsync::new();
        let driven = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = driven.controller().clone();
        let mut run = controller.forward().expect("run before wake installation");
        let outcomes = Rc::new(Cell::new(0));
        let delivered = outcomes.clone();
        run.when_complete_or_cancel(move |result| {
            assert!(result.is_err(), "owner release cancels the admitted run");
            delivered.set(delivered.get() + 1);
        });
        let owner = Rc::new(RefCell::new(Some(driven)));
        let released = owner.clone();
        let armed = Rc::new(Cell::new(false));
        let callback_armed = armed.clone();
        let wakes = Rc::new(Cell::new(0));
        let observed = wakes.clone();
        registry.set_frame_requester(Some(Rc::new(move || {
            if !callback_armed.get() {
                return;
            }
            observed.set(observed.get() + 1);
            let outgoing = released.borrow_mut().take();
            drop(outgoing);
            assert!(!fail, "wake failure after owner release");
        })));
        drop(registry);
        armed.set(true);

        let first = catch_unwind(AssertUnwindSafe(|| {
            controller.set_playback_rate(
                flui_animation::PlaybackRate::new(2.0).expect("valid playback rate"),
            );
        }));
        assert_eq!(first.is_err(), fail);
        if let Err(payload) = first {
            assert_eq!(
                flui_foundation::panic::payload_text(&*payload),
                Some("wake failure after owner release")
            );
        }
        assert!(owner.borrow().is_none());
        assert_eq!(wakes.get(), 1);
        assert_eq!(outcomes.get(), 1, "cancellation is delivered once");
        assert!(matches!(
            Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ));
        assert!(matches!(
            controller.forward(),
            Err(flui_animation::AnimationError::Disposed)
        ));
        assert_eq!(
            wakes.get(),
            1,
            "temporary storage custody cannot revive a driver"
        );

        let (replacement, replacement_wakes) = counted_registry();
        let replacement_owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&replacement));
        let mut replacement_run = replacement_owner
            .controller()
            .forward()
            .expect("replacement owner starts");
        assert_eq!(replacement_wakes.get(), 1);
        let mut clock = flui_animation::MotionClock::new();
        replacement.tick_all(&clock.frame(Duration::ZERO));
        replacement.tick_all(&clock.frame(Duration::from_secs(1)));
        assert!(matches!(
            Pin::new(&mut replacement_run).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(()))
        ));
    }
}

fn wake_failure_preserves_delivery_and_recovery() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    for competing in [false, true] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let inspected = owner.controller().clone();
        registry.set_frame_requester(Some(Rc::new(move || {
            assert!(inspected.is_animating(), "run admission precedes wake");
            assert_eq!(inspected.status(), AnimationStatus::Forward);
            panic!("wake failure");
        })));
        owner
            .controller()
            .subscribe_status(Rc::new(move |status| {
                assert!(
                    !(competing && status == AnimationStatus::Forward),
                    "listener failure"
                );
            }))
            .detach();
        let delivered = Rc::new(Cell::new(0));
        let observed = delivered.clone();
        owner
            .controller()
            .subscribe_status(Rc::new(move |_| observed.set(observed.get() + 1)))
            .detach();
        let failure = catch_unwind(AssertUnwindSafe(|| owner.controller().forward()));
        let payload = failure.expect_err("wake failure propagates after delivery");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"wake failure"));
        assert_eq!(
            delivered.get(),
            1,
            "healthy status tail is delivered despite both failures"
        );
        assert!(
            owner.controller().is_animating(),
            "accepted run survives wake failure"
        );
        let recovered = Rc::new(Cell::new(0));
        let observed = recovered.clone();
        registry.set_frame_requester(Some(Rc::new(move || observed.set(observed.get() + 1))));
        let run = owner.controller().forward().unwrap();
        assert_eq!(
            recovered.get(),
            2,
            "replacement and same-status restart remain deliverable"
        );
        let mut clock = flui_animation::MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_secs(1)));
        assert!(run.is_complete());
    }
}

/// Counts how many times the value it guards is dropped.
struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn drop_probe() -> (Arc<AtomicUsize>, DropProbe) {
    let drops = Arc::new(AtomicUsize::new(0));
    (Arc::clone(&drops), DropProbe(drops))
}

// --- controller ---------------------------------------------------------------------

#[test]
fn dispose_releases_value_listeners() {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = lifecycle.controller().clone();
    let probe = Arc::new(());
    let capture = Arc::clone(&probe);
    let _id = controller.add_listener(std::rc::Rc::new(move || {
        let _ = &capture;
    }));

    lifecycle.dispose();

    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "a disposed controller does not retain its value listeners"
    );
}

#[test]
fn constant_listener_tokens_never_withdraw_live_callbacks() {
    use flui_animation::ConstantAnimation;
    use flui_foundation::ChangeNotifier;
    use std::cell::Cell;
    use std::rc::Rc;

    let notifier = ChangeNotifier::new();
    let calls = Rc::new(Cell::new(0));
    notifier.add_listener({
        let calls = Rc::clone(&calls);
        Rc::new(move || calls.set(calls.get() + 1))
    });
    let constant = ConstantAnimation::new(0.5);
    let capture = Rc::new(());
    let token = constant.add_listener({
        let capture = Rc::clone(&capture);
        Rc::new(move || {
            let _ = &capture;
            panic!("a constant cannot notify");
        })
    });
    assert_eq!(
        Rc::strong_count(&capture),
        1,
        "an inert callback retires at admission"
    );
    notifier.remove_listener(token);
    notifier.notify_listeners();
    assert_eq!(
        calls.get(),
        1,
        "an unregistered constant token cannot remove live work"
    );
    constant.remove_listener(token);
    notifier.notify_listeners();
    assert_eq!(calls.get(), 2);
}

#[test]
fn last_handle_drop_releases_a_running_controller() {
    let (drops, probe) = drop_probe();
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    controller
        .subscribe_status(std::rc::Rc::new(move |_| {
            let _ = &probe;
        }))
        .detach();
    let mut run = controller.forward().expect("run starts");

    drop(controller);

    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the run in flight does not keep its controller alive"
    );
    assert!(
        matches!(
            Pin::new(&mut run).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ),
        "the run of a released controller is canceled"
    );
}

// --- wrappers -------------------------------------------------------------------------

type AnimationHandle = std::rc::Rc<dyn Animation<f64>>;

fn wrapper_over_a_disposed_source(wrap: fn(AnimationHandle) -> AnimationHandle) {
    let mut lifecycle =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let parent = lifecycle.controller().clone();
    parent.set_value(0.5);
    lifecycle.dispose();
    let (drops, value_probe) = drop_probe();
    let status_probe = DropProbe(drops.clone());
    let wrapper = wrap(std::rc::Rc::new(parent.clone()));
    wrapper.add_listener(std::rc::Rc::new(move || {
        let _ = &value_probe;
        panic!("a disposed source cannot notify a new wrapper");
    }));
    wrapper
        .subscribe_status(std::rc::Rc::new(move |_| {
            let _ = &status_probe;
            panic!("a disposed source cannot send a new status");
        }))
        .detach();
    parent.set_value(1.0);
    drop(wrapper);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        2,
        "an inert wrapper releases both listener captures on owner teardown"
    );
}

fn reverse_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| std::rc::Rc::new(ReverseAnimation::new(parent)));
}
fn curved_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| {
        std::rc::Rc::new(CurvedAnimation::new(parent, curves::LINEAR))
    });
}
fn tween_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| {
        std::rc::Rc::new(TweenAnimation::new(Tween::new(0.0, 10.0), parent))
    });
}
fn proxy_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| std::rc::Rc::new(ProxyAnimation::new(parent)));
}
fn switch_over_disposed_source() {
    wrapper_over_a_disposed_source(|parent| std::rc::Rc::new(AnimationSwitch::new(parent, None)));
}

#[test]
fn wrappers_over_disposed_sources_remain_inert() {
    crate::run_table(&[
        ("reverse", reverse_over_disposed_source),
        ("curved", curved_over_disposed_source),
        ("tween", tween_over_disposed_source),
        ("proxy", proxy_over_disposed_source),
        ("switch", switch_over_disposed_source),
    ]);
}

/// Adds a status listener through `wrapper`, drops the wrapper while its parent
/// stays alive, and asserts the listener's capture was released.
fn wrapper_status_listener_dies_with_the_wrapper<W>(wrap: fn(std::rc::Rc<dyn Animation<f64>>) -> W)
where
    W: Animation<f64>,
{
    let parent = controller();
    let probe = Arc::new(());
    {
        let wrapper = wrap(std::rc::Rc::new(parent.clone()));
        let capture = Arc::clone(&probe);
        wrapper
            .subscribe_status(std::rc::Rc::new(move |_| {
                let _ = &capture;
            }))
            .detach();
    }
    assert_eq!(
        Arc::strong_count(&probe),
        1,
        "the parent does not retain a status listener added through a dropped wrapper"
    );
    drop(parent);
}

#[test]
fn reverse_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(ReverseAnimation::new);
}

#[test]
fn curved_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(|parent| {
        CurvedAnimation::new(parent, curves::EASE_IN)
    });
}

#[test]
fn tween_status_listener_dies_with_the_wrapper() {
    wrapper_status_listener_dies_with_the_wrapper(|parent| {
        TweenAnimation::new(Tween::new(0.0, 10.0), parent)
    });
}

// --- switch ---------------------------------------------------------------------------

#[test]
fn switch_dispose_releases_callbacks() {
    let (current, next) = (controller(), controller());
    current.set_value(0.8);
    next.set_value(0.3);
    let (switched, listened) = (Arc::new(()), Arc::new(()));
    let switched_capture = Arc::clone(&switched);
    let switch = AnimationSwitch::new(
        std::rc::Rc::new(current.clone()),
        Some(std::rc::Rc::new(next.clone())),
    )
    .on_switched(move || {
        let _ = &switched_capture;
    });
    let listened_capture = Arc::clone(&listened);
    switch
        .subscribe_status(std::rc::Rc::new(move |_| {
            let _ = &listened_capture;
        }))
        .detach();

    switch.dispose();

    assert_eq!(
        (Arc::strong_count(&switched), Arc::strong_count(&listened)),
        (1, 1),
        "a disposed switch retains neither on_switched nor its status listeners"
    );
    drop(current);
    drop(next);
}

/// The transition-route shape: a proxy parented to a switch whose
/// `on_switched` re-parents that same proxy.
#[test]
fn proxy_parented_to_a_capturing_switch_is_freed() {
    let (current, next) = (controller(), controller());
    current.set_value(0.8);
    next.set_value(0.3);
    let (drops, probe) = drop_probe();
    {
        let proxy = std::rc::Rc::new(ProxyAnimation::new(
            std::rc::Rc::new(current.clone()) as std::rc::Rc<dyn Animation<f64>>
        ));
        let hop_proxy = std::rc::Rc::clone(&proxy);
        let target: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(next.clone());
        let switch = AnimationSwitch::new(
            std::rc::Rc::new(current.clone()),
            Some(std::rc::Rc::clone(&target)),
        )
        .on_switched(move || {
            let _ = &probe;
            hop_proxy.set_parent(std::rc::Rc::clone(&target));
        });
        proxy.set_parent(std::rc::Rc::new(switch.clone()));
        switch.dispose();
    }
    drop(current);
    drop(next);

    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "the proxy, the switch and on_switched are released"
    );
}

/// A fixed-parent wrapper's last owner retires its value listeners before its
/// status observers: with both captures failing, the value capture's failure is
/// the one that propagates.
#[test]
fn fixed_parent_wrappers_retire_value_listeners_before_status_observers() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct ValueBomb;
    impl Drop for ValueBomb {
        fn drop(&mut self) {
            panic!("value capture failure");
        }
    }
    struct StatusBomb;
    impl Drop for StatusBomb {
        fn drop(&mut self) {
            panic!("status capture failure");
        }
    }

    type Wrap = fn(Rc<dyn Animation<f64>>) -> Rc<dyn Animation<f64>>;
    let wrappers: [(&str, Wrap); 3] = [
        ("curved", |parent| {
            Rc::new(CurvedAnimation::new(parent, curves::LINEAR))
        }),
        ("tween", |parent| {
            Rc::new(TweenAnimation::new(Tween::new(0.0, 1.0), parent))
        }),
        ("reverse", |parent| Rc::new(ReverseAnimation::new(parent))),
    ];
    for (name, wrap) in wrappers {
        let parent = controller();
        let wrapper = wrap(Rc::new(parent.clone()));
        let value = ValueBomb;
        wrapper.add_listener(Rc::new(move || {
            let _keep = &value;
        }));
        let status = StatusBomb;
        wrapper
            .subscribe_status(Rc::new(move |_| {
                let _keep = &status;
            }))
            .detach();
        let failure = catch_unwind(AssertUnwindSafe(|| drop(wrapper)))
            .expect_err("a failing capture propagates from the last owner");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"value capture failure"),
            "{name}"
        );
    }
}

/// Every combinator's value channel keeps custody of a listener removed during
/// a delivery, whichever comes first: the failure or the removal. A listener
/// added and removed during the round is outside its snapshot, so only the
/// channel's deferred retirement keeps its destructor off the failed path.
#[test]
fn listener_removal_during_a_failing_delivery_retains_the_capture() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    for ((name, wrap), panic_first) in combinators()
        .into_iter()
        .flat_map(|wrapper| [(wrapper, true), (wrapper, false)])
    {
        let name = format!(
            "{name}, panic {}",
            if panic_first { "first" } else { "last" }
        );
        let parent = controller();
        let wrapper = wrap(Rc::new(parent.clone()));
        let (drops, capture) = drop_probe();
        let capture = std::cell::Cell::new(Some(capture));
        let panicking = || -> flui_foundation::ListenerCallback {
            Rc::new(|| panic!("first listener failure"))
        };
        if panic_first {
            wrapper.add_listener(panicking());
        }
        let weak = Rc::downgrade(&wrapper);
        wrapper.add_listener(Rc::new(move || {
            let (Some(wrapper), Some(capture)) = (weak.upgrade(), capture.take()) else {
                return;
            };
            let id = wrapper.add_listener(Rc::new(move || {
                let _keep = &capture;
            }));
            wrapper.remove_listener(id);
        }));
        if !panic_first {
            wrapper.add_listener(panicking());
        }
        let failure = catch_unwind(AssertUnwindSafe(|| parent.set_value(0.25)))
            .expect_err("the first listener's failure propagates");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"first listener failure"),
            "{name}"
        );
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "{name}: the removed capture is retained on the failed path"
        );
    }
}

type Wrap = fn(std::rc::Rc<dyn Animation<f64>>) -> std::rc::Rc<dyn Animation<f64>>;

/// Every combinator that owns its own channels, over one parent.
fn combinators() -> [(&'static str, Wrap); 5] {
    use std::rc::Rc;
    [
        ("curved", |parent| {
            Rc::new(CurvedAnimation::new(parent, curves::LINEAR))
        }),
        ("tween", |parent| {
            Rc::new(TweenAnimation::new(Tween::new(0.0, 1.0), parent))
        }),
        ("reverse", |parent| Rc::new(ReverseAnimation::new(parent))),
        ("proxy", |parent| Rc::new(ProxyAnimation::new(parent))),
        ("switch", |parent| {
            Rc::new(AnimationSwitch::new(parent, None))
        }),
    ]
}

/// A value listener removed while its combinator delivers a status waits for
/// that round: a later status observer's failure retains the capture, and a
/// healthy round destroys it when the round ends. Outside any round, removal
/// destroys it at once.
#[test]
fn value_listener_removed_during_a_status_round_waits_for_the_round() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    for (name, wrap) in combinators() {
        let parent = controller();
        let wrapper = wrap(Rc::new(parent.clone()));
        let (drops, capture) = drop_probe();
        let id = wrapper.add_listener(Rc::new(move || {
            let _keep = &capture;
        }));
        wrapper.remove_listener(id);
        assert_eq!(drops.load(Ordering::SeqCst), 1, "{name}: no round, at once");

        for fails in [true, false] {
            let parent = controller();
            let wrapper = wrap(Rc::new(parent.clone()));
            let (drops, capture) = drop_probe();
            let id = wrapper.add_listener(Rc::new(move || {
                let _keep = &capture;
            }));
            let weak = Rc::downgrade(&wrapper);
            let _remove = wrapper.subscribe_status(Rc::new(move |_| {
                if let Some(wrapper) = weak.upgrade() {
                    wrapper.remove_listener(id);
                }
            }));
            let _fail = wrapper.subscribe_status(Rc::new(move |_| {
                assert!(!fails, "later status failure");
            }));
            let run = catch_unwind(AssertUnwindSafe(|| parent.forward()));
            assert_eq!(run.is_err(), fails, "{name}");
            let expected = usize::from(!fails);
            assert_eq!(
                drops.load(Ordering::SeqCst),
                expected,
                "{name}, failing round {fails}: the removed capture waits for the round"
            );
        }
    }
}

#[test]
fn proxy_listener_removal_joins_its_enclosing_cleanup() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    struct Capture {
        proxy: Rc<ProxyAnimation<f64>>,
        drops: Rc<Cell<usize>>,
        reentrant_calls: Rc<Cell<usize>>,
        panics: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
            let calls = Rc::clone(&self.reentrant_calls);
            self.proxy
                .add_listener(Rc::new(move || calls.set(calls.get() + 1)));
            assert!(!self.panics, "outgoing proxy capture failure");
        }
    }

    for case in ["healthy", "earlier_failure", "competing"] {
        let parent = controller();
        let proxy = Rc::new(ProxyAnimation::new(Rc::new(parent.clone())));
        let drops = Rc::new(Cell::new(0));
        let calls = Rc::new(Cell::new(0));
        let reentrant_calls = Rc::new(Cell::new(0));
        let capture = Capture {
            proxy: Rc::clone(&proxy),
            drops: Rc::clone(&drops),
            reentrant_calls: Rc::clone(&reentrant_calls),
            panics: case == "competing",
        };
        let observed = Rc::clone(&calls);
        let id = proxy.add_listener(Rc::new(move || {
            let _capture = &capture;
            observed.set(observed.get() + 1);
        }));
        parent.set_value(0.25);
        assert_eq!(calls.get(), 1);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut recovery = flui_foundation::panic::PanicRecovery::new();
            if case != "healthy" {
                recovery.run(|| panic!("first cleanup failure"));
            }
            proxy.remove_listener_with_recovery(id, &mut recovery.scope());
            recovery.finish();
        }));
        if case == "healthy" {
            outcome.expect("healthy cleanup supports capture reentry");
        } else {
            let failure = outcome.expect_err("the enclosing failure remains authoritative");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&"first cleanup failure")
            );
        }
        assert_eq!(drops.get(), usize::from(case == "healthy"));
        parent.set_value(0.5);
        assert_eq!(
            calls.get(),
            1,
            "withdrawal precedes outgoing capture retirement"
        );
        assert_eq!(reentrant_calls.get(), usize::from(case == "healthy"));

        let (fresh_drops, fresh_capture) = drop_probe();
        let fresh_id = proxy.add_listener(Rc::new(move || {
            let _capture = &fresh_capture;
        }));
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        proxy.remove_listener_with_recovery(fresh_id, &mut recovery.scope());
        recovery.finish();
        assert_eq!(
            fresh_drops.load(Ordering::SeqCst),
            1,
            "fresh cleanup retires normally"
        );
    }
}
