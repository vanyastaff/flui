//! Friction, bounds and the bouncing scroll fling.

use flui_animation::simulation::{
    BouncingScrollSimulation, BoundedFrictionSimulation, FrictionSimulation, Simulation,
    SimulationBounds, SimulationError, SimulationParameter, SpringDescription, Tolerance,
};
use flui_painting::styling::Color;

fn friction(drag: f64, position: f64, velocity: f64) -> FrictionSimulation {
    FrictionSimulation::new(drag, position, velocity, Tolerance::DEFAULT)
        .expect("admitted friction")
}

fn weak_drag_preserves_frame_motion() {
    // Constant velocity is the independent limiting oracle as drag → 1.
    let weak_drag = friction(1.0_f64.next_down(), 0.0, 8000.0);
    let frame_time = 1.0 / 60.0;
    let distance = weak_drag.x(frame_time);
    assert!(
        (distance - 8000.0 * frame_time).abs() < 1e-9,
        "weak drag frame distance: {distance}"
    );
}

fn weak_drag_preserves_arrival_time() {
    let weak_drag = friction(1.0_f64.next_down(), 0.0, 8000.0);
    let arrival = weak_drag.time_at_x(0.01);
    assert!(
        (arrival - 0.01 / 8000.0).abs() < 1e-15,
        "weak drag arrival time: {arrival}"
    );
}

fn unreachable_and_stationary_queries() {
    let sim = friction(0.135, 0.0, 100.0);
    assert_eq!(sim.time_at_x(sim.final_x()), f64::INFINITY);
    assert_eq!(
        sim.time_at_x(sim.final_x() * 2.0),
        f64::INFINITY,
        "past the rest"
    );
    assert_eq!(sim.time_at_x(-1.0), f64::INFINITY, "behind the start");
    assert!(sim.time_at_x(f64::NAN).is_nan());
    let stationary = friction(0.135, 0.0, 0.0);
    assert_eq!(stationary.x(1.0), 0.0);
    assert_eq!(stationary.time_at_x(0.0), 0.0);
    assert_eq!(stationary.time_at_x(1.0), f64::INFINITY);
    assert!(stationary.is_done(0.0));
}

fn roundtrip(drag: f64, start: f64, velocity: f64) {
    let sim = friction(drag, start, velocity);
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
    assert_eq!(sim.x(f64::NAN), start);
    assert_eq!(sim.x(f64::INFINITY), sim.final_x());
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

fn assert_drag_refused(drag: f64) {
    match FrictionSimulation::new(drag, 0.0, 1.0, Tolerance::DEFAULT) {
        Err(SimulationError::OutOfRange { parameter, .. }) => {
            assert_eq!(parameter, SimulationParameter::Drag);
        }
        other => panic!("drag {drag} must be refused, got {other:?}"),
    }
}
fn drag_outside_the_open_unit_interval() {
    for drag in [0.0, -0.5, 1.0, 1.5, f64::NAN, f64::INFINITY] {
        assert_drag_refused(drag);
    }
    assert!(FrictionSimulation::new(0.5, f64::NAN, 1.0, Tolerance::DEFAULT).is_err());
    assert!(FrictionSimulation::new(0.5, 0.0, f64::INFINITY, Tolerance::DEFAULT).is_err());
}

/// Rest comes from the remaining glide: `|v·dᵗ / ln d| ≤ distance`.
fn rests_when_the_remaining_glide_is_within_distance() {
    let drag: f64 = 0.135;
    let sim = FrictionSimulation::new(
        drag,
        0.0,
        8000.0,
        Tolerance::new(0.5, f64::INFINITY).expect("tolerance"),
    )
    .expect("friction");
    // Reference: 8000·dᵗ/|ln d| = 0.5 at t = 4.48741315404835 s.
    assert!(!sim.is_done(4.487_413_15));
    assert!(sim.is_done(4.487_413_16));
    let remaining = (sim.final_x() - sim.x(4.4874)).abs();
    assert!(
        remaining > 0.5 && remaining < 0.5001,
        "remaining {remaining}"
    );
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
        (
            "drag outside the open unit interval",
            drag_outside_the_open_unit_interval,
        ),
        (
            "rests when the remaining glide is within distance",
            rests_when_the_remaining_glide_is_within_distance,
        ),
    ]);
}

// --- bounds -------------------------------------------------------------------

