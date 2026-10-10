//! The owning value's public frame, delivery and observer lifetime contracts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    AnimatedValue, Animation, AnimationError, AnimationStatus, ArcCurve, Curves, MotionClock,
    MotionSpec, MotionUpdate, SpringDescription, TwoWayConverter, Vsync, VsyncUpdate,
};
use flui_foundation::geometry::Offset;

fn curve(curve: impl flui_animation::Curve + Send + Sync + 'static) -> MotionSpec {
    MotionSpec::Curve {
        duration: Duration::from_secs(1),
        curve: ArcCurve::new(curve),
    }
}

fn replacement_publishes_all_components_before_cancellation() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut value =
        AnimatedValue::new(Offset::ZERO, curve(Curves::Linear), Some(&registry)).unwrap();
    let view = value.animation();
    let old = value.animate_to(Offset::new(2.0, -4.0)).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let seam = (view.value(), view.velocity());
    assert!(
        seam.0.dx > 0.0 && seam.0.dy < 0.0 && seam.1[0] > 0.0 && seam.1[1] < 0.0,
        "both rendered components must be moving before the interruption"
    );
    let observed = Rc::new(Cell::new(None));
    old.when_complete_or_cancel({
        let observed = Rc::clone(&observed);
        let view = view.clone();
        move |result| {
            assert!(result.is_err());
            observed.set(Some((view.value(), view.velocity(), view.status())));
        }
    });
    let next = value
        .retarget(Offset::new(-3.0, 7.0), curve(Curves::EaseIn))
        .unwrap();
    assert!(old.is_canceled() && next.is_pending());
    assert_eq!(
        observed.get(),
        Some((seam.0, seam.1, AnimationStatus::Forward))
    );
    registry.tick_all(&clock.frame(Duration::from_micros(250_100)));
    let after = view.value();
    for (difference, velocity) in [
        (after.dx - seam.0.dx, seam.1[0]),
        (after.dy - seam.0.dy, seam.1[1]),
    ] {
        assert!(
            (difference / 1e-4 - velocity).abs() < 0.01,
            "the first registered frame must preserve velocity"
        );
    }
}

fn repeated_configuration_keeps_the_remaining_deadline() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut value = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let first = value.animate_to(1.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let repeated = value.animate_to(1.0).unwrap();
    assert!(first.is_pending() && repeated.is_pending());
    let seam = (value.value(), value.velocity());
    let second = value.set_motion(curve(Curves::EaseIn)).unwrap();
    assert_eq!((value.value(), value.velocity()), seam);
    assert!(first.is_canceled() && repeated.is_canceled());
    registry.tick_all(&clock.frame(Duration::from_millis(500)));
    let third = value.set_motion(curve(Curves::EaseOut)).unwrap();
    assert!(second.is_canceled());
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(third.is_complete());
    assert_eq!(value.value(), 1.0);
    assert_eq!(value.velocity(), [0.0]);
}

fn clockless_motion_and_snap_publish_the_exact_target() {
    let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 0.5);
    for motion in [curve(Curves::EaseIn), MotionSpec::Spring(spring)] {
        let mut value = AnimatedValue::new(Offset::ZERO, motion, None).unwrap();
        let run = value.animate_to(Offset::new(3.0, -7.0)).unwrap();
        assert!(run.is_complete());
        assert_eq!(value.value(), Offset::new(3.0, -7.0));
        assert_eq!(value.velocity(), [0.0, 0.0]);
        value.snap_to(Offset::new(-2.0, 9.0)).unwrap();
        assert_eq!(value.value(), Offset::new(-2.0, 9.0));
    }
}

