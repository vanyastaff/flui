//! Owned pointer vocabulary contracts through the production widget pipeline.

use std::{cell::RefCell, rc::Rc};

use crate::common::{lay_out, tight};
use flui_foundation::geometry::{EdgeInsets, Offset, Point};
use flui_interaction::routing::{EventPropagation, HitTestBehavior};
use flui_platform_api::{
    EventTime,
    keyboard::Modifiers,
    pointer::{
        DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerButton, PointerButtons,
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition,
        PointerPress, PointerRelease, PointerRole, PointerSample, ScrollDelta, ScrollEvent,
        ScrollPhase, ScrollPrecision, ScrollUnit,
    },
};
use flui_view::ViewExt;
use flui_widgets::{Listener, ScrollController, Scrollable, SingleChildScrollView, SizedBox};

fn viewer(
    controller: flui_widgets::TransformationController,
    scales: Rc<RefCell<Vec<f64>>>,
) -> flui_widgets::InteractiveViewer {
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, InteractiveViewer};
    InteractiveViewer::new()
        .controller(controller)
        .boundary_margin(EdgeInsets::all(1000.0))
        .on_interaction_update(move |_, details| scales.borrow_mut().push(details.scale))
        .child(ColoredBox::new(Color::rgb(10, 20, 30)))
}

fn pan_zoom(phase: PanZoomPhase) -> PointerEvent {
    PointerEvent::PanZoom(PanZoomEvent::new(
        mouse(),
        EventTime::from_nanos(70),
        position(50.0, 50.0),
        phase,
    ))
}

fn zoom_update(scale: f64) -> PointerEvent {
    pan_zoom(PanZoomPhase::Update(
        PanZoomTransform::try_new(Offset::ZERO, scale, 0.0).expect("positive finite scale"),
    ))
}

fn scale_of(controller: &flui_widgets::TransformationController) -> f64 {
    controller.value().to_col_major_array()[0]
}

fn assert_scale(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "scale {actual}, expected {expected}"
    );
}

/// Native pan is part of the same transform stream as native scale.
pub(crate) fn viewer_native_pan_moves_the_scene_under_the_focal_point() {
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let laid = lay_out(
        viewer(controller.clone(), Rc::new(RefCell::new(Vec::new()))),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Update(
        PanZoomTransform::try_new(Offset::new(20.0, 10.0), 1.0, 0.0).expect("finite native pan"),
    )));
    let scene = controller.to_scene(Offset::new(70.0, 60.0));
    assert_scale(scene.dx, 50.0);
    assert_scale(scene.dy, 50.0);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
}

/// Start/end belong to a source session, rather than to every scale packet.
pub(crate) fn viewer_native_session_reports_one_start_and_one_terminal() {
    use flui_widgets::{GestureEndReason, InteractiveViewer};
    let log = Rc::new(RefCell::new(Vec::new()));
    let starts = log.clone();
    let updates = log.clone();
    let ends = log.clone();
    let laid = lay_out(
        InteractiveViewer::new()
            .boundary_margin(EdgeInsets::all(1000.0))
            .on_interaction_start(move |_, _| starts.borrow_mut().push("start"))
            .on_interaction_update(move |_, _| updates.borrow_mut().push("update"))
            .on_interaction_end(move |_, details| {
                ends.borrow_mut().push(match details.reason {
                    GestureEndReason::Completed => "completed",
                    GestureEndReason::Cancelled => "cancelled",
                });
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.2));
    laid.dispatch_pointer_event(&zoom_update(1.5));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.1));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Cancelled));
    assert_eq!(
        *log.borrow(),
        [
            "start",
            "update",
            "update",
            "completed",
            "start",
            "update",
            "cancelled"
        ]
    );
}

pub(crate) fn viewer_repeated_native_start_retires_the_previous_generation() {
    use flui_widgets::{GestureEndReason, InteractiveViewer, TransformationController};
    let controller = TransformationController::new();
    let reasons = Rc::new(RefCell::new(Vec::new()));
    let log = reasons.clone();
    let laid = lay_out(
        InteractiveViewer::new()
            .controller(controller.clone())
            .boundary_margin(EdgeInsets::all(1000.0))
            .on_interaction_end(move |_, details| log.borrow_mut().push(details.reason))
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.5));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    assert_eq!(
        *reasons.borrow(),
        [GestureEndReason::Cancelled],
        "repeated Start settles the old accepted session"
    );
    laid.dispatch_pointer_event(&zoom_update(1.2));
    assert_scale(scale_of(&controller), 1.8);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    assert_eq!(
        *reasons.borrow(),
        [GestureEndReason::Cancelled, GestureEndReason::Completed]
    );
}

/// A newly enabled descendant cannot steal its ancestor's accepted source.
pub(crate) fn viewer_native_owner_survives_descendant_enable_during_rebuild() {
    use flui_widgets::{InteractiveViewer, TransformationController};
    let outer = TransformationController::new();
    let inner = TransformationController::new();
    let outer_ends = Rc::new(RefCell::new(Vec::new()));
    let inner_ends = Rc::new(RefCell::new(Vec::new()));
    let tree = |inner_enabled| {
        let outer_log = outer_ends.clone();
        let inner_log = inner_ends.clone();
        InteractiveViewer::new()
            .controller(outer.clone())
            .boundary_margin(EdgeInsets::all(1000.0))
            .on_interaction_end(move |_, details| outer_log.borrow_mut().push(details.reason))
            .child(
                InteractiveViewer::new()
                    .controller(inner.clone())
                    .scale_enabled(inner_enabled)
                    .pan_enabled(false)
                    .boundary_margin(EdgeInsets::all(1000.0))
                    .on_interaction_end(move |_, details| {
                        inner_log.borrow_mut().push(details.reason);
                    })
                    .child(SizedBox::new(200.0, 200.0)),
            )
    };
    let mut laid = lay_out(tree(false), tight(200.0, 200.0));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.2));
    assert_scale(scale_of(&outer), 1.2);
    assert_scale(scale_of(&inner), 1.0);
    laid.pump_widget(tree(true));
    laid.pump();
    laid.dispatch_pointer_event(&zoom_update(1.5));
    assert_scale(scale_of(&outer), 1.5);
    assert_scale(scale_of(&inner), 1.0);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    assert_eq!(outer_ends.borrow().len(), 1);
    assert!(
        inner_ends.borrow().is_empty(),
        "unadmitted descendant has no source terminal"
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.1));
    assert_scale(scale_of(&outer), 1.5);
    assert_scale(scale_of(&inner), 1.1);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Cancelled));
    assert_eq!(
        inner_ends.borrow().len(),
        1,
        "next source can choose the now-enabled descendant"
    );
}

