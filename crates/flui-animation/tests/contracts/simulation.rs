//! Public numerical simulation and smoothing behavior.

use flui_animation::{
    AnimatedValue, FrictionSimulation, GravitySimulation, Simulation, SmoothDamp,
    SpringDescription, SpringSimulation, SpringType,
};

fn weak_drag_preserves_frame_motion() {
    // Only public simulation operations: constant velocity is the
    // independent limiting oracle as valid drag approaches one.
    let weak_drag = FrictionSimulation::new(1.0_f64.next_down(), 0.0, 8000.0);
    let frame_time = 1.0 / 60.0;
    let distance = weak_drag.x(frame_time);
    assert!(
        (distance - 8000.0 * frame_time).abs() < 1e-9,
        "weak drag frame distance: {distance}"
    );
}

fn weak_drag_preserves_arrival_time() {
    let weak_drag = FrictionSimulation::new(1.0_f64.next_down(), 0.0, 8000.0);
    let arrival = weak_drag.time_at_x(0.01);
    assert!(
        (arrival - 0.01 / 8000.0).abs() < 1e-15,
        "weak drag arrival time: {arrival}"
    );
}

fn unreachable_and_stationary_queries() {
    let sim = FrictionSimulation::new(0.135, 0.0, 100.0);
    assert_eq!(sim.time_at_x(sim.final_x()), f64::INFINITY);
    assert!(
        sim.time_at_x(sim.final_x() * 2.0).is_nan(),
        "unreachable position"
    );
    let stationary = FrictionSimulation::new(0.135, 0.0, 0.0);
    assert_eq!(stationary.x(1.0), 0.0);
    assert_eq!(stationary.time_at_x(0.0), 0.0);
    assert!(stationary.time_at_x(1.0).is_nan());
}

fn roundtrip(drag: f64, start: f64, velocity: f64) {
    let sim = FrictionSimulation::new(drag, start, velocity);
    assert_eq!(sim.x(0.0), start);
    assert_eq!(sim.dx(0.0), velocity);
    for time in [1e-5, 1.0 / 60.0, 0.5] {
        let position = sim.x(time);
        let arrival = sim.time_at_x(position);
        assert!(
            (arrival - time).abs() < 1e-10,
            "drag {drag}, velocity {velocity}, t {time}: arrival {arrival}"
        );
        assert_eq!(sim.dx(time).is_sign_positive(), velocity.is_sign_positive());
    }
    assert!(sim.time_at_x(f64::NAN).is_nan());
    assert!(sim.x(f64::NAN).is_nan());
    assert!((sim.x(f64::INFINITY) - sim.final_x()).abs() < 1e-9);
}
fn positive_normal_drag() {
    roundtrip(0.135, 10.0, 8000.0);
}
fn negative_normal_drag() {
    roundtrip(0.135, -10.0, -8000.0);
}
fn nearly_unit_drag() {
    roundtrip(1.0_f64.next_down(), 0.0, 8000.0);
}
#[test]
fn friction_preserves_small_decay_and_position_time_roundtrips() {
    crate::run_table(&[
        ("weak drag frame motion", weak_drag_preserves_frame_motion),
        ("weak drag arrival time", weak_drag_preserves_arrival_time),
        ("positive normal drag", positive_normal_drag),
        ("negative normal drag", negative_normal_drag),
        ("nearly unit drag", nearly_unit_drag),
        (
            "unreachable and stationary queries",
            unreachable_and_stationary_queries,
        ),
    ]);
}

fn damp_follows_target_after_idle_tick(initial: f64, target: f64, idle_dt: f64) {
    let mut damp = SmoothDamp::new(0.2);
    assert_eq!(damp.step(initial, initial, idle_dt), initial);
    let mut position = initial;
    for _ in 0..240 {
        let next = damp.step(position, target, 1.0 / 120.0);
        assert!(
            next.is_finite(),
            "motion after an idle tick must stay finite"
        );
        assert!(
            next >= initial.min(target) && next <= initial.max(target),
            "a damped follower must stay between start and target: {next}"
        );
        assert!(
            (next - target).abs() <= (position - target).abs(),
            "motion must approach its new target"
        );
        position = next;
    }
    assert!(
        (position - target).abs() < 0.5,
        "follower must settle: {position}"
    );
}

fn zero_elapsed_idle_then_positive_target() {
    damp_follows_target_after_idle_tick(0.0, 100.0, 0.0);
}

fn zero_elapsed_idle_then_negative_target() {
    damp_follows_target_after_idle_tick(10.0, -90.0, 0.0);
}

fn ordinary_idle_then_positive_target() {
    damp_follows_target_after_idle_tick(0.0, 100.0, 1.0 / 120.0);
}