fn geometry_reaches_rest_before_normalized_components() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let motion = MotionSpec::Spring(SpringDescription::with_damping_ratio(1.0, 100.0, 1.0));
    let mut geometry = AnimatedValue::new(Offset::ZERO, motion.clone(), Some(&registry)).unwrap();
    let mut normalized = AnimatedValue::new(0.0, motion, Some(&registry)).unwrap();
    let geometry_run = geometry.animate_to(Offset::new(1.0, -1.0)).unwrap();
    let normalized_run = normalized.animate_to(1.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    for millis in (10..=5000).step_by(10) {
        registry.tick_all(&clock.frame(Duration::from_millis(millis)));
        if geometry_run.is_complete() {
            assert!(
                normalized_run.is_pending(),
                "logical-pixel geometry must use its own rest threshold"
            );
            assert_eq!(geometry.value(), Offset::new(1.0, -1.0));
            assert_eq!(geometry.velocity(), [0.0, 0.0]);
            registry.tick_all(&clock.frame(Duration::from_secs(5)));
            assert!(normalized_run.is_complete());
            assert_eq!((normalized.value(), normalized.velocity()), (1.0, [0.0]));
            return;
        }
    }
    panic!("the geometry spring must complete in the observed interval");
}

#[derive(Clone)]
struct InvalidThreshold<const CASE: usize>(f64);

impl<const CASE: usize> TwoWayConverter for InvalidThreshold<CASE> {
    type Vector = [f64; 1];
    fn to_vector(&self) -> Self::Vector {
        [self.0]
    }
    fn from_vector([value]: Self::Vector) -> Self {
        Self(value)
    }
    fn rest_thresholds() -> Self::Vector {
        [[0.0, -1.0, f64::NAN, f64::INFINITY][CASE]]
    }
}

fn refused_threshold<const CASE: usize>() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut owner = AnimatedValue::new(
        InvalidThreshold::<CASE>(0.0),
        curve(Curves::Linear),
        Some(&registry),
    )
    .unwrap();
    let old = owner.animate_to(InvalidThreshold(1.0)).unwrap();
    let mut healthy = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let healthy_old = healthy.animate_to(1.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let seam = (owner.value().0, owner.velocity());
    let healthy_seam = (healthy.value(), healthy.velocity());
    let spring = MotionSpec::Spring(SpringDescription::with_damping_ratio(1.0, 100.0, 1.0));
    assert!(matches!(
        owner.retarget(InvalidThreshold(2.0), spring),
        Err(AnimationError::InvalidSpring(_))
    ));
    assert!(old.is_pending(), "refusal cannot cancel accepted motion");
    assert_eq!((owner.value().0, owner.velocity()), seam);
    let refused = MotionUpdate::run(|update| {
        update.retarget(&mut healthy, 3.0, curve(Curves::Linear))?;
        update.retarget(
            &mut owner,
            InvalidThreshold(2.0),
            MotionSpec::Spring(SpringDescription::with_damping_ratio(1.0, 100.0, 1.0)),
        )
    });
    assert!(matches!(refused, Err(AnimationError::InvalidSpring(_))));
    assert!(old.is_pending() && healthy_old.is_pending());
    assert_eq!((healthy.value(), healthy.velocity()), healthy_seam);
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(old.is_complete(), "the previous motion remains deliverable");
    assert_eq!(owner.value().0, 1.0);
    assert!(healthy_old.is_complete());
    assert_eq!(healthy.value(), 1.0);
}

fn invalid_rest_thresholds_preserve_the_accepted_run() {
    for case in [
        refused_threshold::<0> as fn(),
        refused_threshold::<1>,
        refused_threshold::<2>,
        refused_threshold::<3>,
    ] {
        case();
    }
}

fn observers_do_not_prolong_the_run_or_its_seat() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut value = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let view = value.animation();
    let run = value.animate_to(1.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let frozen = view.value();
    assert!(view.velocity()[0] > 0.0);
    drop(value);
    assert!(registry.is_empty() && run.is_canceled());
    registry.tick_all(&clock.frame(Duration::from_secs(10)));
    assert_eq!(view.value(), frozen);
    assert_eq!(view.velocity(), [0.0]);
    assert!(!view.is_animating());
}

fn an_equivalent_motion_target_still_publishes_its_exact_value() {
    use flui_foundation::Listenable;
    use flui_painting::styling::Color;
    let red = Color::rgba(255, 0, 0, 0);
    let blue = Color::rgba(0, 0, 255, 0);
    assert_eq!(red.to_vector(), blue.to_vector());
    let registry = Vsync::new();
    let mut value = AnimatedValue::new(red, curve(Curves::Linear), Some(&registry)).unwrap();
    let view = value.animation();
    let notified = Rc::new(Cell::new(None));
    let _listener = view.add_listener({
        let view = view.clone();
        let notified = Rc::clone(&notified);
        Rc::new(move || notified.set(Some(view.value())))
    });
    let run = value.animate_to(blue).unwrap();
    assert!(run.is_complete());
    assert_eq!(value.value(), blue);
    assert_eq!(
        notified.get(),
        Some(blue),
        "the observer must see the new exact target even when its motion vector is unchanged"
    );
}

