//! The owning value's public frame, delivery and observer lifetime contracts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    AnimatedValue, Animation, AnimationStatus, ArcCurve, Curves, MotionClock, MotionSpec,
    SpringDescription, TwoWayConverter, Vsync,
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
    ]);
}
