//! Spring construction, motion against analytic references, and rest.
//!
//! Reference values are the closed-form solution of `x'' + 2ζω x' + ω² x = 0`
//! cross-checked against a matrix exponential; the property tests compare
//! against the textbook three-regime solution in [`reference`], written
//! independently of the crate's single-form propagator.

// Reference values are quoted to the published digits so they can be
// checked against their source.
#![expect(clippy::unreadable_literal, reason = "published reference values")]

use flui_animation::simulation::{
    BouncingScrollSimulation, FrictionSimulation, Simulation, SimulationBounds, SimulationError,
    SimulationParameter, SpringDescription, SpringSimulation, SpringType, Tolerance,
};
use flui_animation::{AnimatedValue, Animation, AnimationController, AnimationStatus};
use proptest::prelude::*;
use std::f64::consts::{PI, TAU};
use std::time::Duration;

/// The textbook solution `(x(t), v(t))` for displacement `x0`, velocity `v0`.
#[expect(clippy::many_single_char_names, reason = "the textbook's symbols")]
fn reference(omega: f64, zeta: f64, x0: f64, v0: f64, t: f64) -> (f64, f64) {
    if zeta < 1.0 {
        let wd = omega * (1.0 - zeta * zeta).sqrt();
        let a = zeta * omega;
        let c2 = (v0 + a * x0) / wd;
        let e = (-a * t).exp();
        let (s, c) = (wd * t).sin_cos();
        let x = e * (x0 * c + c2 * s);
        let v = e * ((c2 * wd - a * x0) * c - (x0 * wd + a * c2) * s);
        (x, v)
    } else if zeta == 1.0 {
        let e = (-omega * t).exp();
        let c2 = v0 + omega * x0;
        (e * (x0 + c2 * t), e * (c2 - omega * (x0 + c2 * t)))
    } else {
        let root = (zeta * zeta - 1.0).sqrt();
        let r1 = -omega / (zeta + root);
        let r2 = -omega * (zeta + root);
        let c1 = (v0 - r2 * x0) / (r1 - r2);
        let c2 = x0 - c1;
        (
            c1 * (r1 * t).exp() + c2 * (r2 * t).exp(),
            c1 * r1 * (r1 * t).exp() + c2 * r2 * (r2 * t).exp(),
        )
    }
}

fn spring(omega: f64, zeta: f64) -> SpringDescription {
    SpringDescription::with_damping_ratio(1.0, omega * omega, zeta)
}

/// A tolerance small enough that no reference row is at rest yet.
fn exact() -> Tolerance {
    Tolerance::new(f64::MIN_POSITIVE, f64::MIN_POSITIVE).expect("positive tolerance")
}

fn assert_close(label: &str, actual: f64, expected: f64) {
    let error = (actual - expected).abs();
    assert!(
        error <= 1e-12 * expected.abs() || error <= 1e-15,
        "{label}: {actual:e} vs reference {expected:e}"
    );
}

// --- construction --------------------------------------------------------

fn assert_refused<T: std::fmt::Debug>(
    result: Result<T, SimulationError>,
    parameter: SimulationParameter,
) {
    match result {
        Err(SimulationError::OutOfRange { parameter: p, .. }) => assert_eq!(p, parameter),
        other => panic!("expected {parameter} to be refused, got {other:?}"),
    }
}

fn assert_overflow<T: std::fmt::Debug>(result: Result<T, SimulationError>) {
    assert!(
        matches!(result, Err(SimulationError::Overflow)),
        "expected overflow, got {result:?}"
    );
}

