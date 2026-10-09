//! Retargeting keeps the value and the velocity continuous at every seam.
//!
//! Velocities are checked against finite differences of `value()` alone, and
//! curve values against a cubic Bézier evaluated by bisection here, so no
//! oracle reuses the production formula.

use std::time::Duration;

use flui_animation::{
    AnimatedValue, AnimationController, ArcCurve, Cubic, Curve, Curves, JumpAt, MotionSpec,
    PlaybackRate, SpringDescription, Steps, Vsync,
};
use flui_foundation::geometry::Offset;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

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
        probe.advance(Duration::from_secs_f64(dt));
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
    value.animate_to(target).expect("finite motion");
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
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/contracts/retarget.proptest-regressions",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn curved_run_velocity_is_the_curve_slope(
        curve_index in 0..CUBICS.len(),
        millis in 10_u64..10_000,
        fraction in 1_u32..999_999,
        reverse in any::<bool>(),
        rate in 0.1_f64..4.0,
    ) {
        let curve = CUBICS[curve_index];
        let duration = Duration::from_millis(millis);
        let owner = AnimationController::builder(duration)
            .initial_value(if reverse { 1.0 } else { 0.0 })
            .build_on(Some(&Vsync::new()));
        let controller = owner.controller();
        controller.set_playback_rate(PlaybackRate::new(rate).expect("finite positive rate"));
        let curve = Cubic::new(curve.0, curve.1, curve.2, curve.3);
        let _run = if reverse {
            controller.animate_back_curved(0.0, Some(duration), curve)
        } else {
            controller.animate_to_curved(1.0, Some(duration), curve)
        }.expect("finite curved run");
        controller.tick_at(Duration::ZERO);
        let controls = CUBICS[curve_index];
        let initial_slope = if controls.0 == 0.0 {
            controls.3 / controls.2
        } else {
            controls.1 / controls.0
        };
        let sign = if reverse { -1.0 } else { 1.0 };
        prop_assert!(close(controller.velocity(), sign * initial_slope / duration.as_secs_f64() * rate, 1e-7));
        let elapsed = duration.mul_f64(f64::from(fraction) / 1_000_000.0 / rate);
        controller.tick_at(elapsed);
        let local = elapsed.mul_f64(rate);
        let progress = local.as_secs_f64() / duration.as_secs_f64();
        let expected = sign * eased_slope(CUBICS[curve_index], progress)
            / duration.as_secs_f64() * rate;
        prop_assert!(close(controller.velocity(), expected, 1e-7),
            "velocity {} differs from slope {expected} at {progress}", controller.velocity());
        controller.tick_at(Duration::MAX);
        prop_assert_eq!(controller.velocity(), 0.0);
    }

    #[test]
    fn retarget_is_c0_and_c1_at_the_seam(
        old_motion in mode(),
        new_motion in mode(),
        target in -10.0_f64..10.0,
        elapsed in 0.0_f64..1.0,
        playback in 0.1_f64..4.0,
    ) {
        use flui_animation::Animation;
        let owner = AnimationController::builder(Duration::from_secs(1))
            .unbounded().build_on(Some(&Vsync::new()));
        let controller = owner.controller();
        controller.set_playback_rate(PlaybackRate::new(playback).expect("positive rate"));
        let _old = controller.retarget(5.0, &old_motion.spec()).expect("old motion");
        controller.tick_at(Duration::ZERO);
        controller.tick_at(Duration::from_secs_f64(elapsed));
        let (x0, v0) = (controller.value(), controller.velocity());
        let _new = controller.retarget(target, &new_motion.spec()).expect("new motion");
        prop_assert!(close(controller.value(), x0, 1e-12));
        prop_assert!(close(controller.velocity(), v0, 1e-9));
        let h = 1e-5;
        controller.tick_at(Duration::from_secs_f64(h / 2.0));
        let x_half = controller.value();
        controller.tick_at(Duration::from_secs_f64(h));
        let x1 = controller.value();
        controller.tick_at(Duration::from_secs_f64(2.0 * h));
        let x2 = controller.value();
        // Controller time lives on Duration's nanosecond grid. Fit the value
        // samples at their admitted local times, then convert to input seconds.
        let u1 = Duration::from_secs_f64(h * playback).as_secs_f64();
        let u2 = Duration::from_secs_f64(2.0 * h * playback).as_secs_f64();
        let u_half = Duration::from_secs_f64(h / 2.0 * playback).as_secs_f64();
        let fit = |a: f64, xa: f64, b: f64, xb: f64| {
            ((xa - x0) * b / a - (xb - x0) * a / b) / (b - a) * playback
        };
        let coarse = fit(u1, x1, u2, x2);
        let fine = fit(u_half, x_half, u1, x1);
        let derivative = (4.0 * fine - coarse) / 3.0;
        let evaluation_error = match new_motion {
            Mode::Spring { .. } => 1e-15,
            Mode::Curve { .. } => 1e-8,
        };
        let range = (target - x0).abs().max(1.0);
        // Cubics may have a non-analytic second derivative at a flat control
        // point. The coarse/fine gap measures truncation; the solver's output
        // accuracy bounds the separate evaluation error amplified by the fit.
        let tolerance = 3.0 * (coarse - fine).abs()
            + 16.0 * evaluation_error * range / h + 1e-6 * (1.0 + v0.abs());
        prop_assert!((derivative - v0).abs() <= tolerance,
            "reported seam velocity {v0} differs from value derivative {derivative}, tolerance {tolerance}");
    }

    /// Retargeting every frame for 120 frames keeps value and velocity
    /// continuous at every seam, publishes only finite values, and the last
    /// segment still comes to rest at its target.
    #[test]
    fn repeated_interruption_keeps_position_and_velocity_continuous(
        mode in mode(),
        dt in frame_dt(),
        targets in prop::collection::vec((-1.0..1.0_f64, -1.0..1.0_f64), 120),
    ) {
        let mut scalar = AnimatedValue::with_motion(0.0_f64, mode.spec()).expect("finite motion");
        let mut planar = AnimatedValue::with_motion(Offset::new(0.0, 0.0), mode.spec()).expect("finite motion");
        for &(x, y) in &targets {
            scalar.advance(Duration::from_secs_f64(dt));
            planar.advance(Duration::from_secs_f64(dt));
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
            scalar.advance(Duration::from_secs_f64(1e-2));
            planar.advance(Duration::from_secs_f64(1e-2));
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
        let mut value = AnimatedValue::with_motion(0.0_f64, curve_spec(control, millis)).expect("finite motion");
        value.animate_to(first).expect("finite motion");
        value.advance(Duration::from_secs_f64(seam_fraction * duration));
        let x0 = value.value();
        let v0 = value.velocity()[0];
        value.animate_to(second).expect("finite motion");
        let span = second - x0;
        let excess = v0 - span * eased_slope(control, 0.0) / duration;
        let arrival = span * terminal_slope(control);
        let bound = (excess.abs() * duration + arrival.abs()) * 4.0 / 27.0;
        for step in 1..100 {
            let t = duration * f64::from(step) / 100.0;
            let mut probe = value.clone();
            probe.advance(Duration::from_secs_f64(t));
            let plain = x0 + span * eased(control, t / duration);
            prop_assert!(
                (probe.value() - plain).abs() <= bound + 1e-9 * (1.0 + span.abs()),
                "strayed {} past the bound {bound} at {t}", (probe.value() - plain).abs()
            );
            prop_assert!(!probe.is_settled(), "settled early at {t}");
        }
        value.advance(Duration::from_secs_f64(duration));
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
        let mut value = AnimatedValue::with_motion(0.0_f64, from.spec()).expect("finite motion");
        value.animate_to(100.0).expect("finite motion");
        value.advance(Duration::from_secs_f64(elapsed));
        let (x, v) = (value.value(), value.velocity()[0]);
        value.set_motion(to.spec()).expect("finite motion");
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
        let mut value = AnimatedValue::with_motion(5.0_f64, mode.spec()).expect("finite motion");
        value.animate_to(50.0).expect("finite motion");
        seam(&mut value, -20.0, &mode, 70.0);
        assert_eq!(value.value(), 5.0);
        assert_eq!(value.velocity()[0], 0.0);
    }
}

