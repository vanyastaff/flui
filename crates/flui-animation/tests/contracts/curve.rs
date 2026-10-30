//! Curve contract: exact endpoints, declared monotonicity, the input policy for
//! NaN and out-of-range progress, cubic-bezier accuracy against independent
//! reference values, and parameter validation however a curve is made.

use flui_animation::{
    Bounce, Cubic, Curve, CurveError, CurveParameter, Ease, Elastic, Interval, JumpAt, Linear,
    SharedCurve, Split, Steps, ThreePointCubic, curves,
};
use proptest::prelude::*;

/// Whether a catalog curve promises to never decrease.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Monotone,
    Overshoots,
}

/// Predefined curves, plus constructed instances of the parameterized curves
/// and combinators. Authored piecewise/spring curves have their own contract table.
fn catalog() -> Vec<(&'static str, SharedCurve, Shape)> {
    use Shape::{Monotone, Overshoots};
    vec![
        ("Linear", SharedCurve::new(curves::LINEAR), Monotone),
        ("EaseIn", SharedCurve::new(curves::EASE_IN), Monotone),
        ("EaseOut", SharedCurve::new(curves::EASE_OUT), Monotone),
        ("EaseInOut", SharedCurve::new(curves::EASE_IN_OUT), Monotone),
        (
            "FastOutSlowIn",
            SharedCurve::new(curves::FAST_OUT_SLOW_IN),
            Monotone,
        ),
        (
            "LinearOutSlowIn",
            SharedCurve::new(curves::LINEAR_OUT_SLOW_IN),
            Monotone,
        ),
        (
            "EaseInOutCubic",
            SharedCurve::new(curves::EASE_IN_OUT_CUBIC),
            Monotone,
        ),
        (
            "EaseInSine",
            SharedCurve::new(curves::EASE_IN_SINE),
            Monotone,
        ),
        (
            "EaseOutSine",
            SharedCurve::new(curves::EASE_OUT_SINE),
            Monotone,
        ),
        (
            "EaseInOutSine",
            SharedCurve::new(curves::EASE_IN_OUT_SINE),
            Monotone,
        ),
        (
            "EaseInExpo",
            SharedCurve::new(curves::EASE_IN_EXPO),
            Monotone,
        ),
        (
            "EaseOutExpo",
            SharedCurve::new(curves::EASE_OUT_EXPO),
            Monotone,
        ),
        (
            "EaseInOutExpo",
            SharedCurve::new(curves::EASE_IN_OUT_EXPO),
            Monotone,
        ),
        (
            "EaseInCirc",
            SharedCurve::new(curves::EASE_IN_CIRC),
            Monotone,
        ),
        (
            "EaseOutCirc",
            SharedCurve::new(curves::EASE_OUT_CIRC),
            Monotone,
        ),
        (
            "EaseInOutCirc",
            SharedCurve::new(curves::EASE_IN_OUT_CIRC),
            Monotone,
        ),
        (
            "EaseInBack",
            SharedCurve::new(curves::EASE_IN_BACK),
            Overshoots,
        ),
        (
            "EaseOutBack",
            SharedCurve::new(curves::EASE_OUT_BACK),
            Overshoots,
        ),
        (
            "EaseInOutBack",
            SharedCurve::new(curves::EASE_IN_OUT_BACK),
            Overshoots,
        ),
        (
            "ElasticIn",
            SharedCurve::new(curves::ELASTIC_IN),
            Overshoots,
        ),
        (
            "ElasticOut",
            SharedCurve::new(curves::ELASTIC_OUT),
            Overshoots,
        ),
        (
            "ElasticInOut",
            SharedCurve::new(curves::ELASTIC_IN_OUT),
            Overshoots,
        ),
        ("BounceIn", SharedCurve::new(curves::BOUNCE_IN), Overshoots),
        (
            "BounceOut",
            SharedCurve::new(curves::BOUNCE_OUT),
            Overshoots,
        ),
        (
            "BounceInOut",
            SharedCurve::new(curves::BOUNCE_IN_OUT),
            Overshoots,
        ),
        ("Decelerate", SharedCurve::new(curves::DECELERATE), Monotone),
        ("Ease", SharedCurve::new(curves::EASE), Monotone),
        (
            "EaseInQuad",
            SharedCurve::new(curves::EASE_IN_QUAD),
            Monotone,
        ),
        (
            "EaseInCubic",
            SharedCurve::new(curves::EASE_IN_CUBIC),
            Monotone,
        ),
        (
            "EaseInQuart",
            SharedCurve::new(curves::EASE_IN_QUART),
            Monotone,
        ),
        (
            "EaseInQuint",
            SharedCurve::new(curves::EASE_IN_QUINT),
            Monotone,
        ),
        (
            "EaseOutQuad",
            SharedCurve::new(curves::EASE_OUT_QUAD),
            Monotone,
        ),
        (
            "EaseOutCubic",
            SharedCurve::new(curves::EASE_OUT_CUBIC),
            Monotone,
        ),
        (
            "EaseOutQuart",
            SharedCurve::new(curves::EASE_OUT_QUART),
            Monotone,
        ),
        (
            "EaseOutQuint",
            SharedCurve::new(curves::EASE_OUT_QUINT),
            Monotone,
        ),
        (
            "EaseInOutQuad",
            SharedCurve::new(curves::EASE_IN_OUT_QUAD),
            Monotone,
        ),
        (
            "EaseInOutQuart",
            SharedCurve::new(curves::EASE_IN_OUT_QUART),
            Monotone,
        ),
        (
            "EaseInOutQuint",
            SharedCurve::new(curves::EASE_IN_OUT_QUINT),
            Monotone,
        ),
        (
            "FastLinearToSlowEaseIn",
            SharedCurve::new(curves::FAST_LINEAR_TO_SLOW_EASE_IN),
            Monotone,
        ),
        (
            "LinearToEaseOut",
            SharedCurve::new(curves::LINEAR_TO_EASE_OUT),
            Monotone,
        ),
        (
            "EaseInToLinear",
            SharedCurve::new(curves::EASE_IN_TO_LINEAR),
            Monotone,
        ),
        (
            "SlowMiddle",
            SharedCurve::new(curves::SLOW_MIDDLE),
            Monotone,
        ),
        (
            "EaseInOutCubicEmphasized",
            SharedCurve::new(curves::EASE_IN_OUT_CUBIC_EMPHASIZED),
            Monotone,
        ),
        (
            "FastEaseInToSlowEaseOut",
            SharedCurve::new(curves::FAST_EASE_IN_TO_SLOW_EASE_OUT),
            Monotone,
        ),
        (
            "Interval(0.2, 0.8, EaseIn)",
            SharedCurve::new(Interval::new(0.2, 0.8, curves::EASE_IN)),
            Monotone,
        ),
        (
            "Interval(0.5, 0.5) step",
            SharedCurve::new(Interval::linear(0.5, 0.5)),
            Monotone,
        ),
        ("Split(0.4)", SharedCurve::new(Split::new(0.4)), Monotone),
        (
            "EaseIn.flipped()",
            SharedCurve::new(curves::EASE_IN.flipped()),
            Monotone,
        ),
        (
            "Elastic(Out, 0.3)",
            SharedCurve::new(Elastic::new(Ease::Out, 0.3)),
            Overshoots,
        ),
        (
            "Cubic(0.05, 0.7, 0.1, 1)",
            SharedCurve::new(Cubic::new(0.05, 0.7, 0.1, 1.0)),
            Monotone,
        ),
        ("Bounce(In)", SharedCurve::new(Bounce(Ease::In)), Overshoots),
    ]
}

