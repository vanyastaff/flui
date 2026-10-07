//! Curve contract: exact endpoints, declared monotonicity, the input policy for
//! NaN and out-of-range progress, cubic-bezier accuracy against independent
//! reference values, and parameter validation however a curve is made.

use flui_animation::{
    ArcCurve, BounceInCurve, Cubic, Curve, CurveError, Curves, ElasticInCurve, ElasticInOutCurve,
    ElasticOutCurve, Interval, JumpAt, Linear, Split, Steps, ThreePointCubic,
};
use proptest::prelude::*;

/// Whether a catalog curve promises to never decrease.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Monotone,
    Overshoots,
}

/// Every curve the crate ships, plus one constructed instance of each
/// parameterized curve and combinator.
fn catalog() -> Vec<(&'static str, ArcCurve, Shape)> {
    use Shape::{Monotone, Overshoots};
    vec![
        ("Linear", ArcCurve::new(Curves::Linear), Monotone),
        ("EaseIn", ArcCurve::new(Curves::EaseIn), Monotone),
        ("EaseOut", ArcCurve::new(Curves::EaseOut), Monotone),
        ("EaseInOut", ArcCurve::new(Curves::EaseInOut), Monotone),
        (
            "FastOutSlowIn",
            ArcCurve::new(Curves::FastOutSlowIn),
            Monotone,
        ),
        (
            "SlowOutFastIn",
            ArcCurve::new(Curves::SlowOutFastIn),
            Monotone,
        ),
        (
            "EaseInOutCubic",
            ArcCurve::new(Curves::EaseInOutCubic),
            Monotone,
        ),
        ("EaseInSine", ArcCurve::new(Curves::EaseInSine), Monotone),
        ("EaseOutSine", ArcCurve::new(Curves::EaseOutSine), Monotone),
        (
            "EaseInOutSine",
            ArcCurve::new(Curves::EaseInOutSine),
            Monotone,
        ),
        ("EaseInExpo", ArcCurve::new(Curves::EaseInExpo), Monotone),
        ("EaseOutExpo", ArcCurve::new(Curves::EaseOutExpo), Monotone),
        (
            "EaseInOutExpo",
            ArcCurve::new(Curves::EaseInOutExpo),
            Monotone,
        ),
        ("EaseInCirc", ArcCurve::new(Curves::EaseInCirc), Monotone),
        ("EaseOutCirc", ArcCurve::new(Curves::EaseOutCirc), Monotone),
        (
            "EaseInOutCirc",
            ArcCurve::new(Curves::EaseInOutCirc),
            Monotone,
        ),
        ("EaseInBack", ArcCurve::new(Curves::EaseInBack), Overshoots),
        (
            "EaseOutBack",
            ArcCurve::new(Curves::EaseOutBack),
            Overshoots,
        ),
        (
            "EaseInOutBack",
            ArcCurve::new(Curves::EaseInOutBack),
            Overshoots,
        ),
        ("ElasticIn", ArcCurve::new(Curves::ElasticIn), Overshoots),
        ("ElasticOut", ArcCurve::new(Curves::ElasticOut), Overshoots),
        (
            "ElasticInOut",
            ArcCurve::new(Curves::ElasticInOut),
            Overshoots,
        ),
        ("BounceIn", ArcCurve::new(Curves::BounceIn), Overshoots),
        ("BounceOut", ArcCurve::new(Curves::BounceOut), Overshoots),
        (
            "BounceInOut",
            ArcCurve::new(Curves::BounceInOut),
            Overshoots,
        ),
        ("Decelerate", ArcCurve::new(Curves::Decelerate), Monotone),
        ("Ease", ArcCurve::new(Curves::Ease), Monotone),
        ("EaseInQuad", ArcCurve::new(Curves::EaseInQuad), Monotone),
        ("EaseInCubic", ArcCurve::new(Curves::EaseInCubic), Monotone),
        ("EaseInQuart", ArcCurve::new(Curves::EaseInQuart), Monotone),
        ("EaseInQuint", ArcCurve::new(Curves::EaseInQuint), Monotone),
        ("EaseOutQuad", ArcCurve::new(Curves::EaseOutQuad), Monotone),
        (
            "EaseOutCubic",
            ArcCurve::new(Curves::EaseOutCubic),
            Monotone,
        ),
        (
            "EaseOutQuart",
            ArcCurve::new(Curves::EaseOutQuart),
            Monotone,
        ),
        (
            "EaseOutQuint",
            ArcCurve::new(Curves::EaseOutQuint),
            Monotone,
        ),
        (
            "EaseInOutQuad",
            ArcCurve::new(Curves::EaseInOutQuad),
            Monotone,
        ),
        (
            "EaseInOutQuart",
            ArcCurve::new(Curves::EaseInOutQuart),
            Monotone,
        ),
        (
            "EaseInOutQuint",
            ArcCurve::new(Curves::EaseInOutQuint),
            Monotone,
        ),
        (
            "FastLinearToSlowEaseIn",
            ArcCurve::new(Curves::FastLinearToSlowEaseIn),
            Monotone,
        ),
        (
            "LinearToEaseOut",
            ArcCurve::new(Curves::LinearToEaseOut),
            Monotone,
        ),
        (
            "EaseInToLinear",
            ArcCurve::new(Curves::EaseInToLinear),
            Monotone,
        ),
        ("SlowMiddle", ArcCurve::new(Curves::SlowMiddle), Monotone),
        (
            "EaseInOutCubicEmphasized",
            ArcCurve::new(Curves::EaseInOutCubicEmphasized),
            Monotone,
        ),
        (
            "FastEaseInToSlowEaseOut",
            ArcCurve::new(Curves::FastEaseInToSlowEaseOut),
            Monotone,
        ),
        (
            "Interval(0.2, 0.8, EaseIn)",
            ArcCurve::new(Interval::new(0.2, 0.8, Curves::EaseIn)),
            Monotone,
        ),
        (
            "Interval(0.5, 0.5) step",
            ArcCurve::new(Interval::linear(0.5, 0.5)),
            Monotone,
        ),
        ("Split(0.4)", ArcCurve::new(Split::new(0.4)), Monotone),
        (
            "EaseIn.flipped()",
            ArcCurve::new(Curves::EaseIn.flipped()),
            Monotone,
        ),
        (
            "ElasticOutCurve(0.3)",
            ArcCurve::new(ElasticOutCurve::new(0.3)),
            Overshoots,
        ),
        (
            "Cubic(0.05, 0.7, 0.1, 1)",
            ArcCurve::new(Cubic::new(0.05, 0.7, 0.1, 1.0)),
            Monotone,
        ),
        ("BounceInCurve", ArcCurve::new(BounceInCurve), Overshoots),
    ]
}

