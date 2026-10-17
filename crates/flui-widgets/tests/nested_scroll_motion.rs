//! A real release and frame-driven continuation must stay in the same channel.

use super::*;
use flui_widgets::{FlingStep, ScrollCause, VsyncScope};

#[path = "nested_scroll_motion_failures.rs"]
mod failures;
#[path = "nested_scroll_handoff.rs"]
mod handoff;
pub(super) use failures::a_failing_final_derivative_completes_cleanup_and_preserves_first_failure;
pub(super) use handoff::three_level_handoff_keeps_identity_and_uses_the_actual_sampled_derivative;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
enum MotionEvent {
    Started(InputSequenceId),
    Step(ScrollStep),
    Released(FlingStep),
    End(InputSequenceId, ScrollEndReason),
}

struct MotionProbe(Rc<RefCell<Vec<MotionEvent>>>);

impl NestedScrollConnection for MotionProbe {
    fn sequence_started(&self, sequence: InputSequenceId) {
        self.0.borrow_mut().push(MotionEvent::Started(sequence));
    }
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        self.0.borrow_mut().push(MotionEvent::Step(*step));
        Vec2::ZERO
    }
    fn released(&self, step: &FlingStep) {
        self.0.borrow_mut().push(MotionEvent::Released(*step));
    }
    fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        self.0.borrow_mut().push(MotionEvent::End(sequence, reason));
    }
}