/// A pan that has already won can acquire a second contact without restarting.
pub(crate) fn viewer_pan_transitions_to_pinch_without_contact_count_jumps() {
    use flui_interaction::events::{
        make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_widgets::{InteractiveViewer, TransformationController};
    let controller = TransformationController::new();
    let lifecycle = Rc::new(RefCell::new(Vec::new()));
    let start_log = lifecycle.clone();
    let end_log = lifecycle.clone();
    let mut laid = lay_out(
        InteractiveViewer::new()
            .controller(controller.clone())
            .boundary_margin(EdgeInsets::all(1000.0))
            .on_interaction_start(move |_, _| start_log.borrow_mut().push("start"))
            .on_interaction_end(move |_, _| end_log.borrow_mut().push("end"))
            .child(SizedBox::new(200.0, 200.0)),
        tight(200.0, 200.0),
    );
    let first = PointerId::try_from(21_u64).expect("nonzero contact");
    let second = PointerId::try_from(22_u64).expect("nonzero contact");
    let down = |id, x| {
        make_down_event_for_id(id, Offset::new(x, 50.0), PointerKind::Touch)
            .expect("finite contact down")
    };
    let movement = |id, x| {
        make_move_event_for_id(id, Offset::new(x, 50.0), PointerKind::Touch)
            .expect("finite contact move")
    };
    let up = |id, x| {
        make_up_event_for_id(id, Offset::new(x, 50.0), PointerKind::Touch)
            .expect("finite contact up")
    };
    laid.dispatch_pointer_event(&down(first, 20.0));
    laid.dispatch_pointer_event(&movement(first, 50.0));
    laid.pump();
    let panned = controller.value();
    assert_ne!(
        panned,
        flui_foundation::geometry::Matrix4::identity(),
        "first contact really pans"
    );
    laid.dispatch_pointer_event(&down(second, 90.0));
    assert_eq!(
        controller.value(),
        panned,
        "adding a contact does not move the scene"
    );
    laid.dispatch_pointer_event(&movement(first, 30.0));
    laid.pump();
    laid.dispatch_pointer_event(&movement(second, 110.0));
    laid.pump();
    assert_scale(scale_of(&controller), 2.0);
    let pinched = controller.value();
    laid.dispatch_pointer_event(&up(first, 30.0));
    assert_eq!(
        controller.value(),
        pinched,
        "removing a contact rebases without a jump"
    );
    assert_eq!(
        *lifecycle.borrow(),
        ["start"],
        "remaining contact owns the session"
    );
    laid.dispatch_pointer_event(&up(second, 110.0));
    assert_eq!(*lifecycle.borrow(), ["start", "end"]);
}

/// Rotation is authored explicitly and keeps the scene's focal point fixed.
pub(crate) fn viewer_native_rotation_preserves_the_scene_pivot() {
    use flui_widgets::{InteractiveViewer, TransformationController};
    let controller = TransformationController::new();
    let mut laid = lay_out(
        InteractiveViewer::new()
            .controller(controller.clone())
            .rotation_enabled(true)
            .boundary_margin(EdgeInsets::all(1000.0))
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Update(
        PanZoomTransform::try_new(Offset::ZERO, 1.5, std::f64::consts::FRAC_PI_2)
            .expect("finite scale and quarter turn"),
    )));
    let pivot = controller.to_scene(Offset::new(50.0, 50.0));
    assert_scale(pivot.dx, 50.0);
    assert_scale(pivot.dy, 50.0);
    let transformed = controller.value().transform_point(60.0, 50.0);
    assert_scale(transformed.0, 50.0);
    assert_scale(transformed.1, 65.0);
    let rotated = controller.value();
    laid.pump_widget(
        InteractiveViewer::new()
            .controller(controller.clone())
            .rotation_enabled(true)
            .boundary_margin(EdgeInsets::all(1000.0))
            .child(SizedBox::new(100.0, 100.0)),
    );
    laid.pump();
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Update(
        PanZoomTransform::try_new(Offset::ZERO, 1.5, std::f64::consts::FRAC_PI_2)
            .expect("repeated cumulative transform"),
    )));
    assert_eq!(
        controller.value(),
        rotated,
        "rebuild preserves cumulative rotation history"
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
}