fn mass_nan() {
    assert_refused(
        SpringDescription::new(f64::NAN, 1.0, 1.0),
        SimulationParameter::Mass,
    );
}
fn mass_zero() {
    assert_refused(
        SpringDescription::new(0.0, 1.0, 1.0),
        SimulationParameter::Mass,
    );
}
fn stiffness_infinite() {
    assert_refused(
        SpringDescription::new(1.0, f64::INFINITY, 1.0),
        SimulationParameter::Stiffness,
    );
}
fn stiffness_negative() {
    assert_refused(
        SpringDescription::new(1.0, -100.0, 1.0),
        SimulationParameter::Stiffness,
    );
}
fn damping_zero_never_rests() {
    assert_refused(
        SpringDescription::new(1.0, 100.0, 0.0),
        SimulationParameter::Damping,
    );
}
fn damping_negative() {
    assert_refused(
        SpringDescription::new(1.0, 100.0, -1.0),
        SimulationParameter::Damping,
    );
}
fn frequency_overflows() {
    assert_overflow(SpringDescription::new(1e-300, 1e300, 1.0));
}
fn damping_ratio_overflows() {
    assert_overflow(SpringDescription::new(1e-300, 1e-300, 1e300));
}
fn duration_zero() {
    assert_refused(
        SpringDescription::with_duration_and_bounce(Duration::ZERO, 0.0),
        SimulationParameter::Duration,
    );
}
fn bounce_one_is_undamped() {
    assert_refused(
        SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 1.0),
        SimulationParameter::Bounce,
    );
}
fn bounce_above_one() {
    assert_refused(
        SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 2.0),
        SimulationParameter::Bounce,
    );
}
fn bounce_minus_one() {
    assert_refused(
        SpringDescription::with_duration_and_bounce(Duration::from_millis(500), -1.0),
        SimulationParameter::Bounce,
    );
}
fn bounce_nan() {
    assert_refused(
        SpringDescription::with_duration_and_bounce(Duration::from_millis(500), f64::NAN),
        SimulationParameter::Bounce,
    );
}
fn response_zero() {
    assert_refused(
        SpringDescription::with_response_and_damping(Duration::ZERO, 1.0),
        SimulationParameter::Duration,
    );
}
fn damping_fraction_negative() {
    assert_refused(
        SpringDescription::with_response_and_damping(Duration::from_millis(300), -0.5),
        SimulationParameter::DampingFraction,
    );
}
fn damping_fraction_zero() {
    assert_refused(
        SpringDescription::with_response_and_damping(Duration::from_millis(300), 0.0),
        SimulationParameter::DampingFraction,
    );
}
fn response_overflows() {
    assert_overflow(SpringDescription::with_response_and_damping(
        Duration::from_nanos(1),
        1e300,
    ));
}
fn damping_ratio_zero_panics() {
    let refused = std::panic::catch_unwind(|| SpringDescription::with_damping_ratio(1.0, 1.0, 0.0));
    assert!(refused.is_err(), "an undamped spring must be refused");
}
fn simulation_start_nan() {
    assert_refused(
        SpringSimulation::try_new(spring(10.0, 1.0), f64::NAN, 0.0, 0.0, Tolerance::DEFAULT),
        SimulationParameter::Position,
    );
}
fn simulation_end_infinite() {
    assert_refused(
        SpringSimulation::try_new(
            spring(10.0, 1.0),
            0.0,
            f64::INFINITY,
            0.0,
            Tolerance::DEFAULT,
        ),
        SimulationParameter::Position,
    );
}
fn simulation_velocity_nan() {
    assert_refused(
        SpringSimulation::try_new(spring(10.0, 1.0), 0.0, 1.0, f64::NAN, Tolerance::DEFAULT),
        SimulationParameter::Velocity,
    );
}
fn simulation_displacement_overflows() {
    assert_overflow(SpringSimulation::try_new(
        spring(10.0, 1.0),
        f64::MAX,
        -f64::MAX,
        0.0,
        Tolerance::DEFAULT,
    ));
}

#[test]
fn spring_constructors_refuse_outside_the_admitted_domain() {
    crate::run_table(&[
        ("mass nan", mass_nan),
        ("mass zero", mass_zero),
        ("stiffness infinite", stiffness_infinite),
        ("stiffness negative", stiffness_negative),
        ("damping zero never rests", damping_zero_never_rests),
        ("damping negative", damping_negative),
        ("frequency overflows", frequency_overflows),
        ("damping ratio overflows", damping_ratio_overflows),
        ("duration zero", duration_zero),
        ("bounce one is undamped", bounce_one_is_undamped),
        ("bounce above one", bounce_above_one),
        ("bounce minus one", bounce_minus_one),
        ("bounce nan", bounce_nan),
        ("response zero", response_zero),
        ("damping fraction negative", damping_fraction_negative),
        ("damping fraction zero", damping_fraction_zero),
        ("response overflows", response_overflows),
        ("damping ratio zero panics", damping_ratio_zero_panics),
        ("simulation start nan", simulation_start_nan),
        ("simulation end infinite", simulation_end_infinite),
        ("simulation velocity nan", simulation_velocity_nan),
        (
            "simulation displacement overflows",
            simulation_displacement_overflows,
        ),
    ]);
}

