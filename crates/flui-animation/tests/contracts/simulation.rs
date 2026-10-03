//! Public friction simulation behavior.

use flui_animation::{FrictionSimulation, Simulation};

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
