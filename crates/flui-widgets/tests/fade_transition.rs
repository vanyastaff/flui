//! End-to-end animation: a `FadeTransition` driven by a real
//! `AnimationController`. Proves the full reactive chain through the widget
//! layer — a listenable tick schedules the transition's rebuild (via the
//! external build inbox, NOT a root `setState`), which re-reads the animation
//! value into its `Opacity` render object.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{lay_out, tight};
use flui_animation::{Animation, AnimationController};
use flui_scheduler::Scheduler;
use flui_widgets::{FadeTransition, SizedBox};

#[test]
fn fade_transition_reads_animation_opacity_on_each_tick() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    controller.set_value(0.25);
    // The controller's clone shares its notifier, so `set_value` on the handle
    // below notifies the listener the FadeTransition registers on this `Arc`.
    let opacity: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    let mut laid = lay_out(
        FadeTransition::new(opacity, SizedBox::new(100.0, 50.0)),
        tight(100.0, 50.0),
    );

    // The render-tree root is the FadeTransition's `Opacity` render object.
    let render_opacity = laid.root();
    assert!(
        (laid.opacity(render_opacity) - 0.25).abs() < 1e-4,
        "initial opacity comes from the animation value: {}",
        laid.opacity(render_opacity),
    );

    // An animation tick between frames: change the value + notify. The
    // FadeTransition's listenable subscription must schedule its rebuild so the
    // next frame re-reads the new value — `tick()` does NOT mark the root, so
    // only the external-inbox path can drive this.
    controller.set_value(0.8);
    laid.tick();

    assert!(
        (laid.opacity(render_opacity) - 0.8).abs() < 1e-4,
        "the tick re-read the updated animation value into the Opacity: {}",
        laid.opacity(render_opacity),
    );
}

#[test]
fn fade_transition_lays_its_child_out_as_a_passthrough() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    controller.set_value(1.0);
    let opacity: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    // Opacity is paint-only; the child keeps its size and the transition sizes
    // to it.
    let laid = lay_out(
        FadeTransition::new(opacity, SizedBox::new(120.0, 80.0)),
        tight(120.0, 80.0),
    );

    let render_opacity = laid.root();
    assert_eq!(laid.size(render_opacity), crate::common::size(120.0, 80.0));
}