struct ReentrantValue {
    position: f64,
    clone_hook: Rc<dyn Fn()>,
    vector_hook: Rc<dyn Fn()>,
}

impl Clone for ReentrantValue {
    fn clone(&self) -> Self {
        (self.clone_hook)();
        Self {
            position: self.position,
            clone_hook: Rc::clone(&self.clone_hook),
            vector_hook: Rc::clone(&self.vector_hook),
        }
    }
}

impl TwoWayConverter for ReentrantValue {
    type Vector = [f64; 1];
    fn rest_thresholds() -> Self::Vector {
        [0.001]
    }
    fn to_vector(&self) -> Self::Vector {
        (self.vector_hook)();
        [self.position]
    }
    fn from_vector([position]: Self::Vector) -> Self {
        plain(position)
    }
}

fn plain(position: f64) -> ReentrantValue {
    ReentrantValue {
        position,
        clone_hook: Rc::new(|| {}),
        vector_hook: Rc::new(|| {}),
    }
}

fn converters_can_reenter_the_registry_and_replace_a_visible_target() {
    let registry = Vsync::new();
    let clock = Rc::new(RefCell::new(MotionClock::new()));
    let mut value = AnimatedValue::new(plain(0.0), curve(Curves::Linear), Some(&registry)).unwrap();
    let old = value.animate_to(plain(1.0)).unwrap();
    registry.tick_all(&clock.borrow_mut().frame(Duration::ZERO));
    registry.tick_all(&clock.borrow_mut().frame(Duration::from_millis(250)));
    let calls = Rc::new(Cell::new(0));
    let target = ReentrantValue {
        position: 2.0,
        clone_hook: Rc::new(|| {}),
        vector_hook: {
            let clock = Rc::clone(&clock);
            let calls = Rc::clone(&calls);
            Rc::new(move || {
                let count = calls.get();
                calls.set(count + 1);
                if count == 0 {
                    registry.tick_all(&clock.borrow_mut().frame(Duration::from_millis(300)));
                }
            })
        },
    };
    let next = value.animate_to(target).unwrap();
    assert_eq!(calls.get(), 2, "preparation must retry the changed seam");
    assert!(old.is_canceled() && next.is_pending());
    assert!((value.value().position - 0.216).abs() < 1e-12);
    assert!((value.velocity()[0] - 1.26).abs() < 1e-12);
    drop(value);

    let owner = Rc::new(RefCell::new(
        AnimatedValue::new(plain(0.0), curve(Curves::Linear), None).unwrap(),
    ));
    let view = owner.borrow().animation();
    let once = Rc::new(Cell::new(false));
    let replacement = ReentrantValue {
        position: 0.0,
        vector_hook: Rc::new(|| {}),
        clone_hook: {
            let owner = Rc::downgrade(&owner);
            Rc::new(move || {
                if !once.replace(true) {
                    owner
                        .upgrade()
                        .unwrap()
                        .borrow_mut()
                        .snap_to(plain(1.0))
                        .unwrap();
                }
            })
        },
    };
    let _run = owner.borrow_mut().animate_to(replacement).unwrap();
    assert_eq!(
        view.value().position,
        0.0,
        "the read owns its captured target through reentry"
    );
    assert_eq!(
        view.value().position,
        1.0,
        "replacement committed without a target borrow held across Clone"
    );
}