/// A spring retargeted by less than its distance tolerance, from rest, starts
/// where it is instead of jumping to the target at the seam, remains moving
/// on the next frame, then reaches exact rest continuously.
fn a_retarget_within_the_spring_tolerance_starts_at_the_seam() {
    let mode = Mode::Spring {
        omega: 1.0,
        zeta: 0.1,
    };
    let mut value =
        AnimatedValue::with_motion(Offset::new(0.0, 0.0), mode.spec()).expect("finite motion");
    let target = Offset::new(0.0, -1.463_116_746_455_488e-4);
    seam(&mut value, target, &mode, 1e-3);
    assert_eq!(value.value(), Offset::new(0.0, 0.0));
    assert!(!value.is_settled(), "settled away from its target");
    value.advance(Duration::from_secs_f64(1.0 / 60.0));
    assert!(
        !value.is_settled(),
        "a tolerance-sized goal still has a continuous rest transition"
    );
    let y = value.value().dy;
    assert!(y < 0.0 && y > target.dy, "not converging continuously: {y}");
    assert_velocity_is_the_derivative(&value, 1, &mode, 1e-3);
    value.advance(Duration::from_secs(1));
    assert!(value.is_settled());
    assert_eq!(value.value(), target);
    assert_eq!(value.velocity(), [0.0, 0.0]);
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
    let mut value = AnimatedValue::with_motion(0.0_f64, mode.spec()).expect("finite motion");
    value.animate_to(1.0).expect("finite motion");
    let frame = 1.0 / 60.0;
    let mut frames = 0;
    while !value.is_settled() {
        value.advance(Duration::from_secs_f64(frame));
        frames += 1;
        assert!(frames < 600, "never settled");
    }
    // Step back to the last unsettled frame and straddle the boundary with a
    // central difference.
    let mut before = AnimatedValue::with_motion(0.0_f64, mode.spec()).expect("finite motion");
    before.animate_to(1.0).expect("finite motion");
    before.advance(Duration::from_secs_f64(frame * f64::from(frames - 1)));
    assert!(!before.is_settled());
    let mut after = before.clone();
    after.advance(Duration::from_secs_f64(frame));
    assert!(after.is_settled());
    let mut middle = before.clone();
    middle.advance(Duration::from_secs_f64(frame / 2.0));
    let difference = (after.value() - before.value()) / frame;
    let v = middle.velocity()[0];
    // Central-difference truncation for this spring is |x'''|·h²/24, far
    // below 1e-4 here; a snap would add up to 1e-3/h = 6e-2.
    assert!(
        (difference - v).abs() <= 1e-4,
        "finite difference {difference} across the rest boundary is not the velocity {v}"
    );
    assert_eq!(after.value(), 1.0, "arrival must reach the exact target");
    assert_eq!(after.velocity(), [0.0]);
}

fn seam_on_the_completing_frame() {
    let mut value =
        AnimatedValue::with_motion(0.0_f64, curve_spec(CUBICS[0], 200)).expect("finite motion");
    value.animate_to(10.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.2));
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
    let mut value = AnimatedValue::with_motion(0.0_f64, MotionSpec::Spring(spring(10.0, 0.5)))
        .expect("finite motion");
    value.animate_to(1.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.05));
    let x = value.value();
    value.advance(Duration::from_secs_f64(0.0));
    assert_eq!(value.value(), x, "zero duration moved time");
    value.advance(Duration::from_secs_f64(1e6));
    assert_eq!(value.value(), 1.0);
    assert!(value.value().is_finite() && value.velocity()[0].is_finite());
}

/// Same target, same spring: the seams are invisible. Reference values from
/// the closed form `e^{−5t}(cos ω_d t + (5/ω_d) sin ω_d t)`, ω_d = √75.
fn retargeting_to_the_same_target_is_invisible() {
    let mut value = AnimatedValue::with_motion(1.0_f64, MotionSpec::Spring(spring(10.0, 0.5)))
        .expect("finite motion");
    value.animate_to(0.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.1));
    value.animate_to(0.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.07));
    value.animate_to(0.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.08));
    let at_quarter = value.value();
    assert!(
        (at_quarter - -0.023_359_579_906_692_3).abs() <= 1e-12,
        "x(0.25) = {at_quarter}"
    );
    value.advance(Duration::from_secs_f64(0.75));
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
    let mut value = AnimatedValue::with_motion(0.0_f64, linear).expect("finite motion");
    value.animate_to(100.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.05));
    value.animate_to(0.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.049));
    assert!(!value.is_settled(), "a reversal at 50 ms lasts 50 ms");
    value.advance(Duration::from_secs_f64(0.001));
    assert!(value.is_settled(), "a reversal at 50 ms lasts 50 ms");
    assert_eq!(value.value(), 0.0);

    let ease = CUBICS[0];
    let shortened = eased(ease, 0.25);
    assert!(
        (shortened - 0.408_510_591_355_396).abs() <= 1e-12,
        "S' = {shortened}"
    );
    let mut value =
        AnimatedValue::with_motion(0.0_f64, curve_spec(ease, 200)).expect("finite motion");
    value.animate_to(100.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.05));
    value.animate_to(0.0).expect("finite motion");
    let first = 0.2 * shortened;
    value.advance(Duration::from_secs_f64(first - 1e-9));
    assert!(!value.is_settled(), "settled before {first} s");
    value.advance(Duration::from_secs_f64(2e-9));
    assert!(value.is_settled(), "not settled by {first} s");

    // A second reversal uses the first one's shortening as S.
    let mut value =
        AnimatedValue::with_motion(0.0_f64, curve_spec(ease, 200)).expect("finite motion");
    value.animate_to(100.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.05));
    value.animate_to(0.0).expect("finite motion");
    let into_first = first * 0.5;
    value.advance(Duration::from_secs_f64(into_first));
    value.animate_to(100.0).expect("finite motion");
    let twice = (eased(ease, into_first / first) * shortened + (1.0 - shortened)).abs();
    let second = 0.2 * twice;
    value.advance(Duration::from_secs_f64(second - 1e-9));
    assert!(!value.is_settled(), "settled before {second} s");
    value.advance(Duration::from_secs_f64(2e-9));
    assert!(value.is_settled(), "not settled by {second} s");
    assert_eq!(value.value(), 100.0);
}

