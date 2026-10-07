//! Retargeting keeps the value and the velocity continuous at every seam.
//!
//! Velocities are checked against finite differences of `value()` alone, and
//! curve values against a cubic Bézier evaluated by bisection here, so no
//! oracle reuses the production formula.

use std::time::Duration;

use flui_animation::{AnimatedValue, ArcCurve, Cubic, Curves, MotionSpec, SpringDescription};
use flui_foundation::geometry::Offset;
use proptest::prelude::*;

/// Control points of the cubic curves the curve-mode properties use.
const CUBICS: [(f64, f64, f64, f64); 5] = [
    (0.25, 0.1, 0.25, 1.0),
    (0.42, 0.0, 1.0, 1.0),
    (0.0, 0.0, 0.58, 1.0),
    (0.42, 0.0, 0.58, 1.0),
    (0.05, 0.7, 0.1, 1.0),
];

/// One coordinate of a cubic Bézier from (0,0) to (1,1) at parameter `s`.
fn bezier(p1: f64, p2: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s
}

fn bezier_derivative(p1: f64, p2: f64, s: f64) -> f64 {
    let u = 1.0 - s;
    3.0 * u * u * p1 + 6.0 * u * s * (p2 - p1) + 3.0 * s * s * (1.0 - p2)
}

/// The Bézier parameter whose x is `t`, by 200 bisection steps.
fn parameter_at(curve: (f64, f64, f64, f64), t: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..200 {
        let mid = f64::midpoint(lo, hi);
        if bezier(curve.0, curve.2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    f64::midpoint(lo, hi)
}

/// The eased progress of `curve` at `t`, independent of `Cubic::transform`.
fn eased(curve: (f64, f64, f64, f64), t: f64) -> f64 {
    bezier(curve.1, curve.3, parameter_at(curve, t))
}

/// The slope of `curve` at `t` as `y'(s)/x'(s)`, independent of `Cubic::slope`.
fn eased_slope(curve: (f64, f64, f64, f64), t: f64) -> f64 {
    let s = parameter_at(curve, t);
    bezier_derivative(curve.1, curve.3, s) / bezier_derivative(curve.0, curve.2, s)
}

/// The terminal slope of `curve`, as the limit of `y'(s)/x'(s)` at `s = 1`:
/// the last control leg's direction, or the middle leg's when the last one
/// is degenerate (an end control point at `(1, 1)`).
fn terminal_slope(curve: (f64, f64, f64, f64)) -> f64 {
    if curve.2 < 1.0 {
        (1.0 - curve.3) / (1.0 - curve.2)
    } else {
        (curve.3 - curve.1) / (curve.2 - curve.0)
    }
}

fn curve_spec(curve: (f64, f64, f64, f64), millis: u64) -> MotionSpec {
    MotionSpec::Curve {
        duration: Duration::from_millis(millis),
        curve: ArcCurve::new(Cubic::new(curve.0, curve.1, curve.2, curve.3)),
    }
}

/// `ω` in rad/s and `ζ` for a unit mass.
fn spring(omega: f64, zeta: f64) -> SpringDescription {
    SpringDescription::with_damping_ratio(1.0, omega * omega, zeta)
}

/// The right derivative of `value()` at the current instant, from probes `h`
/// and `2h` ahead: `(−3x(0) + 4x(h) − x(2h)) / 2h`.
fn right_derivative<T: flui_animation::TwoWayConverter>(
    value: &AnimatedValue<T>,
    component: usize,
    h: f64,
) -> f64 {
    let at = |dt: f64| {
        let mut probe = value.clone();
        probe.advance(dt);
        probe.value().to_vector().as_ref()[component]
    };
    let x0 = value.value().to_vector().as_ref()[component];
    (-3.0 * x0 + 4.0 * at(h) - at(2.0 * h)) / (2.0 * h)
}

fn close(actual: f64, expected: f64, relative: f64) -> bool {
    (actual - expected).abs() <= relative * expected.abs().max(1.0)
}

/// Retargets `value` toward `target`, asserting C⁰ and C¹ at the seam and that
/// the reported velocity is the derivative of the value on both sides.
fn seam<T: flui_animation::TwoWayConverter>(
    value: &mut AnimatedValue<T>,
    target: T,
    mode: &Mode,
    range: f64,
) {
    let before = value.value().to_vector();
    let velocity_before = value.velocity();
    value.animate_to(target);
    let after = value.value().to_vector();
    let velocity_after = value.velocity();
    for component in 0..before.as_ref().len() {
        let (x0, x1) = (before.as_ref()[component], after.as_ref()[component]);
        let (v0, v1) = (
            velocity_before.as_ref()[component],
            velocity_after.as_ref()[component],
        );
        assert!(
            x1.is_finite() && v1.is_finite(),
            "non-finite seam: {x1}, {v1}"
        );
        assert!(close(x1, x0, 1e-12), "C0 broken: {x0} -> {x1}");
        assert!(close(v1, v0, 1e-9), "C1 broken: {v0} -> {v1}");
        assert_velocity_is_the_derivative(value, component, mode, range);
    }
}

/// The reported velocity matches a finite difference of the value.
///
/// The difference is Richardson-extrapolated from steps `h` and `h/2`, and
/// their disagreement bounds its truncation error, so steep curves (whose
/// higher derivatives are large near an end) need no hand-tuned step. The
/// evaluation noise a difference amplifies by `8/h` is the value's own: a
/// spring is closed form (rounding only), a cubic curve's `transform` is
/// solved to about 1e-8 of its output.
fn assert_velocity_is_the_derivative<T: flui_animation::TwoWayConverter>(
    value: &AnimatedValue<T>,
    component: usize,
    mode: &Mode,
    range: f64,
) {
    let v = value.velocity().as_ref()[component];
    let h = 1e-5;
    let coarse = right_derivative(value, component, h);
    let fine = right_derivative(value, component, h / 2.0);
    let extrapolated = (4.0 * fine - coarse) / 3.0;
    let evaluation_error = match mode {
        Mode::Spring { .. } => 1e-15,
        Mode::Curve { .. } => 1e-8,
    };
    let tolerance =
        2.0 * (coarse - fine).abs() + 16.0 * evaluation_error * range / h + 1e-6 * (1.0 + v.abs());
    assert!(
        (extrapolated - v).abs() <= tolerance,
        "velocity {v} is not the derivative {extrapolated} of the value (tolerance {tolerance})"
    );
}

#[derive(Clone, Debug)]
enum Mode {
    Spring { omega: f64, zeta: f64 },
    Curve { curve: usize, millis: u64 },
}

impl Mode {
    fn spec(&self) -> MotionSpec {
        match *self {
            Mode::Spring { omega, zeta } => MotionSpec::Spring(spring(omega, zeta)),
            Mode::Curve { curve, millis } => curve_spec(CUBICS[curve], millis),
        }
    }
}

fn mode() -> impl Strategy<Value = Mode> {
    prop_oneof![
        (1.0..100.0_f64, 0.1..4.0_f64).prop_map(|(omega, zeta)| Mode::Spring { omega, zeta }),
        (0..CUBICS.len(), 50..1000_u64).prop_map(|(curve, millis)| Mode::Curve { curve, millis }),
    ]
}

fn frame_dt() -> impl Strategy<Value = f64> {
    prop_oneof![Just(0.0), (30.0..240.0_f64).prop_map(|hz| 1.0 / hz)]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Retargeting every frame for 120 frames keeps value and velocity
    /// continuous at every seam, publishes only finite values, and the last
    /// segment still comes to rest at its target.
    #[test]
    fn repeated_interruption_keeps_position_and_velocity_continuous(
        mode in mode(),
        dt in frame_dt(),
        targets in prop::collection::vec((-1.0..1.0_f64, -1.0..1.0_f64), 120),
    ) {
        let mut scalar = AnimatedValue::with_motion(0.0_f64, mode.spec());
        let mut planar = AnimatedValue::with_motion(Offset::new(0.0, 0.0), mode.spec());
        for &(x, y) in &targets {
            scalar.advance(dt);
            planar.advance(dt);
            seam(&mut scalar, x * 100.0, &mode, 200.0);
            seam(&mut planar, Offset::new(x * 100.0, y * 100.0), &mode, 200.0);
        }
        let last = targets[targets.len() - 1];
        // The slowest generated spring decays as e^(-0.1 t) (omega = 1 at
        // zeta = 0.1, or the overdamped slow root at zeta = 4): a 200-unit
        // swing takes about 125 s to reach the 1e-3 tolerance, so the budget
        // is 600 s of 10 ms steps.
        for _ in 0..60_000 {
            if scalar.is_settled() && planar.is_settled() {
                break;
            }
            scalar.advance(1e-2);
            planar.advance(1e-2);
            prop_assert!(scalar.value().is_finite());
        }
        prop_assert!(scalar.is_settled() && planar.is_settled(), "never settled");
        prop_assert!(close(scalar.value(), last.0 * 100.0, 1e-3));
        let resting = planar.value();
        prop_assert!(close(resting.dx, last.0 * 100.0, 1e-3) && close(resting.dy, last.1 * 100.0, 1e-3));
    }

    /// A curve segment that inherits a velocity starts with it, lands on its
    /// target exactly after its duration, and strays from the plain curve by
    /// at most `(|r|·D + |a|)·4/27`, `r = v0 − Δ·c'(0)/D`, `a = Δ·c'(1)`.
    #[test]
    fn a_curve_segment_lands_on_time_and_bounds_its_overshoot(
        curve in 0..CUBICS.len(),
        millis in 50..1000_u64,
        first in -100.0..100.0_f64,
        second in -100.0..100.0_f64,
        seam_fraction in 0.05..0.95_f64,
    ) {
        prop_assume!((second - first).abs() > 1e-3 && second.abs() > 1e-3);
        let control = CUBICS[curve];
        let duration = millis as f64 / 1e3;
        let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(control, millis));
        value.animate_to(first);
        value.advance(seam_fraction * duration);
        let x0 = value.value();
        let v0 = value.velocity()[0];
        value.animate_to(second);
        let span = second - x0;
        let excess = v0 - span * eased_slope(control, 0.0) / duration;
        let arrival = span * terminal_slope(control);
        let bound = (excess.abs() * duration + arrival.abs()) * 4.0 / 27.0;
        for step in 1..100 {
            let t = duration * f64::from(step) / 100.0;
            let mut probe = value.clone();
            probe.advance(t);
            let plain = x0 + span * eased(control, t / duration);
            prop_assert!(
                (probe.value() - plain).abs() <= bound + 1e-9 * (1.0 + span.abs()),
                "strayed {} past the bound {bound} at {t}", (probe.value() - plain).abs()
            );
            prop_assert!(!probe.is_settled(), "settled early at {t}");
        }
        value.advance(duration);
        prop_assert_eq!(value.value(), second);
        prop_assert!(value.is_settled());
    }

    /// Replacing only the motion spec mid-flight keeps value and velocity.
    #[test]
    fn changing_only_the_motion_spec_is_c1(
        from in mode(),
        to in mode(),
        elapsed in 0.0..0.2_f64,
    ) {
        let mut value = AnimatedValue::with_motion(0.0_f64, from.spec());
        value.animate_to(100.0);
        value.advance(elapsed);
        let (x, v) = (value.value(), value.velocity()[0]);
        value.set_motion(to.spec());
        prop_assert!(close(value.value(), x, 1e-12));
        prop_assert!(close(value.velocity()[0], v, 1e-9));
        assert_velocity_is_the_derivative(&value, 0, &to, 100.0);
    }
}

fn seam_at_zero() {
    // Two retargets with no time between them: the second starts from the
    // first one's start, at rest.
    for mode in [
        Mode::Spring {
            omega: 10.0,
            zeta: 0.5,
        },
        Mode::Curve {
            curve: 3,
            millis: 200,
        },
    ] {
        let mut value = AnimatedValue::with_motion(5.0_f64, mode.spec());
        value.animate_to(50.0);
        seam(&mut value, -20.0, &mode, 70.0);
        assert_eq!(value.value(), 5.0);
        assert_eq!(value.velocity()[0], 0.0);
    }
}

/// A spring retargeted by less than its distance tolerance, from rest, starts
/// where it is instead of jumping to the target at the seam, is settled on the
/// next frame, and is still moving continuously toward the target there.
fn a_retarget_within_the_spring_tolerance_starts_at_the_seam() {
    let mode = Mode::Spring {
        omega: 1.0,
        zeta: 0.1,
    };
    let mut value = AnimatedValue::with_motion(Offset::new(0.0, 0.0), mode.spec());
    let target = Offset::new(0.0, -1.463_116_746_455_488e-4);
    seam(&mut value, target, &mode, 1e-3);
    assert_eq!(value.value(), Offset::new(0.0, 0.0));
    assert!(!value.is_settled(), "settled away from its target");
    value.advance(1.0 / 60.0);
    assert!(value.is_settled());
    let y = value.value().dy;
    assert!(y < 0.0 && y > target.dy, "not converging continuously: {y}");
    assert_velocity_is_the_derivative(&value, 1, &mode, 1e-3);
}

/// A spring whose rest boundary falls between two frames: the frame before is
/// not settled, the frame after is, and the finite difference across the
/// boundary matches the reported velocity there. A spring that snapped onto
/// its target at the boundary would jump by up to its tolerance instead.
fn the_spring_rest_boundary_is_c1() {
    let mode = Mode::Spring {
        omega: 10.0,
        zeta: 1.0,
    };
    let mut value = AnimatedValue::with_motion(0.0_f64, mode.spec());
    value.animate_to(1.0);
    let frame = 1.0 / 60.0;
    let mut frames = 0;
    while !value.is_settled() {
        value.advance(frame);
        frames += 1;
        assert!(frames < 600, "never settled");
    }
    // Step back to the last unsettled frame and straddle the boundary with a
    // central difference.
    let mut before = AnimatedValue::with_motion(0.0_f64, mode.spec());
    before.animate_to(1.0);
    before.advance(frame * f64::from(frames - 1));
    assert!(!before.is_settled());
    let mut after = before.clone();
    after.advance(frame);
    assert!(after.is_settled());
    let mut middle = before.clone();
    middle.advance(frame / 2.0);
    let difference = (after.value() - before.value()) / frame;
    let v = middle.velocity()[0];
    // Central-difference truncation for this spring is |x'''|·h²/24, far
    // below 1e-4 here; a snap would add up to 1e-3/h = 6e-2.
    assert!(
        (difference - v).abs() <= 1e-4,
        "finite difference {difference} across the rest boundary is not the velocity {v}"
    );
    assert!(
        after.value() < 1.0,
        "jumped onto the target: {}",
        after.value()
    );
}

fn seam_on_the_completing_frame() {
    let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(CUBICS[0], 200));
    value.animate_to(10.0);
    value.advance(0.2);
    assert!(value.is_settled());
    seam(
        &mut value,
        30.0,
        &Mode::Curve {
            curve: 0,
            millis: 200,
        },
        20.0,
    );
    assert_eq!(value.value(), 10.0);
    assert_eq!(value.velocity()[0], 0.0);
}