pub(crate) fn viewer_rotation_refuses_an_unfittable_quad_then_recovers() {
    use flui_widgets::{InteractiveViewer, TransformationController};
    let controller = TransformationController::new();
    let updates = Rc::new(RefCell::new(Vec::new()));
    let log = updates.clone();
    let laid = lay_out(
        InteractiveViewer::new()
            .controller(controller.clone())
            .rotation_enabled(true)
            .on_interaction_update(move |_, details| log.borrow_mut().push(details.scale))
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    let rotated = |scale| {
        pan_zoom(PanZoomPhase::Update(
            PanZoomTransform::try_new(Offset::ZERO, scale, std::f64::consts::FRAC_PI_4)
                .expect("finite requested rotation"),
        ))
    };
    laid.dispatch_pointer_event(&rotated(1.0));
    assert_eq!(
        controller.value(),
        flui_foundation::geometry::Matrix4::identity(),
        "a rotated viewport cannot fit without extra zoom"
    );
    assert!(
        updates.borrow().is_empty(),
        "unadmitted native input has no recognized update"
    );
    laid.dispatch_pointer_event(&rotated(1.5));
    let matrix = controller.value();
    let m = matrix.to_col_major_array();
    assert_scale(m[0].hypot(m[1]), 1.5);
    assert!(m[1] > 1.0, "the recoverable input actually rotates");
    for point in [
        Offset::new(0.0, 0.0),
        Offset::new(100.0, 0.0),
        Offset::new(0.0, 100.0),
        Offset::new(100.0, 100.0),
    ] {
        let scene = controller.to_scene(point);
        assert!(scene.dx >= -1e-9 && scene.dx <= 100.0 + 1e-9);
        assert!(scene.dy >= -1e-9 && scene.dy <= 100.0 + 1e-9);
    }
    let updates = updates.borrow();
    assert_eq!(updates.len(), 1);
    assert_scale(updates[0], 1.5);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
}

/// Large finite motion must clamp to the edge, not cancel away to the origin.
pub(crate) fn viewer_extreme_finite_pan_preserves_the_boundary_result() {
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let laid = lay_out(
        viewer(controller.clone(), Rc::new(RefCell::new(Vec::new()))),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    let movement = |pan| {
        pan_zoom(PanZoomPhase::Update(
            PanZoomTransform::try_new(Offset::new(pan, 0.0), 1.0, 0.0)
                .expect("finite requested pan"),
        ))
    };
    laid.dispatch_pointer_event(&movement(f64::MAX));
    let shifted_origin = controller.value().transform_point(0.0, 0.0);
    assert_scale(shifted_origin.0, 1000.0);
    assert_scale(shifted_origin.1, 0.0);
    assert!(
        controller
            .value()
            .to_col_major_array()
            .iter()
            .all(|value| value.is_finite())
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&movement(-20.0));
    assert_scale(controller.value().transform_point(0.0, 0.0).0, 980.0);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
}

/// Focal inertia retains the admitted profile while raw reporting stays intact.
pub(crate) fn viewer_touch_focal_inertia_uses_the_admitted_profile() {
    viewer_focal_inertia_uses_profile(false);
}

pub(crate) fn viewer_native_focal_inertia_uses_the_admitted_profile() {
    viewer_focal_inertia_uses_profile(true);
}

fn viewer_focal_inertia_uses_profile(native: bool) {
    use flui_animation::Vsync;
    use flui_widgets::{InteractiveViewer, TransformationController, VsyncScope};
    use std::time::Duration;
    let profile = |min, max| {
        flui_interaction::GestureSettings::default()
            .try_with_fling_velocity(min, max)
            .expect("valid focal fling range")
    };
    let settings = flui_interaction::settings::GestureSettingsSource::new(profile(5000.0, 5000.0));
    let controller = TransformationController::new();
    let ends = Rc::new(RefCell::new(Vec::new()));
    let recorder = Rc::clone(&ends);
    let vsync = Vsync::new();
    let mut laid = lay_out(
        crate::gesture_settings::SettingsScope::new(
            settings.provider(),
            VsyncScope::new(
                vsync.clone(),
                InteractiveViewer::new()
                    .controller(controller.clone())
                    .boundary_margin(EdgeInsets::all(1000.0))
                    .on_interaction_end(move |_, details| recorder.borrow_mut().push(details))
                    .child(SizedBox::new(200.0, 200.0)),
            ),
        ),
        tight(200.0, 200.0),
    );
    laid.adopt_vsync(vsync);
    for attempt in 0..3 {
        let info = PointerInfo::new(
            PointerId::try_from(100_u64 + attempt).expect("contact identity"),
            if native {
                PointerKind::Mouse
            } else {
                PointerKind::Touch
            },
        );
        let held = PointerButtons::NONE.with(PointerButton::PRIMARY);
        let base = attempt * 1000;
        let sample = |millis, x| {
            PointerSample::new(
                EventTime::from_nanos((base + millis) * 1_000_000),
                position(x, 100.0),
            )
        };
        let native_packet = |millis, phase| {
            PointerEvent::PanZoom(PanZoomEvent::new(
                info,
                EventTime::from_nanos((base + millis) * 1_000_000),
                position(50.0, 100.0),
                phase,
            ))
        };
        let start = if native {
            native_packet(0, PanZoomPhase::Start)
        } else {
            PointerEvent::Down(PointerPress::new(
                info,
                PointerButton::PRIMARY,
                held,
                sample(0, 20.0),
            ))
        };
        laid.dispatch_pointer_event(&start);
        for (millis, distance) in [(10, 20.0), (20, 40.0), (30, 60.0), (40, 80.0)] {
            let movement = if native {
                native_packet(
                    millis,
                    PanZoomPhase::Update(
                        PanZoomTransform::try_new(Offset::new(distance, 0.0), 1.0, 0.0)
                            .expect("finite focal pan"),
                    ),
                )
            } else {
                PointerEvent::Move(PointerMove::new(
                    info,
                    held,
                    sample(millis, 20.0 + distance),
                ))
            };
            laid.dispatch_pointer_event(&movement);
            laid.pump_for(Duration::from_millis(10));
        }
        if attempt == 0 {
            settings.replace(profile(50.0, 100.0));
        }
        if attempt == 1 {
            settings.replace(profile(50.0, 600.0));
        }
        let end = if native {
            native_packet(41, PanZoomPhase::End)
        } else {
            PointerEvent::Up(PointerRelease::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample(41, 100.0),
            ))
        };
        laid.dispatch_pointer_event(&end);
        let before = controller.value().transform_point(0.0, 0.0).0;
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        let coast = controller.value().transform_point(0.0, 0.0).0 - before;
        let log = ends.borrow();
        assert_eq!(
            log.len(),
            usize::try_from(attempt + 1).expect("three sessions"),
            "each session reports raw terminal measurement"
        );
        assert!(
            log.last()
                .expect("terminal callback")
                .velocity
                .pixels_per_second
                .dx
                > 1000.0,
            "raw focal measurement remains available when inertia is filtered"
        );
        assert_eq!(
            log.last().expect("terminal callback").scale_velocity,
            0.0,
            "pure pan has no dimensionless scale velocity"
        );
        match attempt {
            0 => assert_eq!(
                coast, 0.0,
                "native={native}: first admitted minimum blocks inertia despite source restoration before terminal"
            ),
            1 => assert!(
                coast > 0.0 && coast < 4.0,
                "native={native}: captured max100 caps focal inertia despite source600, coast {coast}"
            ),
            _ => assert!(
                coast > 4.0 && coast < 20.0,
                "native={native}: new max600 admission recovers, coast {coast}"
            ),
        }
    }
}

/// The real presentation ticks the release velocity and new input retires it.
pub(crate) fn viewer_focal_fling_advances_then_stops_on_new_input() {
    use flui_animation::Vsync;
    use flui_widgets::{InteractiveViewer, TransformationController, VsyncScope};
    use std::time::Duration;
    let controller = TransformationController::new();
    let released = Rc::new(RefCell::new(Vec::new()));
    let ends = released.clone();
    let vsync = Vsync::new();
    let mut laid = lay_out(
        VsyncScope::new(
            vsync.clone(),
            InteractiveViewer::new()
                .controller(controller.clone())
                .boundary_margin(EdgeInsets::all(1000.0))
                .on_interaction_end(move |_, details| ends.borrow_mut().push(details.velocity))
                .child(SizedBox::new(200.0, 200.0)),
        ),
        tight(200.0, 200.0),
    );
    laid.adopt_vsync(vsync);
    let source = PointerInfo::new(
        PointerId::try_from(81_u64).expect("contact"),
        PointerKind::Touch,
    );
    let sample = |millis: u64, x| {
        PointerSample::new(
            EventTime::from_nanos(millis * 1_000_000),
            position(x, 100.0),
        )
    };
    let down = |millis, x| {
        PointerEvent::Down(PointerPress::new(
            source,
            PointerButton::PRIMARY,
            PointerButtons::NONE.with(PointerButton::PRIMARY),
            sample(millis, x),
        ))
    };
    let movement = |millis, x| {
        PointerEvent::Move(PointerMove::new(
            source,
            PointerButtons::NONE.with(PointerButton::PRIMARY),
            sample(millis, x),
        ))
    };
    laid.dispatch_pointer_event(&down(0, 20.0));
    for (millis, x) in [(10, 50.0), (20, 80.0), (30, 110.0)] {
        laid.dispatch_pointer_event(&movement(millis, x));
        laid.pump_for(Duration::from_millis(10));
    }
    laid.dispatch_pointer_event(&PointerEvent::Up(PointerRelease::new(
        source,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(31, 110.0),
    )));
    assert!(
        released
            .borrow()
            .last()
            .expect("release callback")
            .pixels_per_second
            .dx
            > 1000.0
    );
    let before = controller.value().transform_point(0.0, 0.0).0;
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    let after = controller.value().transform_point(0.0, 0.0).0;
    assert!(
        after > before,
        "measured focal velocity continues through presentation ticks"
    );
    laid.dispatch_pointer_event(&down(70, 80.0));
    let stopped = controller.value();
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(
        controller.value(),
        stopped,
        "new contact stops the previous focal fling"
    );
    viewer_focal_fling_without_clock_preserves_the_scene();
}

fn viewer_focal_fling_without_clock_preserves_the_scene() {
    use flui_animation::Vsync;
    use flui_widgets::{InteractiveViewer, TransformationController, VsyncScope};
    use std::time::Duration;

    for initially_bound in [false, true] {
        let controller = TransformationController::new();
        let released = Rc::new(RefCell::new(Vec::new()));
        let vsync = Vsync::new();
        let tree = |bound| {
            let ends = released.clone();
            let viewer = InteractiveViewer::new()
                .controller(controller.clone())
                .boundary_margin(EdgeInsets::all(1000.0))
                .on_interaction_end(move |_, details| ends.borrow_mut().push(details.velocity))
                .child(SizedBox::new(200.0, 200.0));
            if bound {
                VsyncScope::new(vsync.clone(), viewer)
            } else {
                VsyncScope::detached(viewer)
            }
        };
        let mut laid = lay_out(tree(initially_bound), tight(200.0, 200.0));
        laid.adopt_vsync(vsync.clone());
        let post_frame = laid.post_frame_handle();
        let packet = |millis: u64, phase| {
            PointerEvent::PanZoom(PanZoomEvent::new(
                mouse(),
                EventTime::from_nanos(millis * 1_000_000),
                position(50.0, 100.0),
                phase,
            ))
        };
        let release = |laid: &mut crate::common::LaidOut, base: u64, bound: bool| {
            laid.dispatch_pointer_event(&packet(base, PanZoomPhase::Start));
            for (elapsed, pan) in [(10, 20.0), (20, 40.0), (30, 60.0)] {
                laid.dispatch_pointer_event(&packet(
                    base + elapsed,
                    PanZoomPhase::Update(
                        PanZoomTransform::try_new(Offset::new(pan, 0.0), 1.0, 0.0)
                            .expect("finite native release"),
                    ),
                ));
            }
            let before_end = controller.value();
            laid.dispatch_pointer_event(&packet(base + 31, PanZoomPhase::End));
            if !bound {
                for _ in 0..4 {
                    laid.pump_for(Duration::from_millis(16));
                    assert_eq!(
                        post_frame.pending_len(),
                        0,
                        "a detached release cannot perpetually queue geometry validation"
                    );
                }
                assert_eq!(
                    controller.value(),
                    before_end,
                    "a missing clock cannot move the scene synchronously at release"
                );
            }
            assert!(
                released
                    .borrow()
                    .last()
                    .expect("release callback")
                    .pixels_per_second
                    .dx
                    > 1000.0,
                "a missing clock does not erase the measured release"
            );
        };
        if initially_bound {
            release(&mut laid, 0, true);
            laid.pump_for(Duration::from_millis(16));
            let moving = controller.value();
            laid.pump_widget(tree(false));
            laid.pump_for(Duration::from_millis(16));
            assert_eq!(
                controller.value(),
                moving,
                "clock removal cancels the old run without settlement motion"
            );
        }
        release(&mut laid, 100, false);
        let settled = controller.value();
        for _ in 0..4 {
            laid.pump_for(Duration::from_millis(16));
            assert_eq!(
                controller.value(),
                settled,
                "a detached release preserves the scene without ongoing motion"
            );
            assert_eq!(
                post_frame.pending_len(),
                0,
                "a detached release cannot perpetually queue geometry validation"
            );
        }
        laid.pump_widget(tree(true));
        release(&mut laid, 200, true);
        let admitted = controller.value().transform_point(0.0, 0.0).0;
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        assert!(
            controller.value().transform_point(0.0, 0.0).0 > admitted,
            "a fresh bound release recovers focal inertia"
        );
    }
}

/// A fling keeps immutable limits until real layout or authored bounds change.
pub(crate) fn viewer_focal_fling_rebuild_preserves_or_retires_geometry() {
    use flui_animation::Vsync;
    use flui_widgets::{InteractiveViewer, TransformationController, VsyncScope};
    use std::time::Duration;
    for change_viewport in [false, true] {
        let controller = TransformationController::new();
        let vsync = Vsync::new();
        let tree = |width, margin| {
            VsyncScope::new(
                vsync.clone(),
                SizedBox::new(width, 200.0).child(
                    InteractiveViewer::new()
                        .controller(controller.clone())
                        .boundary_margin(EdgeInsets::all(margin))
                        .child(SizedBox::new(200.0, 200.0)),
                ),
            )
        };
        let mut laid = lay_out(tree(200.0, 1000.0), crate::common::loose(300.0));
        laid.adopt_vsync(vsync.clone());
        let packet = |millis: u64, phase| {
            PointerEvent::PanZoom(PanZoomEvent::new(
                mouse(),
                EventTime::from_nanos(millis * 1_000_000),
                position(50.0, 100.0),
                phase,
            ))
        };
        laid.dispatch_pointer_event(&packet(0, PanZoomPhase::Start));
        for (millis, pan) in [(10, 20.0), (20, 40.0), (30, 60.0)] {
            laid.dispatch_pointer_event(&packet(
                millis,
                PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::new(pan, 0.0), 1.0, 0.0)
                        .expect("finite native pan"),
                ),
            ));
        }
        laid.dispatch_pointer_event(&packet(31, PanZoomPhase::End));
        laid.pump_for(Duration::from_millis(16));
        let before_rebuild = controller.value().transform_point(0.0, 0.0).0;
        laid.pump_widget(tree(200.0, 1000.0));
        laid.pump_for(Duration::from_millis(16));
        assert!(
            controller.value().transform_point(0.0, 0.0).0 > before_rebuild,
            "an unchanged rebuild preserves the admitted fling"
        );
        let repeated = controller.value();
        laid.pump();
        assert_eq!(
            controller.value(),
            repeated,
            "a repeated frame has no elapsed motion"
        );
        laid.pump_for(Duration::from_millis(16));
        assert!(
            controller.value().transform_point(0.0, 0.0).0 > repeated.transform_point(0.0, 0.0).0,
            "a repeated frame does not retire an unchanged fling"
        );
        laid.pump_widget(if change_viewport {
            tree(100.0, 1000.0)
        } else {
            tree(200.0, 500.0)
        });
        laid.pump();
        let stopped = controller.value();
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        assert_eq!(
            controller.value(),
            stopped,
            "changing {} retires the immutable fling limits",
            if change_viewport {
                "the viewport"
            } else {
                "the boundary"
            }
        );
        laid.dispatch_pointer_event(&packet(100, PanZoomPhase::Start));
        for (millis, pan) in [(110, 20.0), (120, 40.0), (130, 60.0)] {
            laid.dispatch_pointer_event(&packet(
                millis,
                PanZoomPhase::Update(
                    PanZoomTransform::try_new(Offset::new(pan, 0.0), 1.0, 0.0)
                        .expect("finite recovery pan"),
                ),
            ));
        }
        laid.dispatch_pointer_event(&packet(131, PanZoomPhase::End));
        let recovered = controller.value().transform_point(0.0, 0.0).0;
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        assert!(
            controller.value().transform_point(0.0, 0.0).0 > recovered,
            "fresh release uses the replacement geometry"
        );
        laid.pump_for(Duration::from_secs(20));
        let rested = controller.value();
        assert!(
            !vsync.has_running(),
            "settled motion no longer requests Vsync"
        );
        laid.pump_for(Duration::from_millis(16));
        assert_eq!(
            controller.value(),
            rested,
            "settled continuation stays retired"
        );
    }
}

