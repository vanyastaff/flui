//! Authored easing is immutable, canonical, finite and independent of query order.

use flui_animation::{
    Curve, LinearStop, PiecewiseLinear, PiecewiseLinearError, SharedCurve, SimulationError,
    SimulationParameter, SpringCurve, SpringDescription, Tolerance,
};
use std::time::Duration;

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

fn missing_stops_are_evenly_placed() {
    let curve = PiecewiseLinear::new(&[
        LinearStop::new(0.0),
        LinearStop::new(0.25),
        LinearStop::new(0.75),
        LinearStop::new(0.9).at(0.6),
        LinearStop::new(1.0),
    ])
    .unwrap();
    for (input, expected) in [(0.1, 0.125), (0.3, 0.5), (0.5, 0.825), (0.8, 0.95)] {
        close(curve.transform(input), expected);
    }
    let explicit = PiecewiseLinear::new(&[
        LinearStop::new(0.0).at(0.0),
        LinearStop::new(0.25).at(0.2),
        LinearStop::new(0.75).at(0.4),
        LinearStop::new(0.9).at(0.6),
        LinearStop::new(1.0).at(1.0),
    ])
    .unwrap();
    // Decimal ratios can differ by an ulp during canonicalization. Compare
    // exact value equality on binary-exact placements in its dedicated case.
    close(explicit.transform(0.3), curve.transform(0.3));
}

fn decreasing_inputs_form_a_right_continuous_jump() {
    let curve = PiecewiseLinear::new(&[
        LinearStop::new(0.0),
        LinearStop::new(0.2).at(0.4),
        LinearStop::new(0.5),
        LinearStop::new(0.7).at(0.3),
        LinearStop::new(1.0),
    ])
    .unwrap();
    close(curve.transform(0.2), 0.1);
    assert_eq!(curve.transform(0.4), 0.7);
    close(curve.transform(0.7), 0.85);
    close(curve.slope(0.2), 0.5);
    assert_eq!(curve.slope(0.4), 0.0, "a jump inherits no derivative");
}

fn plateaus_overshoot_and_knot_slopes() {
    let curve = PiecewiseLinear::new(&[
        LinearStop::new(0.0),
        LinearStop::new(0.5).at(0.25),
        LinearStop::new(0.5).at(0.5),
        LinearStop::new(1.25).at(0.75),
        LinearStop::new(1.0),
    ])
    .unwrap();
    for (input, expected, slope) in [
        (0.125, 0.25, 2.0),
        (0.25, 0.5, 0.0),
        (0.375, 0.5, 0.0),
        (0.625, 0.875, 3.0),
        (0.75, 1.25, -1.0),
        (0.875, 1.125, -1.0),
    ] {
        close(curve.transform(input), expected);
        close(curve.slope(input), slope);
    }
}

fn refused_stops_are_typed() {
    assert_eq!(
        PiecewiseLinear::new(&[]).unwrap_err(),
        PiecewiseLinearError::TooFewStops
    );
    for (input, expected) in [
        (f64::NAN, PiecewiseLinearError::NonFiniteInput { index: 1 }),
        (
            f64::NEG_INFINITY,
            PiecewiseLinearError::NonFiniteInput { index: 1 },
        ),
        (
            f64::INFINITY,
            PiecewiseLinearError::NonFiniteInput { index: 1 },
        ),
        (
            -0.1,
            PiecewiseLinearError::InputOutOfRange {
                index: 1,
                value: -0.1,
            },
        ),
        (
            1.1,
            PiecewiseLinearError::InputOutOfRange {
                index: 1,
                value: 1.1,
            },
        ),
    ] {
        assert_eq!(
            PiecewiseLinear::new(&[
                LinearStop::new(0.0),
                LinearStop::new(0.5).at(input),
                LinearStop::new(1.0)
            ])
            .unwrap_err(),
            expected
        );
    }
    for output in [f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
        assert_eq!(
            PiecewiseLinear::new(&[
                LinearStop::new(0.0),
                LinearStop::new(output),
                LinearStop::new(1.0)
            ])
            .unwrap_err(),
            PiecewiseLinearError::NonFiniteOutput { index: 1 }
        );
    }
    for stops in [
        [LinearStop::new(0.2), LinearStop::new(1.0)],
        [LinearStop::new(0.0).at(0.1), LinearStop::new(1.0)],
        [LinearStop::new(0.0), LinearStop::new(0.8)],
    ] {
        assert_eq!(
            PiecewiseLinear::new(&stops).unwrap_err(),
            PiecewiseLinearError::InvalidEndpoints
        );
    }
}

