//! Consumer contracts for integer animation progress.

use flui_animation::{Animatable, IntTween, Rounding};

use Rounding::{Ceil, Floor, Nearest, TowardZero};

const MODES: [Rounding; 4] = [Nearest, Floor, Ceil, TowardZero];

/// `begin → end` sampled at `t`, rounded each way: `(rounding, expected)`.
fn rounds(begin: i32, end: i32, t: f64, expected: [(Rounding, i32); 4]) {
    for (rounding, value) in expected {
        let tween = IntTween::new(begin, end).with_rounding(rounding);
        assert_eq!(
            tween.transform(t),
            value,
            "{begin} → {end} at {t} under {rounding:?}"
        );
    }
}

/// The full range reaches both endpoints exactly; its midpoint is −0.5.
fn ascending_extreme_integer_endpoints() {
    rounds(
        i32::MIN,
        i32::MAX,
        0.0,
        [
            (Nearest, i32::MIN),
            (Floor, i32::MIN),
            (Ceil, i32::MIN),
            (TowardZero, i32::MIN),
        ],
    );
    rounds(
        i32::MIN,
        i32::MAX,
        0.5,
        [(Nearest, 0), (Floor, -1), (Ceil, 0), (TowardZero, 0)],
    );
    rounds(
        i32::MIN,
        i32::MAX,
        1.0,
        [
            (Nearest, i32::MAX),
            (Floor, i32::MAX),
            (Ceil, i32::MAX),
            (TowardZero, i32::MAX),
        ],
    );
}

fn descending_extreme_integer_endpoints() {
    rounds(
        i32::MAX,
        i32::MIN,
        0.0,
        [
            (Nearest, i32::MAX),
            (Floor, i32::MAX),
            (Ceil, i32::MAX),
            (TowardZero, i32::MAX),
        ],
    );
    rounds(
        i32::MAX,
        i32::MIN,
        0.5,
        [(Nearest, 0), (Floor, -1), (Ceil, 0), (TowardZero, 0)],
    );
    rounds(
        i32::MAX,
        i32::MIN,
        1.0,
        [
            (Nearest, i32::MIN),
            (Floor, i32::MIN),
            (Ceil, i32::MIN),
            (TowardZero, i32::MIN),
        ],
    );
}

/// Fractions that are not halves tell every mode apart; outside `[0, 1]`
/// progress clamps to the endpoints.
fn ordinary_integer_rounding_and_clamping() {
    // −2 → 3 at 0.46 is 0.3, at 0.26 is −0.7.
    rounds(
        -2,
        3,
        0.46,
        [(Nearest, 0), (Floor, 0), (Ceil, 1), (TowardZero, 0)],
    );
    rounds(
        -2,
        3,
        0.26,
        [(Nearest, -1), (Floor, -1), (Ceil, 0), (TowardZero, 0)],
    );
    for rounding in MODES {
        let ordinary = IntTween::new(-2, 3).with_rounding(rounding);
        assert_eq!(ordinary.transform(-1.0), -2, "{rounding:?}");
        assert_eq!(ordinary.transform(2.0), 3, "{rounding:?}");
    }
}

/// CSS Values 4 §5.2.1 rounds an interpolated integer's half toward +∞.
fn nearest_rounds_halves_toward_positive_infinity() {
    rounds(
        -5,
        0,
        0.5,
        [(Nearest, -2), (Floor, -3), (Ceil, -2), (TowardZero, -2)],
    );
    rounds(
        0,
        5,
        0.5,
        [(Nearest, 3), (Floor, 2), (Ceil, 3), (TowardZero, 2)],
    );
}

/// The largest value below one half is not a half: adding 0.5 to it would
/// round up to 1.0 in `f64`.
fn nearest_keeps_values_just_below_a_half() {
    let below_half = 0.5 - f64::EPSILON / 4.0;
    rounds(
        0,
        1,
        below_half,
        [(Nearest, 0), (Floor, 0), (Ceil, 1), (TowardZero, 0)],
    );
}

fn nan_progress_preserves_the_begin_endpoint() {
    for rounding in MODES {
        for (begin, end) in [(7, -3), (-9, 4)] {
            let tween = IntTween::new(begin, end).with_rounding(rounding);
            assert_eq!(tween.transform(f64::NAN), begin);
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
            "nearest keeps values just below a half",
            nearest_keeps_values_just_below_a_half,
        ),
        (
            "NaN progress preserves the begin endpoint",
            nan_progress_preserves_the_begin_endpoint,
        ),
    ]);
}

/// A rounding round-trips by name; a payload written before `rounding`
/// existed decodes as `Nearest`.
#[cfg(feature = "serde")]
#[test]
fn integer_tweens_decode_payloads_without_a_rounding() {
    let floor = IntTween::new(-5, 0).with_rounding(Floor);
    let wire = serde_json::to_string(&floor).expect("serialize");
    assert_eq!(wire, r#"{"begin":-5,"end":0,"rounding":"Floor"}"#);
    assert_eq!(
        serde_json::from_str::<IntTween>(&wire).expect("deserialize"),
        floor
    );

    let legacy: IntTween = serde_json::from_str(r#"{"begin":-5,"end":0}"#).expect("legacy");
    assert_eq!(legacy, IntTween::new(-5, 0));
    assert_eq!(legacy.transform(0.5), -2);
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
