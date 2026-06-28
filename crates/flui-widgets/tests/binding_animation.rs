//! End-to-end animation through [`HeadlessBinding::pump_frame`]: a
//! `FadeTransition` over an [`AnimationController`] *registered with the
//! binding* advances its opacity frame-to-frame as virtual time is pumped — no
//! `set_value`, no `thread::sleep`.
//!
//! This exercises the full Phase-1b chain inside a single `pump_frame`: the
//! binding ticks the registered controller (`tick_at`) → the controller
//! notifies its listeners → the `FadeTransition`'s subscription marks it dirty
//! into the build inbox → `build_scope` drains it and rebuilds → `run_frame`
//! re-reads the new value into the `Opacity` render object. It would fail if the
//! controller were not ticked inside the frame, or if the tick ran *after*
//! `build_scope` (the dirty entry would miss this frame's drain — a one-frame
//! lag).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{lay_out, tight};
use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_scheduler::Scheduler;
use flui_widgets::{FadeTransition, SizedBox};

/// A registered, running controller drives a `FadeTransition`'s opacity upward
/// frame-to-frame as `pump_for` advances virtual time.
#[test]
fn registered_controller_advances_fade_opacity_frame_to_frame() {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(100), scheduler);
    // The `Arc<dyn Animation>` handed to the FadeTransition and the clone
    // registered with the binding share the same inner notifier, so a binding
    // tick notifies the transition's listener.
    let opacity: Arc<dyn Animation<f32>> = Arc::new(controller.clone());

    let mut laid = lay_out(
        FadeTransition::new(opacity, SizedBox::new(100.0, 50.0)),
        tight(100.0, 50.0),
    );
    let render_opacity = laid.root();

    // Register before starting, then start: the binding re-anchors this run's
    // `t = 0` on the first pump that observes the new run-generation.
    laid.register_controller(controller.clone());
    controller.forward().expect("a fresh controller forwards");

    // The detection frame (first pump after `forward`) holds the run-start value
    // — Flutter's first ticker tick delivers elapsed 0. Movement begins next pump.
    laid.pump_for(Duration::from_millis(20));
    assert!(
        laid.opacity(render_opacity).abs() < 1e-4,
        "first pump holds the run-start opacity (0.0), got {}",
        laid.opacity(render_opacity),
    );

    // Five more 20ms pumps over a 100ms run: opacity climbs 0.2, 0.4, 0.6, 0.8,
    // 1.0 (the controller's default tween is linear).
    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(Duration::from_millis(20));
        samples.push(laid.opacity(render_opacity));
    }

    // Strictly increasing — proves the value is re-read each frame, not stuck.
    for pair in samples.windows(2) {
        assert!(
            pair[1] > pair[0] - 1e-6,
            "opacity must not regress across pumps: {samples:?}",
        );
    }
    // An intermediate frame sits strictly between the endpoints (~0.6 here),
    // proving the in-between frames render partial opacity, not a snap.
    let intermediate = samples[2];
    assert!(
        intermediate > 0.05 && intermediate < 0.95,
        "an intermediate frame shows partial opacity, got {intermediate}",
    );
    // The run completes at full opacity.
    let final_opacity = samples[4];
    assert!(
        (final_opacity - 1.0).abs() < 1e-4,
        "the run ends at full opacity, got {final_opacity}",
    );
    assert_eq!(
        controller.status(),
        AnimationStatus::Completed,
        "a fully pumped forward run completes",
    );
}