fn extreme_outputs_and_intervals_stay_finite() {
    let curve = PiecewiseLinear::new(&[
        LinearStop::new(0.0),
        LinearStop::new(f64::MAX).at(0.25),
        LinearStop::new(-f64::MAX).at(0.75),
        LinearStop::new(1.0),
    ])
    .unwrap();
    assert_eq!(
        curve.transform(0.5),
        0.0,
        "opposite extremes retain their midpoint"
    );
    for input in [0.1, 0.25, 0.375, 0.625, 0.75, 0.875] {
        assert!(curve.transform(input).is_finite());
        assert!(curve.slope(input).is_finite());
    }
    let tiny = PiecewiseLinear::new(&[
        LinearStop::new(0.0),
        LinearStop::new(0.5).at(f64::from_bits(1)),
        LinearStop::new(1.0),
    ])
    .unwrap();
    assert!(tiny.slope(0.0).is_finite());
    assert_eq!(tiny.transform(f64::from_bits(1)), 0.5);
}

fn curves_keep_normalized_input_policy_and_value_equality() {
    let first = PiecewiseLinear::new(&[
        LinearStop::new(0.0),
        LinearStop::new(0.25),
        LinearStop::new(1.0),
    ])
    .unwrap();
    let explicit = PiecewiseLinear::new(&[
        LinearStop::new(0.0).at(0.0),
        LinearStop::new(0.25).at(0.5),
        LinearStop::new(1.0).at(1.0),
    ])
    .unwrap();
    assert_eq!(SharedCurve::new(first.clone()), SharedCurve::new(explicit));
    let spring = SpringDescription::new(1.0, 100.0, 6.0).unwrap();
    let one = SpringCurve::new(spring, Tolerance::DEFAULT).unwrap();
    let two = SpringCurve::new(spring, Tolerance::DEFAULT).unwrap();
    assert_eq!(SharedCurve::new(one.clone()), SharedCurve::new(two));
    for curve in [SharedCurve::new(first), SharedCurve::new(one)] {
        for (input, expected) in [
            (f64::NEG_INFINITY, 0.0),
            (-1.0, 0.0),
            (0.0, 0.0),
            (1.0, 1.0),
            (2.0, 1.0),
            (f64::INFINITY, 1.0),
        ] {
            assert_eq!(curve.transform(input), expected);
        }
        for input in [-1.0, 2.0, f64::NEG_INFINITY, f64::INFINITY] {
            assert_eq!(curve.slope(input), 0.0);
        }
        assert!(curve.transform(f64::NAN).is_nan());
        assert!(curve.slope(f64::NAN).is_nan());
        let before = curve.transform(0.25);
        curve.transform(0.9);
        curve.transform(0.1);
        assert_eq!(curve.transform(0.25), before);
    }
}