fn time_that_does_not_move_forward_samples_the_seam() {
    let mut value = AnimatedValue::with_motion(0.0_f64, MotionSpec::Spring(spring(10.0, 0.5)));
    value.animate_to(1.0);
    value.advance(0.05);
    let x = value.value();
    for dt in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY] {
        value.advance(dt);
        assert_eq!(value.value(), x, "dt {dt} moved time");
    }
    value.advance(1e6);
    assert_eq!(value.value(), 1.0);
    assert!(value.value().is_finite() && value.velocity()[0].is_finite());
}

/// Same target, same spring: the seams are invisible. Reference values from
/// the closed form `e^{−5t}(cos ω_d t + (5/ω_d) sin ω_d t)`, ω_d = √75.
fn retargeting_to_the_same_target_is_invisible() {
    let mut value = AnimatedValue::with_motion(1.0_f64, MotionSpec::Spring(spring(10.0, 0.5)));
    value.animate_to(0.0);
    value.advance(0.1);
    value.animate_to(0.0);
    value.advance(0.07);
    value.animate_to(0.0);
    value.advance(0.08);
    let at_quarter = value.value();
    assert!(
        (at_quarter - -0.023_359_579_906_692_3).abs() <= 1e-12,
        "x(0.25) = {at_quarter}"
    );
    value.advance(0.75);
    let at_one = value.value();
    assert!(
        (at_one - -0.002_170_116_739_326_20).abs() <= 1e-12,
        "x(1.0) = {at_one}"
    );
}