// --- analytic reference --------------------------------------------------

fn check_reference(zeta: f64, omega: f64, x0: f64, v0: f64, t: f64, x: f64, v: Option<f64>) {
    let sim = SpringSimulation::try_new(spring(omega, zeta), x0, 0.0, v0, exact())
        .expect("admitted spring");
    assert_close("x", sim.x(t), x);
    if let Some(v) = v {
        assert_close("dx", sim.dx(t), v);
    }
}

fn underdamped_half() {
    check_reference(
        0.5,
        10.0,
        1.0,
        0.0,
        0.1,
        0.659700153391702,
        Some(-5.33507195114693),
    );
    check_reference(
        0.5,
        10.0,
        1.0,
        0.0,
        0.25,
        -0.0233595799066923,
        Some(-2.74109898705702),
    );
    check_reference(
        0.5,
        10.0,
        1.0,
        0.0,
        1.0,
        -0.00217011673932620,
        Some(-0.0538548061605957),
    );
}
fn critical() {
    check_reference(
        1.0,
        10.0,
        1.0,
        0.0,
        0.1,
        0.735758882342885,
        Some(-3.67879441171442),
    );
    check_reference(
        1.0,
        10.0,
        1.0,
        0.0,
        0.5,
        0.0404276819945128,
        Some(-0.336897349954273),
    );
    check_reference(
        1.0,
        10.0,
        -1.0,
        5.0,
        0.2,
        -0.270670566473225,
        Some(2.03002924854919),
    );
}
fn overdamped() {
    check_reference(
        2.0,
        10.0,
        1.0,
        0.0,
        0.1,
        0.822263423901810,
        Some(-2.13909130260279),
    );
    check_reference(
        2.0,
        10.0,
        1.0,
        0.0,
        0.5,
        0.282171173975153,
        Some(-0.756075360853215),
    );
    check_reference(
        2.0,
        10.0,
        0.0,
        10.0,
        0.3,
        0.129208025818252,
        Some(-0.346074592636829),
    );
}
fn lightly_damped_with_velocity() {
    check_reference(
        0.2,
        TAU,
        -1.0,
        5.0,
        0.3,
        0.588257945757625,
        Some(2.62367533686863),
    );
}
fn perceptual_half_second() {
    check_reference(
        0.7,
        4.0 * PI,
        -1.0,
        0.0,
        0.25,
        -0.0159125090286291,
        Some(1.52626471111652),
    );
}
fn critical_compose_default() {
    check_reference(
        1.0,
        1500.0_f64.sqrt(),
        -1.0,
        0.0,
        0.05,
        -0.423468514838734,
        Some(10.8156746718564),
    );
}
fn continuous_through_critical() {
    check_reference(0.999, 10.0, 1.0, 0.0, 0.3, 0.198699920833023, None);
    check_reference(1.0 - 1e-6, 10.0, 1.0, 0.0, 0.3, 0.199147825387572, None);
    check_reference(1.0, 10.0, 1.0, 0.0, 0.3, 0.199148273471459, None);
    check_reference(1.0 + 1e-6, 10.0, 1.0, 0.0, 0.3, 0.199148721554790, None);
    check_reference(1.001, 10.0, 1.0, 0.0, 0.3, 0.199596088409306, None);
}
fn overdamped_large_time_stays_finite() {
    check_reference(3.0, 100.0, 1.0, 0.0, 3.0, 4.56068935005893e-23, None);
    check_reference(50.0, 1000.0, 1.0, 0.0, 10.0, 3.68342164948964e-44, None);
}
fn heavy_damping_keeps_its_slow_root() {
    // λs = −ω/(ζ + √(ζ² − 1)) ≈ −ω/(2ζ): the spring still moves.
    for zeta in [1e6, 1e8, 1e12] {
        let sim = SpringSimulation::try_new(spring(10.0, zeta), 1.0, 0.0, 0.0, Tolerance::DEFAULT)
            .expect("admitted spring");
        let expected = (-10.0 / (2.0 * zeta)).exp();
        assert_close("slow root", sim.x(1.0), expected);
        assert_ne!(sim.x(1.0), sim.x(0.0), "ζ = {zeta} must not freeze");
    }
}

