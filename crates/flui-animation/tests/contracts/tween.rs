//! Consumer contracts for integer and weighted animation progress.

use flui_animation::{
    Animatable, FloatTween, IntTween, StepTween, TweenSequence, TweenSequenceItem,
};

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

fn nan_progress_keeps_begin() {
    assert_eq!(IntTween::new(-2, 3).transform(f64::NAN), -2);
    assert_eq!(StepTween::new(7, 3).transform(f64::NAN), 7);
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
        ("nan progress keeps begin", nan_progress_keeps_begin),
    ]);
}

fn sequence(first_weight: f64, last_weight: f64) -> TweenSequence<f64, FloatTween> {
    TweenSequence::new(vec![
        TweenSequenceItem::new(FloatTween::new(0.0, 1.0), first_weight),
        TweenSequenceItem::new(FloatTween::new(1.0, 2.0), last_weight),
    ])
}

fn overflowing_finite_weights_keep_interior_progress() {
    let tween = sequence(f64::MAX, f64::MAX);
    assert_eq!(tween.transform(0.0), 0.0);
    assert_eq!(tween.transform(0.25), 0.5);
    assert_eq!(tween.transform(0.5), 1.0);
    assert_eq!(tween.transform(0.75), 1.5);
    assert_eq!(tween.transform(1.0), 2.0);
}

fn tiny_final_interval_reaches_its_endpoint_and_interior() {
    let tween = sequence(1.0, 1e-8);
    assert_eq!(tween.transform(1.0), 2.0);
    assert!((tween.transform(1.0 - 5e-9) - 1.5).abs() < 1e-7);
}

fn tiny_first_interval_keeps_its_interior() {
    let tween = sequence(1e-8, 1.0);
    assert_eq!(tween.transform(0.0), 0.0);
    assert!((tween.transform(5e-9) - 0.5).abs() < 1e-7);
}

fn ordinary_weighted_progress_and_clamping() {
    let tween = sequence(1.0, 3.0);
    assert_eq!(tween.transform(0.25), 1.0);
    assert_eq!(tween.transform(0.625), 1.5);
    assert_eq!(tween.transform(-1.0), 0.0);
    assert_eq!(tween.transform(2.0), 2.0);
}

fn unresolvable_final_interval_still_reaches_its_endpoint() {
    let tween = sequence(f64::MAX, f64::MIN_POSITIVE);
    assert_eq!(tween.transform(1.0), 2.0);
}

#[test]
fn weighted_sequences_preserve_endpoints_and_relative_progress() {
    crate::run_table(&[
        (
            "overflowing finite weights keep interior progress",
            overflowing_finite_weights_keep_interior_progress,
        ),
        (
            "tiny final interval reaches its endpoint and interior",
            tiny_final_interval_reaches_its_endpoint_and_interior,
        ),
        (
            "tiny first interval keeps its interior",
            tiny_first_interval_keeps_its_interior,
        ),
        (
            "ordinary weighted progress and clamping",
            ordinary_weighted_progress_and_clamping,
        ),
        (
            "unresolvable final interval still reaches its endpoint",
            unresolvable_final_interval_still_reaches_its_endpoint,
        ),
    ]);
}

fn edited_zero_weight_is_rejected() {
    reject_edited_weight(0.0);
}

fn edited_negative_weight_is_rejected() {
    reject_edited_weight(-0.5);
}

fn edited_infinite_weight_is_rejected() {
    reject_edited_weight(f64::INFINITY);
}

fn edited_nan_weight_is_rejected() {
    reject_edited_weight(f64::NAN);
}

fn reject_edited_weight(weight: f64) {
    let mut item = TweenSequenceItem::new(FloatTween::new(0.0, 1.0), 1.0);
    item.weight = weight;
    assert!(
        std::panic::catch_unwind(|| TweenSequence::new(vec![
            item,
            TweenSequenceItem::new(FloatTween::new(1.0, 2.0), 1.0)
        ]))
        .is_err()
    );
    assert_eq!(sequence(1.0, 1.0).transform(0.75), 1.5);
}

#[test]
fn weighted_sequences_reject_invalid_edited_configuration() {
    crate::run_table(&[
        (
            "edited zero weight is rejected",
            edited_zero_weight_is_rejected,
        ),
        (
            "edited negative weight is rejected",
            edited_negative_weight_is_rejected,
        ),
        (
            "edited infinite weight is rejected",
            edited_infinite_weight_is_rejected,
        ),
        (
            "edited nan weight is rejected",
            edited_nan_weight_is_rejected,
        ),
    ]);
}
