//! The owning value's public frame, delivery and observer lifetime contracts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    AnimatedValue, Animation, AnimationError, AnimationStatus, MotionClock, MotionSpec,
    MotionUpdate, SharedCurve, SpringDescription, TwoWayConverter, Vsync, VsyncUpdate, curves,
};
use flui_foundation::geometry::Offset;

fn curve(curve: impl flui_animation::Curve + 'static) -> MotionSpec {
    MotionSpec::Curve {
        duration: Duration::from_secs(1),
        curve: SharedCurve::new(curve),
    }
}

fn replacement_publishes_all_components_before_cancellation() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut value =
        AnimatedValue::new(Offset::ZERO, curve(curves::LINEAR), Some(&registry)).unwrap();
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
        .retarget(Offset::new(-3.0, 7.0), curve(curves::EASE_IN))
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
    let mut value = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let first = value.animate_to(1.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let repeated = value.animate_to(1.0).unwrap();
    assert!(first.is_pending() && repeated.is_pending());
    let seam = (value.value(), value.velocity());
    let second = value.set_motion(curve(curves::EASE_IN)).unwrap();
    assert_eq!((value.value(), value.velocity()), seam);
    assert!(first.is_canceled() && repeated.is_canceled());
    registry.tick_all(&clock.frame(Duration::from_millis(500)));
    let third = value.set_motion(curve(curves::EASE_OUT)).unwrap();
    assert!(second.is_canceled());
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(third.is_complete());
    assert_eq!(value.value(), 1.0);
    assert_eq!(value.velocity(), [0.0]);
}