/// Progress samples across `[0, 1]`, both ends included.
fn grid() -> impl Iterator<Item = f64> {
    (0..=4000).map(|i| f64::from(i) / 4000.0)
}

/// Runs `violates` over the catalog and fails with every offending name.
fn assert_catalog(property: &str, violates: impl Fn(&ArcCurve, Shape) -> bool) {
    let offenders: Vec<_> = catalog()
        .into_iter()
        .filter(|(_, curve, shape)| violates(curve, *shape))
        .map(|(name, ..)| name)
        .collect();
    assert!(offenders.is_empty(), "{property}: {offenders:?}");
}

fn endpoints_are_exact() {
    assert_catalog("transform(0) != 0 or transform(1) != 1", |curve, _| {
        curve.transform(0.0).to_bits() != 0.0_f64.to_bits()
            || curve.transform(1.0).to_bits() != 1.0_f64.to_bits()
    });
}

fn interior_is_finite() {
    assert_catalog("non-finite value inside [0, 1]", |curve, _| {
        grid().any(|t| !curve.transform(t).is_finite())
    });
}

fn declared_monotone_curves_never_decrease() {
    assert_catalog("monotone curve decreases", |curve, shape| {
        shape == Shape::Monotone
            && grid()
                .zip(grid().skip(1))
                .any(|(a, b)| curve.transform(b) < curve.transform(a))
    });
}

#[test]
fn curve_catalog_endpoints_and_monotonicity() {
    crate::run_table(&[
        ("endpoints are exact", endpoints_are_exact),
        ("interior is finite", interior_is_finite),
        (
            "declared monotone curves never decrease",
            declared_monotone_curves_never_decrease,
        ),
    ]);
}

fn nan_maps_to_nan() {
    assert_catalog("transform(NaN) is not NaN", |curve, _| {
        !curve.transform(f64::NAN).is_nan()
    });
}

fn below_zero_is_the_start_value() {
    assert_catalog("t < 0 differs from transform(0)", |curve, _| {
        [-1e-9, -0.5, -1e300, f64::NEG_INFINITY]
            .into_iter()
            .any(|t| curve.transform(t).to_bits() != curve.transform(0.0).to_bits())
    });
}

fn above_one_is_the_end_value() {
    assert_catalog("t > 1 differs from transform(1)", |curve, _| {
        [1.0 + 1e-9, 1.5, 1e300, f64::INFINITY]
            .into_iter()
            .any(|t| curve.transform(t).to_bits() != curve.transform(1.0).to_bits())
    });
}

#[test]
fn curve_input_policy_for_nan_and_out_of_range() {
    crate::run_table(&[
        ("NaN maps to NaN", nan_maps_to_nan),
        (
            "below zero is the start value",
            below_zero_is_the_start_value,
        ),
        ("above one is the end value", above_one_is_the_end_value),
    ]);
}

// ---------------------------------------------------------------------------
// Cubic-bezier accuracy
// ---------------------------------------------------------------------------

/// Bernstein form of one coordinate of a unit cubic bezier.
fn bezier(s: f64, p1: f64, p2: f64) -> f64 {
    let r = 1.0 - s;
    3.0 * r * r * s * p1 + 3.0 * r * s * s * p2 + s * s * s
}