/// Native cumulative scale and focal motion have different release units.
pub(crate) fn viewer_reports_scale_velocity_separately_from_focal_velocity() {
    use flui_widgets::InteractiveViewer;
    let ends = Rc::new(RefCell::new(Vec::new()));
    let log = ends.clone();
    let laid = lay_out(
        InteractiveViewer::new()
            .boundary_margin(EdgeInsets::all(1000.0))
            .on_interaction_end(move |_, details| log.borrow_mut().push(details))
            .child(SizedBox::new(200.0, 200.0)),
        tight(200.0, 200.0),
    );
    let packet = |millis: u64, phase| {
        PointerEvent::PanZoom(PanZoomEvent::new(
            mouse(),
            EventTime::from_nanos(millis * 1_000_000),
            position(50.0, 50.0),
            phase,
        ))
    };
    laid.dispatch_pointer_event(&packet(0, PanZoomPhase::Start));
    for (millis, pan, scale) in [(10, 20.0, 1.2), (20, 40.0, 1.5), (30, 60.0, 1.9)] {
        laid.dispatch_pointer_event(&packet(
            millis,
            PanZoomPhase::Update(
                PanZoomTransform::try_new(Offset::new(pan, 0.0), scale, 0.0)
                    .expect("finite authored native history"),
            ),
        ));
    }
    laid.dispatch_pointer_event(&packet(31, PanZoomPhase::End));
    let observed = ends.borrow();
    assert_eq!(observed.len(), 1, "one source release");
    assert!(
        observed[0].scale_velocity > 5.0,
        "scale units per second remain observable"
    );
    assert!(
        observed[0].velocity.pixels_per_second.dx > 1000.0,
        "pan carries measured logical pixels per second"
    );
    assert!(observed[0].velocity.pixels_per_second.dy.abs() < 1e-9);
}