/// Progress samples across `[0, 1]`, both ends included.
fn grid() -> impl Iterator<Item = f64> {
    (0..=4000).map(|i| f64::from(i) / 4000.0)
}

/// Runs `violates` over the catalog and fails with every offending name.
fn assert_catalog(property: &str, violates: impl Fn(&SharedCurve, Shape) -> bool) {
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
        &curves::EASE,
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
        &curves::EASE_IN,
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
        &curves::EASE_OUT,
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
        &curves::EASE_IN_OUT,
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
        let got = curves::EASE_IN_OUT_EXPO.transform(x);
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
    let near = Elastic::new(Ease::Out, 0.4).transform(1.0 - 1e-12);
    assert!(
        (near - 1.0).abs() < 1e-9,
        "elastic out at 1 - 1e-12 = {near}"
    );
}

fn elastic_in_leaves_zero_without_a_jump() {
    let near = Elastic::new(Ease::In, 0.4).transform(1e-12);
    assert!(near.abs() < 1e-9, "elastic in at 1e-12 = {near}");
}

fn elastic_in_out_is_continuous_at_both_ends() {
    let curve = Elastic::new(Ease::InOut, 0.4);
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
        let curve = Elastic::new(Ease::Out, period);
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
// SharedCurve equality
// ---------------------------------------------------------------------------

fn equal_builtin_curves_wrapped_separately_are_equal() {
    assert_eq!(
        SharedCurve::new(curves::EASE_IN),
        SharedCurve::new(curves::EASE_IN)
    );
    assert_eq!(
        SharedCurve::new(Cubic::new(0.42, 0.0, 1.0, 1.0)),
        SharedCurve::new(curves::EASE_IN),
    );
    assert_eq!(SharedCurve::new(Linear), SharedCurve::new(curves::LINEAR));
    assert_eq!(
        SharedCurve::new(Elastic::new(Ease::Out, 0.3)),
        SharedCurve::new(Elastic::new(Ease::Out, 0.3)),
    );
    assert_eq!(
        SharedCurve::new(curves::EASE_IN_OUT_CUBIC_EMPHASIZED),
        SharedCurve::new(curves::EASE_IN_OUT_CUBIC_EMPHASIZED),
    );
    assert_eq!(
        SharedCurve::new(Bounce(Ease::In)),
        SharedCurve::new(curves::BOUNCE_IN)
    );
}

fn different_builtin_curves_are_unequal() {
    assert_ne!(
        SharedCurve::new(curves::EASE_IN),
        SharedCurve::new(curves::EASE_OUT)
    );
    assert_ne!(
        SharedCurve::new(Elastic::new(Ease::Out, 0.3)),
        SharedCurve::new(Elastic::new(Ease::Out, 0.4)),
    );
    assert_ne!(
        SharedCurve::new(Elastic::new(Ease::In, 0.4)),
        SharedCurve::new(Elastic::new(Ease::Out, 0.4)),
    );
    assert_ne!(
        SharedCurve::new(curves::LINEAR),
        SharedCurve::new(curves::DECELERATE)
    );
}

fn combinators_of_builtins_compare_by_value() {
    assert_eq!(
        SharedCurve::new(curves::EASE_IN.flipped()),
        SharedCurve::new(curves::EASE_IN.flipped()),
    );
    assert_ne!(
        SharedCurve::new(curves::EASE_IN.flipped()),
        SharedCurve::new(curves::EASE_IN),
    );
    assert_eq!(
        SharedCurve::new(Interval::new(0.2, 0.8, curves::EASE)),
        SharedCurve::new(Interval::new(0.2, 0.8, curves::EASE)),
    );
    assert_ne!(
        SharedCurve::new(Interval::new(0.2, 0.8, curves::EASE)),
        SharedCurve::new(Interval::new(0.2, 0.9, curves::EASE)),
    );
    // Re-wrapping an erased built-in keeps its value identity.
    assert_eq!(
        SharedCurve::new(SharedCurve::new(curves::EASE_IN)),
        SharedCurve::new(curves::EASE_IN),
    );
    // A split of built-in halves is a value, so an implicit animation rebuilt
    // with an equal split keeps its run instead of restarting it.
    assert_eq!(
        SharedCurve::new(Split::new(0.4)),
        SharedCurve::new(Split::new(0.4)),
    );
    assert_ne!(
        SharedCurve::new(Split::new(0.4)),
        SharedCurve::new(Split::new(0.5)),
    );
    assert_ne!(
        SharedCurve::new(Split::with_curves(0.4, Linear, curves::EASE)),
        SharedCurve::new(Split::with_curves(0.4, Linear, curves::EASE_IN)),
    );
    // A user half has no value identity, so the split compares by identity.
    assert_ne!(
        SharedCurve::new(Split::with_curves(0.4, Quadratic, curves::EASE)),
        SharedCurve::new(Split::with_curves(0.4, Quadratic, curves::EASE)),
    );
}

/// An invalid split point is a typed error from the checked constructor.
fn split_try_with_curves_refuses_an_invalid_point() {
    assert_eq!(
        Split::try_with_curves(1.5, Linear, curves::EASE).err(),
        Some(range_error(CurveParameter::Split, 1.5))
    );
    assert_eq!(
        Split::try_with_curves(f64::NAN, Linear, curves::EASE).err(),
        Some(CurveError::NonFinite {
            parameter: CurveParameter::Split,
        })
    );
    let split = Split::try_with_curves(0.25, Linear, curves::EASE).expect("valid split");
    assert_eq!(split.split(), 0.25);
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
    let steps = || SharedCurve::new(Steps::new(4, JumpAt::End));
    assert_eq!(steps(), steps());
    assert_ne!(steps(), SharedCurve::new(Steps::new(4, JumpAt::Start)));
}

/// A step has no finite derivative at a jump: its slope is 0, also once
/// erased, never a difference across the jump.
fn steps_slope_is_zero_through_erasure() {
    let steps = Steps::new(4, JumpAt::End);
    for t in [0.0, 0.25, 0.3, 1.0] {
        assert_eq!(steps.slope(t), 0.0, "Steps at {t}");
        assert_eq!(SharedCurve::new(steps).slope(t), 0.0, "erased Steps at {t}");
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
    assert_eq!(SharedCurve::new(DeclaredSlope).slope(0.5), 2.0);
}

fn custom_curves_compare_by_identity() {
    let custom = SharedCurve::new(Quadratic);
    assert_eq!(custom, custom.clone());
    assert_ne!(SharedCurve::new(Quadratic), SharedCurve::new(Quadratic));
    assert_ne!(
        SharedCurve::new(Quadratic),
        SharedCurve::new(curves::EASE_IN)
    );
    // A combinator over a custom curve is opaque too.
    assert_ne!(
        SharedCurve::new(Quadratic.flipped()),
        SharedCurve::new(Quadratic.flipped()),
    );
}

// The erased handle shares a custom curve through `Rc`, so the curve may hold
// owner-local state, and the handle stays on its thread.
static_assertions::assert_not_impl_any!(SharedCurve: Send, Sync);

/// Squares progress and counts its evaluations in owner-local state.
struct CountingCurve(std::rc::Rc<std::cell::Cell<u32>>);

impl Curve for CountingCurve {
    fn transform(&self, t: f64) -> f64 {
        self.0.set(self.0.get() + 1);
        t * t
    }
}

fn erased_custom_curve_holds_owner_local_state() {
    let evaluations = std::rc::Rc::new(std::cell::Cell::new(0));
    let curve = SharedCurve::new(CountingCurve(evaluations.clone()));
    let clone = curve.clone();
    assert_eq!(clone.transform(0.5), 0.25);
    assert_eq!(evaluations.get(), 1, "the clone evaluates the shared curve");
    assert_eq!(clone, curve, "clones share one curve");
}

fn erased_curves_evaluate_like_the_curve() {
    let pairs: [(SharedCurve, &dyn Curve); 4] = [
        (SharedCurve::new(curves::EASE_IN), &curves::EASE_IN),
        (
            SharedCurve::new(Interval::new(0.2, 0.8, curves::EASE)),
            &Interval::new(0.2, 0.8, curves::EASE),
        ),
        (
            SharedCurve::new(curves::ELASTIC_OUT.flipped()),
            &curves::ELASTIC_OUT.flipped(),
        ),
        (SharedCurve::new(Quadratic), &Quadratic),
    ];
    for (erased, curve) in pairs {
        for t in grid().step_by(97) {
            assert_eq!(erased.transform(t).to_bits(), curve.transform(t).to_bits());
        }
    }
}

#[test]
fn shared_curve_compares_builtins_by_value_and_custom_curves_by_identity() {
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
            "split try_with_curves refuses an invalid point",
            split_try_with_curves_refuses_an_invalid_point,
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
        (
            "erased custom curve holds owner-local state",
            erased_custom_curve_holds_owner_local_state,
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
            curves::EASE_IN_OUT_EXPO.slope(x),
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
        assert_close("linear", curves::LINEAR.slope(t), 1.0, 0.0);
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
    assert_close("erased", SharedCurve::new(square()).slope(0.3), 0.6, 1e-6);
    assert_close(
        "erased custom",
        SharedCurve::new(Quadratic).slope(0.3),
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
    let slope = curves::EASE_IN_OUT_EXPO.slope(0.5);
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

fn out_of_range(error: Result<impl Sized, CurveError>, name: CurveParameter) {
    assert!(
        matches!(error, Err(CurveError::OutOfRange { parameter, .. }) if parameter == name),
        "expected `{name}` out of range"
    );
}

fn non_finite(error: Result<impl Sized, CurveError>, name: CurveParameter) {
    assert!(
        matches!(error, Err(CurveError::NonFinite { parameter }) if parameter == name),
        "expected `{name}` not finite"
    );
}

fn cubic_x_outside_the_unit_interval() {
    out_of_range(Cubic::try_new(1.5, 0.0, 0.5, 1.0), CurveParameter::X1);
    out_of_range(Cubic::try_new(0.5, 0.0, -0.1, 1.0), CurveParameter::X2);
    assert_panics("Cubic x1", || Cubic::new(1.5, 0.0, 0.5, 1.0));
    assert_panics("Cubic x2", || Cubic::new(0.5, 0.0, -0.1, 1.0));
}

fn cubic_non_finite_control_point() {
    non_finite(Cubic::try_new(f64::NAN, 0.0, 0.5, 1.0), CurveParameter::X1);
    non_finite(
        Cubic::try_new(0.5, f64::INFINITY, 0.5, 1.0),
        CurveParameter::Y1,
    );
    non_finite(
        Cubic::try_new(0.5, 0.0, 0.5, f64::NEG_INFINITY),
        CurveParameter::Y2,
    );
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
    out_of_range(
        Cubic::try_new(0.25, f64::MAX, 0.75, f64::MAX),
        CurveParameter::Y1,
    );
    out_of_range(Cubic::try_new(0.25, 0.0, 0.75, -1.5e6), CurveParameter::Y2);
    assert_panics("Cubic y1", || Cubic::new(0.25, f64::MAX, 0.75, 1.0));
    finite_everywhere("Cubic(y = ±1e6)", &Cubic::new(0.25, 1e6, 0.75, -1e6));
}

/// A huge period overflowed the phase product and a tiny one its quotient,
/// both to NaN. The admitted extremes evaluate finitely.
fn elastic_period_beyond_the_admitted_range() {
    out_of_range(
        Elastic::try_new(Ease::Out, f64::MAX),
        CurveParameter::Period,
    );
    out_of_range(Elastic::try_new(Ease::In, 1e-300), CurveParameter::Period);
    for period in [1e-6, 1e6] {
        finite_everywhere("ElasticOut", &Elastic::new(Ease::Out, period));
        finite_everywhere("ElasticInOut", &Elastic::new(Ease::InOut, period));
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
    out_of_range(huge, CurveParameter::A1Y);
    let extreme =
        ThreePointCubic::new((0.1, 1e6), (0.2, -1e6), (0.5, 0.5), (0.6, 1e6), (0.7, -1e6));
    finite_everywhere("ThreePointCubic(y = ±1e6)", &extreme);
}

fn three_point_midpoint_on_the_boundary() {
    let midpoint =
        ThreePointCubic::try_new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0));
    out_of_range(midpoint, CurveParameter::MidpointX);
    let flat = ThreePointCubic::try_new((0.1, 0.0), (0.2, 0.0), (0.5, 0.0), (0.6, 1.0), (0.7, 1.0));
    out_of_range(flat, CurveParameter::MidpointY);
    assert_panics("ThreePointCubic midpoint", || {
        ThreePointCubic::new((0.0, 0.0), (0.0, 0.0), (1.0, 0.5), (1.0, 1.0), (1.0, 1.0))
    });
}

fn three_point_control_outside_its_segment() {
    let first =
        ThreePointCubic::try_new((0.6, 0.0), (0.2, 0.0), (0.5, 0.5), (0.6, 1.0), (0.7, 1.0));
    out_of_range(first, CurveParameter::A1X);
    let second =
        ThreePointCubic::try_new((0.1, 0.0), (0.2, 0.0), (0.5, 0.5), (0.4, 1.0), (0.7, 1.0));
    out_of_range(second, CurveParameter::A2X);
    non_finite(
        ThreePointCubic::try_new(
            (0.1, 0.0),
            (0.2, f64::NAN),
            (0.5, 0.5),
            (0.6, 1.0),
            (0.7, 1.0),
        ),
        CurveParameter::B1Y,
    );
    assert_panics("ThreePointCubic a2.x", || {
        ThreePointCubic::new((0.1, 0.0), (0.2, 0.0), (0.5, 0.5), (0.4, 1.0), (0.7, 1.0))
    });
}

fn interval_bounds() {
    out_of_range(Interval::try_new(-0.1, 0.5, Linear), CurveParameter::Begin);
    out_of_range(Interval::try_new(0.2, 1.5, Linear), CurveParameter::End);
    non_finite(
        Interval::try_new(f64::NAN, 0.5, Linear),
        CurveParameter::Begin,
    );
    assert!(matches!(
        Interval::try_new(0.6, 0.4, Linear),
        Err(CurveError::IntervalReversed { .. })
    ));
    assert_panics("Interval reversed", || Interval::new(0.6, 0.4, Linear));
}

fn elastic_period() {
    out_of_range(Elastic::try_new(Ease::In, 0.0), CurveParameter::Period);
    out_of_range(Elastic::try_new(Ease::Out, -0.4), CurveParameter::Period);
    non_finite(
        Elastic::try_new(Ease::InOut, f64::INFINITY),
        CurveParameter::Period,
    );
    assert_panics("ElasticIn period", || Elastic::new(Ease::In, 0.0));
    assert_panics("ElasticOut period", || Elastic::new(Ease::Out, f64::NAN));
    assert_panics("ElasticInOut period", || Elastic::new(Ease::InOut, -1.0));
}

fn error_messages_name_the_parameter() {
    assert_eq!(
        Cubic::try_new(1.5, 0.0, 0.5, 1.0)
            .expect_err("x1")
            .to_string(),
        "curve parameter `x1` = 1.5 is outside [0, 1]"
    );
    assert_eq!(
        Elastic::try_new(Ease::Out, f64::NAN)
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

fn range_error(parameter: CurveParameter, value: f64) -> CurveError {
    CurveError::OutOfRange { parameter, value }
}

// These numeric fields exercise serde admission itself, including IEEE values
// JSON cannot represent. Parser rejection is not parameter validation.
#[cfg(feature = "serde")]
#[derive(Clone, Copy)]
enum CurveField {
    Number(f64),
    Point([f64; 2]),
    Unit,
}

#[cfg(feature = "serde")]
impl serde::de::IntoDeserializer<'_, serde::de::value::Error> for CurveField {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserializer<'de> for CurveField {
    type Error = serde::de::value::Error;

    fn deserialize_any<V: serde::de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self {
            Self::Number(value) => visitor.visit_f64(value),
            Self::Point(value) => {
                visitor.visit_seq(serde::de::value::SeqDeserializer::new(value.into_iter()))
            }
            Self::Unit => visitor.visit_unit(),
        }
    }

    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string
    bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map struct enum
    identifier ignored_any }
}

#[cfg(feature = "serde")]
fn rejects_numeric_fields<T: serde::de::DeserializeOwned + std::fmt::Debug, const N: usize>(
    fields: [(&'static str, CurveField); N],
    error: &CurveError,
) {
    let decoder =
        serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(fields.into_iter());
    let failure = T::deserialize(decoder).expect_err("invalid curve parameters");
    assert_eq!(failure.to_string(), error.to_string());
}

fn cubic_validation_is_shared_by_construction_paths() {
    for (index, value, expected) in [
        (0, -0.1, range_error(CurveParameter::X1, -0.1)),
        (2, 1.1, range_error(CurveParameter::X2, 1.1)),
        (1, -1.5e6, range_error(CurveParameter::Y1, -1.5e6)),
        (3, 1.5e6, range_error(CurveParameter::Y2, 1.5e6)),
        (
            0,
            f64::NAN,
            CurveError::NonFinite {
                parameter: CurveParameter::X1,
            },
        ),
        (
            1,
            f64::INFINITY,
            CurveError::NonFinite {
                parameter: CurveParameter::Y1,
            },
        ),
        (
            3,
            f64::NEG_INFINITY,
            CurveError::NonFinite {
                parameter: CurveParameter::Y2,
            },
        ),
    ] {
        let mut values = [0.25, 0.1, 0.75, 1.0];
        values[index] = value;
        let [a, b, c, d] = values;
        assert_eq!(Cubic::try_new(a, b, c, d).err(), Some(expected));
        assert_panics("Cubic admission", || Cubic::new(a, b, c, d));
        #[cfg(feature = "serde")]
        rejects_numeric_fields::<Cubic, 4>(
            [
                ("a", CurveField::Number(a)),
                ("b", CurveField::Number(b)),
                ("c", CurveField::Number(c)),
                ("d", CurveField::Number(d)),
            ],
            &expected,
        );
    }
}

fn three_point_validation_is_shared_by_construction_paths() {
    for (point, axis, value, expected) in [
        (2, 0, 1.0, range_error(CurveParameter::MidpointX, 1.0)),
        (2, 1, 0.0, range_error(CurveParameter::MidpointY, 0.0)),
        (0, 0, 0.6, range_error(CurveParameter::A1X, 0.6)),
        (1, 0, -0.1, range_error(CurveParameter::B1X, -0.1)),
        (3, 0, 0.4, range_error(CurveParameter::A2X, 0.4)),
        (4, 0, 1.1, range_error(CurveParameter::B2X, 1.1)),
        (0, 1, 1.5e6, range_error(CurveParameter::A1Y, 1.5e6)),
        (1, 1, -1.5e6, range_error(CurveParameter::B1Y, -1.5e6)),
        (3, 1, 1.5e6, range_error(CurveParameter::A2Y, 1.5e6)),
        (4, 1, -1.5e6, range_error(CurveParameter::B2Y, -1.5e6)),
        (
            1,
            1,
            f64::NAN,
            CurveError::NonFinite {
                parameter: CurveParameter::B1Y,
            },
        ),
        (
            3,
            1,
            f64::INFINITY,
            CurveError::NonFinite {
                parameter: CurveParameter::A2Y,
            },
        ),
        (
            4,
            0,
            f64::NEG_INFINITY,
            CurveError::NonFinite {
                parameter: CurveParameter::B2X,
            },
        ),
    ] {
        let mut points = [[0.1, 0.0], [0.2, 0.1], [0.5, 0.5], [0.6, 0.9], [0.7, 1.0]];
        points[point][axis] = value;
        let [a1, b1, midpoint, a2, b2] = points.map(|[x, y]| (x, y));
        assert_eq!(
            ThreePointCubic::try_new(a1, b1, midpoint, a2, b2).err(),
            Some(expected)
        );
        assert_panics("ThreePointCubic admission", || {
            ThreePointCubic::new(a1, b1, midpoint, a2, b2)
        });
        #[cfg(feature = "serde")]
        rejects_numeric_fields::<ThreePointCubic, 5>(
            [
                ("a1", CurveField::Point(points[0])),
                ("b1", CurveField::Point(points[1])),
                ("midpoint", CurveField::Point(points[2])),
                ("a2", CurveField::Point(points[3])),
                ("b2", CurveField::Point(points[4])),
            ],
            &expected,
        );
    }
    for midpoint_y in [f64::from_bits(1), 1e-310, 1e-301] {
        let points = [
            [0.1, 1.0],
            [0.2, 1.0],
            [0.5, midpoint_y],
            [0.6, 0.9],
            [0.7, 1.0],
        ];
        let [a1, b1, midpoint, a2, b2] = points.map(|[x, y]| (x, y));
        let expected = CurveError::RescalingOutOfRange { midpoint_y };
        assert_eq!(
            ThreePointCubic::try_new(a1, b1, midpoint, a2, b2).err(),
            Some(expected)
        );
        #[cfg(feature = "serde")]
        rejects_numeric_fields::<ThreePointCubic, 5>(
            [
                ("a1", CurveField::Point(points[0])),
                ("b1", CurveField::Point(points[1])),
                ("midpoint", CurveField::Point(points[2])),
                ("a2", CurveField::Point(points[3])),
                ("b2", CurveField::Point(points[4])),
            ],
            &expected,
        );
    }
}

fn interval_validation_is_shared_by_construction_paths() {
    for (begin, end, expected) in [
        (-0.1, 0.5, range_error(CurveParameter::Begin, -0.1)),
        (0.2, 1.5, range_error(CurveParameter::End, 1.5)),
        (
            0.6,
            0.4,
            CurveError::IntervalReversed {
                begin: 0.6,
                end: 0.4,
            },
        ),
        (
            f64::NAN,
            0.5,
            CurveError::NonFinite {
                parameter: CurveParameter::Begin,
            },
        ),
        (
            0.2,
            f64::INFINITY,
            CurveError::NonFinite {
                parameter: CurveParameter::End,
            },
        ),
        (
            f64::NEG_INFINITY,
            0.5,
            CurveError::NonFinite {
                parameter: CurveParameter::Begin,
            },
        ),
    ] {
        assert_eq!(Interval::try_new(begin, end, Linear).err(), Some(expected));
        assert_panics("Interval admission", || Interval::new(begin, end, Linear));
        #[cfg(feature = "serde")]
        rejects_numeric_fields::<Interval, 3>(
            [
                ("begin", CurveField::Number(begin)),
                ("end", CurveField::Number(end)),
                ("curve", CurveField::Unit),
            ],
            &expected,
        );
    }
}

fn elastic_validation_is_shared_by_construction_paths() {
    for period in [
        0.0,
        -0.4,
        1e-300,
        1.5e6,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let expected = if period.is_finite() {
            range_error(CurveParameter::Period, period)
        } else {
            CurveError::NonFinite {
                parameter: CurveParameter::Period,
            }
        };
        assert_eq!(Elastic::try_new(Ease::In, period).err(), Some(expected));
        assert_eq!(Elastic::try_new(Ease::Out, period).err(), Some(expected));
        assert_eq!(Elastic::try_new(Ease::InOut, period).err(), Some(expected));
        assert_panics("ElasticIn admission", || Elastic::new(Ease::In, period));
        assert_panics("ElasticOut admission", || Elastic::new(Ease::Out, period));
        assert_panics("ElasticInOut admission", || {
            Elastic::new(Ease::InOut, period)
        });
        #[cfg(feature = "serde")]
        {
            let fields = [("period", CurveField::Number(period))];
            rejects_numeric_fields::<Elastic, 1>(fields, &expected);
        }
    }
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
        (
            "cubic construction paths",
            cubic_validation_is_shared_by_construction_paths,
        ),
        (
            "three-point construction paths",
            three_point_validation_is_shared_by_construction_paths,
        ),
        (
            "interval construction paths",
            interval_validation_is_shared_by_construction_paths,
        ),
        (
            "elastic construction paths",
            elastic_validation_is_shared_by_construction_paths,
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
        &curves::EASE_IN_OUT_CUBIC_EMPHASIZED,
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
    round_trips(
        &Elastic::new(Ease::Out, 0.3),
        r#"{"ease":"Out","period":0.3}"#,
    );
    round_trips(
        &Elastic::new(Ease::In, 0.3),
        r#"{"ease":"In","period":0.3}"#,
    );
    round_trips(
        &Elastic::new(Ease::InOut, 0.3),
        r#"{"ease":"InOut","period":0.3}"#,
    );
    round_trips(&Bounce(Ease::InOut), r#""InOut""#);
    rejects::<Elastic>(
        r#"{"ease":"In","period":0.0}"#,
        &Elastic::try_new(Ease::In, 0.0).expect_err("period"),
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
            parameter: CurveParameter::StepCount,
            ..
        })
    ));
    assert_eq!(
        Steps::try_new(1, JumpAt::None),
        Err(CurveError::TooFewSteps)
    );
    assert!(Steps::try_new(2, JumpAt::None).is_ok());
    assert_panics("Steps zero count", || Steps::new(0, JumpAt::End));
    assert_panics("Steps no jump", || Steps::new(1, JumpAt::None));
}

#[test]
fn steps_follow_css_easing() {
    crate::run_table(&[
        ("CSS values", css_steps_values),
        ("curve contract", steps_keep_the_curve_contract),
        ("invalid counts", steps_reject_invalid_counts),
    ]);
}
