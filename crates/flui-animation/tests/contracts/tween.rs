//! Consumer contracts for integer animation progress.

use flui_animation::{Animatable, IntTween, StepTween};

fn ascending_extreme_integer_endpoints() {
    let rounded = IntTween::new(i32::MIN, i32::MAX);
    let stepped = StepTween::new(i32::MIN, i32::MAX);
    assert_eq!(rounded.transform(0.0), i32::MIN);
    assert_eq!(rounded.transform(0.5), -1);
    assert_eq!(rounded.transform(1.0), i32::MAX);
    assert_eq!(stepped.transform(0.0), i32::MIN);
    assert_eq!(stepped.transform(0.5), -1);
    assert_eq!(stepped.transform(1.0), i32::MAX);
}

fn descending_extreme_integer_endpoints() {
    let rounded = IntTween::new(i32::MAX, i32::MIN);
    let stepped = StepTween::new(i32::MAX, i32::MIN);
    assert_eq!(rounded.transform(0.0), i32::MAX);
    assert_eq!(rounded.transform(0.5), -1);
    assert_eq!(rounded.transform(1.0), i32::MIN);
    assert_eq!(stepped.transform(0.0), i32::MAX);
    assert_eq!(stepped.transform(0.5), -1);
    assert_eq!(stepped.transform(1.0), i32::MIN);
}

fn ordinary_integer_rounding_and_clamping() {
    let rounded = IntTween::new(-2, 3);
    let stepped = StepTween::new(-2, 3);
    assert_eq!(rounded.transform(0.5), 1);
    assert_eq!(stepped.transform(0.5), 0);
    assert_eq!(rounded.transform(-1.0), -2);
    assert_eq!(stepped.transform(2.0), 3);
}

fn nan_progress_preserves_the_begin_endpoint() {
    for (begin, end) in [(7, -3), (-9, 4)] {
        assert_eq!(IntTween::new(begin, end).transform(f64::NAN), begin);
        assert_eq!(StepTween::new(begin, end).transform(f64::NAN), begin);
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