/// y(x) by 200 bisection steps on the bezier parameter — independent of the
/// production solver.
fn reference_y(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..200 {
        let mid = f64::midpoint(lo, hi);
        if bezier(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier(f64::midpoint(lo, hi), y1, y2)
}

const CSS_XS: [f64; 5] = [0.1, 0.25, 0.5, 0.75, 0.9];

/// Asserts `curve` against y values from the CSS Easing reference table.
fn assert_css(curve: &dyn Curve, expected: [f64; 5]) {
    for (x, want) in CSS_XS.into_iter().zip(expected) {
        let got = curve.transform(x);
        assert!(
            (got - want).abs() < 1e-6,
            "y({x}) = {got}, reference {want}, |dy| = {}",
            (got - want).abs()
        );
    }
}

fn css_ease() {
    assert_css(
        &Curves::Ease,
        [
            0.094_796_305_716_043_2,
            0.408_510_591_355_396,
            0.802_403_387_584_857,
            0.960_458_978_348_974,
            0.994_316_477_484_557,
        ],
    );
}

fn css_ease_in() {
    assert_css(
        &Curves::EaseIn,
        [
            0.017_026_609_651_562_9,
            0.093_464_650_718_824_8,
            0.315_356_812_572_539,
            0.621_861_869_174_890,
            0.839_427_845_762_466,
        ],
    );
}

fn css_ease_out() {
    assert_css(
        &Curves::EaseOut,
        [
            0.160_572_154_237_533,
            0.378_138_130_825_110,
            0.684_643_187_427_461,
            0.906_535_349_281_175,
            0.982_973_390_348_437,
        ],
    );
}

fn css_ease_in_out() {
    assert_css(
        &Curves::EaseInOut,
        [
            0.019_722_453_548_311_2,
            0.129_161_931_047_320,
            0.5,
            0.870_838_068_952_680,
            0.980_277_546_451_689,
        ],
    );
}

fn steep_start_cubic() {
    assert_css(
        &Cubic::new(0.05, 0.7, 0.1, 1.0),
        [
            0.621_384_395_577_187,
            0.831_529_746_487_112,
            0.950_247_475_324_022,
            0.990_510_956_928_150,
            0.998_666_345_989_190,
        ],
    );
}

/// `EaseInOutExpo` is `(1, 0, 0, 1)`: x(s) = 4(s - 0.5)^3 + 0.5 has a vertical
/// tangent at s = 0.5, so a solver that stops on the x residual is far off
/// in y there. Analytic inverse: s = 0.5 + cbrt((x - 0.5) / 4),
/// y = 3s^2 - 2s^3.
fn ease_in_out_expo_next_to_its_vertical_tangent() {
    for dx in [1e-6_f64, -1e-6, 9e-7, -9e-7] {
        let x = 0.5 + dx;
        let s = 0.5 + ((x - 0.5) / 4.0).cbrt();
        let want = 3.0 * s * s - 2.0 * s * s * s;
        let got = Curves::EaseInOutExpo.transform(x);
        assert!(
            (got - want).abs() < 1e-6,
            "y({x}) = {got}, analytic {want}, |dy| = {}",
            (got - want).abs()
        );
    }
}

#[test]
fn cubic_bezier_matches_css_reference_values() {
    crate::run_table(&[
        ("ease", css_ease),
        ("ease-in", css_ease_in),
        ("ease-out", css_ease_out),
        ("ease-in-out", css_ease_in_out),
        ("cubic-bezier(0.05, 0.7, 0.1, 1)", steep_start_cubic),
        (
            "ease-in-out-expo next to its vertical tangent",
            ease_in_out_expo_next_to_its_vertical_tangent,
        ),
    ]);
}

proptest! {
    #[test]
    fn cubic_solver_bounds_output_error(
        x1 in 0.0..=1.0_f64,
        y1 in -2.0..3.0_f64,
        x2 in 0.0..=1.0_f64,
        y2 in -2.0..3.0_f64,
        x in 0.0..=1.0_f64,
    ) {
        let got = Cubic::new(x1, y1, x2, y2).transform(x);
        let want = reference_y(x1, y1, x2, y2, x);
        prop_assert!((got - want).abs() < 1e-6, "y({x}) = {got}, reference {want}");
    }

    #[test]
    fn monotone_cubics_keep_endpoints_and_never_decrease(
        x1 in 0.0..=1.0_f64,
        y1 in 0.0..=1.0_f64,
        x2 in 0.0..=1.0_f64,
        y2 in 0.0..=1.0_f64,
        a in 0.0..=1.0_f64,
        b in 0.0..=1.0_f64,
    ) {
        let curve = Cubic::new(x1, y1, x2, y2);
        prop_assert_eq!(curve.transform(0.0).to_bits(), 0.0_f64.to_bits());
        prop_assert_eq!(curve.transform(1.0).to_bits(), 1.0_f64.to_bits());
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        // The solver's output error bound is the only slack allowed.
        prop_assert!(curve.transform(lo) <= curve.transform(hi) + 2e-7);
    }
}

// ---------------------------------------------------------------------------
// Elastic continuity
// ---------------------------------------------------------------------------

/// The Penner elastic-out with its un-normalized `2^(-10 t)` envelope.
fn penner_elastic_out(period: f64, t: f64) -> f64 {
    let s = period / 4.0;
    2.0_f64.powf(-10.0 * t) * ((t - s) * std::f64::consts::TAU / period).sin() + 1.0
}

fn elastic_out_reaches_one_without_a_jump() {
    let near = ElasticOutCurve::new(0.4).transform(1.0 - 1e-12);
    assert!(
        (near - 1.0).abs() < 1e-9,
        "elastic out at 1 - 1e-12 = {near}"
    );
}

fn elastic_in_leaves_zero_without_a_jump() {
    let near = ElasticInCurve::new(0.4).transform(1e-12);
    assert!(near.abs() < 1e-9, "elastic in at 1e-12 = {near}");
}

fn elastic_in_out_is_continuous_at_both_ends() {
    let curve = ElasticInOutCurve::new(0.4);
    let start = curve.transform(1e-12);
    let end = curve.transform(1.0 - 1e-12);
    assert!(start.abs() < 1e-9, "elastic in-out at 1e-12 = {start}");
    assert!(
        (end - 1.0).abs() < 1e-9,
        "elastic in-out at 1 - 1e-12 = {end}"
    );
}

/// Removing the jump reshapes the curve by less than the jump itself.
fn elastic_shape_moves_less_than_the_removed_jump() {
    for period in [0.3, 0.4, 0.75] {
        let curve = ElasticOutCurve::new(period);
        for t in grid() {
            let moved = (curve.transform(t) - penner_elastic_out(period, t)).abs();
            assert!(
                moved <= 2.0_f64.powi(-10),
                "period {period}, t {t}: {moved}"
            );
        }
    }
}

#[test]
fn elastic_curves_are_continuous_at_endpoints() {
    crate::run_table(&[
        (
            "elastic out reaches one without a jump",
            elastic_out_reaches_one_without_a_jump,
        ),
        (
            "elastic in leaves zero without a jump",
            elastic_in_leaves_zero_without_a_jump,
        ),
        (
            "elastic in-out is continuous at both ends",
            elastic_in_out_is_continuous_at_both_ends,
        ),
        (
            "elastic shape moves less than the removed jump",
            elastic_shape_moves_less_than_the_removed_jump,
        ),
    ]);
}

// ---------------------------------------------------------------------------
// ArcCurve equality
// ---------------------------------------------------------------------------

fn equal_builtin_curves_wrapped_separately_are_equal() {
    assert_eq!(ArcCurve::new(Curves::EaseIn), ArcCurve::new(Curves::EaseIn));
    assert_eq!(
        ArcCurve::new(Cubic::new(0.42, 0.0, 1.0, 1.0)),
        ArcCurve::new(Curves::EaseIn),
    );
    assert_eq!(ArcCurve::new(Linear), ArcCurve::new(Curves::Linear));
    assert_eq!(
        ArcCurve::new(ElasticOutCurve::new(0.3)),
        ArcCurve::new(ElasticOutCurve::new(0.3)),
    );
    assert_eq!(
        ArcCurve::new(Curves::EaseInOutCubicEmphasized),
        ArcCurve::new(Curves::EaseInOutCubicEmphasized),
    );
    assert_eq!(
        ArcCurve::new(BounceInCurve),
        ArcCurve::new(Curves::BounceIn)
    );
}

fn different_builtin_curves_are_unequal() {
    assert_ne!(
        ArcCurve::new(Curves::EaseIn),
        ArcCurve::new(Curves::EaseOut)
    );
    assert_ne!(
        ArcCurve::new(ElasticOutCurve::new(0.3)),
        ArcCurve::new(ElasticOutCurve::new(0.4)),
    );
    assert_ne!(
        ArcCurve::new(ElasticInCurve::new(0.4)),
        ArcCurve::new(ElasticOutCurve::new(0.4)),
    );
    assert_ne!(
        ArcCurve::new(Curves::Linear),
        ArcCurve::new(Curves::Decelerate)
    );
}

fn combinators_of_builtins_compare_by_value() {
    assert_eq!(
        ArcCurve::new(Curves::EaseIn.flipped()),
        ArcCurve::new(Curves::EaseIn.flipped()),
    );
    assert_ne!(
        ArcCurve::new(Curves::EaseIn.flipped()),
        ArcCurve::new(Curves::EaseIn),
    );
    assert_eq!(
        ArcCurve::new(Interval::new(0.2, 0.8, Curves::Ease)),
        ArcCurve::new(Interval::new(0.2, 0.8, Curves::Ease)),
    );
    assert_ne!(
        ArcCurve::new(Interval::new(0.2, 0.8, Curves::Ease)),
        ArcCurve::new(Interval::new(0.2, 0.9, Curves::Ease)),
    );
    // Re-wrapping an erased built-in keeps its value identity.
    assert_eq!(
        ArcCurve::new(ArcCurve::new(Curves::EaseIn)),
        ArcCurve::new(Curves::EaseIn),
    );
}

/// A curve the crate does not know.
struct Quadratic;

impl Curve for Quadratic {
    fn transform(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        t * t
    }
}

fn equal_steps_compare_by_value() {
    let steps = || ArcCurve::new(Steps::new(4, JumpAt::End));
    assert_eq!(steps(), steps());
    assert_ne!(steps(), ArcCurve::new(Steps::new(4, JumpAt::Start)));
}

/// A step has no finite derivative at a jump: its slope is 0, also once
/// erased, never a difference across the jump.
fn steps_slope_is_zero_through_erasure() {
    let steps = Steps::new(4, JumpAt::End);
    for t in [0.0, 0.25, 0.3, 1.0] {
        assert_eq!(steps.slope(t), 0.0, "Steps at {t}");
        assert_eq!(ArcCurve::new(steps).slope(t), 0.0, "erased Steps at {t}");
    }
    assert!(steps.slope(f64::NAN).is_nan());
}

/// Linear in value but declaring slope 2: erasure keeps the declaration.
struct DeclaredSlope;

impl Curve for DeclaredSlope {
    fn transform(&self, t: f64) -> f64 {
        t.clamp(0.0, 1.0)
    }

    fn slope(&self, _t: f64) -> f64 {
        2.0
    }
}

fn erased_custom_curve_keeps_its_slope() {
    assert_eq!(ArcCurve::new(DeclaredSlope).slope(0.5), 2.0);
}

fn custom_curves_compare_by_identity() {
    let custom = ArcCurve::new(Quadratic);
    assert_eq!(custom, custom.clone());
    assert_ne!(ArcCurve::new(Quadratic), ArcCurve::new(Quadratic));
    assert_ne!(ArcCurve::new(Quadratic), ArcCurve::new(Curves::EaseIn));
    // A combinator over a custom curve is opaque too.
    assert_ne!(
        ArcCurve::new(Quadratic.flipped()),
        ArcCurve::new(Quadratic.flipped()),
    );
}

fn erased_curves_evaluate_like_the_curve() {
    let pairs: [(ArcCurve, &dyn Curve); 4] = [
        (ArcCurve::new(Curves::EaseIn), &Curves::EaseIn),
        (
            ArcCurve::new(Interval::new(0.2, 0.8, Curves::Ease)),
            &Interval::new(0.2, 0.8, Curves::Ease),
        ),
        (
            ArcCurve::new(Curves::ElasticOut.flipped()),
            &Curves::ElasticOut.flipped(),
        ),
        (ArcCurve::new(Quadratic), &Quadratic),
    ];
    for (erased, curve) in pairs {
        for t in grid().step_by(97) {
            assert_eq!(erased.transform(t).to_bits(), curve.transform(t).to_bits());
        }
    }
}

#[test]
fn arc_curve_compares_builtins_by_value_and_custom_curves_by_identity() {
    crate::run_table(&[
        (
            "equal built-in curves wrapped separately are equal",
            equal_builtin_curves_wrapped_separately_are_equal,
        ),
        (
            "different built-in curves are unequal",
            different_builtin_curves_are_unequal,
        ),
        (
            "combinators of built-ins compare by value",
            combinators_of_builtins_compare_by_value,
        ),
        (
            "custom curves compare by identity",
            custom_curves_compare_by_identity,
        ),
        (
            "erased curves evaluate like the curve",
            erased_curves_evaluate_like_the_curve,
        ),
        ("equal steps compare by value", equal_steps_compare_by_value),
        (
            "steps slope is zero through erasure",
            steps_slope_is_zero_through_erasure,
        ),
        (
            "erased custom curve keeps its slope",
            erased_custom_curve_keeps_its_slope,
        ),
    ]);
}

// ---------------------------------------------------------------------------
// Slope
// ---------------------------------------------------------------------------

/// `cubic-bezier(1/3, 0, 2/3, 1/3)` has x(s) = s and y(s) = s², so its
/// output is exactly x² with derivative 2x.
fn square() -> Cubic {
    Cubic::new(1.0 / 3.0, 0.0, 2.0 / 3.0, 1.0 / 3.0)
}

/// `cubic-bezier(1/3, 0, 2/3, 0)`: y = x³, derivative 3x².
fn cube() -> Cubic {
    Cubic::new(1.0 / 3.0, 0.0, 2.0 / 3.0, 0.0)
}

fn assert_close(what: &str, got: f64, want: f64, tolerance: f64) {
    assert!(
        (got - want).abs() <= tolerance,
        "{what}: slope {got}, expected {want}"
    );
}

fn cubic_slope_matches_polynomial_derivatives() {
    for t in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
        assert_close(&format!("x^2 at {t}"), square().slope(t), 2.0 * t, 1e-6);
        assert_close(&format!("x^3 at {t}"), cube().slope(t), 3.0 * t * t, 1e-6);
    }
}

/// Central differences of the independent bisection reference.
fn cubic_slope_matches_differences_of_the_css_reference() {
    let h = 1e-5;
    for (x1, y1, x2, y2) in [
        (0.25, 0.1, 0.25, 1.0),
        (0.42, 0.0, 1.0, 1.0),
        (0.0, 0.0, 0.58, 1.0),
        (0.42, 0.0, 0.58, 1.0),
        (0.05, 0.7, 0.1, 1.0),
    ] {
        let curve = Cubic::new(x1, y1, x2, y2);
        for x in CSS_XS {
            let want = (reference_y(x1, y1, x2, y2, x + h) - reference_y(x1, y1, x2, y2, x - h))
                / (2.0 * h);
            assert_close(
                &format!("cubic-bezier({x1}, {y1}, {x2}, {y2}) at {x}"),
                curve.slope(x),
                want,
                1e-4,
            );
        }
    }
}

/// Next to `EaseInOutExpo`'s vertical tangent a `1e-4`-step difference
/// straddles the tangent and is off by a large factor; the exact cubic
/// derivative is not. Analytic: s = 0.5 + cbrt((x − 0.5)/4),
/// dy/dx = y'(s)/x'(s) = 6s(1 − s) / (12 (s − 0.5)²). One ulp either side of
/// the tangent the solved `s` is within the solver's tolerance, but `x'(s)`
/// there is rounding rather than slope; the slope still matches within 0.1 %.
fn cubic_slope_is_exact_where_a_difference_is_not() {
    for dx in [
        1e-6_f64,
        -1e-6,
        1e-5,
        f64::EPSILON / 2.0,
        -f64::EPSILON / 4.0,
    ] {
        let x = 0.5 + dx;
        let s = 0.5 + ((x - 0.5) / 4.0).cbrt();
        let want = 6.0 * s * (1.0 - s) / (12.0 * (s - 0.5) * (s - 0.5));
        assert_close(
            &format!("next to the vertical tangent at {x}"),
            Curves::EaseInOutExpo.slope(x),
            want,
            1e-3 * want,
        );
    }
}

/// Near an endpoint whose `x'` has no stationary point inside [0, 1] the
/// quadratic term of `x` dominates and must not be dropped:
/// `Cubic(0, 1/3, 0.1, 2/3)` has x(s) = 0.7s³ + 0.3s², y(s) = s, so
/// dy/dx = 1 / (2.1s² + 0.6s) at the parameter solved by bisection.
fn cubic_slope_near_an_end_keeps_the_quadratic_term() {
    let curve = Cubic::new(0.0, 1.0 / 3.0, 0.1, 2.0 / 3.0);
    for x in [1e-16_f64, 1e-12, 1e-9] {
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for _ in 0..200 {
            let mid = f64::midpoint(lo, hi);
            if bezier(mid, 0.0, 0.1) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let s = f64::midpoint(lo, hi);
        let want = 1.0 / (2.1 * s * s + 0.6 * s);
        assert_close(
            &format!("near the start at {x}"),
            curve.slope(x),
            want,
            1e-3 * want,
        );
    }
}

fn default_difference_is_second_order() {
    // Exact for a quadratic, including the one-sided ends.
    for t in [0.0, 1e-5, 0.3, 1.0 - 1e-5, 1.0] {
        assert_close(
            &format!("quadratic at {t}"),
            Quadratic.slope(t),
            2.0 * t,
            1e-8,
        );
    }
}

fn linear_interval_and_flipped_use_the_chain_rule() {
    for t in [0.0, 0.5, 1.0] {
        assert_close("linear", Curves::Linear.slope(t), 1.0, 0.0);
    }
    let interval = Interval::new(0.2, 0.6, square());
    assert_close(
        "interval inside",
        interval.slope(0.4),
        2.0 * 0.5 / 0.4,
        1e-6,
    );
    assert_close("interval before", interval.slope(0.1), 0.0, 0.0);
    assert_close("interval after", interval.slope(0.8), 0.0, 0.0);
    let flipped = square().flipped();
    assert_close("flipped", flipped.slope(0.3), 2.0 * 0.7, 1e-6);
    assert_close("erased", ArcCurve::new(square()).slope(0.3), 0.6, 1e-6);
    assert_close(
        "erased custom",
        ArcCurve::new(Quadratic).slope(0.3),
        0.6,
        1e-8,
    );
}

fn slope_input_policy() {
    assert_catalog("slope(NaN) is not NaN", |curve, _| {
        !curve.slope(f64::NAN).is_nan()
    });
    assert_catalog("slope outside [0, 1] is not 0", |curve, _| {
        [-0.5, 1.5, f64::INFINITY, f64::NEG_INFINITY]
            .into_iter()
            .any(|t| curve.slope(t).to_bits() != 0.0_f64.to_bits())
    });
    assert_catalog("non-finite slope inside [0, 1]", |curve, _| {
        grid().any(|t| !curve.slope(t).is_finite())
    });
}

/// A small but nonzero `x'(0)` is not a degenerate start: the slope is
/// `y'(0) / x'(0)`, here exactly 1, not the curvature ratio (about 2).
fn small_nonzero_endpoint_derivative_is_the_ratio() {
    let curve = Cubic::new(1e-10, 1e-10, 0.5, 1.0);
    assert_close("cubic slope at 0", curve.slope(0.0), 1.0, 1e-6);
}

/// A slope of `1e307` is finite, but rescaled into a `1e-5`-wide interval
/// the chain rule overflows; the interval still reports a finite slope.
#[derive(Clone, Copy)]
struct Steep;

impl Curve for Steep {
    fn transform(&self, t: f64) -> f64 {
        t.clamp(0.0, 1.0)
    }

    fn slope(&self, _t: f64) -> f64 {
        1e307
    }
}

fn narrow_interval_of_a_steep_curve_has_a_finite_slope() {
    let interval = Interval::new(0.5, 0.5 + 1e-5, Steep);
    for t in [0.5, 0.5 + 5e-6, 0.5 + 1e-5] {
        let slope = interval.slope(t);
        assert!(slope.is_finite() && slope > 0.0, "slope at {t}: {slope}");
    }
}

fn vertical_tangent_slope_is_finite_and_steep() {
    let slope = Curves::EaseInOutExpo.slope(0.5);
    assert!(
        slope.is_finite() && slope > 10.0,
        "slope at the vertical tangent: {slope}"
    );
}

#[test]
fn curve_slope_is_the_derivative_of_transform() {
    crate::run_table(&[
        (
            "nonpositive cubic endpoint slope",
            nonpositive_cubic_endpoint_slope,
        ),
        (
            "cubic slope matches polynomial derivatives",
            cubic_slope_matches_polynomial_derivatives,
        ),
        (
            "cubic slope matches differences of the css reference",
            cubic_slope_matches_differences_of_the_css_reference,
        ),
        (
            "cubic slope is exact where a difference is not",
            cubic_slope_is_exact_where_a_difference_is_not,
        ),
        (
            "cubic slope near an end keeps the quadratic term",
            cubic_slope_near_an_end_keeps_the_quadratic_term,
        ),
        (
            "default difference is second order",
            default_difference_is_second_order,
        ),
        (
            "linear, interval and flipped use the chain rule",
            linear_interval_and_flipped_use_the_chain_rule,
        ),
        ("slope input policy", slope_input_policy),
        (
            "vertical tangent slope is finite and steep",
            vertical_tangent_slope_is_finite_and_steep,
        ),
        (
            "small nonzero endpoint derivative is the ratio",
            small_nonzero_endpoint_derivative_is_the_ratio,
        ),
        (
            "narrow interval of a steep curve has a finite slope",
            narrow_interval_of_a_steep_curve_has_a_finite_slope,
        ),
    ]);
}

fn nonpositive_cubic_endpoint_slope() {
    let curve = Cubic::new(0.0, 1.0 / 3.0, 1.0, 2.0 / 3.0);
    // x(s) = 3s² - 2s³, y(s) = s; solve x independently by bisection.
    for x in [1e-16_f64, 1e-12, 1e-8] {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..200 {
            let s = f64::midpoint(lo, hi);
            if s * s * (3.0 - 2.0 * s) < x {
                lo = s;
            } else {
                hi = s;
            }
        }
        let s = f64::midpoint(lo, hi);
        assert_close(
            "near endpoint",
            curve.slope(x),
            1.0 / (6.0 * s * (1.0 - s)),
            1e-6,
        );
    }
}

// ---------------------------------------------------------------------------
// Parameter validation
// ---------------------------------------------------------------------------

/// Asserts that `make` panics, without running the payload's destructor
/// outside the test's control.
fn assert_panics<T>(what: &str, make: impl FnOnce() -> T + std::panic::UnwindSafe) {
    let outcome = std::panic::catch_unwind(make);
    assert!(outcome.is_err(), "{what}: `new` accepted invalid input");
}

fn out_of_range(error: Result<impl Sized, CurveError>, name: &str) {
    assert!(
        matches!(error, Err(CurveError::OutOfRange { parameter, .. }) if parameter == name),
        "expected `{name}` out of range"
    );
}

fn non_finite(error: Result<impl Sized, CurveError>, name: &str) {
    assert!(
        matches!(error, Err(CurveError::NonFinite { parameter }) if parameter == name),
        "expected `{name}` not finite"
    );
}

fn cubic_x_outside_the_unit_interval() {
    out_of_range(Cubic::try_new(1.5, 0.0, 0.5, 1.0), "x1");
    out_of_range(Cubic::try_new(0.5, 0.0, -0.1, 1.0), "x2");
    assert_panics("Cubic x1", || Cubic::new(1.5, 0.0, 0.5, 1.0));
    assert_panics("Cubic x2", || Cubic::new(0.5, 0.0, -0.1, 1.0));
}

fn cubic_non_finite_control_point() {
    non_finite(Cubic::try_new(f64::NAN, 0.0, 0.5, 1.0), "x1");
    non_finite(Cubic::try_new(0.5, f64::INFINITY, 0.5, 1.0), "y1");
    non_finite(Cubic::try_new(0.5, 0.0, 0.5, f64::NEG_INFINITY), "y2");
    assert_panics("Cubic y1", || Cubic::new(0.5, f64::NAN, 0.5, 1.0));
}

fn cubic_overshooting_y_is_admitted() {
    assert!(Cubic::try_new(0.68, -0.55, 0.265, 1.55).is_ok());
}

/// Every sample of `curve` over `[0, 1]`, value and slope, is finite.
fn finite_everywhere(what: &str, curve: &dyn Curve) {
    for t in grid() {
        let (value, slope) = (curve.transform(t), curve.slope(t));
        assert!(
            value.is_finite() && slope.is_finite(),
            "{what} at {t}: value {value}, slope {slope}"
        );
    }
}

/// Past `1e6` the solver's coefficients overflow: `f64::MAX` evaluated to
/// NaN. The admitted extremes evaluate finitely.
fn cubic_y_beyond_the_admitted_range() {
    out_of_range(Cubic::try_new(0.25, f64::MAX, 0.75, f64::MAX), "y1");
    out_of_range(Cubic::try_new(0.25, 0.0, 0.75, -1.5e6), "y2");
    assert_panics("Cubic y1", || Cubic::new(0.25, f64::MAX, 0.75, 1.0));
    finite_everywhere("Cubic(y = ±1e6)", &Cubic::new(0.25, 1e6, 0.75, -1e6));
}

/// A huge period overflowed the phase product and a tiny one its quotient,
/// both to NaN. The admitted extremes evaluate finitely.
fn elastic_period_beyond_the_admitted_range() {
    out_of_range(ElasticOutCurve::try_new(f64::MAX), "period");
    out_of_range(ElasticInCurve::try_new(1e-300), "period");
    for period in [1e-6, 1e6] {
        finite_everywhere("ElasticOut", &ElasticOutCurve::new(period));
        finite_everywhere("ElasticInOut", &ElasticInOutCurve::new(period));
    }
}

/// A control y past the admitted range is blamed on itself, not on a central
/// midpoint; the extreme admitted y rescales and evaluates finitely.
fn three_point_control_y_beyond_the_admitted_range() {
    let huge = ThreePointCubic::try_new(
        (0.1, f64::MAX),
        (0.2, 0.5),
        (0.5, 0.5),
        (0.6, 1.0),
        (0.7, 1.0),
    );
    out_of_range(huge, "a1.y");
    let extreme =
        ThreePointCubic::new((0.1, 1e6), (0.2, -1e6), (0.5, 0.5), (0.6, 1e6), (0.7, -1e6));
    finite_everywhere("ThreePointCubic(y = ±1e6)", &extreme);
}

fn three_point_midpoint_on_the_boundary() {
    let midpoint =
        ThreePointCubic::try_new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0));
    out_of_range(midpoint, "midpoint.x");
    let flat = ThreePointCubic::try_new((0.1, 0.0), (0.2, 0.0), (0.5, 0.0), (0.6, 1.0), (0.7, 1.0));
    out_of_range(flat, "midpoint.y");
    assert_panics("ThreePointCubic midpoint", || {
        ThreePointCubic::new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0))
    });
}