#[test]
fn spring_matches_analytic_reference() {
    crate::run_table(&[
        ("underdamped half", underdamped_half),
        ("critical", critical),
        ("overdamped", overdamped),
        ("lightly damped with velocity", lightly_damped_with_velocity),
        ("perceptual half second", perceptual_half_second),
        ("critical compose default", critical_compose_default),
        ("continuous through critical", continuous_through_critical),
        (
            "overdamped large time stays finite",
            overdamped_large_time_stays_finite,
        ),
        (
            "heavy damping keeps its slow root",
            heavy_damping_keeps_its_slow_root,
        ),
    ]);
}

// --- perceptual parameterizations ----------------------------------------

/// SwiftUI's documented `Spring(duration:bounce:)` ↔ `(mass 1, k, c)` pairs.
fn matches_published(duration_ms: u64, bounce: f64, stiffness: f64, damping: f64) {
    let perceptual =
        SpringDescription::with_duration_and_bounce(Duration::from_millis(duration_ms), bounce)
            .expect("admitted perceptual spring");
    let physical = SpringDescription::new(1.0, stiffness, damping).expect("admitted spring");
    let a = SpringSimulation::try_new(perceptual, 1.0, 0.0, 0.0, exact()).expect("simulation");
    let b = SpringSimulation::try_new(physical, 1.0, 0.0, 0.0, exact()).expect("simulation");
    for t in [0.05, 0.1, 0.25, 0.5] {
        assert_close("perceptual", a.x(t), b.x(t));
    }
}
fn half_second_bounce_three_tenths() {
    matches_published(500, 0.3, 157.913670417430, 17.5929188601028);
}
fn half_second_no_bounce() {
    matches_published(500, 0.0, 157.913670417430, 25.1327412287183);
}
fn three_tenths_bounce_three_tenths() {
    matches_published(300, 0.3, 438.649084492860, 29.3215314335047);
}
fn one_second_bounce_half() {
    matches_published(1000, 0.5, 39.4784176043574, TAU);
}

#[test]
fn perceptual_springs_match_published_conversions() {
    crate::run_table(&[
        ("half second bounce 0.3", half_second_bounce_three_tenths),
        ("half second no bounce", half_second_no_bounce),
        ("0.3 s bounce 0.3", three_tenths_bounce_three_tenths),
        ("one second bounce 0.5", one_second_bounce_half),
    ]);
}

fn perceptual(bounce: f64) -> SpringDescription {
    SpringDescription::with_duration_and_bounce(Duration::from_millis(400), bounce)
        .expect("admitted bounce")
}
fn x_at(spring: SpringDescription, t: f64) -> f64 {
    SpringSimulation::try_new(spring, 1.0, 0.0, 0.0, exact())
        .expect("simulation")
        .x(t)
}
fn bounce_sign_selects_the_regime() {
    assert_eq!(perceptual(-0.5).spring_type(), SpringType::Overdamped);
    assert_eq!(perceptual(0.0).spring_type(), SpringType::CriticallyDamped);
    assert_eq!(perceptual(0.5).spring_type(), SpringType::Underdamped);
}
fn damping_is_continuous_at_zero_bounce() {
    for t in [0.05, 0.2] {
        let gap = (x_at(perceptual(-1e-9), t) - x_at(perceptual(1e-9), t)).abs();
        assert!(gap < 1e-8, "discontinuity {gap} at t = {t}");
    }
}
fn more_bounce_overshoots_further() {
    // Lower ζ ⇒ the displacement falls faster early on.
    let samples: Vec<f64> = [-0.6, -0.3, 0.0, 0.3, 0.6]
        .into_iter()
        .map(|b| x_at(perceptual(b), 0.1))
        .collect();
    assert!(
        samples.windows(2).all(|pair| pair[1] < pair[0]),
        "x(0.1) must decrease with bounce: {samples:?}"
    );
}
fn response_form_equals_duration_form() {
    for bounce in [0.0, 0.25, 0.9] {
        let response =
            SpringDescription::with_response_and_damping(Duration::from_millis(400), 1.0 - bounce)
                .expect("admitted response spring");
        assert_eq!(response, perceptual(bounce), "bounce {bounce}");
    }
}

