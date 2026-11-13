//! The restart-aware controller registry: a controller run **twice** (forward
//! to completion, then reverse) is ticked from the *second* run's own start, not
//! a stale anchor.
//!
//! This is the discriminating test for the `run_generation` re-anchoring in
//! [`flui_animation::Vsync`]. With a naive fixed anchor (recorded once at
//! registration), the first reverse pump would feed `tick_at(huge_elapsed)` and
//! snap the value straight to the target (0.0, ReverseSettled) on a single
//! frame. Re-anchoring on the observed run-generation bump makes the reverse leg
//! advance from 1.0 over its own timeline. Each registry is adopted by a UI
//! runtime that ticks it beside its own, and no `thread::sleep`.

use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus, Vsync};
use flui_testing::widgets::{LaidOut, lay_out_animated, tight};
use flui_widgets::SizedBox;

/// One frame's worth of virtual time at 20ms — five of these span the 100ms run.
const FRAME: Duration = Duration::from_millis(20);

pub(crate) fn widget_registry_adoption_preserves_its_driver_on_refusal() {
    use flui_animation::VsyncRegistrationError;
    let current = Vsync::new();
    let mut laid = lay_out_animated(
        SizedBox::new(40.0, 24.0),
        tight(40.0, 24.0),
        current.clone(),
    );
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&current));
    let run = owner.controller().forward().expect("resident run");
    laid.adopt_vsync(current).expect("same registry is a no-op");
    let foreign_parent = Vsync::new();
    let foreign_child = Vsync::new();
    let foreign_seat = foreign_parent
        .attach_child(&foreign_child)
        .expect("foreign owner");
    assert_eq!(
        laid.adopt_vsync(foreign_child.clone()),
        Err(VsyncRegistrationError::AlreadyAttached)
    );
    let closed = Vsync::new();
    closed.prepare_close().finish();
    assert_eq!(
        laid.adopt_vsync(closed),
        Err(VsyncRegistrationError::Closed)
    );
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_secs(1));
    assert!(
        run.is_complete(),
        "refused replacements preserve the current driver"
    );
    foreign_parent.detach_child(&foreign_seat);
    laid.adopt_vsync(foreign_child.clone())
        .expect("released child can be adopted");
    let replacement =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&foreign_child));
    let next = replacement
        .controller()
        .forward()
        .expect("new registry run");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_secs(1));
    assert!(next.is_complete());
}

/// A tree laid out with `vsync` adopted, pumped until no frame is due, so a
/// test sees exactly the frames its own run requests.
fn idle_with(vsync: &Vsync) -> LaidOut {
    let mut laid = lay_out_animated(SizedBox::new(40.0, 24.0), tight(40.0, 24.0), vsync.clone());
    laid.pump_for(Duration::ZERO);
    assert!(
        !laid.scheduler().is_frame_scheduled(),
        "a laid-out tree with no run in flight is idle"
    );
    laid
}

pub(crate) fn adopting_a_registry_revokes_the_previous_driver_binding() {
    let old_registry = Vsync::new();
    let mut laid = idle_with(&old_registry);
    let current = Vsync::new();
    laid.adopt_vsync(current.clone())
        .expect("an open registry without a parent is admitted");
    let old_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old_registry));
    old_owner
        .controller()
        .forward()
        .expect("standalone old registry run");
    assert!(
        !laid.scheduler().is_frame_scheduled(),
        "an obsolete registry cannot wake the runtime that replaced it"
    );
    let new_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&current));
    new_owner
        .controller()
        .forward()
        .expect("current registry run");
    assert!(
        laid.scheduler().is_frame_scheduled(),
        "the replacement registry retains its frame driver"
    );
}

pub(crate) fn starting_an_idle_bound_controller_requests_its_first_frame() {
    let vsync = Vsync::new();
    let mut laid = idle_with(&vsync);
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&vsync));
    assert!(
        !laid.scheduler().is_frame_scheduled(),
        "registering an idle controller must leave the frame driver idle",
    );

    owner
        .controller()
        .forward()
        .expect("a fresh controller forwards");
    assert!(
        laid.scheduler().is_frame_scheduled(),
        "accepting a bound run must request its first frame before any manual pump",
    );

    for _ in 0..6 {
        laid.pump_for(FRAME);
    }
    assert_eq!(owner.controller().status(), AnimationStatus::ForwardSettled);
    assert_eq!(owner.controller().value(), 1.0);
}

pub(crate) fn second_run_ticks_from_its_own_start_not_a_stale_anchor() {
    let vsync = Vsync::new();
    let mut laid = idle_with(&vsync);
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&vsync));
    let controller = owner.controller();
    // --- Run 1: forward to completion. ---
    controller.forward().expect("a fresh controller forwards");
    // Detection frame holds the start value; subsequent frames climb to 1.0.
    for _ in 0..6 {
        laid.pump_for(FRAME);
    }
    assert_eq!(
        controller.status(),
        AnimationStatus::ForwardSettled,
        "the forward run completes after being fully pumped",
    );
    assert!(
        (controller.value() - 1.0).abs() < 1e-4,
        "forward ends at the upper bound, got {}",
        controller.value(),
    );

    // --- Run 2: reverse. This re-zeros the controller's run epoch; the runtime
    // must re-anchor instead of carrying run 1's stale start. ---
    controller
        .reverse()
        .expect("a completed controller reverses");

    // The detection frame of the reverse run holds ~1.0 (first tick is elapsed
    // 0). The NAIVE fixed-anchor model fails here: it would feed a large elapsed
    // and snap straight to 0.0 / ReverseSettled on this very frame.
    laid.pump_for(FRAME);
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
        laid.pump_for(FRAME);
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
        AnimationStatus::ReverseSettled,
        "a fully pumped reverse run dismisses",
    );

    drop(owner);
}