fn three_point_control_outside_its_segment() {
    let first =
        ThreePointCubic::try_new((0.6, 0.0), (0.2, 0.0), (0.5, 0.5), (0.6, 1.0), (0.7, 1.0));
    out_of_range(first, "a1.x");
    let second =
        ThreePointCubic::try_new((0.1, 0.0), (0.2, 0.0), (0.5, 0.5), (0.4, 1.0), (0.7, 1.0));
    out_of_range(second, "a2.x");
    non_finite(
        ThreePointCubic::try_new(
            (0.1, 0.0),
            (0.2, f64::NAN),
            (0.5, 0.5),
            (0.6, 1.0),
            (0.7, 1.0),
        ),
        "b1.y",
    );
    assert_panics("ThreePointCubic a2.x", || {
        ThreePointCubic::new((0.1, 0.0), (0.2, 0.0), (0.5, 0.5), (0.4, 1.0), (0.7, 1.0))
    });
}

fn interval_bounds() {
    out_of_range(Interval::try_new(-0.1, 0.5, Linear), "begin");
    out_of_range(Interval::try_new(0.2, 1.5, Linear), "end");
    non_finite(Interval::try_new(f64::NAN, 0.5, Linear), "begin");
    assert!(matches!(
        Interval::try_new(0.6, 0.4, Linear),
        Err(CurveError::IntervalReversed { .. })
    ));
    assert_panics("Interval reversed", || Interval::new(0.6, 0.4, Linear));
}