/// A segment whose target is not the reversing-adjusted start runs its full
/// duration.
fn a_retarget_elsewhere_runs_the_full_duration() {
    let mut value =
        AnimatedValue::with_motion(0.0_f64, curve_spec(CUBICS[0], 200)).expect("finite motion");
    value.animate_to(100.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.05));
    value.animate_to(10.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.2 - 1e-9));
    assert!(!value.is_settled());
    value.advance(Duration::from_secs_f64(2e-9));
    assert_eq!(value.value(), 10.0);
}

fn linear_spec(millis: u64) -> MotionSpec {
    MotionSpec::Curve {
        duration: Duration::from_millis(millis),
        curve: ArcCurve::new(Curves::Linear),
    }
}

fn controller_velocity_boundaries() {
    for reverse in [false, true] {
        for duration in [Duration::ZERO, Duration::from_secs(1)] {
            let owner = AnimationController::builder(duration)
                .initial_value(if reverse { 1.0 } else { 0.0 })
                .build_on(Some(&Vsync::new()));
            let controller = owner.controller();
            let _run = if reverse {
                controller.animate_back_curved(0.0, Some(duration), Curves::Linear)
            } else {
                controller.animate_to_curved(1.0, Some(duration), Curves::Linear)
            }
            .expect("linear run");
            let expected = if duration.is_zero() {
                0.0
            } else if reverse {
                -1.0
            } else {
                1.0
            };
            assert_eq!(controller.velocity(), expected, "initial linear derivative");
            controller.tick_at(Duration::from_nanos(999_999_999));
            assert_eq!(
                controller.velocity(),
                expected,
                "linear derivative before completion"
            );
            controller.set_playback_rate(PlaybackRate::PAUSED);
            controller.tick_at(Duration::from_nanos(999_999_999));
            assert_eq!(controller.velocity(), 0.0);
        }
    }
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller();
    let _run = controller
        .animate_to_curved(1.0, None, Steps::new(4, JumpAt::End))
        .expect("discontinuous curve");
    for millis in [0, 100, 250, 500, 999, 1000] {
        controller.tick_at(Duration::from_millis(millis));
        assert!(
            controller.velocity().is_finite(),
            "finite step derivative at {millis}"
        );
        if millis == 100 {
            assert_eq!(controller.velocity(), 0.0, "constant step interval");
        }
    }
}

struct SuppliedSlope(f64);
impl Curve for SuppliedSlope {
    fn transform(&self, t: f64) -> f64 {
        t
    }
    fn slope(&self, _: f64) -> f64 {
        self.0
    }
}

fn controller_velocity_keeps_representable_products() {
    for (span, slope, duration, playback, expected) in [
        (1e308, 1e-308, Duration::from_nanos(1), 1.0, 1e9),
        (
            1.0,
            1e308,
            Duration::from_secs(10_000_000_000_000_000_000),
            1e10,
            1e299,
        ),
        (1e308, 1e-308, Duration::from_nanos(1), 1e-308, 1e-299),
        (1e308, 1.0, Duration::from_nanos(1), 1e-308, 1e9),
        (1.0, f64::NAN, Duration::from_secs(1), 1.0, 0.0),
        (1.0, f64::INFINITY, Duration::from_secs(1), 1.0, 0.0),
        (1.0, f64::NEG_INFINITY, Duration::from_secs(1), 1.0, 0.0),
    ] {
        let owner = AnimationController::builder(duration)
            .unbounded()
            .build_on(Some(&Vsync::new()));
        let controller = owner.controller();
        controller.set_playback_rate(PlaybackRate::new(playback).expect("finite playback"));
        let _run = controller
            .animate_to_curved(span, Some(duration), SuppliedSlope(slope))
            .expect("finite span");
        controller.tick_at(Duration::ZERO);
        let actual = controller.velocity();
        assert!(actual.is_finite());
        if expected == 0.0 {
            assert_eq!(actual, 0.0);
        } else {
            assert!(
                (actual / expected - 1.0).abs() < 1e-12,
                "expected {expected}, got {actual}"
            );
        }
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
        let mut value = AnimatedValue::with_motion(0.0_f64, spec.clone()).expect("finite motion");
        value.animate_to(1.0).expect("finite motion");
        value.advance(Duration::from_secs_f64(0.01));
        value.advance(Duration::MAX);
        value.advance(Duration::MAX);
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
    )
    .expect("finite motion");
    value.animate_to(1e308).expect("finite motion");
    assert!(!value.is_settled(), "snapped to the target at the seam");
    value.advance(Duration::from_secs_f64(0.5));
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
    )
    .expect("finite motion");
    value.animate_to(1e308).expect("finite motion");
    assert!(!value.is_settled(), "snapped to the target at the seam");
    assert_eq!(value.value(), 0.0, "the seam keeps the start value");
    value.advance(Duration::from_secs_f64(0.25));
    let (x, v) = (value.value(), value.velocity()[0]);
    assert!(x.is_finite() && v.is_finite(), "x {x}, v {v}");
    assert!(x > 0.0 && x < 1e308, "x {x} is not between the ends");
}

/// A near-overflow velocity handed to a long curve segment: the Hermite
/// correction is in range although `excess · duration` alone is not.
fn a_large_finite_curve_correction_stays_finite() {
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(1)).expect("finite motion");
    value.animate_to(1e305).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.0005));
    assert!(value.velocity()[0] >= 1e308, "v {}", value.velocity()[0]);
    value
        .set_motion(linear_spec(10_000))
        .expect("finite motion");
    for t in [0.1, 2.5, 5.0, 7.5, 9.9] {
        let mut probe = value.clone();
        probe.advance(Duration::from_secs_f64(t));
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
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(200)).expect("finite motion");
    value.animate_to(100.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.2 - 1e-6));
    let early = value.velocity()[0];
    assert!(early.abs() <= 0.1, "{early} units/s just before arrival");
    assert_velocity_is_the_derivative(&value, 0, &mode, 100.0);
    value.advance(Duration::from_secs_f64(1e-6));
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
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(200)).expect("finite motion");
    value.animate_to(100.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.0005));
    seam(&mut value, 0.0, &mode, 100.0);
    assert!(!value.is_settled());
    value.advance(Duration::from_secs_f64(0.01));
    assert!(value.is_settled());
    assert_eq!(value.value(), 0.0);
}

/// Assigning a curve its current target leaves the segment running on its
/// schedule instead of restarting the full duration.
fn retargeting_a_curve_to_its_target_keeps_its_schedule() {
    let mut value =
        AnimatedValue::with_motion(0.0_f64, curve_spec(CUBICS[0], 200)).expect("finite motion");
    value.animate_to(10.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.1));
    let (x, v) = (value.value(), value.velocity()[0]);
    value.animate_to(10.0).expect("finite motion");
    assert_eq!((value.value(), value.velocity()[0]), (x, v));
    value.advance(Duration::from_secs_f64(0.1));
    assert!(value.is_settled());
    assert_eq!(value.value(), 10.0);
}

