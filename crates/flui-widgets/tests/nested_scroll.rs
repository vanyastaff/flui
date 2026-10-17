//! Public mounted witnesses for the connection channel, with real offsets.

use std::{cell::RefCell, rc::Rc};

use flui_foundation::{
    InputSequenceId,
    geometry::{Axis, Vec2},
};
use flui_widgets::{
    NestedScroll, NestedScrollConnection, ScrollController, ScrollEndReason, ScrollStep,
    Scrollable, SizedBox,
};

use crate::common::{lay_out, tight};

#[path = "nested_scroll/failures.rs"]
mod failures;
#[path = "nested_scroll/motion.rs"]
mod motion;

#[test]
fn nested_scroll_channel() {
    crate::common::cases::run_cases(
        "nested_scroll_channel",
        &[
            (
                "three-level sampled handoff",
                motion::three_level_handoff_keeps_identity_and_uses_the_actual_sampled_derivative
                    as fn(),
            ),
            (
                "new contact preempts exact continuation",
                motion::a_new_contact_preempts_the_exact_continuation_before_stopping_animation
                    as fn(),
            ),
            (
                "final derivative failure cleanup",
                motion::a_failing_final_derivative_completes_cleanup_and_preserves_first_failure,
            ),
            (
                "settled run keeps consumed motion",
                motion::a_settled_run_keeps_the_motion_its_participants_consumed as fn(),
            ),
            (
                "release and actual framework continuation",
                motion::release_and_framework_frames_retain_one_sequence_until_actual_rest
                    as fn(),
            ),
            (
                "authored pre/post with real offsets",
                authored_pre_and_post_wrap_real_offset_consumption as fn(),
            ),
            (
                "orthogonal owners share a drag",
                orthogonal_scroll_owners_share_the_same_root_motion,
            ),
            (
                "same-axis directional ownership",
                same_axis_latch_survives_extent_until_source_ends,
            ),
            (
                "transformed participant planes",
                each_connection_receives_its_own_committed_plane,
            ),
            (
                "consumption validation",
                invalid_consumption_is_clamped_before_reaching_offsets,
            ),
            (
                "same-configuration rebuild",
                rebuilding_does_not_replace_the_active_contact,
            ),
            (
                "replacement withdraws accepted membership",
                replacement_does_not_retarget_an_active_source,
            ),
            (
                "overscroll containment",
                containment_stops_only_its_owner_axis,
            ),
            (
                "single and competing callout failures",
                failures::participant_failure_closes_exact_links_and_a_new_sequence_recovers,
            ),
        ],
    );
}

fn replacement_does_not_retarget_an_active_source() {
    let old = Rc::new(RefCell::new(Vec::new()));
    let new = Rc::new(RefCell::new(Vec::new()));
    let controller = ScrollController::new();
    let content = |log| {
        NestedScroll::new(Rc::new(Header(log))).child(
            Scrollable::new()
                .controller(controller.clone())
                .child(SizedBox::new(100.0, 600.0)),
        )
    };
    let mut laid = lay_out(content(old.clone()), tight(100.0, 100.0));
    laid.dispatch_pointer_down(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 40.0);
    assert_eq!(controller.pixels(), 25.0);
    laid.pump_widget(content(new.clone()));
    assert!(matches!(
        old.borrow().last(),
        Some(Observed::End(_, ScrollEndReason::Cancelled))
    ));
    laid.dispatch_pointer_move(50.0, 20.0);
    assert_eq!(
        controller.pixels(),
        25.0,
        "the old contact has no replacement owner"
    );
    assert!(
        new.borrow().is_empty(),
        "replacement is not discovered by an accepted source"
    );
    laid.dispatch_pointer_cancel();
    laid.dispatch_pointer_down(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 40.0);
    assert_eq!(controller.pixels(), 50.0);
    laid.dispatch_pointer_cancel();
    assert_eq!(
        old.borrow()
            .iter()
            .filter(|event| matches!(event, Observed::End(..)))
            .count(),
        1
    );
    assert_eq!(
        new.borrow()
            .iter()
            .filter(|event| matches!(event, Observed::Start(_)))
            .count(),
        1
    );
}