fn elastic_period() {
    out_of_range(ElasticInCurve::try_new(0.0), "period");
    out_of_range(ElasticOutCurve::try_new(-0.4), "period");
    non_finite(ElasticInOutCurve::try_new(f64::INFINITY), "period");
    assert_panics("ElasticIn period", || ElasticInCurve::new(0.0));
    assert_panics("ElasticOut period", || ElasticOutCurve::new(f64::NAN));
    assert_panics("ElasticInOut period", || ElasticInOutCurve::new(-1.0));
}

fn error_messages_name_the_parameter() {
    assert_eq!(
        Cubic::try_new(1.5, 0.0, 0.5, 1.0)
            .expect_err("x1")
            .to_string(),
        "curve parameter `x1` = 1.5 is outside [0, 1]"
    );
    assert_eq!(
        ElasticOutCurve::try_new(f64::NAN)
            .expect_err("period")
            .to_string(),
        "curve parameter `period` is not finite"
    );
    assert_eq!(
        Interval::try_new(0.6, 0.4, Linear)
            .expect_err("reversed")
            .to_string(),
        "interval begin 0.6 is after end 0.4"
    );
}

#[test]
fn curve_parameters_reject_invalid_input() {
    crate::run_table(&[
        (
            "cubic x outside the unit interval",
            cubic_x_outside_the_unit_interval,
        ),
        (
            "cubic non-finite control point",
            cubic_non_finite_control_point,
        ),
        (
            "cubic overshooting y is admitted",
            cubic_overshooting_y_is_admitted,
        ),
        (
            "three-point midpoint on the boundary",
            three_point_midpoint_on_the_boundary,
        ),
        (
            "three-point control outside its segment",
            three_point_control_outside_its_segment,
        ),
        ("interval bounds", interval_bounds),
        (
            "cubic y beyond the admitted range",
            cubic_y_beyond_the_admitted_range,
        ),
        (
            "elastic period beyond the admitted range",
            elastic_period_beyond_the_admitted_range,
        ),
        (
            "three-point control y beyond the admitted range",
            three_point_control_y_beyond_the_admitted_range,
        ),
        ("elastic period", elastic_period),
        (
            "error messages name the parameter",
            error_messages_name_the_parameter,
        ),
    ]);
}