fn spring_matches_independent_physical_references() {
    for damping in [10.0, 20.0, 40.0] {
        let spring = SpringDescription::new(1.0, 100.0, damping).unwrap();
        let curve = SpringCurve::new(spring, Tolerance::new(1e-6, 1e-6).unwrap()).unwrap();
        let omega: f64 = 10.0;
        let zeta = damping / 20.0;
        for time in [0.05, 0.1, 0.25, 0.3] {
            let (position, velocity) = if zeta < 1.0 {
                let wd = omega * (1.0 - zeta * zeta).sqrt();
                let decay = (-zeta * omega * time).exp();
                let phase = wd * time;
                (
                    1.0 - decay * (phase.cos() + zeta * omega / wd * phase.sin()),
                    decay * omega * omega / wd * phase.sin(),
                )
            } else if zeta == 1.0 {
                let decay = (-omega * time).exp();
                (
                    1.0 - (1.0 + omega * time) * decay,
                    omega * omega * time * decay,
                )
            } else {
                let slow = -omega * (zeta - (zeta * zeta - 1.0).sqrt());
                let fast = -omega * (zeta + (zeta * zeta - 1.0).sqrt());
                let a = fast / (slow - fast);
                let b = -slow / (slow - fast);
                (
                    1.0 + a * (slow * time).exp() + b * (fast * time).exp(),
                    a * slow * (slow * time).exp() + b * fast * (fast * time).exp(),
                )
            };
            let progress = time / curve.duration().as_secs_f64();
            close(curve.transform(progress), position);
            close(
                curve.slope(progress),
                velocity * curve.duration().as_secs_f64(),
            );
        }
        assert_eq!(curve.slope(1.0), 0.0);
    }
    let overshoot = SpringCurve::new(
        SpringDescription::new(1.0, 100.0, 6.0).unwrap(),
        Tolerance::DEFAULT,
    )
    .unwrap();
    assert!(overshoot.transform(0.33 / overshoot.duration().as_secs_f64()) > 1.0);
}

fn spring_refuses_unusable_duration_and_admits_nanosecond_motion() {
    let spring = SpringDescription::new(1.0, 100.0, 20.0).unwrap();
    assert!(matches!(
        SpringCurve::new(spring, Tolerance::new(1.0, 1.0).unwrap()),
        Err(SimulationError::OutOfRange {
            parameter: SimulationParameter::UnitDistance,
            value: 1.0
        })
    ));
    let slow = SpringDescription::with_damping_ratio(1.0, 1e-40, 1.0).unwrap();
    assert_eq!(
        SpringCurve::new(slow, Tolerance::DEFAULT).unwrap_err(),
        SimulationError::Overflow
    );
    let fast = SpringCurve::new(
        SpringDescription::new(1.0, 1e100, 1e50).unwrap(),
        Tolerance::DEFAULT,
    )
    .unwrap();
    assert_eq!(fast.duration(), Duration::from_nanos(1));
    assert!(fast.transform(0.5).is_finite());
}

fn equivalent_recreated_curves_keep_the_admitted_run() {
    use flui_animation::{AnimatedValue, MotionClock, MotionSpec, Vsync};
    for spring in [false, true] {
        let make = || {
            if spring {
                SharedCurve::new(
                    SpringCurve::new(
                        SpringDescription::new(1.0, 100.0, 10.0).unwrap(),
                        Tolerance::DEFAULT,
                    )
                    .unwrap(),
                )
            } else {
                SharedCurve::new(
                    PiecewiseLinear::new(&[
                        LinearStop::new(0.0),
                        LinearStop::new(0.25),
                        LinearStop::new(1.0),
                    ])
                    .unwrap(),
                )
            }
        };
        let motion = || MotionSpec::Curve {
            duration: Duration::from_secs(1),
            curve: make(),
        };
        let registry = Vsync::new();
        let mut clock = MotionClock::new();
        let mut original = AnimatedValue::new(0.0, motion(), Some(&registry)).unwrap();
        let mut reference = AnimatedValue::new(0.0, motion(), Some(&registry)).unwrap();
        let run = original.animate_to(1.0).unwrap();
        let reference_run = reference.animate_to(1.0).unwrap();
        registry.tick_all(&clock.frame(Duration::ZERO));
        registry.tick_all(&clock.frame(Duration::from_millis(100)));
        let seam = (original.value(), original.velocity());
        let alias = original.set_motion(motion()).unwrap();
        assert!(
            run.is_pending(),
            "equal configuration does not cancel the original run"
        );
        assert_eq!((original.value(), original.velocity()), seam);
        for millis in [250, 500, 750, 1000] {
            registry.tick_all(&clock.frame(Duration::from_millis(millis)));
            assert_eq!(original.value(), reference.value());
            assert_eq!(original.velocity(), reference.velocity());
            assert_eq!(run.is_pending(), reference_run.is_pending());
        }
        assert!(run.is_complete() && alias.is_complete());
    }
}