fn unordered() {
    assert_eq!(
        SimulationBounds::new(1.0, 0.0),
        Err(SimulationError::InvalidBounds { min: 1.0, max: 0.0 })
    );
}
fn not_finite() {
    for (min, max) in [
        (f64::NAN, 1.0),
        (0.0, f64::NAN),
        (f64::NEG_INFINITY, 0.0),
        (0.0, f64::INFINITY),
    ] {
        assert!(SimulationBounds::new(min, max).is_err(), "[{min}, {max}]");
    }
}
fn empty_range_is_admitted() {
    let bounds = SimulationBounds::new(5.0, 5.0).expect("a point range");
    let sim = BoundedFrictionSimulation::new(0.135, 5.0, 100.0, bounds, Tolerance::DEFAULT)
        .expect("bounded friction");
    assert!(sim.is_done(0.0));
    assert_eq!(sim.x(1.0), 5.0);
}
fn bounded_friction_stops_at_the_bound_it_travels_toward() {
    let bounds = SimulationBounds::new(0.0, 100.0).expect("bounds");
    let sim = BoundedFrictionSimulation::new(0.135, 50.0, 8000.0, bounds, Tolerance::DEFAULT)
        .expect("bounded friction");
    let reach = friction(0.135, 50.0, 8000.0).time_at_x(100.0);
    assert!(!sim.is_done(reach * 0.999));
    assert!(sim.is_done(reach));
    assert_eq!(sim.x(reach), 100.0);
    assert_eq!(sim.dx(reach), 0.0);
}

#[test]
fn bounds_refuse_unordered_and_nan() {
    crate::run_table(&[
        ("unordered", unordered),
        ("not finite", not_finite),
        ("empty range is admitted", empty_range_is_admitted),
        (
            "bounded friction stops at the bound it travels toward",
            bounded_friction_stops_at_the_bound_it_travels_toward,
        ),
    ]);
}

// --- bouncing -----------------------------------------------------------------

fn edge_spring() -> SpringDescription {
    SpringDescription::with_damping_ratio(1.0, 500.0, 0.75)
}

/// A fling from `position` at `velocity` that the friction carries past `edge`.
fn hands_over_at(position: f64, velocity: f64, edge: f64) {
    let bounds = SimulationBounds::new(0.0, 100.0).expect("bounds");
    let drag: f64 = 0.135;
    let sim = BouncingScrollSimulation::new(
        edge_spring(),
        drag,
        position,
        velocity,
        bounds,
        Tolerance::DEFAULT,
    )
    .expect("bouncing");
    // Reference: x(t) = x₀ + v(dᵗ − 1)/ln d = edge at t = ln(1 + ln d·Δx/v)/ln d.
    let ln_d = drag.ln();
    let t_edge = (ln_d * (edge - position) / velocity).ln_1p() / ln_d;
    let v_edge = velocity * drag.powf(t_edge);
    let h = 1e-9;
    let (before, after) = (sim.x(t_edge - h), sim.x(t_edge + h));
    assert!(
        (before - edge).abs() < 1e-5 && (after - edge).abs() < 1e-5,
        "{before} {after}"
    );
    let (v_before, v_after) = (sim.dx(t_edge - h), sim.dx(t_edge + h));
    assert!(
        (v_after - v_before).abs() <= 1e-9 * v_edge.abs() + 1e-3,
        "velocity jumps from {v_before} to {v_after} at the edge"
    );
    assert!((v_before - v_edge).abs() <= 1e-9 * v_edge.abs() + 1e-3);
    // It overshoots the edge, then comes back to rest exactly on it.
    let outside = |x: f64| if edge > 50.0 { x > edge } else { x < edge };
    let overshoot = (1..400)
        .map(|i| sim.x(t_edge + f64::from(i) * 1e-3))
        .any(outside);
    assert!(overshoot, "a fling into the edge overscrolls");
    assert!(sim.is_done(t_edge + 10.0));
    assert_eq!(sim.x(t_edge + 10.0), edge);
}
fn into_the_upper_edge() {
    hands_over_at(90.0, 2000.0, 100.0);
}
fn into_the_lower_edge() {
    hands_over_at(10.0, -2000.0, 0.0);
}
fn from_the_edge_outward() {
    hands_over_at(100.0, 500.0, 100.0);
}
fn friction_that_rests_inside_never_springs() {
    let bounds = SimulationBounds::new(0.0, 1000.0).expect("bounds");
    let sim = BouncingScrollSimulation::new(
        edge_spring(),
        0.135,
        100.0,
        200.0,
        bounds,
        Tolerance::DEFAULT,
    )
    .expect("bouncing");
    let glide = friction(0.135, 100.0, 200.0);
    for t in [0.1, 0.5, 1.0] {
        assert_eq!(sim.x(t), glide.x(t));
    }
    assert!(sim.is_done(100.0));
    assert_eq!(sim.x(100.0), glide.final_x());
}
fn outside_the_range_springs_back() {
    let bounds = SimulationBounds::new(0.0, 100.0).expect("bounds");
    let sim =
        BouncingScrollSimulation::new(edge_spring(), 0.135, 120.0, 0.0, bounds, Tolerance::DEFAULT)
            .expect("bouncing");
    assert_eq!(sim.x(0.0), 120.0);
    assert!(sim.x(0.05) < 120.0);
    assert!(sim.is_done(10.0));
    assert_eq!(sim.x(10.0), 100.0);
}