fn containment_stops_only_its_owner_axis() {
    for behavior in [
        flui_widgets::OverscrollBehavior::Auto,
        flui_widgets::OverscrollBehavior::Contain,
    ] {
        let outer = ScrollController::new();
        let inner = ScrollController::with_initial_scroll_offset(500.0);
        let laid = lay_out(
            Scrollable::new().controller(outer.clone()).child(
                SizedBox::new(100.0, 600.0).child(
                    flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT).child(
                        SizedBox::new(100.0, 100.0).child(
                            Scrollable::new()
                                .controller(inner.clone())
                                .overscroll_behavior(behavior)
                                .child(SizedBox::new(100.0, 600.0)),
                        ),
                    ),
                ),
            ),
            tight(100.0, 100.0),
        );
        laid.dispatch_pointer_down(50.0, 90.0);
        laid.dispatch_pointer_move(50.0, 60.0);
        laid.dispatch_pointer_move(50.0, 30.0);
        assert_eq!(
            outer.pixels(),
            if behavior == flui_widgets::OverscrollBehavior::Auto {
                30.0
            } else {
                0.0
            }
        );
        assert_eq!(inner.pixels(), 500.0);
        laid.dispatch_pointer_cancel();
    }
}

fn each_connection_receives_its_own_committed_plane() {
    use flui_foundation::geometry::Matrix4;
    use flui_widgets::Transform;
    let log = Rc::new(RefCell::new(Vec::new()));
    let controller = ScrollController::new();
    let laid = lay_out(
        Transform::new(Matrix4::scaling(2.0, 2.0, 1.0))
            .alignment(flui_painting::Alignment::CENTER)
            .child(
                NestedScroll::new(Rc::new(Header(log.clone()))).child(
                    Transform::new(Matrix4::scaling(1.0, 0.5, 1.0))
                        .alignment(flui_painting::Alignment::CENTER)
                        .child(
                            Scrollable::new()
                                .controller(controller.clone())
                                .child(SizedBox::new(100.0, 600.0)),
                        ),
                ),
            ),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_down(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 40.0);
    assert_eq!(
        controller.pixels(),
        10.0,
        "15 header units consume 30 root units, leaving 10 owner units"
    );
    laid.dispatch_pointer_cancel();
    let log = log.borrow();
    assert!(
        log.contains(&Observed::Pre(Vec2::new(0.0, -20.0))),
        "{log:?}"
    );
    assert!(
        log.contains(&Observed::Post(Vec2::ZERO, Vec2::new(0.0, -5.0))),
        "{log:?}"
    );
}

struct Invalid(Vec2);
impl NestedScrollConnection for Invalid {
    fn pre_scroll(&self, _: &ScrollStep) -> Vec2 {
        self.0
    }
}

fn invalid_consumption_is_clamped_before_reaching_offsets() {
    for (claimed, expected) in [
        (Vec2::new(0.0, -400.0), 0.0),
        (Vec2::new(0.0, 40.0), 40.0),
        (Vec2::new(f64::NAN, f64::INFINITY), 40.0),
    ] {
        let controller = ScrollController::new();
        let laid = lay_out(
            NestedScroll::new(Rc::new(Invalid(claimed))).child(
                Scrollable::new()
                    .controller(controller.clone())
                    .child(SizedBox::new(100.0, 600.0)),
            ),
            tight(100.0, 100.0),
        );
        laid.dispatch_pointer_down(50.0, 80.0);
        laid.dispatch_pointer_move(50.0, 40.0);
        assert_eq!(controller.pixels(), expected, "claim {claimed:?}");
        laid.dispatch_pointer_cancel();
    }
}

fn rebuilding_does_not_replace_the_active_contact() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let header: Rc<dyn NestedScrollConnection> = Rc::new(Header(log.clone()));
    let controller = ScrollController::new();
    let content = || {
        NestedScroll::new(header.clone()).child(
            Scrollable::new()
                .controller(controller.clone())
                .child(SizedBox::new(100.0, 600.0)),
        )
    };
    let mut laid = lay_out(content(), tight(100.0, 100.0));
    laid.dispatch_pointer_down(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 40.0);
    assert_eq!(controller.pixels(), 25.0);
    laid.pump_widget(content());
    laid.dispatch_pointer_move(50.0, 20.0);
    assert_eq!(
        controller.pixels(),
        30.0,
        "the same connection still takes 15 of the next 20 pixels"
    );
    laid.dispatch_pointer_cancel();
    let log = log.borrow();
    assert_eq!(
        log.iter()
            .filter(|event| matches!(event, Observed::Start(_)))
            .count(),
        1,
        "{log:?}"
    );
    assert_eq!(
        log.iter()
            .filter(|event| matches!(event, Observed::End(..)))
            .count(),
        1,
        "{log:?}"
    );
}

#[derive(Debug, Clone, PartialEq)]
enum Observed {
    Start(InputSequenceId),
    Pre(Vec2),
    Post(Vec2, Vec2),
    End(InputSequenceId, ScrollEndReason),
}

struct Header(Rc<RefCell<Vec<Observed>>>);