pub(super) fn release_and_framework_frames_retain_one_sequence_until_actual_rest() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let controller = ScrollController::new();
    let vsync = flui_animation::Vsync::new();
    let mut laid = lay_out(
        VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(Rc::new(MotionProbe(log.clone()))).child(
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(100.0, 5100.0)),
            ),
        ),
        tight(100.0, 100.0),
    );
    laid.adopt_vsync(vsync).expect("owner clock");
    laid.pipeline_owner()
        .with_mut(|owner| owner.set_device_pixel_ratio(2.0));
    laid.dispatch_pointer_down(50.0, 80.0);
    laid.dispatch_pointer_move_after(50.0, 60.0, Duration::from_millis(20));
    laid.dispatch_pointer_move_after(50.0, 40.0, Duration::from_millis(20));
    laid.dispatch_pointer_move_after(50.0, 20.0, Duration::from_millis(20));
    laid.dispatch_pointer_up(50.0, 20.0);
    let id = match log.borrow().first() {
        Some(MotionEvent::Started(id)) => *id,
        other => panic!("missing source: {other:?}"),
    };
    {
        let log = log.borrow();
        let releases: Vec<_> = log
            .iter()
            .filter_map(|event| {
                if let MotionEvent::Released(step) = event {
                    Some(step)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            releases.len(),
            1,
            "release is delivered before animation admission: {log:?}"
        );
        assert_eq!(releases[0].sequence, id);
        assert_eq!(releases[0].device_pixel_ratio, 2.0);
        assert!(releases[0].available.pixels_per_second.dy < -500.0);
        assert!(
            !log.iter()
                .any(|event| matches!(event, MotionEvent::End(..))),
            "a running continuation still owes its terminal event"
        );
    }
    let released_pixels = controller.pixels();
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert!(
        controller.pixels() > released_pixels,
        "the actual viewport advances"
    );
    assert!(log.borrow().iter().any(|event| matches!(event, MotionEvent::Step(step) if step.cause == ScrollCause::SideEffect && step.sequence == id && step.available.y < 0.0)), "simulation deltas reach authored pre/post hooks");
    laid.pump_for(Duration::from_secs(6));
    let log = log.borrow();
    let ends: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let MotionEvent::End(sequence, reason) = event {
                Some((*sequence, *reason))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(ends, [(id, ScrollEndReason::Released)]);
    assert!(!controller.position().is_scrolling());
    assert!(log.iter().all(|event| match event {
        MotionEvent::Started(sequence) | MotionEvent::End(sequence, _) => *sequence == id,
        MotionEvent::Step(step) => step.sequence == id,
        MotionEvent::Released(step) => step.sequence == id,
    }));
}

pub(super) fn a_new_contact_preempts_the_exact_continuation_before_stopping_animation() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let controller = ScrollController::new();
    let vsync = flui_animation::Vsync::new();
    let mut laid = lay_out(
        VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(Rc::new(MotionProbe(log.clone()))).child(
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(100.0, 5100.0)),
            ),
        ),
        tight(100.0, 100.0),
    );
    laid.adopt_vsync(vsync).expect("owner clock");
    laid.dispatch_pointer_down(50.0, 80.0);
    for y in [60.0, 40.0, 20.0] {
        laid.dispatch_pointer_move_after(50.0, y, Duration::from_millis(20));
    }
    laid.dispatch_pointer_up(50.0, 20.0);
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    let first = match log.borrow().first() {
        Some(MotionEvent::Started(id)) => *id,
        other => panic!("source start: {other:?}"),
    };
    assert!(
        !log.borrow()
            .iter()
            .any(|event| matches!(event, MotionEvent::End(..)))
    );
    laid.dispatch_pointer_down(50.0, 80.0);
    let after_grab = log.borrow().len();
    let ends: Vec<_> = log
        .borrow()
        .iter()
        .filter_map(|event| {
            if let MotionEvent::End(sequence, reason) = event {
                Some((*sequence, *reason))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        ends,
        [(first, ScrollEndReason::Preempted)],
        "the sequence is withdrawn before animation cancellation can report a generic Cancelled"
    );
    laid.dispatch_pointer_move(50.0, 60.0);
    let pixels = controller.pixels();
    laid.pump_for(Duration::from_millis(100));
    assert_eq!(
        controller.pixels(),
        pixels,
        "old continuation cannot move the new contact"
    );
    assert!(
        !log.borrow()[after_grab..]
            .iter()
            .any(|event| matches!(event, MotionEvent::Step(step) if step.sequence == first))
    );
    laid.dispatch_pointer_cancel();
    let log = log.borrow();
    let starts: Vec<_> = log
        .iter()
        .filter_map(|event| {
            if let MotionEvent::Started(sequence) = event {
                Some(*sequence)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(starts.len(), 2);
    assert_ne!(starts[0], starts[1]);
    assert_eq!(
        log.iter()
            .filter(|event| matches!(event, MotionEvent::End(sequence, _) if *sequence == first))
            .count(),
        1
    );
}

/// Takes half of every frame-driven step, so the leaf travels the other half.
struct HalfOfEachFrame(Rc<std::cell::Cell<f64>>);

impl NestedScrollConnection for HalfOfEachFrame {
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        if step.cause != ScrollCause::SideEffect {
            return Vec2::ZERO;
        }
        let half = Vec2::new(step.available.x * 0.5, step.available.y * 0.5);
        self.0.set(self.0.get() + half.y.abs());
        half
    }
}

// A coast that settles keeps exactly the share its participants left the
/// leaf; the raw simulation never overwrites what they took.
pub(super) fn a_settled_run_keeps_the_motion_its_participants_consumed() {
    let taken = Rc::new(std::cell::Cell::new(0.0));
    let controller = ScrollController::new();
    let vsync = flui_animation::Vsync::new();
    let mut laid = lay_out(
        VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(Rc::new(HalfOfEachFrame(Rc::clone(&taken)))).child(
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(100.0, 5100.0)),
            ),
        ),
        tight(100.0, 100.0),
    );
    laid.adopt_vsync(vsync).expect("owner clock");
    laid.dispatch_pointer_down(50.0, 80.0);
    for y in [60.0, 40.0, 20.0] {
        laid.dispatch_pointer_move_after(50.0, y, Duration::from_millis(20));
    }
    laid.dispatch_pointer_up(50.0, 20.0);
    let released = controller.pixels();
    for _ in 0..400 {
        laid.pump_for(Duration::from_millis(16));
    }
    let travelled = controller.pixels() - released;
    assert!(taken.get() > 10.0, "the participant took part of the coast");
    assert!(
        (travelled - taken.get()).abs() < 1.0,
        "the leaf keeps only its half: travelled {travelled:.3}, taken {:.3}",
        taken.get()
    );
}