fn clockless_motion_and_snap_publish_the_exact_target() {
    let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 0.5).expect("admitted spring");
    for motion in [curve(curves::EASE_IN), MotionSpec::Spring(spring)] {
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
    let motion = MotionSpec::Spring(
        SpringDescription::with_damping_ratio(1.0, 100.0, 1.0).expect("admitted spring"),
    );
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

/// The component-0 rest-distance refusal for threshold `CASE`: a finite
/// non-positive distance is out of range, NaN and infinity are not finite.
fn threshold_refusal<const CASE: usize>(error: &AnimationError) -> bool {
    use flui_animation::{SimulationError, SimulationParameter};
    let AnimationError::ComponentSimulation {
        component: 0,
        source,
    } = error
    else {
        return false;
    };
    match source {
        SimulationError::OutOfRange {
            parameter: SimulationParameter::Distance,
            value,
        } => CASE < 2 && *value == InvalidThreshold::<CASE>::rest_thresholds()[0],
        SimulationError::NonFinite {
            parameter: SimulationParameter::Distance,
        } => CASE >= 2,
        _ => false,
    }
}

fn refused_threshold<const CASE: usize>() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut owner = AnimatedValue::new(
        InvalidThreshold::<CASE>(0.0),
        curve(curves::LINEAR),
        Some(&registry),
    )
    .unwrap();
    let old = owner.animate_to(InvalidThreshold(1.0)).unwrap();
    let mut healthy = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let healthy_old = healthy.animate_to(1.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let seam = (owner.value().0, owner.velocity());
    let healthy_seam = (healthy.value(), healthy.velocity());
    let spring = MotionSpec::Spring(
        SpringDescription::with_damping_ratio(1.0, 100.0, 1.0).expect("admitted spring"),
    );
    let error = owner
        .retarget(InvalidThreshold(2.0), spring)
        .expect_err("invalid rest threshold");
    #[cfg(feature = "serde")]
    {
        let wire = serde_json::to_value(&error).expect("serialize component cause");
        let decoded: AnimationError =
            serde_json::from_value(wire.clone()).expect("decode component cause");
        let source = &wire["ComponentSimulation"]["source"];
        if CASE < 2 {
            let finite = [serde_json::json!(0.0), serde_json::json!(-1.0)][CASE].clone();
            assert_eq!(source["OutOfRange"]["value"], finite);
        } else {
            assert_eq!(source["NonFinite"]["parameter"], "Distance");
        }
        assert!(threshold_refusal::<CASE>(&decoded));
    }
    assert!(threshold_refusal::<CASE>(&error));
    assert!(old.is_pending(), "refusal cannot cancel accepted motion");
    assert_eq!((owner.value().0, owner.velocity()), seam);
    let refused = MotionUpdate::run(|update| {
        update.retarget(&mut healthy, 3.0, curve(curves::LINEAR))?;
        update.retarget(
            &mut owner,
            InvalidThreshold(2.0),
            MotionSpec::Spring(
                SpringDescription::with_damping_ratio(1.0, 100.0, 1.0).expect("admitted spring"),
            ),
        )
    });
    assert!(threshold_refusal::<CASE>(
        &refused.expect_err("the group refuses")
    ));
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
    let mut value = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
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

fn color_motion_publishes_fractional_running_samples() {
    use flui_painting::styling::Color;
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut value = AnimatedValue::new(
        Color::WHITE.with_opacity(0.0),
        curve(curves::LINEAR),
        Some(&registry),
    )
    .unwrap();
    let view = value.animation();
    let run = value.animate_to(Color::WHITE).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    // A fresh value has zero velocity: its curve segment joins both resting
    // endpoints with the cubic Hermite correction, even for a linear curve.
    for micros in [30_000, 40_000] {
        registry.tick_all(&clock.frame(Duration::from_micros(micros)));
        let sample = view.value();
        assert!(run.is_pending() && view.is_animating());
        assert_eq!(sample, value.value());
        let t = f64::from(micros as u32) / 1_000_000.0;
        let expected = t * t * (3.0 - 2.0 * t);
        assert!(
            (f64::from(sample.opacity()) - expected).abs() < 1e-8,
            "{micros}us: {sample:?}"
        );
        assert!(
            sample.to_rgba_f32_array()[..3]
                .iter()
                .all(|channel| *channel > 0.999)
        );
    }
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(run.is_complete());
    assert_eq!(view.value(), Color::WHITE);
}

fn admitting_motion_preserves_the_exact_displayed_origin() {
    use flui_foundation::Listenable;
    use flui_painting::styling::Color;

    for (initial, target) in [
        (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)),
        (Color::rgba(255, 0, 0, 0), Color::WHITE),
        (Color::rgba(0, 0, 255, 0), Color::rgb(0, 255, 0)),
    ] {
        let registry = Vsync::new();
        let mut clock = MotionClock::new();
        let mut owner =
            AnimatedValue::new(initial, curve(curves::LINEAR), Some(&registry)).unwrap();
        let view = owner.animation();
        let heard = Rc::new(Cell::new(None));
        let subscription = view.add_listener({
            let view = view.clone();
            let heard = Rc::clone(&heard);
            Rc::new(move || heard.set(Some(view.value())))
        });
        let run = owner.animate_to(target).unwrap();
        assert!(run.is_pending());
        assert_eq!(
            owner.value(),
            initial,
            "admission cannot move the displayed origin"
        );
        assert_eq!(
            view.value(),
            initial,
            "the observer sees the exact same origin"
        );
        assert_eq!(
            heard.get(),
            Some(initial),
            "admission delivery preserves the origin"
        );
        registry.tick_all(&clock.frame(Duration::ZERO));
        assert_eq!(
            view.value(),
            initial,
            "anchoring time is not a value change"
        );
        registry.tick_all(&clock.frame(Duration::from_millis(100)));
        let seam = view.value();
        assert_ne!(seam, initial, "a later frame must actually advance");
        let next = owner.animate_to(Color::BLACK).unwrap();
        assert!(run.is_canceled() && next.is_pending());
        assert_eq!(
            view.value(),
            seam,
            "interruption retains the displayed sample"
        );
        registry.tick_all(&clock.frame(Duration::from_secs(2)));
        assert!(next.is_complete());
        assert_eq!(
            view.value(),
            Color::BLACK,
            "settlement retains the exact target"
        );
        MotionUpdate::run(|update| {
            update.restart(&mut owner, initial, Color::WHITE, curve(curves::LINEAR))
        })
        .unwrap();
        assert_eq!(
            view.value(),
            initial,
            "grouped restart retains its exact origin"
        );
        registry.tick_all(&clock.frame(Duration::from_secs(2)));
        assert_eq!(
            view.value(),
            initial,
            "a zero-time restart retains its origin"
        );
        registry.tick_all(&clock.frame(Duration::from_secs(3)));
        assert_eq!(view.value(), Color::WHITE);
        view.remove_listener(subscription);
    }
}

fn an_equivalent_motion_target_still_publishes_its_exact_value() {
    use flui_foundation::Listenable;
    use flui_painting::styling::Color;
    let red = Color::rgba(255, 0, 0, 0);
    let blue = Color::rgba(0, 0, 255, 0);
    assert_eq!(red.to_vector(), blue.to_vector());
    let registry = Vsync::new();
    let mut value = AnimatedValue::new(red, curve(curves::LINEAR), Some(&registry)).unwrap();
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

fn immediate_motion_does_not_reconstruct_the_displaced_sample() {
    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static REFUSE_RECONSTRUCTION: Cell<bool> = const { Cell::new(false) };
    }
    #[derive(Clone)]
    struct Value(f64);
    impl TwoWayConverter for Value {
        type Vector = [f64; 1];
        fn to_vector(&self) -> Self::Vector {
            [self.0]
        }
        fn from_vector([position]: Self::Vector) -> Self {
            REFUSE_RECONSTRUCTION.with(|refuse| {
                assert!(!refuse.get(), "unused displaced sample was reconstructed");
            });
            Self(position)
        }
        fn rest_thresholds() -> Self::Vector {
            [0.001]
        }
    }
    for snap in [false, true] {
        let registry = Vsync::new();
        let mut clock = MotionClock::new();
        let mut owner =
            AnimatedValue::new(Value(0.0), curve(curves::LINEAR), Some(&registry)).unwrap();
        let old = owner.animate_to(Value(1.0)).unwrap();
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_millis(250)));
        REFUSE_RECONSTRUCTION.with(|refuse| refuse.set(true));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if snap {
                owner.snap_to(Value(2.0)).unwrap();
            } else {
                let run = owner
                    .retarget(
                        Value(2.0),
                        MotionSpec::Curve {
                            duration: Duration::ZERO,
                            curve: SharedCurve::new(curves::LINEAR),
                        },
                    )
                    .unwrap();
                assert!(run.is_complete());
            }
        }));
        REFUSE_RECONSTRUCTION.with(|refuse| refuse.set(false));
        result.unwrap();
        assert!(old.is_canceled() && owner.is_settled());
        assert_eq!(owner.value().0, 2.0);
    }
}