#[test]
fn retarget_seams() {
    crate::run_table(&[
        (
            "controller velocity boundaries",
            controller_velocity_boundaries,
        ),
        (
            "controller velocity products",
            controller_velocity_keeps_representable_products,
        ),
        ("overflowing rates cancel", overflowing_rates_cancel),
        (
            "a_large_retarget_keeps_modest_velocity",
            a_large_retarget_keeps_modest_velocity,
        ),
        (
            "a_large_arrival_correction_is_weighted_before_scaling",
            a_large_arrival_correction_is_weighted_before_scaling,
        ),
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

#[test]
fn controller_retarget_is_c0_and_c1_at_the_seam() {
    crate::run_table(&[
        (
            "a_failed_tick_keeps_the_published_seam",
            a_failed_tick_keeps_the_published_seam,
        ),
        (
            "a_failed_tick_preserves_a_pending_playback_rate",
            a_failed_tick_preserves_a_pending_playback_rate,
        ),
        (
            "a_rate_requested_by_a_source_remains_pending",
            a_rate_requested_by_a_source_remains_pending,
        ),
        (
            "a_failed_completion_query_keeps_the_published_seam",
            a_failed_completion_query_keeps_the_published_seam,
        ),
        (
            "retarget_retries_a_changed_sample_without_publishing_stale_velocity",
            retarget_retries_a_changed_sample_without_publishing_stale_velocity,
        ),
        ("curve controller seam", controller_curve_seam),
        ("spring controller seam", controller_spring_seam),
        (
            "a_controller_curve_arrives_at_rest",
            a_controller_curve_arrives_at_rest,
        ),
        (
            "a_controller_spring_arrives_at_rest_independently_of_frames",
            a_controller_spring_arrives_at_rest_independently_of_frames,
        ),
        (
            "a_controller_spring_enters_and_leaves_rest_continuously",
            a_controller_spring_enters_and_leaves_rest_continuously,
        ),
        (
            "a_panicking_curve_slope_leaves_the_old_segment_running",
            a_panicking_curve_slope_leaves_the_old_segment_running,
        ),
        (
            "a_non_finite_controller_retarget_changes_nothing",
            a_non_finite_controller_retarget_changes_nothing,
        ),
        (
            "a_non_finite_inherited_velocity_changes_nothing",
            a_non_finite_inherited_velocity_changes_nothing,
        ),
        (
            "a_retarget_from_a_listener_is_ordered_and_lock_free",
            a_retarget_from_a_listener_is_ordered_and_lock_free,
        ),
        (
            "a_retarget_without_a_clock_settles_at_its_target",
            a_retarget_without_a_clock_settles_at_its_target,
        ),
    ]);
}

struct FailingQuadratic {
    panic: bool,
}

fn a_controller_spring_arrives_at_rest_independently_of_frames() {
    use flui_animation::{Animation, MotionClock};
    for zeta in [0.5, 1.0, 4.0] {
        for frame in [
            Duration::from_millis(1),
            Duration::from_millis(17),
            Duration::from_secs(20),
        ] {
            let registry = Vsync::new();
            let mut clock = MotionClock::new();
            let owner =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
            let controller = owner.controller();
            let mut run = controller
                .retarget(0.8, &MotionSpec::Spring(spring(10.0, zeta)))
                .expect("spring segment");
            registry.tick_all(&clock.frame(Duration::ZERO));
            let horizon = Duration::from_secs(20);
            let mut now = Duration::ZERO;
            while now < horizon {
                now = now.saturating_add(frame).min(horizon);
                registry.tick_all(&clock.frame(now));
            }
            assert_eq!(
                controller.value(),
                0.8,
                "zeta={zeta}, frame={frame:?}: completion must reach the same exact target"
            );
            assert_eq!(controller.velocity(), 0.0);
            assert!(matches!(poll_run(&mut run), std::task::Poll::Ready(Ok(()))));
        }
    }
}

fn a_controller_spring_enters_and_leaves_rest_continuously() {
    use flui_animation::Animation;
    for zeta in [0.5, 1.0, 4.0] {
        let sample = |nanos: u64| {
            let registry = Vsync::new();
            let owner =
                AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
            let controller = owner.controller();
            let _run = controller
                .retarget(0.8, &MotionSpec::Spring(spring(10.0, zeta)))
                .expect("spring segment");
            controller.tick_at(Duration::from_nanos(nanos));
            (
                controller.value(),
                controller.velocity(),
                !controller.is_animating(),
            )
        };
        // Find completion through the public run, without its private rest time.
        let (mut lo, mut hi) = (0, 20_000_000_000_u64);
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if sample(mid).2 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        assert!(sample(hi).2 && !sample(lo).2);
        // With omega=10 the inverse frequency is 100ms; default speed tolerance
        // cannot shorten that transition. Probe each join with actual positions.
        let start = hi - 100_000_000;
        let step = 1_000;
        for join in [start, hi] {
            let left = sample(join - step);
            let at = sample(join);
            let right = sample(join + step);
            let arriving = (at.0 - left.0) / 1e-6;
            let leaving = (right.0 - at.0) / 1e-6;
            assert!(
                (arriving - at.1).abs() < 1e-6 && (leaving - at.1).abs() < 1e-6,
                "zeta={zeta}, join={join}: position derivatives {arriving}, {leaving} must match velocity {}",
                at.1
            );
        }
        assert_eq!(sample(hi).0, 0.8);
        assert_eq!(sample(hi).1, 0.0);
        if zeta == 0.5 {
            let t = 0.25;
            let frequency = 75.0_f64.sqrt();
            let expected = 0.8
                * (1.0
                    - (-5.0_f64 * t).exp()
                        * ((frequency * t).cos() + 5.0 / frequency * (frequency * t).sin()));
            assert!(
                close(sample(250_000_000).0, expected, 1e-12),
                "native spring must be unchanged before its rest transition"
            );
        }
    }
}

fn a_controller_curve_arrives_at_rest() {
    use flui_animation::Animation;
    for moving in [false, true] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = owner.controller();
        if moving {
            let _old = controller.forward().expect("old run");
            controller.tick_at(Duration::from_millis(250));
        }
        let (x0, v0) = (controller.value(), controller.velocity());
        let mut run = controller
            .retarget(0.8, &linear_spec(1000))
            .expect("curve segment");
        let h = 1e-4;
        controller.tick_at(Duration::from_micros(999_900));
        let before = (controller.value(), controller.velocity());
        // For a cubic Hermite path, its endpoint acceleration is bounded by
        // 6*span + 4*initial velocity for this one-second segment.
        let acceleration_bound = 6.0 * (0.8 - x0).abs() + 4.0 * v0.abs();
        assert!(before.1.abs() <= acceleration_bound * h);
        controller.tick_at(Duration::from_secs(1));
        assert_eq!(controller.value(), 0.8);
        assert_eq!(controller.velocity(), 0.0);
        let arriving_velocity = (controller.value() - before.0) / h;
        assert!(
            arriving_velocity.abs() <= acceleration_bound * h,
            "the last position interval must approach rest, not hide a derivative jump by stopping: {arriving_velocity}"
        );
        assert!(matches!(poll_run(&mut run), std::task::Poll::Ready(Ok(()))));
    }
}

impl Curve for FailingQuadratic {
    fn transform(&self, t: f64) -> f64 {
        if t == 0.5 {
            assert!(!self.panic, "quadratic sample failure");
            return f64::NAN;
        }
        t * t
    }

    fn slope(&self, t: f64) -> f64 {
        2.0 * t
    }
}

fn a_failed_tick_keeps_the_published_seam() {
    use flui_animation::Animation;
    for panic in [false, true] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = owner.controller();
        let mut old = controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                FailingQuadratic { panic },
            )
            .expect("quadratic source");
        controller.tick_at(Duration::from_millis(250));
        let before = (controller.value(), controller.velocity());
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controller.tick_at(Duration::from_millis(500));
        }));
        assert_eq!(failure.is_err(), panic);
        assert_eq!(controller.value(), before.0);
        assert_eq!(
            controller.velocity(),
            before.1,
            "a rejected sample cannot move the published derivative"
        );
        assert!(poll_run(&mut old).is_pending());
        let mut next = controller
            .retarget(0.8, &linear_spec(1000))
            .expect("inherit the published seam");
        assert_eq!(controller.value(), before.0);
        assert!(close(controller.velocity(), before.1, 1e-12));
        assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Err(_))));
        controller.tick_at(Duration::from_secs(1));
        assert_eq!(controller.value(), 0.8);
        assert!(matches!(
            poll_run(&mut next),
            std::task::Poll::Ready(Ok(()))
        ));
    }
}