/// CSS Transitions reversal: `S' = |c(τ)·S + (1 − S)|`, `D' = D·S'`.
fn reversal_shortens_by_the_eased_fraction() {
    let linear = MotionSpec::Curve {
        duration: Duration::from_millis(200),
        curve: ArcCurve::new(Curves::Linear),
    };
    let mut value = AnimatedValue::with_motion(0.0_f64, linear);
    value.animate_to(100.0);
    value.advance(0.05);
    value.animate_to(0.0);
    value.advance(0.049);
    assert!(!value.is_settled(), "a reversal at 50 ms lasts 50 ms");
    value.advance(0.001);
    assert!(value.is_settled(), "a reversal at 50 ms lasts 50 ms");
    assert_eq!(value.value(), 0.0);

    let ease = CUBICS[0];
    let shortened = eased(ease, 0.25);
    assert!(
        (shortened - 0.408_510_591_355_396).abs() <= 1e-12,
        "S' = {shortened}"
    );
    let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(ease, 200));
    value.animate_to(100.0);
    value.advance(0.05);
    value.animate_to(0.0);
    let first = 0.2 * shortened;
    value.advance(first - 1e-9);
    assert!(!value.is_settled(), "settled before {first} s");
    value.advance(2e-9);
    assert!(value.is_settled(), "not settled by {first} s");

    // A second reversal uses the first one's shortening as S.
    let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(ease, 200));
    value.animate_to(100.0);
    value.advance(0.05);
    value.animate_to(0.0);
    let into_first = first * 0.5;
    value.advance(into_first);
    value.animate_to(100.0);
    let twice = (eased(ease, into_first / first) * shortened + (1.0 - shortened)).abs();
    let second = 0.2 * twice;
    value.advance(second - 1e-9);
    assert!(!value.is_settled(), "settled before {second} s");
    value.advance(2e-9);
    assert!(value.is_settled(), "not settled by {second} s");
    assert_eq!(value.value(), 100.0);
}