fn displaced_exact_endpoints_share_failure_custody() {
    use flui_foundation::Listenable;
    struct Value {
        position: f64,
        retirement: Option<(Rc<Cell<usize>>, bool, &'static str)>,
    }
    impl Clone for Value {
        fn clone(&self) -> Self {
            Self {
                position: self.position,
                retirement: None,
            }
        }
    }
    impl TwoWayConverter for Value {
        type Vector = [f64; 1];
        fn to_vector(&self) -> Self::Vector {
            [self.position]
        }
        fn from_vector([position]: Self::Vector) -> Self {
            Self {
                position,
                retirement: None,
            }
        }
        fn rest_thresholds() -> Self::Vector {
            [0.001]
        }
    }
    impl Drop for Value {
        fn drop(&mut self) {
            if let Some((drops, fails, message)) = &self.retirement {
                drops.set(drops.get() + 1);
                assert!(!fails, "{message}");
            }
        }
    }
    for (origin_fails, target_fails, delivery_fails) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (true, true, true),
    ] {
        let registry = Vsync::new();
        let mut clock = MotionClock::new();
        let origin_drops = Rc::new(Cell::new(0));
        let target_drops = Rc::new(Cell::new(0));
        let mut owner = AnimatedValue::new(
            Value {
                position: 0.0,
                retirement: Some((Rc::clone(&origin_drops), origin_fails, "origin retirement")),
            },
            curve(curves::LINEAR),
            Some(&registry),
        )
        .unwrap();
        let old = owner
            .animate_to(Value {
                position: 1.0,
                retirement: Some((Rc::clone(&target_drops), target_fails, "target retirement")),
            })
            .unwrap();
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_millis(250)));
        assert_eq!(
            (origin_drops.get(), target_drops.get()),
            (0, 0),
            "frames retain exact endpoints"
        );
        let view = owner.animation();
        let listener = view.add_listener(Rc::new(move || {
            assert!(!delivery_fails, "admission delivery");
        }));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            owner
                .animate_to(Value {
                    position: 2.0,
                    retirement: None,
                })
                .unwrap()
        }));
        view.remove_listener(listener);
        let expected_failure = if delivery_fails {
            Some("admission delivery")
        } else if origin_fails {
            Some("origin retirement")
        } else if target_fails {
            Some("target retirement")
        } else {
            None
        };
        match (result, expected_failure) {
            (Ok(next), None) => assert!(next.is_pending()),
            (Err(payload), Some(expected)) => {
                assert_eq!(
                    flui_foundation::panic::payload_text(&*payload),
                    Some(expected)
                );
            }
            _ => panic!("endpoint retirement did not preserve first failure"),
        }
        assert!(old.is_canceled());
        assert_eq!(origin_drops.get(), usize::from(!delivery_fails));
        assert_eq!(
            target_drops.get(),
            usize::from(!delivery_fails && !origin_fails)
        );
        registry.tick_all(&clock.frame(Duration::from_secs(2)));
        assert_eq!(
            view.value().position,
            2.0,
            "accepted replacement survives containment"
        );
        owner
            .snap_to(Value {
                position: 3.0,
                retirement: None,
            })
            .unwrap();
        assert_eq!(
            view.value().position,
            3.0,
            "later healthy admission remains usable"
        );
        drop(owner);
        assert!(registry.is_empty());
        assert_eq!(view.value().position, 3.0);
    }
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
    let mut value = AnimatedValue::new(plain(0.0), curve(curves::LINEAR), Some(&registry)).unwrap();
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
        AnimatedValue::new(plain(0.0), curve(curves::LINEAR), None).unwrap(),
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

    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let anchor = clock.frame(Duration::ZERO);
    let advance = clock.frame(Duration::from_millis(250));
    let origin = ReentrantValue {
        position: 0.0,
        vector_hook: Rc::new(|| {}),
        clone_hook: Rc::new({
            let registry = registry.clone();
            let once = Cell::new(false);
            move || {
                if !once.replace(true) {
                    registry.tick_all(&advance);
                }
            }
        }),
    };
    let mut owner = AnimatedValue::new(origin, curve(curves::LINEAR), Some(&registry)).unwrap();
    let run = owner.animate_to(plain(1.0)).unwrap();
    let view = owner.animation();
    registry.tick_all(&anchor);
    assert_eq!(
        view.value().position,
        0.0,
        "origin Clone retains its read snapshot through a nested frame"
    );
    assert!(
        view.value().position > 0.0,
        "nested sampling completes without an endpoint borrow"
    );
    assert!(run.is_pending());
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
        curve: SharedCurve::new(ClosingCurve {
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
        curve: SharedCurve::new(FailingComponent {
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
        .retarget(Offset::new(-3.0, 7.0), curve(curves::LINEAR))
        .unwrap();
    assert!(old.is_canceled() && next.is_pending());
    assert_eq!((value.value(), value.velocity()), published);
    registry.tick_all(&clock.frame(Duration::from_millis(400)));
    assert_ne!(value.value(), published.0);
    assert!(value.value().dx.is_finite() && value.value().dy.is_finite());
}

/// `with_duration` keeps the easing and `with_curve` keeps the duration; a
/// spring has neither, so it takes the default one.
fn motion_spec_changes_keep_the_other_half() {
    let spring = flui_animation::MotionSpec::Spring(
        flui_animation::SpringDescription::with_damping_ratio(1.0, 100.0, 1.0)
            .expect("critical spring"),
    );
    fn curve(
        millis: u64,
        curve: impl flui_animation::Curve + 'static,
    ) -> flui_animation::MotionSpec {
        flui_animation::MotionSpec::Curve {
            duration: Duration::from_millis(millis),
            curve: flui_animation::SharedCurve::new(curve),
        }
    }
    let cases = [
        (
            curve(500, flui_animation::curves::LINEAR).with_duration(Duration::from_millis(80)),
            curve(80, flui_animation::curves::LINEAR),
        ),
        (
            curve(500, flui_animation::curves::LINEAR).with_curve(flui_animation::curves::EASE_IN),
            curve(500, flui_animation::curves::EASE_IN),
        ),
        (
            spring.clone().with_duration(Duration::from_millis(80)),
            curve(80, flui_animation::curves::EASE_IN_OUT),
        ),
        (
            spring.with_curve(flui_animation::curves::EASE_IN),
            curve(200, flui_animation::curves::EASE_IN),
        ),
        (
            flui_animation::MotionSpec::default(),
            curve(200, flui_animation::curves::EASE_IN_OUT),
        ),
    ];
    for (index, (actual, expected)) in cases.into_iter().enumerate() {
        assert_eq!(actual, expected, "case {index}");
    }
}

#[test]
fn owning_animated_value_contract() {
    crate::run_table(&[
        (
            "motion spec changes keep the other half",
            motion_spec_changes_keep_the_other_half,
        ),
        (
            "owner retirement joins every delivery channel",
            owner_retirement_joins_every_delivery_channel,
        ),
        (
            "owner retirement keeps its first destructor failure",
            owner_retirement_keeps_its_first_destructor_failure,
        ),
        (
            "retired owner cleanup passes a stale settlement fence",
            retired_owner_cleanup_passes_a_stale_settlement_fence,
        ),
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
            "motion admission preserves exact displayed origin",
            admitting_motion_preserves_the_exact_displayed_origin,
        ),
        (
            "immediate motion avoids displaced reconstruction",
            immediate_motion_does_not_reconstruct_the_displaced_sample,
        ),
        (
            "exact endpoints share retirement custody",
            displaced_exact_endpoints_share_failure_custody,
        ),
        (
            "equivalent motion publishes exact target",
            an_equivalent_motion_target_still_publishes_its_exact_value,
        ),
        (
            "color motion publishes fractional running samples",
            color_motion_publishes_fractional_running_samples,
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

fn owner_retirement_joins_every_delivery_channel() {
    use flui_foundation::Listenable;
    use std::{
        future::Future,
        pin::Pin,
        sync::Arc,
        task::{Context, Wake, Waker},
    };

    #[derive(Clone)]
    struct Target {
        position: f64,
        drops: Option<Rc<Cell<usize>>>,
    }
    impl Drop for Target {
        fn drop(&mut self) {
            if let Some(drops) = &self.drops {
                drops.set(drops.get() + 1);
            }
        }
    }
    impl TwoWayConverter for Target {
        type Vector = [f64; 1];
        fn to_vector(&self) -> Self::Vector {
            [self.position]
        }
        fn from_vector([position]: Self::Vector) -> Self {
            Self {
                position,
                drops: None,
            }
        }
        fn rest_thresholds() -> Self::Vector {
            [0.001]
        }
    }

    thread_local! {
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        static OWNER: RefCell<Option<AnimatedValue<Target>>> = const { RefCell::new(None) };
    }
    fn release_owner() {
        let outgoing = OWNER.with(|owner| owner.borrow_mut().take());
        drop(outgoing);
    }
    fn fail() {
        panic!("owner retirement delivery failure");
    }
    fn healthy() {}
    struct ProbeWake(fn());
    impl Wake for ProbeWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            (self.0)();
        }
    }

    let mut observed = Vec::new();
    let mut expected = Vec::new();
    for channel in ["value", "status", "continuation", "waiter"] {
        for earlier_failure in [false, true] {
            let registry = Vsync::new();
            let mut clock = MotionClock::new();
            let drops = Rc::new(Cell::new(0));
            let tail = Rc::new(Cell::new(0));
            let mut value = AnimatedValue::new(
                Target {
                    position: 0.0,
                    drops: None,
                },
                curve(curves::LINEAR),
                Some(&registry),
            )
            .unwrap();
            let run = value
                .animate_to(Target {
                    position: 1.0,
                    drops: Some(Rc::clone(&drops)),
                })
                .unwrap();
            registry.tick_all(&clock.frame(Duration::ZERO));
            let view = value.animation();
            assert!(OWNER.with(|owner| owner.replace(Some(value))).is_none());
            let first: fn() = if earlier_failure { fail } else { healthy };
            let mut subscriptions = Vec::new();
            let mut waiters = Vec::new();
            match channel {
                "value" => {
                    view.add_listener(Rc::new(first));
                    view.add_listener(Rc::new(release_owner));
                }
                "status" => {
                    subscriptions.push(view.subscribe_status(Rc::new(move |_| first())));
                    subscriptions.push(view.subscribe_status(Rc::new(|_| release_owner())));
                }
                "continuation" => {
                    run.when_complete_or_cancel(move |_| first());
                    run.when_complete_or_cancel(|_| release_owner());
                }
                "waiter" => {
                    for action in [first, release_owner] {
                        let wake = Waker::from(Arc::new(ProbeWake(action)));
                        let mut future = run.clone();
                        assert!(
                            Pin::new(&mut future)
                                .poll(&mut Context::from_waker(&wake))
                                .is_pending()
                        );
                        waiters.push((future, wake));
                    }
                }
                _ => unreachable!("every delivery channel is listed above"),
            }
            run.when_complete_or_cancel({
                let tail = Rc::clone(&tail);
                move |outcome| {
                    assert_eq!(outcome.is_err(), channel == "value");
                    tail.set(tail.get() + 1);
                }
            });
            drop(view);
            let elapsed = if channel == "value" {
                Duration::from_millis(250)
            } else {
                Duration::from_secs(1)
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                registry.tick_all(&clock.frame(elapsed));
            }));
            assert_eq!(outcome.is_err(), earlier_failure, "{channel}");
            if let Err(payload) = outcome {
                assert_eq!(
                    flui_foundation::panic::payload_text(&*payload),
                    Some("owner retirement delivery failure")
                );
            }
            assert!(OWNER.with(|owner| owner.borrow().is_none()));
            assert!(registry.is_empty());
            assert_eq!(run.is_canceled(), channel == "value");
            assert_eq!(tail.get(), 1, "the accepted future tail finishes");
            for (mut future, wake) in waiters {
                assert!(
                    Pin::new(&mut future)
                        .poll(&mut Context::from_waker(&wake))
                        .is_ready()
                );
            }
            drop(subscriptions);

            let fresh_drops = Rc::new(Cell::new(0));
            let mut fresh = AnimatedValue::new(
                Target {
                    position: 0.0,
                    drops: None,
                },
                curve(curves::LINEAR),
                Some(&registry),
            )
            .unwrap();
            let fresh_run = fresh
                .animate_to(Target {
                    position: 2.0,
                    drops: Some(Rc::clone(&fresh_drops)),
                })
                .unwrap();
            registry.tick_all(&clock.frame(Duration::from_secs(2)));
            registry.tick_all(&clock.frame(Duration::from_secs(3)));
            assert!(fresh_run.is_complete());
            drop(fresh);
            assert!(registry.is_empty());
            assert_eq!(
                fresh_drops.get(),
                1,
                "fresh healthy cleanup remains healthy"
            );
            observed.push((channel, earlier_failure, drops.get()));
            expected.push((channel, earlier_failure, usize::from(!earlier_failure)));
        }
    }
    assert_eq!(
        observed, expected,
        "opaque owner targets retire under the enclosing delivery's failure custody"
    );
}

fn owner_retirement_keeps_its_first_destructor_failure() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct CurveCapture(Arc<AtomicUsize>);
    impl flui_animation::Curve for CurveCapture {
        fn transform(&self, t: f64) -> f64 {
            t
        }
    }
    impl Drop for CurveCapture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("motion retirement failure");
        }
    }
    #[derive(Clone)]
    struct TargetCapture(Rc<Cell<usize>>);
    impl TwoWayConverter for TargetCapture {
        type Vector = [f64; 1];
        fn to_vector(&self) -> Self::Vector {
            [0.0]
        }
        fn from_vector(_: Self::Vector) -> Self {
            unreachable!("the resting target is never converted")
        }
        fn rest_thresholds() -> Self::Vector {
            [0.001]
        }
    }
    impl Drop for TargetCapture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
            panic!("competing target retirement");
        }
    }
    let registry = Vsync::new();
    let motion_drops = Arc::new(AtomicUsize::new(0));
    let target_drops = Rc::new(Cell::new(0));
    let value = AnimatedValue::new(
        TargetCapture(Rc::clone(&target_drops)),
        curve(CurveCapture(Arc::clone(&motion_drops))),
        Some(&registry),
    )
    .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value)))
        .expect_err("the first destructor failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("motion retirement failure")
    );
    assert_eq!(motion_drops.load(Ordering::Relaxed), 1);
    assert_eq!(
        target_drops.get(),
        0,
        "remaining opaque resources are retained"
    );
    assert!(registry.is_empty());
    let mut fresh = AnimatedValue::new(0.0_f64, curve(curves::LINEAR), Some(&registry)).unwrap();
    let run = fresh.animate_to(1.0).unwrap();
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert!(run.is_complete());
    drop(fresh);
    assert!(registry.is_empty());
}