fn mouse() -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(7_u64).expect("valid pointer"),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(11_u64).expect("valid device"))
    .with_role(PointerRole::Primary)
}

fn position(x: f64, y: f64) -> PointerPosition {
    PointerPosition::try_new(Point::new(x, y)).expect("finite position")
}

pub(crate) fn scroll_claim_preserves_owned_source_units_and_phase() {
    let observed = Rc::new(RefCell::new(Vec::<ScrollEvent>::new()));
    let sink = observed.clone();
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_scroll_claim(move |event| {
                // A claim receives the platform vocabulary itself, not a second
                // structure that silently discards device identity or units.
                let event: &ScrollEvent = event;
                sink.borrow_mut().push(*event);
                EventPropagation::Stop
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    for unit in [ScrollUnit::Pixels, ScrollUnit::Lines, ScrollUnit::Pages] {
        let event = ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(42),
            position(20.0, 30.0),
            ScrollDelta::try_new(unit, 1.25, -0.5).expect("finite delta"),
        )
        .with_precision(ScrollPrecision::Precise)
        .with_phase(ScrollPhase::MomentumChanged)
        .with_modifiers(Modifiers::SHIFT);
        laid.dispatch_pointer_event(&PointerEvent::Scroll(event));
        assert_eq!(observed.borrow().last(), Some(&event), "{unit:?}");
    }
    let terminal = ScrollEvent::new(
        mouse(),
        EventTime::from_nanos(43),
        position(20.0, 30.0),
        ScrollDelta::zero(ScrollUnit::Pages),
    )
    .with_phase(ScrollPhase::MomentumEnded);
    laid.dispatch_pointer_event(&PointerEvent::Scroll(terminal));
    assert_eq!(
        observed.borrow().last(),
        Some(&terminal),
        "zero delta still carries phase"
    );
}

pub(crate) fn pointer_delivery_preserves_source_and_sample_families() {
    use flui_foundation::geometry::Size;
    use flui_platform_api::pointer::{ContactSize, PenOrientation, PenTool, Pressure, Twist};

    let observed = Rc::new(RefCell::new(Vec::<PointerMove>::new()));
    let sink = observed.clone();
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(move |_, dispatch| {
                if let PointerEvent::Move(event) = dispatch.local {
                    sink.borrow_mut().push(event.clone());
                }
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    let pointer = PointerInfo::new(
        PointerId::try_from(3_u64).expect("authored contact"),
        PointerKind::Pen {
            tool: PenTool::Eraser,
        },
    )
    .with_device(DeviceId::try_from(13_u64).expect("authored hardware"))
    .with_role(PointerRole::Additional);
    let sample = |time, x, pressure| {
        PointerSample::new(EventTime::from_nanos(time), position(x, 30.0))
            .with_pressure(Pressure::try_new(pressure).expect("authored pressure"))
            .with_contact_size(
                ContactSize::try_new(Size::new(x / 10.0, 2.0)).expect("finite contact"),
            )
            .with_orientation(PenOrientation::try_altitude(0.5).expect("authored altitude"))
            .with_twist(Twist::try_new(x / 100.0).expect("finite twist"))
    };
    let down = PointerPress::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(10, 10.0, 0.1),
    );
    laid.dispatch_pointer_event(&PointerEvent::Down(down));
    let held = PointerButtons::NONE.with(PointerButton::PRIMARY);
    let event = PointerMove::new(pointer, held, sample(30, 30.0, 0.25))
        .with_coalesced(vec![sample(0, 20.0, 0.0)])
        .with_predicted(vec![sample(40, 40.0, 1.0)])
        .with_modifiers(Modifiers::CONTROL);
    laid.dispatch_pointer_event(&PointerEvent::Move(event.clone()));
    // Terminal dispatch drains pending Move through GestureBinding.
    let up = PointerRelease::new(pointer, PointerButton::PRIMARY, held, sample(50, 50.0, 0.0));
    laid.dispatch_pointer_event(&PointerEvent::Up(up));
    assert_eq!(observed.borrow().as_slice(), &[event]);
}

pub(crate) fn page_scroll_resolves_against_the_actual_viewport() {
    for height in [100.0, 275.0] {
        let controller = ScrollController::new();
        let laid = lay_out(
            Scrollable::new()
                .controller(controller.clone())
                .viewport_builder(Rc::new(|position| {
                    SingleChildScrollView::new()
                        .position(position)
                        .child(SizedBox::new(100.0, 1000.0))
                        .boxed()
                })),
            tight(100.0, height),
        );
        assert_eq!(
            controller.position().viewport_dimension(),
            height,
            "real viewport producer"
        );
        let scroll = ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(60),
            position(50.0, height / 2.0),
            ScrollDelta::try_new(ScrollUnit::Pages, 0.0, 0.5).expect("finite pages"),
        );
        laid.dispatch_pointer_event(&PointerEvent::Scroll(scroll));
        assert_eq!(
            controller.pixels(),
            height * 0.5,
            "half page in {height}px viewport"
        );
    }
    // Counts belong to the scrollable: a geometric transform must not divide
    // line/page counts before this producer applies its line height/viewport.
    // The projective plane maps local (10,10) to screen (200/19,200/19).
    use flui_foundation::geometry::Matrix4;
    use flui_widgets::Transform;
    let perspective = Matrix4::from([
        1.0, 0.0, 0.0, -0.005, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    for (transform, focal) in [
        (Matrix4::scaling(2.0, 2.0, 1.0), position(20.0, 20.0)),
        (perspective, position(200.0 / 19.0, 200.0 / 19.0)),
    ] {
        for (unit, expected) in [(ScrollUnit::Lines, 26.5), (ScrollUnit::Pages, 137.5)] {
            let controller = ScrollController::new();
            let laid = lay_out(
                Transform::new(transform).child(
                    Scrollable::new()
                        .controller(controller.clone())
                        .viewport_builder(Rc::new(|position| {
                            SingleChildScrollView::new()
                                .position(position)
                                .child(SizedBox::new(100.0, 1000.0))
                                .boxed()
                        })),
                ),
                tight(100.0, 275.0),
            );
            assert_eq!(controller.position().viewport_dimension(), 275.0);
            let scroll = ScrollEvent::new(
                mouse(),
                EventTime::from_nanos(61),
                focal,
                ScrollDelta::try_new(unit, 0.0, 0.5).expect("finite counts"),
            )
            .with_precision(ScrollPrecision::Precise);
            laid.dispatch_pointer_event(&PointerEvent::Scroll(scroll));
            assert!(
                (controller.pixels() - expected).abs() < 1e-9,
                "transformed {unit:?} resolves in actual scrollable: {} != {expected}",
                controller.pixels()
            );
        }
    }
}

#[derive(Clone, flui_view::prelude::StatelessView)]
struct ZoomWheelPreferences {
    child: flui_view::BoxedView,
    count: u32,
}

impl flui_view::StatelessView for ZoomWheelPreferences {
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        flui_widgets::GestureArenaScope::new(
            flui_widgets::GestureArenaScope::of(ctx),
            self.child.clone(),
        )
        .wheel_preferences(
            flui_platform_api::WheelPreferences::default()
                .with_vertical(flui_platform_api::WheelStep::Lines(self.count)),
        )
    }
}

pub(crate) fn viewer_raw_detents_zoom_without_stealing_plain_scrolls() {
    use flui_widgets::{InteractiveViewer, TransformationController, WheelScaleGate};

    // Raw Win32 rotation retains the authored zoom step. Normalized lines are
    // a control, and fractions remain fractions under the authored divisor.
    for unit in [ScrollUnit::Lines, ScrollUnit::Detents] {
        for (factor, count) in [(100.0, 0), (200.0, 3)] {
            let transform = TransformationController::new();
            let scroll = ScrollController::new();
            let scales = Rc::new(RefCell::new(Vec::new()));
            let updates = scales.clone();
            let content = Scrollable::new().controller(scroll.clone()).child(
                SizedBox::new(300.0, 1000.0).child(
                    InteractiveViewer::new()
                        .controller(transform.clone())
                        .wheel_scale_gate(WheelScaleGate::CtrlWheel)
                        .scale_factor(factor)
                        .boundary_margin(EdgeInsets::all(1000.0))
                        .on_interaction_update(move |_, details| {
                            updates.borrow_mut().push(details.scale);
                        })
                        .child(SizedBox::new(300.0, 1000.0)),
                ),
            );
            let mut laid = lay_out(
                ZoomWheelPreferences {
                    child: content.boxed(),
                    count,
                },
                tight(300.0, 300.0),
            );
            let packet = |dy, modifiers| {
                PointerEvent::Scroll(
                    ScrollEvent::new(
                        mouse(),
                        EventTime::from_nanos(60),
                        position(100.0, 100.0),
                        ScrollDelta::try_new(unit, 0.0, dy).expect("finite wheel delta"),
                    )
                    .with_modifiers(modifiers),
                )
            };
            laid.dispatch_pointer_event(&packet(-1.0, Modifiers::CONTROL));
            let first = (53.0_f64 / factor).exp();
            assert_scale(scale_of(&transform), first);
            assert_eq!(scroll.pixels(), 0.0, "zoom claims {unit:?}");
            assert_scale(scales.borrow()[0], first);

            laid.dispatch_pointer_event(&packet(-0.25, Modifiers::CONTROL));
            let second = (66.25_f64 / factor).exp();
            assert_scale(scale_of(&transform), second);
            assert_eq!(scroll.pixels(), 0.0, "fractional zoom stays claimed");
            assert_scale(scales.borrow()[1], (13.25_f64 / factor).exp());

            // A phase-less burst retains its claimant until owner-clock
            // inactivity. The next chord is a fresh burst on the same source.
            laid.pump_for(std::time::Duration::from_millis(500));
            laid.dispatch_pointer_event(&packet(0.25, Modifiers::NONE));
            let plain_distance = if unit == ScrollUnit::Detents {
                13.25 * f64::from(count)
            } else {
                13.25
            };
            assert_eq!(
                scroll.pixels(),
                plain_distance,
                "plain tick follows outer scroll policy"
            );
            assert_scale(scale_of(&transform), second);
            assert_eq!(scales.borrow().len(), 2);

            laid.pump_for(std::time::Duration::from_millis(500));
            laid.dispatch_pointer_event(&packet(f64::MAX, Modifiers::CONTROL));
            assert_eq!(
                scroll.pixels(),
                plain_distance,
                "invalid zoom cannot scroll"
            );
            assert_scale(scale_of(&transform), second);
            laid.dispatch_pointer_event(&packet(-0.25, Modifiers::CONTROL));
            assert_scale(scale_of(&transform), (79.5_f64 / factor).exp());
            assert_eq!(
                scroll.pixels(),
                plain_distance,
                "healthy zoom recovers claim"
            );
            assert_eq!(scales.borrow().len(), 3);
        }
    }
}

pub(crate) fn viewer_page_zoom_resolves_against_the_actual_viewport() {
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, InteractiveViewer, TransformationController};

    for height in [100.0, 275.0] {
        let controller = TransformationController::new();
        let laid = lay_out(
            InteractiveViewer::new()
                .controller(controller.clone())
                .child(ColoredBox::new(Color::rgb(10, 20, 30))),
            tight(100.0, height),
        );
        let scroll = ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(60),
            position(50.0, height / 2.0),
            ScrollDelta::try_new(ScrollUnit::Pages, 0.0, -0.5).expect("finite pages"),
        );
        laid.dispatch_pointer_event(&PointerEvent::Scroll(scroll));
        let scale = controller.value().to_col_major_array()[0];
        let expected = (height * 0.5 / 200.0_f64).exp();
        assert!(
            (scale - expected).abs() < 1e-12,
            "half page in {height}px viewer"
        );
    }
}