#[test]
fn perceptual_branches_are_consistent() {
    crate::run_table(&[
        (
            "bounce sign selects the regime",
            bounce_sign_selects_the_regime,
        ),
        (
            "damping is continuous at zero bounce",
            damping_is_continuous_at_zero_bounce,
        ),
        (
            "more bounce overshoots further",
            more_bounce_overshoots_further,
        ),
        (
            "response form equals duration form",
            response_form_equals_duration_form,
        ),
    ]);
}

// --- time domain -----------------------------------------------------------

fn assert_time_domain(sim: &dyn Simulation, start: f64, velocity: f64, end: f64) {
    for before in [-1.0, -f64::MIN_POSITIVE, f64::NAN, f64::NEG_INFINITY] {
        assert_eq!(sim.x(before), start, "x({before})");
        assert_eq!(sim.dx(before), velocity, "dx({before})");
        assert!(!sim.is_done(before), "is_done({before})");
    }
    assert_eq!(sim.x(f64::INFINITY), end);
    assert_eq!(sim.dx(f64::INFINITY), 0.0);
    assert!(sim.is_done(f64::INFINITY));
    let mut done = false;
    for step in 0..20_000 {
        let t = f64::from(step) * 1e-3;
        let now = sim.is_done(t);
        assert!(!done || now, "is_done must stay true once true (t = {t})");
        done = now;
        if now {
            assert_eq!(sim.x(t), end, "at rest x is the resting position");
            assert_eq!(sim.dx(t), 0.0);
        }
        assert!(sim.x(t).is_finite() && sim.dx(t).is_finite(), "t = {t}");
    }
    assert!(done, "must rest within 20 s");
}
fn spring_edges() {
    for zeta in [0.3, 1.0, 3.0] {
        let sim =
            SpringSimulation::try_new(spring(20.0, zeta), 3.0, 10.0, -4.0, Tolerance::DEFAULT)
                .expect("spring");
        assert_time_domain(&sim, 3.0, -4.0, 10.0);
    }
}
fn friction_edges() {
    let sim = FrictionSimulation::new(0.135, 5.0, 300.0, Tolerance::DEFAULT).expect("friction");
    assert_time_domain(&sim, 5.0, 300.0, sim.final_x());
}
fn bouncing_edges() {
    let bounds = SimulationBounds::new(0.0, 100.0).expect("bounds");
    let sim = BouncingScrollSimulation::new(
        spring(22.0, 0.75),
        0.135,
        90.0,
        2000.0,
        bounds,
        Tolerance::DEFAULT,
    )
    .expect("bouncing");
    assert_time_domain(&sim, 90.0, 2000.0, 100.0);
}

#[test]
fn simulation_time_domain_edges() {
    crate::run_table(&[
        ("spring", spring_edges),
        ("friction", friction_edges),
        ("bouncing", bouncing_edges),
    ]);
}

// --- properties --------------------------------------------------------------

fn log_uniform(low: f64, high: f64) -> impl Strategy<Value = f64> {
    (low.ln()..=high.ln()).prop_map(f64::exp)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn spring_outputs_are_finite_over_the_admitted_domain(
        omega in log_uniform(1e-3, 1e4),
        zeta in log_uniform(1e-3, 1e8),
        x0 in -1e6..=1e6_f64,
        v0 in -1e7..=1e7_f64,
    ) {
        let sim = SpringSimulation::try_new(spring(omega, zeta), x0, 0.0, v0, Tolerance::DEFAULT)
            .expect("an admitted spring builds");
        for t in [0.0, 1e-9, 1e-3, 0.1, 1.0, 10.0, 1e3, 1e9, 1e300, f64::INFINITY] {
            let (x, v) = (sim.x(t), sim.dx(t));
            prop_assert!(x.is_finite() && v.is_finite(), "t = {}: x = {}, dx = {}", t, x, v);
        }
        prop_assert_eq!(sim.x(f64::INFINITY), 0.0);
    }
}

