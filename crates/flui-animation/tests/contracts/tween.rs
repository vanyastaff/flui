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
    ]);
}