/// A segment whose target is not the reversing-adjusted start runs its full
/// duration.
fn a_retarget_elsewhere_runs_the_full_duration() {
    let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(CUBICS[0], 200));
    value.animate_to(100.0);
    value.advance(0.05);
    value.animate_to(10.0);
    value.advance(0.2 - 1e-9);
    assert!(!value.is_settled());
    value.advance(2e-9);
    assert_eq!(value.value(), 10.0);
}

fn linear_spec(millis: u64) -> MotionSpec {
    MotionSpec::Curve {
        duration: Duration::from_millis(millis),
        curve: ArcCurve::new(Curves::Linear),
    }
}

/// Finite steps whose sum overflows saturate time instead of publishing a
/// non-finite value, and a spring whose phase or polynomial term overflows
/// reads as its limit: the target, at rest.
fn time_steps_that_overflow_publish_the_limit() {
    for spec in [
        MotionSpec::Spring(spring(10.0, 0.5)),
        MotionSpec::Spring(spring(10.0, 1.0)),
        MotionSpec::Spring(spring(10.0, 2.0)),
        linear_spec(200),
    ] {
        let mut value = AnimatedValue::with_motion(0.0_f64, spec.clone());
        value.animate_to(1.0);
        value.advance(0.01);
        value.advance(f64::MAX);
        value.advance(f64::MAX);
        let (x, v) = (value.value(), value.velocity()[0]);
        assert!(x.is_finite() && v.is_finite(), "{spec:?}: x {x}, v {v}");
        assert!(close(x, 1.0, 1e-9), "{spec:?}: x {x}");
        assert!(value.is_settled(), "{spec:?} never settled");
    }
}