fn natural_spring_duration_controls_once_only_completion() {
    use flui_animation::{AnimationController, Simulation, SpringSimulation};
    use std::cell::Cell;
    use std::rc::Rc;
    // The critical unit spring's displacement is (1+10t)e^-10t and
    // velocity is 100t e^-10t. Both decrease after t=0.1; bisect the last
    // violation of the published default distance/velocity limits.
    let (mut lower, mut upper) = (0.1_f64, 10.0_f64);
    for _ in 0..100 {
        let time = lower.midpoint(upper);
        let decay = (-10.0 * time).exp();
        if (1.0 + 10.0 * time) * decay > 0.001 || 100.0 * time * decay > 0.001 {
            lower = time;
        } else {
            upper = time;
        }
    }
    let curve = SpringCurve::new(
        SpringDescription::new(1.0, 100.0, 20.0).unwrap(),
        Tolerance::DEFAULT,
    )
    .unwrap();
    let duration = curve.duration();
    assert!(duration.as_secs_f64() >= lower);
    // The physical solver admits a conservative rest estimate, bounded by
    // 1.25 * last_violation + 2/omega, rather than the earliest root.
    assert!(
        duration.as_secs_f64() <= 1.25 * upper + 0.2,
        "natural duration respects the independent conservative bound"
    );
    let before_rest = duration.checked_sub(Duration::from_nanos(1)).unwrap();
    let physical = SpringSimulation::new(
        SpringDescription::new(1.0, 100.0, 20.0).unwrap(),
        0.0,
        1.0,
        0.0,
        Tolerance::DEFAULT,
    )
    .unwrap();
    assert!(!physical.is_done(before_rest.as_secs_f64()));
    assert!(
        physical.is_done(duration.as_secs_f64()),
        "easing duration rounds the producer's rest upward"
    );
    let controller = AnimationController::builder(duration).build();
    let run = controller.animate_to_curved(1.0, None, curve).unwrap();
    let calls = Rc::new(Cell::new(0));
    run.when_complete_or_cancel({
        let calls = Rc::clone(&calls);
        move |outcome| {
            assert!(outcome.is_ok());
            calls.set(calls.get() + 1);
        }
    });
    controller.tick_at(Duration::ZERO);
    controller.tick_at(before_rest);
    assert!(run.is_pending());
    assert_eq!(calls.get(), 0);
    controller.tick_at(duration);
    controller.tick_at(duration);
    assert!(run.is_complete());
    assert_eq!(calls.get(), 1);
}

#[cfg(feature = "serde")]
fn serialized_stops_share_curve_admission() {
    let curve: PiecewiseLinear = serde_json::from_str(
        r#"[{"output":0,"input":null},{"output":0.25,"input":null},{"output":1,"input":null}]"#,
    )
    .unwrap();
    assert_eq!(curve.transform(0.5), 0.25);
    let wire = serde_json::to_string(&curve).unwrap();
    assert_eq!(
        serde_json::from_str::<PiecewiseLinear>(&wire).unwrap(),
        curve
    );
    for wire in [
        "[]",
        r#"[{"output":0,"input":0},{"output":1,"input":2}]"#,
        r#"[{"output":0.2,"input":0},{"output":1,"input":1}]"#,
    ] {
        assert!(serde_json::from_str::<PiecewiseLinear>(wire).is_err());
    }
}

#[test]
fn authored_easing_contract() {
    crate::run_table(&[
        ("missing inputs", missing_stops_are_evenly_placed),
        (
            "decreasing inputs and jump",
            decreasing_inputs_form_a_right_continuous_jump,
        ),
        ("plateau and overshoot", plateaus_overshoot_and_knot_slopes),
        ("refused stops", refused_stops_are_typed),
        ("finite extremes", extreme_outputs_and_intervals_stay_finite),
        (
            "policy and equality",
            curves_keep_normalized_input_policy_and_value_equality,
        ),
        (
            "physical references",
            spring_matches_independent_physical_references,
        ),
        (
            "duration admission",
            spring_refuses_unusable_duration_and_admits_nanosecond_motion,
        ),
        (
            "recreated motion",
            equivalent_recreated_curves_keep_the_admitted_run,
        ),
        (
            "natural duration and completion",
            natural_spring_duration_controls_once_only_completion,
        ),
        #[cfg(feature = "serde")]
        ("serde admission", serialized_stops_share_curve_admission),
    ]);
}