struct FailingComponent {
    fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

thread_local! {
    static RELEASE_OWNER: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

struct ClosingCurve {
    close: std::sync::Arc<std::sync::atomic::AtomicBool>,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl flui_animation::Curve for ClosingCurve {
    fn transform(&self, t: f64) -> f64 {
        use std::sync::atomic::Ordering;
        if self.close.swap(false, Ordering::Relaxed) {
            let release = RELEASE_OWNER.with(|hook| hook.borrow_mut().take()).unwrap();
            release();
            self.closed.store(true, Ordering::Relaxed);
        }
        t
    }

    fn slope(&self, _t: f64) -> f64 {
        assert!(
            !self.closed.load(std::sync::atomic::Ordering::Relaxed),
            "a curve callback ran after owner release"
        );
        1.0
    }
}

fn releasing_the_owner_stops_the_remaining_component_callouts() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let close = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicBool::new(false));
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let motion = MotionSpec::Curve {
        duration: Duration::from_secs(1),
        curve: ArcCurve::new(ClosingCurve {
            close: Arc::clone(&close),
            closed: Arc::clone(&closed),
        }),
    };
    let owner = Rc::new(RefCell::new(Some(
        AnimatedValue::new(Offset::ZERO, motion, Some(&registry)).unwrap(),
    )));
    let (view, run) = {
        let mut owner = owner.borrow_mut();
        let value = owner.as_mut().unwrap();
        (
            value.animation(),
            value.animate_to(Offset::new(2.0, -4.0)).unwrap(),
        )
    };
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let frozen = view.value();
    RELEASE_OWNER.with(|hook| {
        let owner = Rc::downgrade(&owner);
        *hook.borrow_mut() = Some(Box::new(move || {
            let outgoing = owner.upgrade().unwrap().borrow_mut().take();
            drop(outgoing);
        }));
    });
    close.store(true, Ordering::Relaxed);
    registry.tick_all(&clock.frame(Duration::from_millis(350)));
    assert!(closed.load(Ordering::Relaxed));
    assert!(owner.borrow().is_none() && registry.is_empty());
    assert!(run.is_canceled());
    assert_eq!(view.value(), frozen);
    assert_eq!(view.velocity(), [0.0, 0.0]);
}

impl flui_animation::Curve for FailingComponent {
    fn transform(&self, t: f64) -> f64 {
        use std::sync::atomic::Ordering;
        if t > 0.0
            && self.fail.load(Ordering::Relaxed)
            && self.calls.fetch_add(1, Ordering::Relaxed) % 2 == 1
        {
            panic!("second component sample failed");
        }
        t
    }
    fn slope(&self, _t: f64) -> f64 {
        1.0
    }
}

fn a_failed_component_keeps_the_whole_published_sample() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let fail = Arc::new(AtomicBool::new(false));
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let motion = MotionSpec::Curve {
        duration: Duration::from_secs(1),
        curve: ArcCurve::new(FailingComponent {
            fail: Arc::clone(&fail),
            calls: Arc::new(AtomicUsize::new(0)),
        }),
    };
    let mut value = AnimatedValue::new(Offset::ZERO, motion, Some(&registry)).unwrap();
    let old = value.animate_to(Offset::new(2.0, -4.0)).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let published = (value.value(), value.velocity());
    fail.store(true, Ordering::Relaxed);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.tick_all(&clock.frame(Duration::from_millis(350)));
        }))
        .is_err()
    );
    assert_eq!(
        (value.value(), value.velocity()),
        published,
        "a successful first component must not leak out of a failed vector sample"
    );
    assert!(old.is_pending());
    fail.store(false, Ordering::Relaxed);
    let next = value
        .retarget(Offset::new(-3.0, 7.0), curve(Curves::Linear))
        .unwrap();
    assert!(old.is_canceled() && next.is_pending());
    assert_eq!((value.value(), value.velocity()), published);
    registry.tick_all(&clock.frame(Duration::from_millis(400)));
    assert_ne!(value.value(), published.0);
    assert!(value.value().dx.is_finite() && value.value().dy.is_finite());
}

