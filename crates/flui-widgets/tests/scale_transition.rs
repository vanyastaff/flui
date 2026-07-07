//! End-to-end animation: a `ScaleTransition` driven by a real
//! `AnimationController`. Like the `FadeTransition` test, but proves the
//! transition spine generalizes from `Opacity` to a `Transform` property — a
//! tick re-reads the animation value into the render object's scale matrix.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{lay_out, size, tight};
use flui_animation::{Animation, AnimationController};
use flui_scheduler::Scheduler;
use flui_widgets::{ScaleTransition, SizedBox};

#[test]
fn scale_transition_reads_animation_scale_on_each_tick() {
    let scheduler = Arc::new(Scheduler::new());
    // Bounds 0..2 so a scale-up past 1.0 is representable (the default
    // controller clamps to 0..1).
    let controller =
        AnimationController::with_bounds(Duration::from_millis(300), scheduler, 0.0, 2.0)
            .expect("0.0 < 2.0 is valid bounds");
    controller.set_value(0.5);
    let scale: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    let mut laid = lay_out(
        ScaleTransition::new(scale, SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    let render_transform = laid.root();
    assert!(
        (laid.transform_scale(render_transform) - 0.5).abs() < 1e-4,
        "initial scale from the animation: {}",
        laid.transform_scale(render_transform),
    );

    // Tick (no root mark): only the external-inbox path can drive the rebuild.
    controller.set_value(1.5);
    laid.tick();

    assert!(
        (laid.transform_scale(render_transform) - 1.5).abs() < 1e-4,
        "the tick re-read the updated scale into the Transform: {}",
        laid.transform_scale(render_transform),
    );
}

#[test]
fn scale_transition_lays_its_child_out_as_a_passthrough() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    controller.set_value(1.0);
    let scale: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    // Transform is paint-only: the child keeps its size regardless of scale.
    let laid = lay_out(
        ScaleTransition::new(scale, SizedBox::new(80.0, 60.0)),
        tight(80.0, 60.0),
    );

    assert_eq!(laid.size(laid.root()), size(80.0, 60.0));
}