fn a_failed_tick_preserves_a_pending_playback_rate() {
    use flui_animation::Animation;
    for panic in [false, true] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let controller = owner.controller();
        let mut run = controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                FailingQuadratic { panic },
            )
            .expect("quadratic source");
        controller.tick_at(Duration::from_millis(250));
        controller.set_playback_rate(PlaybackRate::new(2.0).expect("finite rate"));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controller.tick_at(Duration::from_millis(500));
        }));
        assert_eq!(failure.is_err(), panic);
        assert_eq!(controller.playback_rate(), PlaybackRate::NORMAL);
        assert_eq!(controller.velocity(), 0.5);
        controller.tick_at(Duration::from_millis(750));
        assert_eq!(
            controller.value(),
            0.5625,
            "the successful sample uses the old rate up to its epoch"
        );
        assert_eq!(controller.playback_rate().get(), 2.0);
        assert_eq!(controller.velocity(), 3.0);
        controller.tick_at(Duration::from_millis(875));
        assert_eq!(controller.value(), 1.0);
        assert!(matches!(poll_run(&mut run), std::task::Poll::Ready(Ok(()))));
    }
}

struct FailingCompletion;

struct RateChangingCurve {
    controller: AnimationController,
    armed: std::cell::Cell<bool>,
}

impl Curve for RateChangingCurve {
    fn transform(&self, t: f64) -> f64 {
        use flui_animation::Animation;
        if self.armed.replace(false) {
            assert_eq!(self.controller.value(), 0.0);
            assert_eq!(self.controller.velocity(), 1.0);
            assert_eq!(self.controller.playback_rate(), PlaybackRate::NORMAL);
            self.controller
                .set_playback_rate(PlaybackRate::new(3.0).expect("finite rate"));
        }
        t
    }

    fn slope(&self, _: f64) -> f64 {
        1.0
    }
}

fn a_rate_requested_by_a_source_remains_pending() {
    use flui_animation::Animation;
    let registry = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let controller = owner.controller();
    let mut run = controller
        .animate_to_curved(
            1.0,
            Some(Duration::from_secs(1)),
            RateChangingCurve {
                controller: controller.clone(),
                armed: std::cell::Cell::new(true),
            },
        )
        .expect("source can request a newer rate");
    controller.set_playback_rate(PlaybackRate::new(2.0).expect("finite rate"));
    controller.tick_at(Duration::from_millis(250));
    assert_eq!(controller.value(), 0.25);
    assert_eq!(controller.playback_rate().get(), 2.0);
    assert_eq!(controller.velocity(), 2.0);
    controller.tick_at(Duration::from_millis(375));
    assert_eq!(controller.value(), 0.5, "the accepted interval uses rate 2");
    assert_eq!(controller.playback_rate().get(), 3.0);
    assert_eq!(controller.velocity(), 3.0);
    controller.tick_at(Duration::from_millis(500));
    assert_eq!(
        controller.value(),
        0.875,
        "the subsequent interval uses rate 3"
    );
    controller.tick_at(Duration::from_secs(1));
    assert!(matches!(poll_run(&mut run), std::task::Poll::Ready(Ok(()))));
}

impl flui_animation::Simulation for FailingCompletion {
    fn x(&self, t: f64) -> f64 {
        t * t
    }

    fn dx(&self, t: f64) -> f64 {
        2.0 * t
    }

    fn is_done(&self, t: f64) -> bool {
        assert_ne!(t, 0.5, "completion query failure");
        t >= 1.0
    }
}

fn a_failed_completion_query_keeps_the_published_seam() {
    use flui_animation::Animation;
    let registry = Vsync::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let controller = owner.controller();
    let mut old = controller
        .animate_with(FailingCompletion)
        .expect("quadratic simulation");
    controller.tick_at(Duration::from_millis(250));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        controller.tick_at(Duration::from_millis(500));
    }));
    assert!(failure.is_err());
    assert_eq!(controller.value(), 0.0625);
    assert_eq!(controller.velocity(), 0.5);
    let _next = controller
        .retarget(0.8, &linear_spec(1000))
        .expect("published seam");
    assert_eq!(controller.value(), 0.0625);
    assert_eq!(controller.velocity(), 0.5);
    assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Err(_))));
}

struct ReplacingSlope {
    controller: AnimationController,
    calls: std::rc::Rc<std::cell::Cell<u32>>,
    limit: u32,
    invalid_return: bool,
    latest: std::rc::Rc<std::cell::RefCell<Option<flui_animation::AnimationRunFuture>>>,
}

impl Curve for ReplacingSlope {
    fn transform(&self, t: f64) -> f64 {
        t
    }
    fn slope(&self, _: f64) -> f64 {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call < self.limit {
            let next = self
                .controller
                .animate_to_curved(
                    0.9,
                    Some(Duration::from_secs(1)),
                    Self {
                        controller: self.controller.clone(),
                        calls: self.calls.clone(),
                        limit: self.limit,
                        invalid_return: self.invalid_return,
                        latest: self.latest.clone(),
                    },
                )
                .expect("source callback replaces the run");
            let outgoing = self.latest.borrow_mut().replace(next);
            drop(outgoing);
            if self.invalid_return {
                return f64::NAN;
            }
        }
        1.0
    }
}