fn retired_owner_cleanup_passes_a_stale_settlement_fence() {
    use flui_foundation::Listenable;

    struct Capture(Rc<Cell<usize>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let registry = Vsync::new();
    let mut value = AnimatedValue::new(0.0_f64, curve(curves::LINEAR), Some(&registry)).unwrap();
    let first_run = value.animate_to(1.0).unwrap();
    let view = value.animation();
    let mut clock = MotionClock::new();
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(250)));
    let storage = Rc::new(RefCell::new(None::<AnimatedValue<f64>>));
    let next_run = Rc::new(RefCell::new(None));
    let drops = Rc::new(Cell::new(0));
    let capture = Capture(Rc::clone(&drops));
    view.add_listener(Rc::new(move || {
        let _keep = &capture;
    }));
    let first = view.subscribe_status({
        let storage = Rc::downgrade(&storage);
        let next_run = Rc::clone(&next_run);
        Rc::new(move |status| {
            if status != AnimationStatus::ForwardSettled {
                return;
            }
            let storage = storage.upgrade().unwrap();
            let mut owner = storage.borrow_mut().take().unwrap();
            let run = owner.animate_to(0.0).unwrap();
            next_run.replace(Some(run));
            assert!(storage.replace(Some(owner)).is_none());
        })
    });
    let second = view.subscribe_status({
        let storage = Rc::downgrade(&storage);
        Rc::new(move |status| {
            if status != AnimationStatus::ForwardSettled {
                return;
            }
            let storage = storage.upgrade().unwrap();
            let outgoing = storage.borrow_mut().take();
            drop(outgoing);
        })
    });
    drop(view);
    let publication = VsyncUpdate::prepare(|update| {
        update.rebind(&mut value, None);
    });
    storage.replace(Some(value));
    publication.publish().unwrap();
    drop((first, second));
    assert!(storage.borrow().is_none() && registry.is_empty() && first_run.is_complete());
    assert!(next_run.borrow().as_ref().unwrap().is_canceled());
    assert_eq!(
        drops.get(),
        1,
        "a retired owner's resources must not remain behind a stale settlement fence"
    );
}

