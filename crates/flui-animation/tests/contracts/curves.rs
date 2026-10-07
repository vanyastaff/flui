//! The curve catalog: every `Curves::*` constant maps the unit interval's
//! endpoints onto themselves and stays finite in between.

use flui_animation::{Curve, Curves};
use proptest::prelude::*;

/// Every constant on [`Curves`], by name.
const CATALOG: &[(&str, &dyn Curve)] = &[
    ("Linear", &Curves::Linear),
    ("EaseIn", &Curves::EaseIn),
    ("EaseOut", &Curves::EaseOut),
    ("EaseInOut", &Curves::EaseInOut),
    ("FastOutSlowIn", &Curves::FastOutSlowIn),
    ("SlowOutFastIn", &Curves::SlowOutFastIn),
    ("EaseInOutCubic", &Curves::EaseInOutCubic),
    ("EaseInSine", &Curves::EaseInSine),
    ("EaseOutSine", &Curves::EaseOutSine),
    ("EaseInOutSine", &Curves::EaseInOutSine),
    ("EaseInExpo", &Curves::EaseInExpo),
    ("EaseOutExpo", &Curves::EaseOutExpo),
    ("EaseInOutExpo", &Curves::EaseInOutExpo),
    ("EaseInCirc", &Curves::EaseInCirc),
    ("EaseOutCirc", &Curves::EaseOutCirc),
    ("EaseInOutCirc", &Curves::EaseInOutCirc),
    ("EaseInBack", &Curves::EaseInBack),
    ("EaseOutBack", &Curves::EaseOutBack),
    ("EaseInOutBack", &Curves::EaseInOutBack),
    ("ElasticIn", &Curves::ElasticIn),
    ("ElasticOut", &Curves::ElasticOut),
    ("ElasticInOut", &Curves::ElasticInOut),
    ("BounceIn", &Curves::BounceIn),
    ("BounceOut", &Curves::BounceOut),
    ("BounceInOut", &Curves::BounceInOut),
    ("Decelerate", &Curves::Decelerate),
    ("Ease", &Curves::Ease),
    ("EaseInQuad", &Curves::EaseInQuad),
    ("EaseInCubic", &Curves::EaseInCubic),
    ("EaseInQuart", &Curves::EaseInQuart),
    ("EaseInQuint", &Curves::EaseInQuint),
    ("EaseOutQuad", &Curves::EaseOutQuad),
    ("EaseOutCubic", &Curves::EaseOutCubic),
    ("EaseOutQuart", &Curves::EaseOutQuart),
    ("EaseOutQuint", &Curves::EaseOutQuint),
    ("EaseInOutQuad", &Curves::EaseInOutQuad),
    ("EaseInOutQuart", &Curves::EaseInOutQuart),
    ("EaseInOutQuint", &Curves::EaseInOutQuint),
    ("FastLinearToSlowEaseIn", &Curves::FastLinearToSlowEaseIn),
    ("LinearToEaseOut", &Curves::LinearToEaseOut),
    ("EaseInToLinear", &Curves::EaseInToLinear),
    ("SlowMiddle", &Curves::SlowMiddle),
    (
        "EaseInOutCubicEmphasized",
        &Curves::EaseInOutCubicEmphasized,
    ),
    ("FastEaseInToSlowEaseOut", &Curves::FastEaseInToSlowEaseOut),
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