#[test]
fn owning_animated_value_contract() {
    crate::run_table(&[
        (
            "invalid rest threshold preserves run",
            invalid_rest_thresholds_preserve_the_accepted_run,
        ),
        (
            "type-specific spring rest",
            geometry_reaches_rest_before_normalized_components,
        ),
        (
            "replacement publishes components before cancellation",
            replacement_publishes_all_components_before_cancellation,
        ),
        (
            "repeated configuration keeps remaining deadline",
            repeated_configuration_keeps_the_remaining_deadline,
        ),
        (
            "clockless motion and snap",
            clockless_motion_and_snap_publish_the_exact_target,
        ),
        (
            "observers do not prolong the owner",
            observers_do_not_prolong_the_run_or_its_seat,
        ),
        (
            "converter reentry",
            converters_can_reenter_the_registry_and_replace_a_visible_target,
        ),
        (
            "failed component keeps the whole sample",
            a_failed_component_keeps_the_whole_published_sample,
        ),
        (
            "equivalent motion publishes exact target",
            an_equivalent_motion_target_still_publishes_its_exact_value,
        ),
        (
            "owner release stops remaining callouts",
            releasing_the_owner_stops_the_remaining_component_callouts,
        ),
        (
            "group publishes all owners before delivery",
            grouped_motion_publishes_all_owners_before_delivery,
        ),
        (
            "ignored refusal cannot admit a prefix",
            an_ignored_refusal_keeps_every_old_run,
        ),
        (
            "caught preparation panic cannot admit a prefix",
            a_caught_preparation_panic_keeps_every_old_run,
        ),
        (
            "later converter invalidates earlier preparation",
            a_later_converter_invalidates_the_whole_group,
        ),
        (
            "refused optional owner releases its seat",
            a_refused_optional_owner_releases_its_registry_seat,
        ),
        (
            "grouped removal commits every seat before cancellation",
            grouped_removal_commits_every_seat_before_cancellation,
        ),
        (
            "registry migration commits every seat before delivery",
            grouped_registry_migration_commits_before_delivery,
        ),
    ]);
}

fn grouped_registry_migration_commits_before_delivery() {
    for (unbound, fails) in [(false, false), (false, true), (true, false), (true, true)] {
        let old = Vsync::new();
        let next = Vsync::new();
        let mut left = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&old)).unwrap();
        let mut right = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&old)).unwrap();
        let left_run = left.animate_to(1.0).unwrap();
        let right_run = right.animate_to(2.0).unwrap();
        let mut clock = MotionClock::new();
        old.tick_all(&clock.frame(Duration::ZERO));
        old.tick_all(&clock.frame(Duration::from_millis(250)));
        let seam = (left.value(), right.value());
        let delivered = Rc::new(Cell::new(0));
        let check = {
            let old = old.clone();
            let delivered = Rc::clone(&delivered);
            move || {
                assert!(old.is_empty(), "every old seat leaves before any callout");
                delivered.set(delivered.get() + 1);
                assert!(!fails, "first migration delivery failure");
            }
        };
        if unbound {
            left_run.when_complete_or_cancel({
                let check = check.clone();
                move |result| {
                    assert!(result.is_ok());
                    check();
                }
            });
            right_run.when_complete_or_cancel(move |result| {
                assert!(result.is_ok());
                check();
            });
        } else {
            next.set_frame_requester(Some(Rc::new(check)));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            VsyncUpdate::run(|update| {
                update.rebind(&mut left, (!unbound).then_some(&next));
                update.rebind(&mut right, (!unbound).then_some(&next));
            })
        }));
        if fails {
            let payload = result.unwrap_err();
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("first migration delivery failure")
            );
        } else {
            result.unwrap().unwrap();
        }
        assert!(old.is_empty());
        assert!(delivered.get() > 0);
        next.set_frame_requester(None);
        if unbound {
            assert_eq!(
                delivered.get(),
                2,
                "settlement tail survives callback failure"
            );
            assert!(left_run.is_complete() && right_run.is_complete());
        } else {
            assert_eq!((left.value(), right.value()), seam);
            assert!(!left_run.is_canceled() && !right_run.is_canceled());
            next.tick_all(&clock.frame(Duration::from_secs(2)));
            next.tick_all(&clock.frame(Duration::from_secs(3)));
        }
        assert_eq!((left.value(), right.value()), (1.0, 2.0));
        assert!(!left.animation().is_animating() && !right.animation().is_animating());
        drop((left, right));
        assert!(next.is_empty());
    }
}

