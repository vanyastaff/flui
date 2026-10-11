//! The curve catalog: every `curves::*` constant maps the unit interval's
//! endpoints onto themselves and stays finite in between.

use flui_animation::{Curve, curves};
use proptest::prelude::*;

/// Every constant on [`curves`], by name.
const CATALOG: &[(&str, &dyn Curve)] = &[
    ("Linear", &curves::LINEAR),
    ("EaseIn", &curves::EASE_IN),
    ("EaseOut", &curves::EASE_OUT),
    ("EaseInOut", &curves::EASE_IN_OUT),
    ("FastOutSlowIn", &curves::FAST_OUT_SLOW_IN),
    ("LinearOutSlowIn", &curves::LINEAR_OUT_SLOW_IN),
    ("EaseInOutCubic", &curves::EASE_IN_OUT_CUBIC),
    ("EaseInSine", &curves::EASE_IN_SINE),
    ("EaseOutSine", &curves::EASE_OUT_SINE),
    ("EaseInOutSine", &curves::EASE_IN_OUT_SINE),
    ("EaseInExpo", &curves::EASE_IN_EXPO),
    ("EaseOutExpo", &curves::EASE_OUT_EXPO),
    ("EaseInOutExpo", &curves::EASE_IN_OUT_EXPO),
    ("EaseInCirc", &curves::EASE_IN_CIRC),
    ("EaseOutCirc", &curves::EASE_OUT_CIRC),
    ("EaseInOutCirc", &curves::EASE_IN_OUT_CIRC),
    ("EaseInBack", &curves::EASE_IN_BACK),
    ("EaseOutBack", &curves::EASE_OUT_BACK),
    ("EaseInOutBack", &curves::EASE_IN_OUT_BACK),
    ("ElasticIn", &curves::ELASTIC_IN),
    ("ElasticOut", &curves::ELASTIC_OUT),
    ("ElasticInOut", &curves::ELASTIC_IN_OUT),
    ("BounceIn", &curves::BOUNCE_IN),
    ("BounceOut", &curves::BOUNCE_OUT),
    ("BounceInOut", &curves::BOUNCE_IN_OUT),
    ("Decelerate", &curves::DECELERATE),
    ("Ease", &curves::EASE),
    ("EaseInQuad", &curves::EASE_IN_QUAD),
    ("EaseInCubic", &curves::EASE_IN_CUBIC),
    ("EaseInQuart", &curves::EASE_IN_QUART),
    ("EaseInQuint", &curves::EASE_IN_QUINT),
    ("EaseOutQuad", &curves::EASE_OUT_QUAD),
    ("EaseOutCubic", &curves::EASE_OUT_CUBIC),
    ("EaseOutQuart", &curves::EASE_OUT_QUART),
    ("EaseOutQuint", &curves::EASE_OUT_QUINT),
    ("EaseInOutQuad", &curves::EASE_IN_OUT_QUAD),
    ("EaseInOutQuart", &curves::EASE_IN_OUT_QUART),
    ("EaseInOutQuint", &curves::EASE_IN_OUT_QUINT),
    (
        "FastLinearToSlowEaseIn",
        &curves::FAST_LINEAR_TO_SLOW_EASE_IN,
    ),
    ("LinearToEaseOut", &curves::LINEAR_TO_EASE_OUT),
    ("EaseInToLinear", &curves::EASE_IN_TO_LINEAR),
    ("SlowMiddle", &curves::SLOW_MIDDLE),
    (
        "EaseInOutCubicEmphasized",
        &curves::EASE_IN_OUT_CUBIC_EMPHASIZED,
    ),
    (
        "FastEaseInToSlowEaseOut",
        &curves::FAST_EASE_IN_TO_SLOW_EASE_OUT,
    ),
];

/// How far an endpoint may land from `0` or `1`.
const ENDPOINT_TOLERANCE: f64 = 1e-9;

#[test]
fn catalog_curves_map_endpoints_onto_themselves() {
    let mut failures = Vec::new();
    for &(name, curve) in CATALOG {
        let (start, end) = (curve.transform(0.0), curve.transform(1.0));
        if start.abs() > ENDPOINT_TOLERANCE || (end - 1.0).abs() > ENDPOINT_TOLERANCE {
            failures.push(format!(
                "{name}: transform(0) = {start}, transform(1) = {end}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

proptest! {
    #[test]
    fn catalog_curves_stay_finite_on_the_unit_interval(
        index in 0..CATALOG.len(),
        t in 0.0..=1.0_f64,
    ) {
        let (name, curve) = CATALOG[index];
        let value = curve.transform(t);
        prop_assert!(value.is_finite(), "{name}: transform({t}) = {value}");
    }
}