fn grouped_registry_migration_commits_before_delivery() {
    #[derive(Clone, Copy)]
    enum Delivery {
        Immediate,
        Drop,
        Unwind,
    }
    for delivery in [Delivery::Immediate, Delivery::Drop, Delivery::Unwind] {
        for (unbound, fails) in [(false, false), (false, true), (true, false), (true, true)] {
            let old = Vsync::new();
            let next = Vsync::new();
            let mut left = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&old)).unwrap();
            let mut right = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&old)).unwrap();
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
                if matches!(delivery, Delivery::Immediate) {
                    VsyncUpdate::run(|update| {
                        update.rebind(&mut left, (!unbound).then_some(&next));
                        update.rebind(&mut right, (!unbound).then_some(&next));
                    })
                } else {
                    let publication = VsyncUpdate::prepare(|update| {
                        update.rebind(&mut left, (!unbound).then_some(&next));
                        update.rebind(&mut right, (!unbound).then_some(&next));
                    });
                    if matches!(delivery, Delivery::Unwind) {
                        panic!("owner restoration failure");
                    }
                    drop(publication);
                    Ok(())
                }
            }));
            if matches!(delivery, Delivery::Unwind) {
                let payload = result.unwrap_err();
                assert_eq!(
                    flui_foundation::panic::payload_text(payload.as_ref()),
                    Some("owner restoration failure")
                );
            } else if fails {
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
}