impl NestedScrollConnection for Header {
    fn sequence_started(&self, sequence: InputSequenceId) {
        self.0.borrow_mut().push(Observed::Start(sequence));
    }
    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        self.0.borrow_mut().push(Observed::Pre(step.available));
        Vec2::new(0.0, step.available.y.clamp(-15.0, 15.0))
    }
    fn post_scroll(&self, step: &ScrollStep, child: Vec2) -> Vec2 {
        self.0
            .borrow_mut()
            .push(Observed::Post(step.available, child));
        Vec2::ZERO
    }
    fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        self.0.borrow_mut().push(Observed::End(sequence, reason));
    }
}

fn authored_pre_and_post_wrap_real_offset_consumption() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let controller = ScrollController::new();
    let laid = lay_out(
        NestedScroll::new(Rc::new(Header(log.clone()))).child(
            Scrollable::new()
                .controller(controller.clone())
                .child(SizedBox::new(100.0, 600.0)),
        ),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_down(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 40.0);
    assert_eq!(
        controller.pixels(),
        25.0,
        "header takes 15 of the 40 pixels before the real viewport"
    );
    laid.dispatch_pointer_cancel();
    let log = log.borrow();
    let id = match log.first() {
        Some(Observed::Start(id)) => *id,
        other => panic!("missing source start: {other:?}"),
    };
    assert!(
        log.contains(&Observed::Pre(Vec2::new(0.0, -40.0))),
        "{log:?}"
    );
    assert!(
        log.contains(&Observed::Post(Vec2::ZERO, Vec2::new(0.0, -25.0))),
        "{log:?}"
    );
    assert_eq!(
        log.last(),
        Some(&Observed::End(id, ScrollEndReason::Cancelled))
    );
    assert_eq!(
        log.iter()
            .filter(|event| matches!(event, Observed::Start(_)))
            .count(),
        1
    );
    assert_eq!(
        log.iter()
            .filter(|event| matches!(event, Observed::End(..)))
            .count(),
        1
    );
}

fn orthogonal_scroll_owners_share_the_same_root_motion() {
    let horizontal = ScrollController::new();
    let vertical = ScrollController::new();
    let laid = lay_out(
        Scrollable::new()
            .controller(horizontal.clone())
            .scroll_direction(Axis::Horizontal)
            .child(
                SizedBox::new(600.0, 100.0).child(
                    Scrollable::new()
                        .controller(vertical.clone())
                        .child(SizedBox::new(600.0, 600.0)),
                ),
            ),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_down(70.0, 80.0);
    laid.dispatch_pointer_move(40.0, 40.0);
    assert_eq!(
        (horizontal.pixels(), vertical.pixels()),
        (0.0, 0.0),
        "the competitive arena resolves at slop before movement is delivered"
    );
    laid.dispatch_pointer_move(10.0, 0.0);
    assert_eq!((horizontal.pixels(), vertical.pixels()), (30.0, 40.0));
    assert!(
        horizontal.position().is_scrolling() && vertical.position().is_scrolling(),
        "both consuming owners publish activity"
    );
    laid.dispatch_pointer_cancel();
    assert!(
        !horizontal.position().is_scrolling() && !vertical.position().is_scrolling(),
        "source cancellation releases both owners"
    );
}

fn same_axis_latch_survives_extent_until_source_ends() {
    let outer = ScrollController::new();
    let inner = ScrollController::new();
    let mut laid = lay_out(
        Scrollable::new().controller(outer.clone()).child(
            SizedBox::new(100.0, 600.0).child(
                flui_widgets::Align::new(flui_painting::Alignment::TOP_LEFT).child(
                    SizedBox::new(100.0, 100.0).child(
                        Scrollable::new()
                            .controller(inner.clone())
                            .child(SizedBox::new(100.0, 600.0)),
                    ),
                ),
            ),
        ),
        tight(100.0, 100.0),
    );
    inner.jump_to(490.0);
    laid.tick();
    laid.dispatch_pointer_down(50.0, 90.0);
    laid.dispatch_pointer_move(50.0, 70.0);
    assert_eq!((outer.pixels(), inner.pixels()), (0.0, 490.0));
    laid.dispatch_pointer_move(50.0, 50.0);
    assert_eq!((outer.pixels(), inner.pixels()), (0.0, 500.0));
    laid.dispatch_pointer_move(50.0, 20.0);
    assert_eq!(
        (outer.pixels(), inner.pixels()),
        (0.0, 500.0),
        "the accepted direction stays with the leaf"
    );
    laid.dispatch_pointer_cancel();
    laid.dispatch_pointer_down(50.0, 90.0);
    laid.dispatch_pointer_move(50.0, 60.0);
    laid.dispatch_pointer_move(50.0, 30.0);
    assert_eq!(
        (outer.pixels(), inner.pixels()),
        (30.0, 500.0),
        "an edge at admission leaves the ancestor eligible"
    );
    laid.dispatch_pointer_cancel();
}