/// The first `t` at which `sim` reports done, to within a nanosecond.
fn observed_rest(sim: &SpringSimulation) -> f64 {
    let mut high = 1.0;
    while !sim.is_done(high) {
        high *= 2.0;
    }
    let mut low = 0.0;
    while high - low > 1e-9 {
        let middle = f64::midpoint(low, high);
        if sim.is_done(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn rest_time_is_conservative_and_tight(
        omega in log_uniform(5.0, 60.0),
        zeta in log_uniform(0.2, 5.0),
        x0 in -100.0..=100.0_f64,
        v0 in -1000.0..=1000.0_f64,
        derived in any::<bool>(),
    ) {
        let tolerance = if derived {
            Tolerance::new(0.5, f64::INFINITY).expect("tolerance")
        } else {
            Tolerance::DEFAULT
        };
        let (distance, speed) = if derived { (0.5, 0.5 * omega) } else { (1e-3, 1e-3) };
        let sim = SpringSimulation::try_new(spring(omega, zeta), x0, 0.0, v0, tolerance)
            .expect("spring");
        let rest = observed_rest(&sim);
        // The settle boundary: exactly the target from `rest` on.
        prop_assert_eq!(sim.x(rest), 0.0);
        prop_assert_eq!(sim.dx(rest), 0.0);

        let horizon = rest * 1.5 + 1.0;
        let mut last_violation = 0.0_f64;
        let mut step = 0_u32;
        loop {
            let t = f64::from(step) * 1e-4;
            if t > horizon {
                break;
            }
            let (x, v) = reference(omega, zeta, x0, v0, t);
            let violates = x.abs() > distance || v.abs() > speed;
            if violates {
                last_violation = t;
                prop_assert!(t < rest, "violates tolerance at {} after rest {}", t, rest);
            }
            step += 1;
        }
        prop_assert!(
            rest <= 1.25 * last_violation + 2.0 / omega,
            "rest {} vs last violation {} (ω = {}, ζ = {})",
            rest, last_violation, omega, zeta
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn spring_run_is_independent_of_frame_partition(
        omega in log_uniform(5.0, 40.0),
        zeta in log_uniform(0.3, 3.0),
        v0 in -20.0..=20.0_f64,
    ) {
        let sim = SpringSimulation::try_new(spring(omega, zeta), 0.0, 1.0, v0, Tolerance::DEFAULT)
            .expect("spring");
        let rest = observed_rest(&sim);
        let mut at_sixths: Vec<Vec<f64>> = Vec::new();
        for hz in [30_u32, 60, 120, 144] {
            let controller = AnimationController::unbounded_without_ticker(Duration::from_secs(1));
            controller.animate_with(sim.clone()).expect("run starts");
            let mut sixths = Vec::new();
            let mut frame = 1_u32;
            loop {
                let t = f64::from(frame) / f64::from(hz);
                controller.tick_at(t);
                if frame.is_multiple_of(hz / 6) {
                    sixths.push(controller.value());
                }
                if controller.status() != AnimationStatus::Forward {
                    prop_assert!(t >= rest, "{} Hz finished at {} before rest {}", hz, t, rest);
                    prop_assert!(
                        f64::from(frame - 1) / f64::from(hz) < rest,
                        "{} Hz finished late at {} (rest {})", hz, t, rest
                    );
                    prop_assert_eq!(controller.value(), 1.0);
                    break;
                }
                prop_assert!(frame < 100_000, "never finished");
                frame += 1;
            }
            at_sixths.push(sixths);
            controller.dispose();
        }
        for run in &at_sixths[1..] {
            for (a, b) in run.iter().zip(&at_sixths[0]) {
                prop_assert!((a - b).abs() <= 1e-12, "{} vs {}", a, b);
            }
        }
    }
}

// --- fling -----------------------------------------------------------------------

/// The default fling spring (`ω = √500`, `ζ = 1`) from 0 at 1 unit/s crosses
/// the upper bound at 0.295369722567422 s: frame 18 at 60 Hz, not frame 17.
#[test]
#[ignore = "contract: a fling completes on the frame that reaches the bound"]
fn fling_completes_on_the_frame_that_reaches_the_bound() {
    let controller = AnimationController::without_ticker(Duration::from_secs(1));
    controller.fling(1.0).expect("fling starts");
    let mut finished = None;
    for frame in 1..=120_u32 {
        controller.tick_at(f64::from(frame) / 60.0);
        if controller.status() != AnimationStatus::Forward {
            finished = Some(frame);
            break;
        }
    }
    assert_eq!(finished, Some(18), "first frame at or after 0.29537 s");
    assert_eq!(controller.status(), AnimationStatus::Completed);
    assert_eq!(controller.value(), 1.0);
}

// --- animated value ----------------------------------------------------------

fn animated_spring() -> SpringDescription {
    SpringDescription::with_response_and_damping(Duration::from_millis(300), 0.7)
        .expect("admitted spring")
}

fn refuses_non_finite_initial_value() {
    assert!(AnimatedValue::new(f64::NAN, animated_spring()).is_err());
    assert!(AnimatedValue::new(f64::INFINITY, animated_spring()).is_err());
}
fn refused_target_leaves_the_value_unchanged() {
    let mut value = AnimatedValue::new(0.0_f64, animated_spring()).expect("finite");
    value.animate_to(100.0).expect("finite target");
    value.advance(Duration::from_millis(50));
    let before = value.value();
    assert!(value.animate_to(f64::NAN).is_err());
    assert!(value.set_value(f64::INFINITY).is_err());
    assert_eq!(value.value(), before);
    assert_eq!(*value.target(), 100.0);
    value.advance(Duration::MAX);
    value.advance(Duration::MAX);
    assert!(value.is_settled());
    assert_eq!(value.value(), 100.0);
}
fn retarget_after_rest_starts_still_at_the_old_target() {
    let mut value = AnimatedValue::new(0.0_f64, animated_spring()).expect("finite");
    value.animate_to(10.0).expect("finite target");
    value.advance(Duration::from_secs(30));
    value.animate_to(20.0).expect("finite target");
    assert_eq!(value.value(), 10.0, "starts from the old target");
    value.advance(Duration::from_millis(1));
    let early = value.value();
    assert!(
        early > 10.0 && early - 10.0 < 1e-2,
        "starts at rest, not with leftover velocity: {early}"
    );
}
fn retarget_preserves_velocity() {
    // Midway toward 100, retarget to 0: momentum carries the value further
    // toward 100 before the new spring pulls it back.
    let mut value = AnimatedValue::new(0.0_f64, animated_spring()).expect("finite");
    value.animate_to(100.0).expect("finite target");
    value.advance(Duration::from_millis(100));
    let position = value.value();
    value.animate_to(0.0).expect("finite target");
    value.advance(Duration::from_millis(16));
    assert!(
        value.value() > position,
        "{} should overshoot past {position}",
        value.value()
    );
}

#[test]
fn animated_value_never_latches_non_finite() {
    crate::run_table(&[
        (
            "refuses non finite initial value",
            refuses_non_finite_initial_value,
        ),
        (
            "refused target leaves the value unchanged",
            refused_target_leaves_the_value_unchanged,
        ),
        (
            "retarget after rest starts still at the old target",
            retarget_after_rest_starts_still_at_the_old_target,
        ),
        ("retarget preserves velocity", retarget_preserves_velocity),
    ]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn animated_value_is_frame_partition_independent(
        frames in 1_u32..400,
        frame_nanos in 1_000_000_u64..40_000_000,
        target in -1e4..=1e4_f64,
    ) {
        let mut stepped = AnimatedValue::new(0.0_f64, animated_spring()).expect("finite");
        stepped.animate_to(target).expect("finite target");
        let mut once = stepped.clone();
        let dt = Duration::from_nanos(frame_nanos);
        for _ in 0..frames {
            stepped.advance(dt);
        }
        once.advance(dt * frames);
        prop_assert_eq!(stepped.value().to_bits(), once.value().to_bits());
        prop_assert_eq!(stepped.is_settled(), once.is_settled());
    }
}