pub(crate) fn viewer_cumulative_zoom_survives_rebuild_and_resets() {
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let scales = Rc::new(RefCell::new(Vec::new()));
    let mut laid = lay_out(
        viewer(controller.clone(), scales.clone()),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.2));
    assert_scale(scale_of(&controller), 1.2);
    laid.pump_widget(viewer(controller.clone(), scales.clone()));
    laid.pump();
    for cumulative in [1.5, 1.5] {
        laid.dispatch_pointer_event(&zoom_update(cumulative));
        assert_scale(scale_of(&controller), 1.5);
    }
    let observed = scales.borrow();
    assert_eq!(observed.len(), 3);
    for (actual, expected) in observed.iter().zip([1.2, 1.25, 1.0]) {
        assert_scale(*actual, expected);
    }
    drop(observed);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.2));
    assert_scale(scale_of(&controller), 1.8);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Cancelled));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.1));
    assert_scale(scale_of(&controller), 1.98);
}

pub(crate) fn viewer_unstarted_pinch_updates_remain_independent_steps() {
    use flui_interaction::events::make_pinch_gesture_event;
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let scales = Rc::new(RefCell::new(Vec::new()));
    let laid = lay_out(
        viewer(controller.clone(), scales.clone()),
        tight(100.0, 100.0),
    );
    // The legacy backend bridge has no Start/End to send. Its two 10% ticks
    // are independent steps, while a started stream carries cumulative values.
    for _ in 0..2 {
        laid.dispatch_pointer_event(
            &make_pinch_gesture_event(Offset::new(50.0, 50.0), 0.1)
                .expect("finite synthetic legacy pinch"),
        );
    }
    assert_scale(scale_of(&controller), 1.21);
    assert_eq!(scales.borrow().len(), 2);
    for step in scales.borrow().iter() {
        assert_scale(*step, 1.1);
    }
}

