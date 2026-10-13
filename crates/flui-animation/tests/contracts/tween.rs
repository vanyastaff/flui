//! Consumer contracts for integer animation progress.

use flui_animation::{Animatable, IntTween, Rounding};

const MODES: [Rounding; 4] = [
    Rounding::Nearest,
    Rounding::Floor,
    Rounding::Ceil,
    Rounding::TowardZero,
];

fn tween(begin: i32, end: i32, rounding: Rounding) -> IntTween {
    IntTween::new(begin, end).with_rounding(rounding)
}

/// The full range reaches both endpoints exactly; its midpoint is −0.5.
fn ascending_extreme_integer_endpoints() {
    for (rounding, midpoint) in MODES.into_iter().zip([0, -1, 0, 0]) {
        let full = tween(i32::MIN, i32::MAX, rounding);
        assert_eq!(full.transform(0.0), i32::MIN, "{rounding:?}");
        assert_eq!(full.transform(0.5), midpoint, "{rounding:?}");
        assert_eq!(full.transform(1.0), i32::MAX, "{rounding:?}");
    }
}

fn descending_extreme_integer_endpoints() {
    for (rounding, midpoint) in MODES.into_iter().zip([0, -1, 0, 0]) {
        let full = tween(i32::MAX, i32::MIN, rounding);
        assert_eq!(full.transform(0.0), i32::MAX, "{rounding:?}");
        assert_eq!(full.transform(0.5), midpoint, "{rounding:?}");
        assert_eq!(full.transform(1.0), i32::MIN, "{rounding:?}");
    }
}

/// `-2 → 3` at 0.3 is −0.5 and at 0.5 is 0.5; progress outside `[0, 1]` clamps.
fn ordinary_integer_rounding_and_clamping() {
    let expected = [(0, 1), (-1, 0), (0, 1), (0, 0)];
    for (rounding, (at_minus_half, at_half)) in MODES.into_iter().zip(expected) {
        let ordinary = tween(-2, 3, rounding);
        assert_eq!(ordinary.transform(0.3), at_minus_half, "{rounding:?}");
        assert_eq!(ordinary.transform(0.5), at_half, "{rounding:?}");
        assert_eq!(ordinary.transform(-1.0), -2, "{rounding:?}");
        assert_eq!(ordinary.transform(2.0), 3, "{rounding:?}");
    }
}

/// CSS Values 4 §5.2.1 rounds an interpolated integer's half toward +∞.
fn nearest_rounds_halves_toward_positive_infinity() {
    assert_eq!(IntTween::new(-5, 0).transform(0.5), -2);
    assert_eq!(IntTween::new(0, 5).transform(0.5), 3);
}

fn nan_progress_preserves_the_begin_endpoint() {
    for rounding in MODES {
        for (begin, end) in [(7, -3), (-9, 4)] {
            assert_eq!(tween(begin, end, rounding).transform(f64::NAN), begin);
        }
    }
}

#[test]
fn integer_tweens_interpolate_across_the_full_range() {
    crate::run_table(&[
        (
            "ascending extreme integer endpoints",
            ascending_extreme_integer_endpoints,
        ),
        (
            "descending extreme integer endpoints",
            descending_extreme_integer_endpoints,
        ),
        (
            "ordinary integer rounding and clamping",
            ordinary_integer_rounding_and_clamping,
        ),
        (
            "nearest rounds halves toward positive infinity",
            nearest_rounds_halves_toward_positive_infinity,
        ),
        (
            "NaN progress preserves the begin endpoint",
            nan_progress_preserves_the_begin_endpoint,
        ),
    ]);
}

#[test]
fn mapped_animation_accepts_values_without_debug() {
    use flui_animation::{
        Animation, AnimationController, AnimationStatus, ConstantAnimation, Curve, CurvedAnimation,
        ProxyAnimation, TweenAnimation,
    };
    use std::rc::Rc;
    use std::time::Duration;

    #[derive(Clone)]
    struct Value(f64);
    struct Mapping;
    struct Easing {
        reverse: bool,
    }
    impl Curve for Easing {
        fn transform(&self, progress: f64) -> f64 {
            let progress = progress.clamp(0.0, 1.0);
            if self.reverse {
                1.0 - (1.0 - progress).powi(2)
            } else {
                progress.powi(2)
            }
        }
    }
    impl Animatable for Mapping {
        type Value = Value;
        fn transform(&self, progress: f64) -> Value {
            Value(10.0 + 20.0 * progress)
        }
    }
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    let curved = Rc::new(
        CurvedAnimation::new(Rc::new(controller.clone()), Easing { reverse: false })
            .with_reverse_curve(Easing { reverse: true }),
    );
    let mapped = TweenAnimation::new(Mapping, curved.clone());
    let composed = Mapping.animate(curved);
    let proxy = ProxyAnimation::new(Rc::new(composed));
    let run = controller.forward().unwrap();
    controller.tick_at(Duration::from_millis(250));
    assert_eq!(mapped.value().0, 11.25);
    assert_eq!(proxy.value().0, 11.25);
    assert!(mapped.is_animating() && proxy.is_animating() && run.is_pending());
    controller.tick_at(Duration::from_secs(1));
    assert_eq!(mapped.value().0, 30.0);
    assert_eq!(proxy.value().0, 30.0);
    assert!(!mapped.is_animating() && !proxy.is_animating() && run.is_complete());

    let reverse = controller.reverse().unwrap();
    controller.tick_at(Duration::from_millis(500));
    assert_eq!(mapped.value().0, 25.0);
    assert_eq!(proxy.value().0, 25.0);
    assert_eq!(proxy.status(), AnimationStatus::Reverse);
    assert!(reverse.is_pending());
    proxy.set_parent(Rc::new(ConstantAnimation::with_status(
        Value(42.0),
        AnimationStatus::ReverseSettled,
    )));
    assert_eq!(proxy.value().0, 42.0);
    assert_eq!(proxy.status(), AnimationStatus::ReverseSettled);
    assert!(!proxy.is_animating());
    controller.tick_at(Duration::from_secs(1));
    assert!(reverse.is_complete());
    assert_eq!(mapped.value().0, 10.0);
    assert_eq!(proxy.value().0, 42.0);
}