// ---------------------------------------------------------------------------
// Serde
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
fn round_trips<T>(value: &T, wire: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).expect("serialize");
    assert_eq!(text, wire, "wire format");
    let back: T = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(&back, value);
}

#[cfg(feature = "serde")]
fn rejects<T: serde::de::DeserializeOwned + std::fmt::Debug>(wire: &str, error: &CurveError) {
    let message = serde_json::from_str::<T>(wire).expect_err(wire).to_string();
    assert!(
        message.starts_with(&error.to_string()),
        "decoding {wire}: `{message}` does not carry `{error}`"
    );
}

#[cfg(feature = "serde")]
fn cubic_wire() {
    round_trips(
        &Cubic::new(0.25, 0.1, 0.25, 1.0),
        r#"{"a":0.25,"b":0.1,"c":0.25,"d":1.0}"#,
    );
    rejects::<Cubic>(
        r#"{"a":1.5,"b":0.1,"c":0.25,"d":1.0}"#,
        &Cubic::try_new(1.5, 0.1, 0.25, 1.0).expect_err("x1"),
    );
}

#[cfg(feature = "serde")]
fn three_point_wire() {
    round_trips(
        &Curves::EaseInOutCubicEmphasized,
        r#"{"a1":[0.05,0.0],"b1":[0.133333,0.06],"midpoint":[0.166666,0.4],"a2":[0.208333,0.82],"b2":[0.25,1.0]}"#,
    );
    rejects::<ThreePointCubic>(
        r#"{"a1":[0.0,0.0],"b1":[0.0,0.0],"midpoint":[1.0,0.5],"a2":[1.0,1.0],"b2":[1.0,1.0]}"#,
        &ThreePointCubic::try_new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0))
            .expect_err("midpoint"),
    );
}