fn retarget_retries_a_changed_sample_without_publishing_stale_velocity() {
    use flui_animation::{Animation, AnimationError};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    for (limit, invalid_return) in [(1, false), (2, false), (1, true), (2, true)] {
        let owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
        let controller = owner.controller();
        let calls = Rc::new(Cell::new(0));
        let latest = Rc::new(RefCell::new(None));
        let mut original = controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                ReplacingSlope {
                    controller: controller.clone(),
                    calls: calls.clone(),
                    limit,
                    invalid_return,
                    latest: latest.clone(),
                },
            )
            .expect("old source");
        controller.tick_at(Duration::from_millis(250));
        let result = controller.retarget(
            0.7,
            &MotionSpec::Curve {
                duration: Duration::from_secs(1),
                curve: ArcCurve::new(Curves::Linear),
            },
        );
        assert_eq!(calls.get(), 2, "one retry queries the new source");
        assert!(matches!(
            poll_run(&mut original),
            std::task::Poll::Ready(Err(_))
        ));
        assert_eq!(controller.value(), 0.25);
        assert!(
            close(controller.velocity(), 0.65, 1e-12),
            "the old velocity is not published"
        );
        let mut latest = latest.borrow_mut().take().expect("source callback's run");
        if limit == 1 {
            let mut admitted = result.expect("one replacement is retried");
            assert!(matches!(
                poll_run(&mut latest),
                std::task::Poll::Ready(Err(_))
            ));
            controller.tick_at(Duration::from_secs(1));
            assert_eq!(controller.value(), 0.7);
            assert!(matches!(
                poll_run(&mut admitted),
                std::task::Poll::Ready(Ok(()))
            ));
        } else {
            assert!(matches!(result, Err(AnimationError::ReentrantMotion)));
            assert!(
                poll_run(&mut latest).is_pending(),
                "latest user-installed run survives refusal"
            );
            controller.tick_at(Duration::from_secs(1));
            assert_eq!(controller.value(), 0.9);
            assert!(matches!(
                poll_run(&mut latest),
                std::task::Poll::Ready(Ok(()))
            ));
        }
    }
}

fn poll_run(
    run: &mut flui_animation::AnimationRunFuture,
) -> std::task::Poll<Result<(), flui_animation::RunCanceled>> {
    use std::future::Future;
    std::pin::Pin::new(run).poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

struct PanickingSlope;
impl Curve for PanickingSlope {
    fn transform(&self, t: f64) -> f64 {
        t
    }
    fn slope(&self, _: f64) -> f64 {
        panic!("retarget slope failure")
    }
}

fn a_panicking_curve_slope_leaves_the_old_segment_running() {
    use flui_animation::Animation;
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller();
    let mut old = controller.forward().expect("old run");
    controller.tick_at(Duration::from_millis(250));
    let motion = MotionSpec::Curve {
        duration: Duration::from_secs(1),
        curve: ArcCurve::new(PanickingSlope),
    };
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        controller.retarget(0.8, &motion)
    }))
    .expect_err("new curve refuses by panic");
    assert_eq!(
        failure.downcast_ref::<&str>().copied(),
        Some("retarget slope failure")
    );
    assert!(poll_run(&mut old).is_pending());
    assert_eq!(controller.value(), 0.25);
    controller.tick_at(Duration::from_millis(500));
    assert_eq!(controller.value(), 0.5, "old run remains installed");
    controller.tick_at(Duration::from_secs(1));
    assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Ok(()))));
}

fn a_non_finite_controller_retarget_changes_nothing() {
    use flui_animation::{Animation, AnimationError};
    for target in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
        let controller = owner.controller();
        let mut old = controller.forward().expect("old run");
        controller.tick_at(Duration::from_millis(250));
        let motion = MotionSpec::Spring(spring(10.0, 0.5));
        assert!(matches!(
            controller.retarget(target, &motion),
            Err(AnimationError::NonFiniteTarget(_))
        ));
        assert_eq!(controller.value(), 0.25);
        assert!(poll_run(&mut old).is_pending());
        controller.tick_at(Duration::from_secs(1));
        assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Ok(()))));
    }
}

struct ConstantSlope(f64);

impl Curve for ConstantSlope {
    fn transform(&self, t: f64) -> f64 {
        t
    }

    fn slope(&self, _: f64) -> f64 {
        self.0
    }
}

fn a_non_finite_inherited_velocity_changes_nothing() {
    use flui_animation::{Animation, AnimationError};
    for (slope, duration) in [
        (f64::NAN, Duration::from_secs(1)),
        (f64::INFINITY, Duration::from_secs(1)),
        (f64::NEG_INFINITY, Duration::from_secs(1)),
        (f64::MAX, Duration::from_nanos(1)),
    ] {
        let registry = Vsync::new();
        let owner = AnimationController::builder(duration).build_on(Some(&registry));
        let controller = owner.controller();
        let mut old = controller
            .animate_to_curved(1.0, Some(duration), ConstantSlope(slope))
            .expect("position source");
        let generation = controller.run_generation();
        assert!(
            matches!(
                controller.retarget(0.8, &linear_spec(1000)),
                Err(AnimationError::NonFiniteTarget(_))
            ),
            "invalid inherited slope {slope} over {duration:?} must refuse"
        );
        assert_eq!(controller.value(), 0.0);
        assert_eq!(controller.run_generation(), generation);
        assert!(poll_run(&mut old).is_pending());
        controller.tick_at(duration);
        assert_eq!(controller.value(), 1.0);
        assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Ok(()))));
    }
}

fn a_retarget_from_a_listener_is_ordered_and_lock_free() {
    use flui_animation::{Animation, AnimationStatus};
    use flui_foundation::Listenable;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller();
    let mut old = controller.forward().expect("old run");
    let canceled = Rc::new(Cell::new(false));
    let sink = canceled.clone();
    let observer = controller.clone();
    old.when_complete_or_cancel(move |result| {
        assert!(result.is_err());
        assert_eq!(observer.status(), AnimationStatus::Reverse);
        assert!(
            observer.is_animating(),
            "replacement is installed before cancellation"
        );
        sink.set(true);
    });
    let next = Rc::new(RefCell::new(None));
    let sink = next.clone();
    let observer = controller.clone();
    controller.add_listener(Rc::new(move || {
        if sink.borrow().is_some() {
            return;
        }
        let run = observer
            .retarget(
                0.0,
                &MotionSpec::Curve {
                    duration: Duration::from_millis(100),
                    curve: ArcCurve::new(Curves::EaseIn),
                },
            )
            .expect("retarget from value callback");
        *sink.borrow_mut() = Some(run);
    }));
    controller.tick_at(Duration::from_millis(250));
    assert!(canceled.get());
    assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Err(_))));
    assert_eq!(controller.value(), 0.25);
    assert_eq!(controller.velocity(), 1.0);
    controller.tick_at(Duration::from_millis(100));
    assert_eq!(controller.value(), 0.0);
    let mut next = next.borrow_mut().take().expect("replacement future");
    assert!(matches!(
        poll_run(&mut next),
        std::task::Poll::Ready(Ok(()))
    ));
}