pub(crate) fn viewer_extreme_zoom_reports_the_finite_applied_change() {
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let scales = Rc::new(RefCell::new(Vec::new()));
    let laid = lay_out(
        viewer(controller.clone(), scales.clone()),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    for (cumulative, expected) in [
        (f64::from_bits(1), 0.8),
        (1e300, 2.5),
        (f64::from_bits(1), 0.8),
    ] {
        let before = scale_of(&controller);
        laid.dispatch_pointer_event(&zoom_update(cumulative));
        assert_scale(scale_of(&controller), expected);
        assert!(
            controller
                .value()
                .to_col_major_array()
                .iter()
                .all(|value| value.is_finite())
        );
        let applied = *scales.borrow().last().expect("update callback");
        assert!(applied.is_finite() && applied > 0.0);
        // The documented callback reports the applied change, including clamps.
        // A saturated unrepresentable raw ratio would be an invented result.
        assert_scale(applied, expected / before);
    }
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.25));
    assert_scale(scale_of(&controller), 1.0);
}

pub(crate) fn viewer_page_overflow_and_empty_viewport_recover() {
    use flui_widgets::TransformationController;
    for height in [0.0, 100.0] {
        let controller = TransformationController::new();
        let scales = Rc::new(RefCell::new(Vec::new()));
        let mut laid = lay_out(
            SizedBox::new(100.0, height).child(viewer(controller.clone(), scales.clone())),
            crate::common::loose(100.0),
        );
        let scroll = |delta| {
            PointerEvent::Scroll(ScrollEvent::new(
                mouse(),
                EventTime::from_nanos(80),
                position(50.0, 0.0),
                ScrollDelta::try_new(ScrollUnit::Pages, 0.0, delta).expect("finite pages"),
            ))
        };
        laid.dispatch_pointer_event(&scroll(-f64::MAX));
        assert_scale(scale_of(&controller), 1.0);
        assert!(
            scales.borrow().is_empty(),
            "no interaction for empty/overflowing page geometry"
        );
        // Reconcile the same viewer under a non-empty actual viewport before
        // the next event, retaining its controller and owner-local state.
        laid.pump_widget(
            SizedBox::new(100.0, 100.0).child(viewer(controller.clone(), scales.clone())),
        );
        laid.pump();
        laid.dispatch_pointer_event(&PointerEvent::Scroll(ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(81),
            position(50.0, 50.0),
            ScrollDelta::try_new(ScrollUnit::Pages, 0.0, -0.5).expect("finite pages"),
        )));
        assert_scale(scale_of(&controller), (50.0 / 200.0_f64).exp());
        assert_eq!(scales.borrow().len(), 1);
    }
}
