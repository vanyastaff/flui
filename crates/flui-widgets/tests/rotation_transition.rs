//! End-to-end animation: a `RotationTransition` driven by a real
//! `AnimationController` — the third transition over the shared spine, proving
//! the value reaches a `Transform` rotation matrix each tick.

mod common;

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;
use std::time::Duration;

use common::{lay_out, size, tight};
use flui_animation::{Animation, AnimationController};
use flui_scheduler::Scheduler;
use flui_widgets::{RotationTransition, SizedBox};

#[test]
fn rotation_transition_reads_animation_turns_on_each_tick() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    // turns are in [0, 1] (one full revolution) — the default bounds fit.
    let turns: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    let mut laid = lay_out(
        RotationTransition::new(turns, SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    let render_transform = laid.root();
    assert!(
        laid.transform_rotation(render_transform).abs() < 1e-4,
        "0 turns is no rotation: {}",
        laid.transform_rotation(render_transform),
    );

    // A quarter turn → π/2 radians. Tick (no root mark) drives it via the inbox.
    controller.set_value(0.25);
    laid.tick();

    assert!(
        (laid.transform_rotation(render_transform) - FRAC_PI_2).abs() < 1e-4,
        "a quarter turn is π/2 radians: {}",
        laid.transform_rotation(render_transform),
    );
}

#[test]
fn rotation_transition_lays_its_child_out_as_a_passthrough() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    controller.set_value(0.5);
    let turns: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    let laid = lay_out(
        RotationTransition::new(turns, SizedBox::new(80.0, 60.0)),
        tight(80.0, 60.0),
    );

    assert_eq!(laid.size(laid.root()), size(80.0, 60.0));
}