fn grouped_motion_publishes_all_owners_before_delivery() {
    use flui_foundation::Listenable;
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut left = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let mut right =
        AnimatedValue::new(Offset::ZERO, curve(Curves::Linear), Some(&registry)).unwrap();
    let left_old = left.animate_to(1.0).unwrap();
    let right_old = right.animate_to(Offset::new(2.0, -4.0)).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let left_view = left.animation();
    let right_view = right.animation();
    let seam = (
        left.value(),
        left.velocity(),
        right.value(),
        right.velocity(),
    );
    let delivered = Rc::new(Cell::new(0));
    let check = {
        let delivered = Rc::clone(&delivered);
        let left_old = left_old.clone();
        let right_old = right_old.clone();
        move || {
            assert!(left_old.is_canceled() && right_old.is_canceled());
            assert_eq!(
                (
                    left_view.value(),
                    left_view.velocity(),
                    right_view.value(),
                    right_view.velocity()
                ),
                seam
            );
            delivered.set(delivered.get() + 1);
        }
    };
    for view in [left.animation().status(), right.animation().status()] {
        assert_eq!(view, AnimationStatus::Forward);
    }
    let left_listener = left.animation().add_listener(Rc::new(check.clone()));
    let right_listener = right.animation().add_listener(Rc::new(check.clone()));
    left_old.when_complete_or_cancel({
        let check = check.clone();
        move |result| {
            assert!(result.is_err());
            check();
        }
    });
    right_old.when_complete_or_cancel(move |result| {
        assert!(result.is_err());
        check();
    });
    MotionUpdate::run(|update| {
        update.retarget(&mut left, -1.0, curve(Curves::EaseIn))?;
        update.retarget(&mut right, Offset::new(-3.0, 7.0), curve(Curves::EaseOut))
    })
    .unwrap();
    assert_eq!(delivered.get(), 4);
    left.animation().remove_listener(left_listener);
    right.animation().remove_listener(right_listener);
    registry.tick_all(&clock.frame(Duration::from_millis(1250)));
    assert_eq!(left.value(), -1.0);
    assert_eq!(right.value(), Offset::new(-3.0, 7.0));
    drop((left, right));
    assert!(registry.is_empty());
}

fn an_ignored_refusal_keeps_every_old_run() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut left = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let mut right = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let left_old = left.animate_to(1.0).unwrap();
    let right_old = right.animate_to(2.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    let result = MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(Curves::Linear))?;
        assert!(
            update
                .retarget(&mut right, f64::NAN, curve(Curves::Linear))
                .is_err()
        );
        Ok(())
    });
    assert!(matches!(result, Err(AnimationError::NonFiniteTarget(_))));
    assert!(left_old.is_pending() && right_old.is_pending());
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!((left.value(), right.value()), (1.0, 2.0));
    MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(Curves::Linear))?;
        update.retarget(&mut right, 4.0, curve(Curves::Linear))
    })
    .unwrap();
    registry.tick_all(&clock.frame(Duration::from_millis(1001)));
    registry.tick_all(&clock.frame(Duration::from_millis(2001)));
    assert_eq!((left.value(), right.value()), (3.0, 4.0));
}

fn a_caught_preparation_panic_keeps_every_old_run() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut left = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let mut right = AnimatedValue::new(plain(0.0), curve(Curves::Linear), Some(&registry)).unwrap();
    let left_old = left.animate_to(1.0).unwrap();
    let right_old = right.animate_to(plain(2.0)).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    let result = MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(Curves::Linear))?;
        let target = ReentrantValue {
            position: 4.0,
            clone_hook: Rc::new(|| {}),
            vector_hook: Rc::new(|| panic!("preparation failure")),
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| update.retarget(
                &mut right,
                target,
                curve(Curves::Linear)
            )))
            .is_err()
        );
        Ok(())
    });
    assert_eq!(result, Err(AnimationError::AdmissionAborted));
    assert!(left_old.is_pending() && right_old.is_pending());
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!((left.value(), right.value().position), (1.0, 2.0));
}

fn a_later_converter_invalidates_the_whole_group() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut left = AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap();
    let mut right = AnimatedValue::new(plain(0.0), curve(Curves::Linear), Some(&registry)).unwrap();
    let left_old = left.animate_to(1.0).unwrap();
    let right_old = right.animate_to(plain(2.0)).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    let tick = clock.frame(Duration::from_millis(250));
    let target = ReentrantValue {
        position: 4.0,
        clone_hook: Rc::new(|| {}),
        vector_hook: Rc::new({
            let registry = registry.clone();
            move || registry.tick_all(&tick)
        }),
    };
    let result = MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(Curves::Linear))?;
        update.retarget(&mut right, target, curve(Curves::Linear))
    });
    assert_eq!(result, Err(AnimationError::ReentrantMotion));
    assert!(left_old.is_pending() && right_old.is_pending());
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!((left.value(), right.value().position), (1.0, 2.0));
}