#[test]
fn damped_motion_remains_usable_after_idle_ticks() {
    crate::run_table(&[
        (
            "zero elapsed idle then positive target",
            zero_elapsed_idle_then_positive_target,
        ),
        (
            "zero elapsed idle then negative target",
            zero_elapsed_idle_then_negative_target,
        ),
        (
            "ordinary idle then positive target",
            ordinary_idle_then_positive_target,
        ),
    ]);
}

fn friction_drag_ge_one_panics_instead_of_hanging() {
    // drag >= 1 would make dx grow forever; construction must reject it.
    let payload = std::panic::catch_unwind(|| FrictionSimulation::new(1.5, 0.0, 100.0))
        .expect_err("drag >= 1 must be rejected at construction");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains("drag >= 1 never decelerates"),
        "unexpected panic message: {message}"
    );
}

fn gravity_accelerates_from_rest() {
    let sim = GravitySimulation::new(9.8, 0.0, 0.0, 100.0);

    // Should start at initial position
    assert_eq!(sim.x(0.0), 0.0);

    // Position should increase with gravity
    assert!(sim.x(1.0) > 0.0);

    // Velocity should increase
    assert!(sim.dx(1.0) > sim.dx(0.0));
}

fn spring_regimes_preserve_initial_conditions_and_settle() {
    let cases = [
        (
            "critical",
            SpringDescription::with_damping_ratio(1.0, 500.0, 1.0),
            SpringType::CriticallyDamped,
        ),
        (
            "underdamped",
            SpringDescription::with_damping_ratio(1.0, 500.0, 0.5),
            SpringType::Underdamped,
        ),
        (
            "overdamped",
            SpringDescription::with_damping_ratio(1.0, 500.0, 2.0),
            SpringType::Overdamped,
        ),
        (
            "near_critical_over",
            SpringDescription::with_damping_ratio(1.0, 500.0, 1.001),
            SpringType::Overdamped,
        ),
        (
            "near_critical_under",
            SpringDescription::with_damping_ratio(1.0, 500.0, 0.999),
            SpringType::Underdamped,
        ),
    ];

    for (label, spring, expected_type) in cases {
        let start = 0.25_f64;
        let end = 1.0_f64;
        let velocity = 2.5_f64;
        let sim = SpringSimulation::new(spring, start, end, velocity);
        assert_eq!(
            sim.spring_type(),
            expected_type,
            "{label}: unexpected spring type"
        );
        assert!(
            (sim.x(0.0) - start).abs() < 1e-5,
            "{label}: x(0)={} expected {start}",
            sim.x(0.0)
        );
        assert!(
            (sim.dx(0.0) - velocity).abs() < 1e-4,
            "{label}: dx(0)={} expected {velocity}",
            sim.dx(0.0)
        );

        // Analytic derivative should agree with a central finite difference.
        for t in [0.05_f64, 0.2] {
            let h = 1e-4_f64;
            let dx = sim.dx(t);
            let fd = (sim.x(t + h) - sim.x(t - h)) / (2.0 * h);
            let tol = (1e-3 * (1.0 + dx.abs())).max(5e-3);
            assert!(
                (fd - dx).abs() < tol,
                "{label}: dx({t})={dx} vs fd={fd} (tol={tol})"
            );
        }

        assert!(
            sim.is_done(10.0),
            "{label}: should settle by t=10; x={} dx={}",
            sim.x(10.0),
            sim.dx(10.0)
        );
    }
}

fn spring_retarget_preserves_velocity() {
    // Animate toward 100; midway (moving fast) retarget to 0. With velocity
    // preserved the value must briefly continue PAST its position toward 100
    // before the new spring pulls it back — momentum is not discarded.
    let mut v = AnimatedValue::new(
        0.0_f64,
        SpringDescription::with_response_and_damping(0.3, 1.0),
    );
    v.animate_to(100.0);
    for _ in 0..6 {
        v.advance(1.0 / 60.0);
    }
    let position = v.value();
    assert!(
        position > 0.0 && position < 100.0,
        "mid-flight pos={position}"
    );

    v.animate_to(0.0);
    v.advance(1.0 / 60.0);
    let v_after = v.value();
    // Momentum carried it further from 0 than where it was when retargeted.
    assert!(
        v_after > position,
        "velocity not preserved: {v_after} should overshoot past {position}"
    );
}

#[test]
fn simulation_contract() {
    crate::run_table(&[
        (
            "friction drag ge one panics instead of hanging",
            friction_drag_ge_one_panics_instead_of_hanging,
        ),
        (
            "gravity accelerates from rest",
            gravity_accelerates_from_rest,
        ),
        (
            "spring regimes preserve initial conditions and settle",
            spring_regimes_preserve_initial_conditions_and_settle,
        ),
        (
            "spring retarget preserves velocity",
            spring_retarget_preserves_velocity,
        ),
    ]);
}