#[test]
fn bouncing_simulation_hands_friction_to_spring_at_the_edge() {
    crate::run_table(&[
        ("into the upper edge", into_the_upper_edge),
        ("into the lower edge", into_the_lower_edge),
        ("from the edge outward", from_the_edge_outward),
        (
            "friction that rests inside never springs",
            friction_that_rests_inside_never_springs,
        ),
        (
            "outside the range springs back",
            outside_the_range_springs_back,
        ),
    ]);
}

// --- tolerance ----------------------------------------------------------------

fn valid_tolerances() {
    assert!(Tolerance::new(1e-3, 1e-3).is_ok());
    assert!(Tolerance::new(0.5, f64::INFINITY).is_ok());
    assert_eq!(
        Tolerance::for_device_pixel_ratio(1.0),
        Tolerance::new(0.5, f64::INFINITY)
    );
    assert_eq!(
        Tolerance::for_device_pixel_ratio(4.0),
        Tolerance::new(0.125, f64::INFINITY)
    );
}
fn refused_tolerances() {
    for (distance, velocity) in [
        (f64::NAN, 1.0),
        (0.0, 1.0),
        (-1.0, 1.0),
        (f64::INFINITY, 1.0),
        (1.0, f64::NAN),
        (1.0, 0.0),
        (1.0, -1.0),
    ] {
        assert!(
            Tolerance::new(distance, velocity).is_err(),
            "({distance}, {velocity})"
        );
    }
    for ratio in [f64::NAN, 0.0, -1.0, f64::INFINITY, 1e-310] {
        match Tolerance::for_device_pixel_ratio(ratio) {
            Err(SimulationError::OutOfRange { parameter, .. }) => {
                assert_eq!(parameter, SimulationParameter::DevicePixelRatio);
            }
            other => panic!("ratio {ratio} must be refused, got {other:?}"),
        }
    }
}
fn device_pixel_ratio_scales_the_rest() {
    // 8000 px/s at drag 0.135 leaves half a device pixel at 4.4874 s (dpr 1)
    // and at 4.8336 s (dpr 2).
    let at = |dpr: f64| {
        FrictionSimulation::new(
            0.135,
            0.0,
            8000.0,
            Tolerance::for_device_pixel_ratio(dpr).expect("ratio"),
        )
        .expect("friction")
    };
    assert!(!at(1.0).is_done(4.4874) && at(1.0).is_done(4.4875));
    assert!(!at(2.0).is_done(4.8335) && at(2.0).is_done(4.8336));
}

fn color_spring_fades_to_transparent_without_darkening() {
    // Springs run in premultiplied Oklab (ADR-0149): a fade to transparent black
    // keeps the opaque end's red instead of passing through dark red.
    let red = Color::rgb(255, 0, 0);
    let spring =
        SpringDescription::with_response_and_damping(std::time::Duration::from_millis(300), 1.0)
            .expect("a critically damped 300 ms spring is valid");
    let mut v = flui_animation::AnimatedValue::new(red, spring).expect("finite colour");
    assert_eq!(v.value(), red);
    v.animate_to(Color::TRANSPARENT).expect("finite colour");
    for frame in 0..120 {
        v.advance(std::time::Duration::from_secs_f64(1.0 / 60.0));
        let color = v.value();
        if color.a > 0 {
            assert!(
                color.r >= 250 && color.g <= 5 && color.b <= 5,
                "frame {frame}: {color:?} darkened on the way out"
            );
        }
    }
    assert!(v.is_settled());
    assert_eq!(v.value().a, 0);
}

#[test]
fn tolerance_constructors_validate_and_scale_with_dpr() {
    crate::run_table(&[
        ("valid tolerances", valid_tolerances),
        ("refused tolerances", refused_tolerances),
        (
            "device pixel ratio scales the rest",
            device_pixel_ratio_scales_the_rest,
        ),
        (
            "color spring fades to transparent without darkening",
            color_spring_fades_to_transparent_without_darkening,
        ),
    ]);
}