/// A span near the f64 limit over a curve that starts steeper than linear:
/// `span · slope` alone overflows although the rate `span · slope / duration`
/// is representable, so the seam is kept instead of snapping.
fn a_large_span_with_a_steep_start_keeps_its_seam() {
    let mut value = AnimatedValue::with_motion(
        0.0_f64,
        MotionSpec::Curve {
            duration: Duration::from_secs(2),
            curve: ArcCurve::new(Cubic::new(0.5, 1.0, 0.75, 1.0)),
        },
    );
    value.animate_to(1e308);
    assert!(!value.is_settled(), "snapped to the target at the seam");
    value.advance(0.5);
    let (x, v) = (value.value(), value.velocity()[0]);
    assert!(x.is_finite() && v.is_finite(), "x {x}, v {v}");
    assert!(x > 0.0 && x < 1e308, "x {x} is not between the ends");
    assert!(v > 0.0, "velocity {v} vanished mid-segment");
}

/// A span near the f64 limit over a short curve: the endpoint rate
/// `span · slope / duration` is finite although `span / duration` alone is
/// not, so the run keeps its seam instead of snapping to the target.
fn a_large_span_over_a_short_curve_keeps_its_seam() {
    let mut value = AnimatedValue::with_motion(
        0.0_f64,
        MotionSpec::Curve {
            duration: Duration::from_millis(500),
            curve: ArcCurve::new(Cubic::new(0.25, 0.125, 0.75, 1.0)),
        },
    );
    value.animate_to(1e308);
    assert!(!value.is_settled(), "snapped to the target at the seam");
    assert_eq!(value.value(), 0.0, "the seam keeps the start value");
    value.advance(0.25);
    let (x, v) = (value.value(), value.velocity()[0]);
    assert!(x.is_finite() && v.is_finite(), "x {x}, v {v}");
    assert!(x > 0.0 && x < 1e308, "x {x} is not between the ends");
}

