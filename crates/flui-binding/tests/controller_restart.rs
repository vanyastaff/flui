//! The restart-aware controller registry: a controller run **twice** (forward
//! to completion, then reverse) is ticked from the *second* run's own start, not
//! a stale anchor.
//!
//! This is the discriminating test for the `run_generation` re-anchoring in
//! [`HeadlessBinding::register_controller`] / `pump_frame`. With a naive fixed
//! anchor (recorded once at registration), the first reverse pump would feed
//! `tick_at(huge_elapsed)` and snap the value straight to the target (0.0,
//! Dismissed) on a single frame. Re-anchoring on the observed run-generation
//! bump makes the reverse leg advance from 1.0 over its own timeline. No tree is
//! bound here, so the registry is exercised in isolation — and no `thread::sleep`.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus, Scheduler};
use flui_binding::HeadlessBinding;

/// One frame's worth of virtual time at 20ms — five of these span the 100ms run.
const FRAME: Duration = Duration::from_millis(20);

#[test]
fn second_run_ticks_from_its_own_start_not_a_stale_anchor() {
    let mut binding = HeadlessBinding::new();
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(100), scheduler);

    // Register before starting so the binding cleanly re-anchors on the first
    // observed run-generation bump.
    binding.register_controller(controller.clone());

    // --- Run 1: forward to completion. ---
    controller.forward().expect("a fresh controller forwards");
    // Detection frame holds the start value; subsequent frames climb to 1.0.
    for _ in 0..6 {
        binding.pump_frame(FRAME);
    }
    assert_eq!(
        controller.status(),
        AnimationStatus::Completed,
        "the forward run completes after being fully pumped",
    );
    assert!(
        (controller.value() - 1.0).abs() < 1e-4,
        "forward ends at the upper bound, got {}",
        controller.value(),
    );

    // --- Run 2: reverse. This re-zeros the controller's run epoch; the binding
    // must re-anchor instead of carrying run 1's stale start. ---
    controller
        .reverse()
        .expect("a completed controller reverses");

    // The detection frame of the reverse run holds ~1.0 (first tick is elapsed
    // 0). The NAIVE fixed-anchor model fails here: it would feed a large elapsed
    // and snap straight to 0.0 / Dismissed on this very frame.
    binding.pump_frame(FRAME);
    assert!(
        controller.value() > 0.5,
        "the first reverse pump must HOLD near 1.0, not snap to 0.0 \
         (stale-anchor regression): got {}",
        controller.value(),
    );
    assert_eq!(
        controller.status(),
        AnimationStatus::Reverse,
        "the reverse run is still in flight after one frame, not settled",
    );

    // Subsequent frames descend strictly toward 0.0.
    let mut samples = Vec::new();
    for _ in 0..5 {
        binding.pump_frame(FRAME);
        samples.push(controller.value());
    }
    for pair in samples.windows(2) {
        assert!(
            pair[1] < pair[0] + 1e-6,
            "the reverse run must descend monotonically: {samples:?}",
        );
    }
    assert!(
        controller.value().abs() < 1e-4,
        "the reverse run ends at the lower bound, got {}",
        controller.value(),
    );
    assert_eq!(
        controller.status(),
        AnimationStatus::Dismissed,
        "a fully pumped reverse run dismisses",
    );

    controller.dispose();
}