#[cfg(feature = "serde")]
fn interval_wire() {
    round_trips(
        &Interval::new(0.2, 0.8, Linear),
        r#"{"begin":0.2,"end":0.8,"curve":null}"#,
    );
    rejects::<Interval>(
        r#"{"begin":0.8,"end":0.2,"curve":null}"#,
        &Interval::try_new(0.8, 0.2, Linear).expect_err("reversed"),
    );
}

#[cfg(feature = "serde")]
fn elastic_wire() {
    round_trips(&ElasticOutCurve::new(0.3), r#"{"period":0.3}"#);
    round_trips(&ElasticInCurve::new(0.3), r#"{"period":0.3}"#);
    round_trips(&ElasticInOutCurve::new(0.3), r#"{"period":0.3}"#);
    rejects::<ElasticInCurve>(
        r#"{"period":0.0}"#,
        &ElasticInCurve::try_new(0.0).expect_err("period"),
    );
}

#[cfg(feature = "serde")]
fn steps_wire() {
    round_trips(
        &Steps::new(4, JumpAt::Start),
        r#"{"count":4,"jump":"Start"}"#,
    );
    rejects::<Steps>(
        r#"{"count":1,"jump":"None"}"#,
        &Steps::try_new(1, JumpAt::None).expect_err("too few"),
    );
    rejects::<Steps>(
        r#"{"count":0,"jump":"End"}"#,
        &Steps::try_new(0, JumpAt::End).expect_err("zero"),
    );
}

#[cfg(feature = "serde")]
#[test]
fn curve_serde_round_trip_keeps_the_wire_format() {
    crate::run_table(&[
        ("cubic", cubic_wire),
        ("three-point cubic", three_point_wire),
        ("interval", interval_wire),
        ("elastic", elastic_wire),
        ("steps", steps_wire),
    ]);
}

// ---- steps ------------------------------------------------------------------

/// CSS Easing 1 §2.3.1 `steps()` values, before flag unset, computed by hand.
fn css_steps_values() {
    let rows: [(Steps, f64, f64); 14] = [
        (Steps::new(4, JumpAt::End), 0.24, 0.0),
        (Steps::new(4, JumpAt::End), 0.25, 0.25),
        (Steps::new(4, JumpAt::End), 0.99, 0.75),
        (Steps::new(4, JumpAt::Start), 0.1, 0.25),
        (Steps::new(4, JumpAt::Start), 0.75, 1.0),
        (Steps::new(5, JumpAt::None), 0.1, 0.0),
        (Steps::new(5, JumpAt::None), 0.2, 0.25),
        (Steps::new(5, JumpAt::None), 0.85, 1.0),
        (Steps::new(3, JumpAt::Both), 0.1, 0.25),
        (Steps::new(3, JumpAt::Both), 0.4, 0.5),
        (Steps::new(3, JumpAt::Both), 0.99, 0.75),
        (Steps::new(1, JumpAt::Start), 0.001, 1.0),
        (Steps::new(1, JumpAt::End), 0.999, 0.0),
        (Steps::new(8, JumpAt::End), 0.375, 0.375),
    ];
    for (steps, t, expected) in rows {
        assert_eq!(steps.transform(t), expected, "{steps:?} at {t}");
    }
}

fn steps_keep_the_curve_contract() {
    for jump in [JumpAt::Start, JumpAt::End, JumpAt::None, JumpAt::Both] {
        let steps = Steps::new(3, jump);
        assert_eq!(steps.transform(0.0), 0.0, "{jump:?} start");
        assert_eq!(steps.transform(1.0), 1.0, "{jump:?} end");
        assert_eq!(steps.transform(-1.0), 0.0, "{jump:?} below");
        assert_eq!(steps.transform(2.0), 1.0, "{jump:?} above");
        assert!(steps.transform(f64::NAN).is_nan(), "{jump:?} NaN");
    }
}

fn steps_reject_invalid_counts() {
    assert!(matches!(
        Steps::try_new(0, JumpAt::End),
        Err(CurveError::OutOfRange {
            parameter: "count",
            ..
        })
    ));
    assert_eq!(
        Steps::try_new(1, JumpAt::None),
        Err(CurveError::TooFewSteps)
    );
    assert!(Steps::try_new(2, JumpAt::None).is_ok());
}

#[test]
fn steps_follow_css_easing() {
    crate::run_table(&[
        ("CSS values", css_steps_values),
        ("curve contract", steps_keep_the_curve_contract),
        ("invalid counts", steps_reject_invalid_counts),
    ]);
}
