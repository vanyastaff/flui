//! The restart-aware controller registry: a controller run **twice** (forward
//! to completion, then reverse) is ticked from the *second* run's own start, not
//! a stale anchor.
//!
//! This is the discriminating test for the `run_generation` re-anchoring in
//! [`flui_animation::Vsync`] / `pump_frame`. With a naive fixed
//! anchor (recorded once at registration), the first reverse pump would feed
//! `tick_at(huge_elapsed)` and snap the value straight to the target (0.0,
//! Dismissed) on a single frame. Re-anchoring on the observed run-generation
//! bump makes the reverse leg advance from 1.0 over its own timeline. No tree is
//! bound here, so the registry is exercised in isolation — and no `thread::sleep`.

use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_testing::HeadlessBinding;

/// One frame's worth of virtual time at 20ms — five of these span the 100ms run.
const FRAME: Duration = Duration::from_millis(20);

pub(crate) fn adopting_a_registry_revokes_the_previous_driver_binding() {
    let mut binding = HeadlessBinding::new();
    let old_registry = binding.vsync().clone();
    binding.adopt_vsync(flui_animation::Vsync::new());
    let old_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&old_registry));
    old_owner
        .controller()
        .forward()
        .expect("standalone old registry run");
    assert!(
        !binding.scheduler().has_scheduled_frame(),
        "an obsolete registry cannot wake the binding that replaced it"
    );
    let new_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(binding.vsync()));
    new_owner
        .controller()
        .forward()
        .expect("current registry run");
    assert!(
        binding.scheduler().has_scheduled_frame(),
        "the replacement registry retains its frame driver"
    );
}

pub(crate) fn driver_replacement_preserves_first_failure_and_recovers() {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;
    use std::sync::Arc;
    struct Capture {
        retired: Rc<Cell<usize>>,
        fail: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.retired.set(self.retired.get() + 1);
            assert!(!self.fail, "retirement failure");
        }
    }
    for (installation_fails, retirement_fails) in [(true, false), (false, true), (true, true)] {
        let mut binding = HeadlessBinding::new();
        let retired = Rc::new(Cell::new(0));
        let capture = Capture {
            retired: retired.clone(),
            fail: retirement_fails,
        };
        binding.vsync().set_frame_requester(Some(Rc::new(move || {
            let _ = &capture;
        })));
        let replacement = flui_animation::Vsync::new();
        let owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&replacement));
        owner
            .controller()
            .forward()
            .expect("run admitted before driver installation");
        binding
            .scheduler()
            .set_on_frame_scheduled(Some(Arc::new(move || {
                assert!(!installation_fails, "installation failure");
            })));
        let failure = catch_unwind(AssertUnwindSafe(|| {
            binding.adopt_vsync(replacement.clone());
        }))
        .expect_err("replacement contains the injected failure");
        let expected = if installation_fails {
            "installation failure"
        } else {
            "retirement failure"
        };
        assert_eq!(failure.downcast_ref::<&str>(), Some(&expected));
        assert_eq!(
            retired.get(),
            1,
            "installation failure cannot skip outgoing retirement"
        );
        assert!(
            binding.vsync().is_same(&replacement),
            "new driver state is committed before callouts"
        );
        binding
            .scheduler()
            .set_on_frame_scheduled(Some(Arc::new(|| {})));
        let run = owner
            .controller()
            .forward()
            .expect("the replacement stays usable");
        binding.pump_frame(Duration::ZERO);
        binding.pump_frame(Duration::from_secs(1));
        assert!(
            run.is_complete(),
            "accepted work progresses after containment"
        );
    }
}

pub(crate) fn starting_an_idle_bound_controller_requests_its_first_frame() {
    let mut binding = HeadlessBinding::new();
    let owner =
        AnimationController::builder(Duration::from_millis(100)).build_on(Some(binding.vsync()));
    assert!(
        !binding.scheduler().has_scheduled_frame(),
        "registering an idle controller must leave the frame driver idle",
    );

    owner
        .controller()
        .forward()
        .expect("a fresh controller forwards");
    assert!(
        binding.scheduler().has_scheduled_frame(),
        "accepting a bound run must request its first frame before any manual pump",
    );

    for _ in 0..6 {
        binding.pump_frame(FRAME);
    }
    assert_eq!(owner.controller().status(), AnimationStatus::Completed);
    assert_eq!(owner.controller().value(), 1.0);
}

pub(crate) fn second_run_ticks_from_its_own_start_not_a_stale_anchor() {
    let mut binding = HeadlessBinding::new();
    let owner =
        AnimationController::builder(Duration::from_millis(100)).build_on(Some(binding.vsync()));
    let controller = owner.controller();

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

    drop(owner);
}