/// A near-overflow velocity handed to a long curve segment: the Hermite
/// correction is in range although `excess · duration` alone is not.
fn a_large_finite_curve_correction_stays_finite() {
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(1));
    value.animate_to(1e305);
    value.advance(0.0005);
    assert!(value.velocity()[0] >= 1e308, "v {}", value.velocity()[0]);
    value.set_motion(linear_spec(10_000));
    for t in [0.1, 2.5, 5.0, 7.5, 9.9] {
        let mut probe = value.clone();
        probe.advance(t);
        let (x, v) = (probe.value(), probe.velocity()[0]);
        assert!(x.is_finite() && v.is_finite(), "at {t}: x {x}, v {v}");
    }
}

/// A curve with a non-zero terminal slope still arrives at rest: its velocity
/// tends to zero before arrival, so a retarget one tick early and one on the
/// completing frame inherit the same momentum.
fn a_curve_arrives_with_zero_velocity() {
    let mode = Mode::Curve {
        curve: 0,
        millis: 200,
    };
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(200));
    value.animate_to(100.0);
    value.advance(0.2 - 1e-6);
    let early = value.velocity()[0];
    assert!(early.abs() <= 0.1, "{early} units/s just before arrival");
    assert_velocity_is_the_derivative(&value, 0, &mode, 100.0);
    value.advance(1e-6);
    assert_eq!(value.value(), 100.0);
    assert_eq!(value.velocity()[0], 0.0);
}

/// A reversal early enough to shorten the segment below a millisecond keeps
/// the seam's value and velocity.
fn an_early_reversal_is_continuous() {
    let mode = Mode::Curve {
        curve: 0,
        millis: 200,
    };
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(200));
    value.animate_to(100.0);
    value.advance(0.0005);
    seam(&mut value, 0.0, &mode, 100.0);
    assert!(!value.is_settled());
    value.advance(0.01);
    assert!(value.is_settled());
    assert_eq!(value.value(), 0.0);
}

/// Assigning a curve its current target leaves the segment running on its
/// schedule instead of restarting the full duration.
fn retargeting_a_curve_to_its_target_keeps_its_schedule() {
    let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(CUBICS[0], 200));
    value.animate_to(10.0);
    value.advance(0.1);
    let (x, v) = (value.value(), value.velocity()[0]);
    value.animate_to(10.0);
    assert_eq!((value.value(), value.velocity()[0]), (x, v));
    value.advance(0.1);
    assert!(value.is_settled());
    assert_eq!(value.value(), 10.0);
}

#[test]
fn retarget_seams() {
    crate::run_table(&[
        ("seam_at_zero", seam_at_zero),
        (
            "a_retarget_within_the_spring_tolerance_starts_at_the_seam",
            a_retarget_within_the_spring_tolerance_starts_at_the_seam,
        ),
        (
            "the_spring_rest_boundary_is_c1",
            the_spring_rest_boundary_is_c1,
        ),
        ("seam_on_the_completing_frame", seam_on_the_completing_frame),
        (
            "time_that_does_not_move_forward_samples_the_seam",
            time_that_does_not_move_forward_samples_the_seam,
        ),
        (
            "retargeting_to_the_same_target_is_invisible",
            retargeting_to_the_same_target_is_invisible,
        ),
        (
            "reversal_shortens_by_the_eased_fraction",
            reversal_shortens_by_the_eased_fraction,
        ),
        (
            "a_retarget_elsewhere_runs_the_full_duration",
            a_retarget_elsewhere_runs_the_full_duration,
        ),
        (
            "time_steps_that_overflow_publish_the_limit",
            time_steps_that_overflow_publish_the_limit,
        ),
        (
            "a_large_span_over_a_short_curve_keeps_its_seam",
            a_large_span_over_a_short_curve_keeps_its_seam,
        ),
        (
            "a_large_span_with_a_steep_start_keeps_its_seam",
            a_large_span_with_a_steep_start_keeps_its_seam,
        ),
        (
            "a_large_finite_curve_correction_stays_finite",
            a_large_finite_curve_correction_stays_finite,
        ),
        (
            "a_curve_arrives_with_zero_velocity",
            a_curve_arrives_with_zero_velocity,
        ),
        (
            "an_early_reversal_is_continuous",
            an_early_reversal_is_continuous,
        ),
        (
            "retargeting_a_curve_to_its_target_keeps_its_schedule",
            retargeting_a_curve_to_its_target_keeps_its_schedule,
        ),
    ]);
}