fn grouped_motion_publishes_all_owners_before_delivery() {
    use flui_foundation::Listenable;
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut left = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let mut right =
        AnimatedValue::new(Offset::ZERO, curve(curves::LINEAR), Some(&registry)).unwrap();
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
        update.retarget(&mut left, -1.0, curve(curves::EASE_IN))?;
        update.retarget(&mut right, Offset::new(-3.0, 7.0), curve(curves::EASE_OUT))
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
    let mut left = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let mut right = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let left_old = left.animate_to(1.0).unwrap();
    let right_old = right.animate_to(2.0).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    let result = MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(curves::LINEAR))?;
        assert!(
            update
                .retarget(&mut right, f64::NAN, curve(curves::LINEAR))
                .is_err()
        );
        Ok(())
    });
    assert!(matches!(result, Err(AnimationError::NonFiniteComponent {
        input: flui_animation::AnimationInput::Target, component: 0, value,
    }) if value.is_nan()));
    assert!(left_old.is_pending() && right_old.is_pending());
    registry.tick_all(&clock.frame(Duration::from_secs(1)));
    assert_eq!((left.value(), right.value()), (1.0, 2.0));
    MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(curves::LINEAR))?;
        update.retarget(&mut right, 4.0, curve(curves::LINEAR))
    })
    .unwrap();
    registry.tick_all(&clock.frame(Duration::from_millis(1001)));
    registry.tick_all(&clock.frame(Duration::from_millis(2001)));
    assert_eq!((left.value(), right.value()), (3.0, 4.0));
}