fn a_retarget_without_a_clock_settles_at_its_target() {
    use flui_animation::{Animation, AnimationError};
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let controller = owner.controller().clone();
    let mut run = controller
        .retarget(
            0.8,
            &MotionSpec::Curve {
                duration: Duration::from_secs(1000),
                curve: ArcCurve::new(Curves::EaseIn),
            },
        )
        .expect("clockless retarget");
    assert_eq!(controller.value(), 0.8);
    assert!(matches!(poll_run(&mut run), std::task::Poll::Ready(Ok(()))));
    owner.dispose();
    assert!(matches!(
        controller.retarget(0.0, &MotionSpec::Spring(spring(10.0, 0.5))),
        Err(AnimationError::Disposed)
    ));
}

fn controller_curve_seam() {
    use flui_animation::Animation;
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&Vsync::new()));
    let controller = owner.controller();
    let _old = controller.forward().expect("initial linear run");
    controller.tick_at(Duration::from_millis(250));
    let before = (controller.value(), controller.velocity());
    let _new = controller
        .retarget(
            0.8,
            &MotionSpec::Curve {
                duration: Duration::from_secs(1),
                curve: ArcCurve::new(Curves::EaseIn),
            },
        )
        .expect("curve retarget");
    assert!(close(controller.value(), before.0, 1e-12), "position seam");
    assert!(
        close(controller.velocity(), before.1, 1e-9),
        "velocity seam: {} -> {}",
        before.1,
        controller.velocity()
    );
}

fn controller_spring_seam() {
    use flui_animation::Animation;
    let owner = AnimationController::builder(Duration::from_secs(1))
        .initial_value(1.0)
        .unbounded()
        .build_on(Some(&Vsync::new()));
    let controller = owner.controller();
    let motion = MotionSpec::Spring(spring(10.0, 0.5));
    let _old = controller.retarget(0.0, &motion).expect("initial spring");
    controller.tick_at(Duration::from_millis(100));
    let before = (controller.value(), controller.velocity());
    assert!(before.1.abs() > 1.0, "moving spring");
    let _new = controller
        .retarget(0.0, &motion)
        .expect("same spring target");
    assert!(close(controller.value(), before.0, 1e-12), "position seam");
    assert!(
        close(controller.velocity(), before.1, 1e-9),
        "velocity seam: {} -> {}",
        before.1,
        controller.velocity()
    );
}

#[test]
fn controller_retarget_frame_boundaries() {
    crate::run_table(&[
        (
            "repeated_retargets_before_a_frame_share_the_seam",
            repeated_retargets_before_a_frame_share_the_seam,
        ),
        (
            "retarget_on_the_completing_frame_starts_at_rest",
            retarget_on_the_completing_frame_starts_at_rest,
        ),
        (
            "a_retarget_after_idle_starts_on_the_next_frame",
            a_retarget_after_idle_starts_on_the_next_frame,
        ),
        (
            "retarget_after_a_failed_frame_uses_the_published_time",
            retarget_after_a_failed_frame_uses_the_published_time,
        ),
        (
            "the_frame_after_a_retarget_has_no_hold",
            the_frame_after_a_retarget_has_no_hold,
        ),
        (
            "retargeting_another_controller_preserves_its_frame_origin",
            retargeting_another_controller_preserves_its_frame_origin,
        ),
    ]);
}

fn repeated_retargets_before_a_frame_share_the_seam() {
    use flui_animation::{Animation, MotionClock};
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let owner = AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build_on(Some(&registry));
    let controller = owner.controller();
    let mut displaced = controller
        .animate_to(1.0, Some(Duration::from_secs(1)))
        .expect("initial run");
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(100)));
    let before = (controller.value(), controller.velocity());
    for (target, motion) in [
        (0.9, linear_spec(1000)),
        (0.0, MotionSpec::Spring(spring(10.0, 0.5))),
        (0.7, linear_spec(1000)),
        (0.0, MotionSpec::Spring(spring(10.0, 0.5))),
    ] {
        let next = controller.retarget(target, &motion).expect("next seam");
        assert_eq!(controller.value(), before.0);
        assert!(close(controller.velocity(), before.1, 1e-12));
        assert!(matches!(
            poll_run(&mut displaced),
            std::task::Poll::Ready(Err(_))
        ));
        displaced = next;
    }
    registry.tick_all(&clock.frame(Duration::from_micros(100_100)));
    let average = (controller.value() - before.0) / 1e-4;
    let acceleration_bound = 100.0 * before.0.abs() + 10.0 * before.1.abs();
    assert!(
        (average - before.1).abs() <= acceleration_bound * 1e-4,
        "each replacement before a tick retains the original seam time: {average} vs {}",
        before.1
    );
    registry.tick_all(&clock.frame(Duration::from_secs(10)));
    assert!(matches!(
        poll_run(&mut displaced),
        std::task::Poll::Ready(Ok(()))
    ));
}

fn retarget_on_the_completing_frame_starts_at_rest() {
    use flui_animation::{Animation, MotionClock};
    use flui_foundation::Listenable;
    use std::cell::RefCell;
    use std::rc::Rc;
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
    let controller = owner.controller();
    let mut old = controller.forward().expect("initial run");
    let next = Rc::new(RefCell::new(None));
    let sink = next.clone();
    let observer = controller.clone();
    controller.add_listener(Rc::new(move || {
        if observer.value() == 1.0 && sink.borrow().is_none() {
            assert_eq!(observer.velocity(), 0.0, "completed source is at rest");
            let run = observer
                .retarget(0.0, &MotionSpec::Spring(spring(10.0, 0.5)))
                .expect("new run from completion");
            *sink.borrow_mut() = Some(run);
        }
    }));
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(100)));
    assert!(matches!(poll_run(&mut old), std::task::Poll::Ready(Ok(()))));
    assert_eq!(controller.value(), 1.0);
    assert_eq!(controller.velocity(), 0.0);
    registry.tick_all(&clock.frame(Duration::from_micros(100_100)));
    assert!(
        controller.value() < 1.0,
        "completion-frame retarget advances on the next frame"
    );
    let mut next = next.borrow_mut().take().expect("run installed by callback");
    registry.tick_all(&clock.frame(Duration::from_secs(10)));
    assert!(matches!(
        poll_run(&mut next),
        std::task::Poll::Ready(Ok(()))
    ));
}

fn a_retarget_after_idle_starts_on_the_next_frame() {
    use flui_animation::{Animation, MotionClock};
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
    let controller = owner.controller();
    let _old = controller.forward().expect("initial run");
    for millis in [0, 100, 200, 300] {
        registry.tick_all(&clock.frame(Duration::from_millis(millis)));
    }
    let _next = controller
        .retarget(0.0, &MotionSpec::Spring(spring(10.0, 0.5)))
        .expect("new run after idle");
    registry.tick_all(&clock.frame(Duration::from_millis(400)));
    assert_eq!(
        controller.value(),
        1.0,
        "idle run anchors on its first observed frame"
    );
    registry.tick_all(&clock.frame(Duration::from_millis(500)));
    assert!(controller.value() < 1.0);
}