fn a_refused_optional_owner_releases_its_registry_seat() {
    for panics in [false, true] {
        let registry = Vsync::new();
        let mut owner =
            AnimatedValue::new(plain(0.0), curve(Curves::Linear), Some(&registry)).unwrap();
        let old = owner.animate_to(plain(1.0)).unwrap();
        let mut appearing = None;
        let target = ReentrantValue {
            position: if panics { 2.0 } else { f64::NAN },
            clone_hook: Rc::new(|| {}),
            vector_hook: Rc::new(move || {
                assert!(!panics, "optional admission failed");
            }),
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            MotionUpdate::run(|update| {
                update.replace(
                    &mut appearing,
                    Some(
                        AnimatedValue::new(0.0_f64, curve(Curves::Linear), Some(&registry))
                            .unwrap(),
                    ),
                );
                update.retarget(&mut owner, target, curve(Curves::Linear))
            })
        }));
        if panics {
            assert!(outcome.is_err());
        } else {
            assert!(outcome.unwrap().is_err());
        }
        assert!(appearing.is_none() && old.is_pending());
        assert_eq!(
            registry.len(),
            1,
            "refused appearance must release its seat; panic={panics}"
        );
        drop(owner);
        assert!(registry.is_empty());
    }
}

fn grouped_removal_commits_every_seat_before_cancellation() {
    for (keep_owners, fails) in [(false, false), (false, true), (true, false), (true, true)] {
        let registry = Vsync::new();
        let mut left =
            Some(AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap());
        let mut right =
            Some(AnimatedValue::new(0.0, curve(Curves::Linear), Some(&registry)).unwrap());
        let left_old = left.as_mut().unwrap().animate_to(1.0).unwrap();
        let right_old = right.as_mut().unwrap().animate_to(2.0).unwrap();
        let left_view = left.as_ref().unwrap().animation();
        let right_view = right.as_ref().unwrap().animation();
        let observed = Rc::new(Cell::new(0));
        let check = {
            let registry = registry.clone();
            let observed = Rc::clone(&observed);
            let left_old = left_old.clone();
            let right_old = right_old.clone();
            move || {
                assert!(
                    registry.is_empty(),
                    "every removed seat must be absent before cancellation delivery"
                );
                assert!(!left_view.is_animating() && !right_view.is_animating());
                assert!(left_old.is_canceled() && right_old.is_canceled());
                observed.set(observed.get() + 1);
            }
        };
        left_old.when_complete_or_cancel({
            let check = check.clone();
            move |result| {
                assert!(result.is_err());
                check();
                assert!(!fails, "first grouped cancellation failure");
            }
        });
        right_old.when_complete_or_cancel(move |result| {
            assert!(result.is_err());
            check();
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            MotionUpdate::run(|update| {
                if keep_owners {
                    update.dispose(left.as_mut().unwrap());
                    update.dispose(right.as_mut().unwrap());
                } else {
                    update.replace(&mut left, None);
                    update.replace(&mut right, None);
                }
                Ok(())
            })
        }));
        if fails {
            let payload = result.unwrap_err();
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("first grouped cancellation failure")
            );
        } else {
            result.unwrap().unwrap();
        }
        assert!(registry.is_empty());
        if keep_owners {
            let left = left.as_mut().unwrap();
            let right = right.as_mut().unwrap();
            assert_eq!((left.value(), right.value()), (0.0, 0.0));
            assert_eq!((left.velocity(), right.velocity()), ([0.0], [0.0]));
            MotionUpdate::run(|update| {
                update.dispose(left);
                update.dispose(right);
                Ok(())
            })
            .unwrap();
            assert!(registry.is_empty());
        } else {
            assert!(left.is_none() && right.is_none());
        }
        assert_eq!(
            observed.get(),
            2,
            "accepted cancellation tail must be delivered after failure"
        );
    }
}