fn a_caught_preparation_panic_keeps_every_old_run() {
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let mut left = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let mut right = AnimatedValue::new(plain(0.0), curve(curves::LINEAR), Some(&registry)).unwrap();
    let left_old = left.animate_to(1.0).unwrap();
    let right_old = right.animate_to(plain(2.0)).unwrap();
    registry.tick_all(&clock.frame(Duration::ZERO));
    let result = MotionUpdate::run(|update| {
        update.retarget(&mut left, 3.0, curve(curves::LINEAR))?;
        let target = ReentrantValue {
            position: 4.0,
            clone_hook: Rc::new(|| {}),
            vector_hook: Rc::new(|| panic!("preparation failure")),
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| update.retarget(
                &mut right,
                target,
                curve(curves::LINEAR)
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
    let mut left = AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap();
    let mut right = AnimatedValue::new(plain(0.0), curve(curves::LINEAR), Some(&registry)).unwrap();
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
        update.retarget(&mut left, 3.0, curve(curves::LINEAR))?;
        update.retarget(&mut right, target, curve(curves::LINEAR))
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
            AnimatedValue::new(plain(0.0), curve(curves::LINEAR), Some(&registry)).unwrap();
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
                        AnimatedValue::new(0.0_f64, curve(curves::LINEAR), Some(&registry))
                            .unwrap(),
                    ),
                );
                update.retarget(&mut owner, target, curve(curves::LINEAR))
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
            Some(AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap());
        let mut right =
            Some(AnimatedValue::new(0.0, curve(curves::LINEAR), Some(&registry)).unwrap());
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