fn retarget_after_a_failed_frame_uses_the_published_time() {
    use flui_animation::{Animation, MotionClock};
    for panic in [false, true] {
        let registry = Vsync::new();
        let mut clock = MotionClock::new();
        let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let reference = AnimationController::builder(Duration::from_secs(1))
            .unbounded()
            .build_on(Some(&Vsync::new()));
        let controller = owner.controller();
        let _old = controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                FailingQuadratic { panic },
            )
            .expect("quadratic source");
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_millis(250)));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.tick_all(&clock.frame(Duration::from_millis(500)));
        }));
        assert_eq!(failure.is_err(), panic);
        let motion = MotionSpec::Spring(spring(10.0, 0.5));
        let _next = controller.retarget(0.8, &motion).expect("published seam");
        // Build the same physical seam on an independent manual controller.
        // Its old quadratic source publishes only the successful 250 ms tick.
        let _reference_old = reference
            .controller()
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                FailingQuadratic { panic: false },
            )
            .expect("reference source");
        reference.controller().tick_at(Duration::from_millis(250));
        let _reference_new = reference
            .controller()
            .retarget(0.8, &motion)
            .expect("reference seam");
        reference.controller().tick_at(Duration::from_millis(350));
        registry.tick_all(&clock.frame(Duration::from_millis(600)));
        assert!(
            close(controller.value(), reference.controller().value(), 1e-12),
            "the next sample is 350 ms after the published seam, rather than 100 ms after the rejected frame: {} vs {}",
            controller.value(),
            reference.controller().value()
        );
    }
}

fn retargeting_another_controller_preserves_its_frame_origin() {
    use flui_animation::{Animation, MotionClock};
    use flui_foundation::Listenable;
    use std::cell::Cell;
    use std::rc::Rc;
    for target_first in [false, true] {
        let registry = Vsync::new();
        let mut clock = MotionClock::new();
        let create =
            || AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let (trigger, target) = if target_first {
            let target = create();
            let trigger = create();
            (trigger, target)
        } else {
            (create(), create())
        };
        let _trigger_run = trigger.controller().forward().expect("trigger run");
        let _target_run = target.controller().forward().expect("target run");
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_millis(100)));
        let seam = Rc::new(Cell::new(None));
        let sink = seam.clone();
        let observer = target.controller().clone();
        trigger.controller().add_listener(Rc::new(move || {
            if sink.get().is_some() {
                return;
            }
            let before = (observer.value(), observer.velocity());
            let _next = observer
                .retarget(0.0, &MotionSpec::Spring(spring(10.0, 0.5)))
                .expect("retarget from another controller");
            assert!(close(observer.value(), before.0, 1e-12));
            assert!(close(observer.velocity(), before.1, 1e-9));
            sink.set(Some(before));
        }));
        registry.tick_all(&clock.frame(Duration::from_micros(100_100)));
        let (x0, v0) = seam.get().expect("trigger delivered retarget");
        if target_first {
            assert!(
                close(target.controller().value(), x0, 1e-12),
                "a visited controller is not sampled twice"
            );
            registry.tick_all(&clock.frame(Duration::from_micros(100_200)));
        }
        let average = (target.controller().value() - x0) / 1e-4;
        let acceleration_bound = 100.0 * x0.abs() + 10.0 * v0.abs();
        assert!(
            (average - v0).abs() <= acceleration_bound * 1e-4,
            "target_first={target_first}: a frame cannot hold at the seam, {average} vs {v0}"
        );
    }
}

fn the_frame_after_a_retarget_has_no_hold() {
    use flui_animation::{Animation, MotionClock};
    let registry = Vsync::new();
    let mut clock = MotionClock::new();
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let controller = owner.controller();
    let _old = controller.forward().expect("initial run");
    registry.tick_all(&clock.frame(Duration::ZERO));
    registry.tick_all(&clock.frame(Duration::from_millis(100)));
    let before = (controller.value(), controller.velocity());
    assert!(before.0 > 0.0 && before.1 > 0.0, "moving before the seam");
    let _next = controller
        .retarget(0.0, &MotionSpec::Spring(spring(10.0, 0.5)))
        .expect("spring inherits forward velocity before reversing");
    let dt = Duration::from_micros(100);
    registry.tick_all(
        &clock.frame(
            Duration::from_millis(100)
                .checked_add(dt)
                .expect("frame time"),
        ),
    );
    let average_velocity = (controller.value() - before.0) / dt.as_secs_f64();
    let acceleration_bound = 100.0 * before.0.abs() + 10.0 * before.1.abs();
    assert!(
        (average_velocity - before.1).abs() <= acceleration_bound * dt.as_secs_f64(),
        "the next frame must advance from the seam: old velocity {}, frame velocity {average_velocity}",
        before.1,
    );
}

fn overflowing_rates_cancel() {
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(1000)).expect("finite motion");
    value.animate_to(-2e307).expect("finite target");
    value.advance(Duration::from_millis(500));
    value.animate_to(1.6e308).expect("finite target");
    value.advance(Duration::from_millis(230));
    assert!(
        close(value.velocity()[0], 1.73481e308, 1e-12),
        "{:?}",
        value.velocity()
    );
}

fn a_large_retarget_keeps_modest_velocity() {
    let mut value = AnimatedValue::with_motion(0.0_f64, linear_spec(1000)).expect("finite motion");
    value.animate_to(1.0).expect("finite motion");
    value.advance(Duration::from_secs_f64(0.5));
    assert_eq!(value.velocity()[0], 1.5);
    let before = value.value();
    value.animate_to(1e308).expect("finite motion");
    assert_eq!(value.value(), before);
    assert_eq!(
        value.velocity()[0],
        1.5,
        "the inherited velocity survives a much larger curve rate"
    );
}

fn a_large_arrival_correction_is_weighted_before_scaling() {
    let mut value = AnimatedValue::with_motion(
        0.0_f64,
        MotionSpec::Curve {
            duration: Duration::from_secs(1),
            curve: ArcCurve::new(Cubic::new(0.25, 0.0, 0.75, 0.5)),
        },
    )
    .expect("finite motion");
    value.animate_to(1e308).expect("finite motion");
    assert_eq!(
        value.value(),
        0.0,
        "an overflowing unweighted coefficient must not snap the segment"
    );
    assert!(!value.is_settled());
    value.advance(Duration::from_secs_f64(0.5));
    // At Bézier parameter 1/2, x = 1/2 and y = 5/16. The endpoint slope is 2,
    // so its zero-velocity arrival correction adds 2 * (1/2)^2 * (1/2) = 1/4.
    assert!(close(value.value() / 1e308, 0.5625, 1e-8));
    value.advance(Duration::from_secs_f64(0.5));
    assert_eq!(value.value(), 1e308);
    assert_eq!(value.velocity()[0], 0.0);
    assert!(value.is_settled());
}
