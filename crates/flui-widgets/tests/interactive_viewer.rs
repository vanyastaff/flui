//! `InteractiveViewer` as a leaf of the nested scroll channel (ADR-0209)
//! and its paths besides the gesture: wheel and keys, assistive actions, the
//! double tap, degenerate geometry and a rebuild from its own callback.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::{EdgeInsets, Matrix4, Offset, Point};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::pointer::{
    DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerEvent, PointerId, PointerInfo,
    PointerKind, PointerPosition, PointerRole, ScrollDelta, ScrollEvent, ScrollUnit,
};
use flui_platform::{EventTime, keyboard::Modifiers};
use flui_testing::a11y::{Action, ActionData, ActionRequest, TreeId};
use flui_testing::{MouseScript, PointerScript, TouchScript};
use flui_view::prelude::*;
use flui_view::{BoxedView, Signal, SignalWriteExt, View, ViewExt};
use flui_widgets::{
    ColoredBox, Directionality, InteractiveViewer, NestedScroll, PageController, PageView,
    ScrollCause, ScrollController, ScrollEndReason, Scrollable, SizedBox, Transform,
    TransformationController, VsyncScope, WheelScaleGate,
};

use crate::common::{LaidOut, lay_out, tight, viewer_alternatives};
use crate::nested_scroll::{
    DEVICES, Device, Probe, Seen, at, drag, line, mounted, replay_contained, settle, slow,
};

const PAGE: f64 = 300.0;

fn photo(controller: &TransformationController) -> InteractiveViewer {
    InteractiveViewer::new(viewer_alternatives())
        .controller(controller.clone())
        .double_tap_zoom(false)
        .max_scale(4.0)
        .child(ColoredBox::new(Color::rgb(20, 40, 60)))
}

/// A three-page horizontal pager whose first page is `viewer`, in
/// `direction`.
fn photo_pager(
    pager: &PageController,
    viewer: InteractiveViewer,
    direction: TextDirection,
    vsync: &Vsync,
) -> BoxedView {
    let pages: Vec<BoxedView> = vec![
        viewer.boxed(),
        SizedBox::new(PAGE, PAGE).boxed(),
        SizedBox::new(PAGE, PAGE).boxed(),
    ];
    VsyncScope::new(
        vsync.clone(),
        Directionality::new(
            direction,
            PageView::new(pages, crate::common::pager_alternatives()).controller(pager.clone()),
        ),
    )
    .boxed()
}

fn page_of(pager: &PageController) -> f64 {
    pager.page().expect("a laid-out pager has a page")
}

/// The scene zoomed twice about the viewport's centre.
fn zoomed() -> Matrix4 {
    Matrix4::translation(-PAGE / 2.0, -PAGE / 2.0, 0.0) * Matrix4::scaling(2.0, 2.0, 1.0)
}

fn translation_x(controller: &TransformationController) -> f64 {
    controller.value().translation_component().0
}

fn scale_of(controller: &TransformationController) -> f64 {
    let m = controller.value().to_col_major_array();
    m[0].hypot(m[1])
}

/// A zoom scales the plane, not depth: the transform's z row and column stay
/// the identity's, so the compositor can draw it. The engine admits only
/// two-dimensional affine transforms (ADR-0102); a depth scale failed every
/// frame after the first zoom.
fn assert_planar(controller: &TransformationController, case: &str) {
    let m = controller.value().to_col_major_array();
    assert!(
        [m[2], m[6], m[8], m[9], m[11], m[14]] == [0.0; 6] && m[10] == 1.0,
        "{case}: the transform scales or moves depth: {:?}",
        controller.value()
    );
}

fn assert_finite(controller: &TransformationController, case: &str) {
    assert!(
        controller
            .value()
            .to_col_major_array()
            .iter()
            .all(|value| value.is_finite()),
        "{case}: the published transform is finite: {:?}",
        controller.value()
    );
}

/// The latch lives in the leaf's plane (ADR-0209 §3): a photo turned 30° on
/// its page, zoomed to its own horizontal edge, takes the vertical share of a
/// pan along its own axes and leaves its horizontal share free, so the pager
/// follows the finger as it does for an upright photo. Latched in the
/// screen's axes, the photo's motion would show on both and keep the whole
/// contact.
pub(crate) fn a_turned_photo_at_its_edge_frees_the_axis_it_cannot_pan() {
    let angle = std::f64::consts::FRAC_PI_6;
    let (sin, cos) = angle.sin_cos();
    let (pager, transform, vsync) = (
        PageController::new(),
        TransformationController::new(),
        Vsync::new(),
    );
    // At the photo's far horizontal edge, mid-way vertically.
    transform
        .set_value(Matrix4::translation(-PAGE, -PAGE / 2.0, 0.0) * Matrix4::scaling(2.0, 2.0, 1.0));
    let pages: Vec<BoxedView> = vec![
        Transform::rotation(angle).child(photo(&transform)).boxed(),
        SizedBox::new(PAGE, PAGE).boxed(),
        SizedBox::new(PAGE, PAGE).boxed(),
    ];
    let tree = VsyncScope::new(
        vsync.clone(),
        PageView::new(pages, crate::common::pager_alternatives()).controller(pager.clone()),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);

    // The finger moves 100 along the photo's horizontal axis, past its edge,
    // and 60 along its vertical, which it can pan.
    let (across, down) = (-100.0, 60.0);
    let from = at(180.0, 150.0);
    let to = at(
        from.dx + across * cos - down * sin,
        from.dy + across * sin + down * cos,
    );
    let before = transform.value();
    let mut moved = 0.0_f64;
    laid.replay_with(&slow(Device::Touch, from, to), |_, _| {
        moved = moved.max(page_of(&pager));
    });
    assert_ne!(
        transform.value(),
        before,
        "the photo panned along its vertical axis"
    );
    assert!(
        moved > 0.05,
        "the pager followed the share the photo could not pan, to {moved}"
    );
}

/// A photo in a pager hands what its pan cannot carry to the pager through
/// the channel: unzoomed at any start angle (30° and 50° off horizontal), at
/// its edge on the next contact (edge at start), with a second finger
/// pinching after the pager took the first one's motion, and back to rest
/// when the contact is cancelled. Under the default remainder rule a photo
/// that moved first keeps the whole contact, even at its edge.
pub(crate) fn photo_in_pager_hands_pan_remainder_to_pager() {
    for device in DEVICES {
        // Unzoomed, at 0°, 30° and 50°: the pager takes the horizontal share
        // and lands on the next page; the photo does not move.
        for (degrees, from, to) in [
            (0.0, at(250.0, 150.0), at(50.0, 150.0)),
            (30.0, at(250.0, 220.0), at(50.0, 104.5)),
            (50.0, at(250.0, 260.0), at(80.0, 57.4)),
        ] {
            let (pager, transform, vsync) = (
                PageController::new(),
                TransformationController::new(),
                Vsync::new(),
            );
            let mut laid = mounted(
                photo_pager(&pager, photo(&transform), TextDirection::Ltr, &vsync),
                vsync,
            );
            laid.replay(&slow(device, from, to));
            settle(&mut laid);
            assert_eq!(
                page_of(&pager),
                1.0,
                "{device:?} at {degrees}°: an unzoomed photo's pan turns the page"
            );
            assert_eq!(
                transform.value(),
                Matrix4::IDENTITY,
                "{device:?} at {degrees}°: and the photo stays"
            );
        }

        // Zoomed: the photo pans first and keeps the contact at its edge;
        // the next contact, starting at the edge, turns the page.
        let (pager, transform, vsync) = (
            PageController::new(),
            TransformationController::new(),
            Vsync::new(),
        );
        transform.set_value(zoomed());
        let mut laid = mounted(
            photo_pager(&pager, photo(&transform), TextDirection::Ltr, &vsync),
            vsync,
        );
        laid.replay(&slow(device, at(250.0, 150.0), at(50.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            translation_x(&transform),
            -PAGE,
            "{device:?}: a zoomed photo pans to its edge"
        );
        assert_eq!(
            page_of(&pager),
            0.0,
            "{device:?}: and keeps the contact past it"
        );
        laid.replay(&slow(device, at(250.0, 150.0), at(50.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            page_of(&pager),
            1.0,
            "{device:?}: a contact that starts at the edge turns the page"
        );
        assert_eq!(
            translation_x(&transform),
            -PAGE,
            "{device:?}: the photo rests"
        );

        // Cancelled after the pager took the motion: the pager returns.
        let (pager, transform, vsync) = (
            PageController::new(),
            TransformationController::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            photo_pager(&pager, photo(&transform), TextDirection::Ltr, &vsync),
            vsync,
        );
        let mut script = TouchScript::new("cancelled pan").down(1, at(250.0, 150.0));
        for point in line(at(250.0, 150.0), at(150.0, 150.0), 10)
            .into_iter()
            .skip(1)
        {
            script = script.wait(Duration::from_millis(16)).move_to(1, point);
        }
        let script = script
            .wait(Duration::from_millis(16))
            .cancel(1)
            .build()
            .expect("one whole contact");
        let mut moved = 0.0_f64;
        laid.replay_with(&script, |_, _| moved = moved.max(page_of(&pager)));
        settle(&mut laid);
        assert!(moved > 0.2, "the pager followed the contact, to {moved}");
        assert_eq!(
            page_of(&pager),
            0.0,
            "a cancelled contact settles the pager from rest, back onto its page"
        );
    }

    // A second finger after the pager took the first one's motion: the
    // pinch zooms the photo, and the pager still lands on a page.
    let (pager, transform, vsync) = (
        PageController::new(),
        TransformationController::new(),
        Vsync::new(),
    );
    let mut laid = mounted(
        photo_pager(&pager, photo(&transform), TextDirection::Ltr, &vsync),
        vsync,
    );
    let mut script = TouchScript::new("pan, then pinch").down(1, at(200.0, 150.0));
    for x in [190.0, 180.0, 170.0, 160.0] {
        script = script
            .wait(Duration::from_millis(16))
            .move_to(1, at(x, 150.0));
    }
    script = script
        .wait(Duration::from_millis(16))
        .down(2, at(100.0, 150.0));
    for step in 1..=8 {
        let spread = f64::from(step) * 10.0;
        script = script
            .wait(Duration::from_millis(16))
            .move_to(1, at(160.0 + spread, 150.0))
            .move_to(2, at(100.0 - spread, 150.0));
    }
    let script = script
        .wait(Duration::from_millis(200))
        .up(1)
        .up(2)
        .build()
        .expect("two whole contacts");
    let mut paged = 0.0_f64;
    laid.replay_with(&script, |_, _| paged = paged.max(page_of(&pager)));
    settle(&mut laid);
    assert!(paged > 0.0, "the pager took the first finger's motion");
    assert!(
        scale_of(&transform) > 1.5,
        "the second finger pinches the photo, to {}",
        scale_of(&transform)
    );
    assert_eq!(page_of(&pager).fract(), 0.0, "the pager rests on a page");
}

/// In a right-to-left pager the next page is to the left: the photo's
/// remainder moves the pager toward the reading end.
pub(crate) fn photo_in_rtl_pager_turns_toward_the_reading_end() {
    for device in DEVICES {
        let (pager, transform, vsync) = (
            PageController::new(),
            TransformationController::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            photo_pager(&pager, photo(&transform), TextDirection::Rtl, &vsync),
            vsync,
        );
        laid.replay(&slow(device, at(250.0, 150.0), at(50.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            page_of(&pager),
            0.0,
            "{device:?}: a leftward pan is backward in a right-to-left pager"
        );
        laid.replay(&slow(device, at(50.0, 150.0), at(250.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            page_of(&pager),
            1.0,
            "{device:?}: a rightward pan turns to the next page"
        );
        assert_eq!(transform.value(), Matrix4::IDENTITY);
    }
}

/// The viewer's inertia is the releasing sequence's motion: a probe above it
/// sees the contact's user steps, then side-effect steps under the same
/// sequence, and the sequence end once, after the run rests at the boundary.
pub(crate) fn viewer_inertia_is_a_side_effect_of_the_releasing_sequence() {
    for device in DEVICES {
        let (transform, probe, vsync) =
            (TransformationController::new(), Probe::new(), Vsync::new());
        let viewer = photo(&transform).boundary_margin(EdgeInsets::all(400.0));
        let mut laid = mounted(
            VsyncScope::new(vsync.clone(), NestedScroll::new(probe.connection(), viewer)).boxed(),
            vsync,
        );
        laid.replay(&drag(
            device,
            &line(at(250.0, 150.0), at(130.0, 150.0), 6),
            Duration::from_millis(8),
            Duration::ZERO,
        ));
        let released = translation_x(&transform);
        settle(&mut laid);
        assert!(
            translation_x(&transform) < released,
            "{device:?}: the photo coasts past where it was released"
        );
        assert_eq!(
            translation_x(&transform),
            -400.0,
            "{device:?}: and rests at its boundary"
        );
        let seen = probe.seen();
        let started = probe.started();
        assert_eq!(started.len(), 1, "{device:?}: one sequence: {seen:?}");
        let sequence = started[0];
        let causes: Vec<ScrollCause> = probe
            .steps()
            .iter()
            .inspect(|step| assert_eq!(step.sequence, sequence, "{device:?}: one sequence"))
            .map(|step| step.cause)
            .collect();
        let first_side_effect = causes
            .iter()
            .position(|cause| *cause == ScrollCause::SideEffect)
            .expect("the inertia's steps reach the probe");
        assert!(
            first_side_effect > 0
                && causes[..first_side_effect]
                    .iter()
                    .all(|cause| *cause == ScrollCause::UserInput)
                && causes[first_side_effect..]
                    .iter()
                    .all(|cause| *cause == ScrollCause::SideEffect),
            "{device:?}: user steps, then the run's: {causes:?}"
        );
        assert!(
            seen.iter().any(|seen| matches!(seen, Seen::Released(_))),
            "{device:?}: the release is announced"
        );
        assert_eq!(
            probe.ended(),
            [(sequence, ScrollEndReason::Released)],
            "{device:?}: the sequence ends once, after the run"
        );
    }
}

/// A participant that panics at the release costs the others nothing: the
/// viewer's inertia still runs as the sequence's motion, the sequence ends
/// once for every participant, and the failure resumes once.
pub(crate) fn viewer_release_failure_still_ends_the_sequence() {
    for device in DEVICES {
        let (transform, near, far, vsync) = (
            TransformationController::new(),
            Probe::new(),
            Probe::new(),
            Vsync::new(),
        );
        near.set_observe(|seen| {
            if matches!(seen, Seen::Released(_)) {
                panic!("a participant fails at the release");
            }
        });
        let viewer = photo(&transform).boundary_margin(EdgeInsets::all(400.0));
        let mut laid = mounted(
            VsyncScope::new(
                vsync.clone(),
                NestedScroll::new(
                    far.connection(),
                    NestedScroll::new(near.connection(), viewer),
                ),
            )
            .boxed(),
            vsync,
        );
        let failures = replay_contained(
            &mut laid,
            &drag(
                device,
                &line(at(250.0, 150.0), at(130.0, 150.0), 6),
                Duration::from_millis(8),
                Duration::ZERO,
            ),
        );
        settle(&mut laid);
        assert_eq!(
            failures,
            vec!["a participant fails at the release".to_owned()],
            "{device:?}: the failure resumes, once"
        );
        let sequence = far.started()[0];
        assert_eq!(
            far.ended(),
            [(sequence, ScrollEndReason::Released)],
            "{device:?}: the sequence still ends for the others: {:?}",
            far.seen()
        );
        assert_eq!(
            translation_x(&transform),
            -400.0,
            "{device:?}: and the photo's inertia still runs to its boundary"
        );
    }
}

fn wheel(dy: f64, modifiers: Modifiers) -> PointerEvent {
    PointerEvent::Scroll(
        ScrollEvent::new(
            PointerInfo::new(
                PointerId::try_from(7_u64).expect("a nonzero pointer"),
                PointerKind::Mouse,
            )
            .with_device(DeviceId::try_from(3_u64).expect("a nonzero device"))
            .with_role(PointerRole::Primary),
            EventTime::from_nanos(60),
            PointerPosition::try_new(Point::new(100.0, 100.0)).expect("a finite position"),
            ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, dy).expect("a finite delta"),
        )
        .with_modifiers(modifiers),
    )
}

/// Under the ctrl gate a plain wheel pans the viewer as a step of the
/// binding's latch; the viewer at its edge leaves the next latch to the list
/// around it, while Ctrl+wheel still zooms.
pub(crate) fn plain_wheel_pans_the_viewer_then_leaves_its_edge_to_the_list() {
    let (transform, list) = (TransformationController::new(), ScrollController::new());
    let viewer = InteractiveViewer::new(viewer_alternatives())
        .controller(transform.clone())
        .wheel_scale_gate(WheelScaleGate::CtrlWheel)
        .boundary_margin(EdgeInsets::all(60.0))
        .child(SizedBox::new(300.0, 300.0));
    let mut laid = lay_out(
        Scrollable::new()
            .controller(list.clone())
            .child(SizedBox::new(300.0, 1000.0).child(viewer)),
        tight(300.0, 300.0),
    );
    let idle = flui_interaction::GestureSettings::default().contactless_latch_idle();
    laid.dispatch_pointer_event(&wheel(40.0, Modifiers::NONE));
    assert_eq!(
        transform.value().translation_component().1,
        -40.0,
        "a plain tick pans the viewer"
    );
    assert_eq!(list.pixels(), 0.0, "and not the list");
    laid.dispatch_pointer_event(&wheel(40.0, Modifiers::NONE));
    assert_eq!(
        transform.value().translation_component().1,
        -60.0,
        "the latch keeps the viewer up to its edge"
    );
    assert_eq!(list.pixels(), 0.0, "the latched viewer keeps its remainder");
    laid.pump_for(idle);
    laid.dispatch_pointer_event(&wheel(40.0, Modifiers::NONE));
    assert_eq!(
        list.pixels(),
        40.0,
        "the next latch at the edge is the list's"
    );
    laid.pump_for(idle);
    laid.dispatch_pointer_event(&wheel(-53.0, Modifiers::CONTROL));
    assert!(scale_of(&transform) > 1.0, "Ctrl+wheel zooms");
    assert_eq!(list.pixels(), 40.0, "without scrolling the list");
}

fn press_key(laid: &LaidOut, key: Key, code: Code, modifiers: Modifiers) -> bool {
    let manager = laid.focus_manager();
    let mut handled = false;
    for state in [KeyState::Down, KeyState::Up] {
        let event = KeyEvent::new(state, key.clone(), code, EventTime::from_nanos(0))
            .with_modifiers(modifiers);
        let result = manager.dispatch_key_event(&event);
        if state == KeyState::Down {
            handled = result == flui_interaction::focus::KeyEventResult::Handled;
        }
    }
    handled
}

fn focus_viewer(laid: &mut LaidOut) {
    press_key(laid, Key::Named(NamedKey::Tab), Code::Tab, Modifiers::NONE);
    laid.pump();
}

/// With focus in the viewer, the arrows pan it one step — and an arrow that
/// cannot move it bubbles to the list around it — and `+`, `=` and `-` zoom
/// about the centre.
pub(crate) fn viewer_keys_pan_and_zoom_from_the_focused_row() {
    let (transform, list) = (TransformationController::new(), ScrollController::new());
    let mut laid = lay_out(
        Scrollable::new()
            .controller(list.clone())
            .child(flui_widgets::Column::new(vec![
                SizedBox::new(300.0, 300.0)
                    .child(
                        InteractiveViewer::new(viewer_alternatives())
                            .controller(transform.clone())
                            .boundary_margin(EdgeInsets::all(100.0))
                            .child(SizedBox::new(300.0, 300.0)),
                    )
                    .boxed(),
                SizedBox::new(300.0, 700.0).boxed(),
            ])),
        tight(300.0, 300.0),
    );
    focus_viewer(&mut laid);
    assert_eq!(list.pixels(), 0.0, "the focused viewer is in view");
    let arrow = |laid: &LaidOut, key: NamedKey, code: Code| {
        press_key(laid, Key::Named(key), code, Modifiers::NONE)
    };
    assert!(arrow(&laid, NamedKey::ArrowRight, Code::ArrowRight));
    assert_eq!(
        transform.value().translation_component(),
        (-53.0, 0.0, 0.0),
        "ArrowRight reveals what is to the right"
    );
    assert!(arrow(&laid, NamedKey::ArrowDown, Code::ArrowDown));
    assert!(arrow(&laid, NamedKey::ArrowDown, Code::ArrowDown));
    assert_eq!(
        transform.value().translation_component().1,
        -100.0,
        "ArrowDown reveals what is below, up to the boundary"
    );
    assert_eq!(list.pixels(), 0.0, "the list stays while the viewer moves");
    assert!(arrow(&laid, NamedKey::ArrowDown, Code::ArrowDown));
    assert!(
        list.pixels() > 0.0,
        "an arrow the viewer cannot take bubbles to the list"
    );
    assert_eq!(transform.value().translation_component().1, -100.0);
    assert!(press_key(
        &laid,
        Key::character("+"),
        Code::Equal,
        Modifiers::SHIFT
    ));
    assert_eq!(scale_of(&transform), 2.0, "+ zooms in one step");
    assert_planar(&transform, "+");
    assert!(press_key(
        &laid,
        Key::character("-"),
        Code::Minus,
        Modifiers::CONTROL
    ));
    assert_eq!(scale_of(&transform), 1.0, "Ctrl+- zooms back out");
}

/// A text field inside the viewer keeps the keys it types and moves its caret
/// with: `+`, `=`, `-` and the caret's arrows pressed in it neither zoom nor
/// pan the viewer, whose own bindings for them answer only what bubbles past
/// a field (a single-line field's ArrowDown does, as it does to a list).
pub(crate) fn keys_typed_in_a_field_inside_the_viewer_stay_in_the_field() {
    use flui_interaction::focus::FocusNode;
    use flui_widgets::{EditableText, TextEditingController};

    let transform = TransformationController::new();
    let field = FocusNode::with_debug_label("field");
    let harness = crate::common::harness::mount_with_ime(
        InteractiveViewer::new(viewer_alternatives())
            .controller(transform.clone())
            .boundary_margin(EdgeInsets::all(100.0))
            .child(SizedBox::new(300.0, 50.0).child(EditableText::new(
                TextEditingController::new(),
                Rc::clone(&field),
            ))),
    );
    let _ = field.request_focus();
    assert!(field.has_primary_focus(), "the field has the focus");
    let before = transform.value();
    let manager = harness.focus_manager();
    for (key, code, modifiers) in [
        (Key::character("+"), Code::Equal, Modifiers::SHIFT),
        (Key::character("="), Code::Equal, Modifiers::NONE),
        (Key::character("-"), Code::Minus, Modifiers::NONE),
        (
            Key::Named(NamedKey::ArrowLeft),
            Code::ArrowLeft,
            Modifiers::NONE,
        ),
        (
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            Modifiers::NONE,
        ),
    ] {
        for state in [KeyState::Down, KeyState::Up] {
            let event = KeyEvent::new(state, key.clone(), code, EventTime::from_nanos(0))
                .with_modifiers(modifiers);
            let _ = manager.dispatch_key_event(&event);
        }
        assert_eq!(
            transform.value(),
            before,
            "{key:?} typed in the field leaves the viewer as it was"
        );
    }
    assert!(field.has_primary_focus(), "and the field keeps the focus");
}

fn viewer_node(laid: &LaidOut) -> flui_testing::a11y::NodeId {
    let tree = laid.a11y_tree().expect("semantics are on");
    tree.nodes()
        .find(|node| node.supports_action(Action::Increment))
        .expect("the viewer publishes its zoom")
        .id()
}

fn request(laid: &LaidOut, action: Action, data: Option<ActionData>) {
    laid.invoke_semantics_action(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: viewer_node(laid),
        data,
    })
    .expect("the viewer routes its action");
}

/// Assistive technology zooms the viewer by increment, decrement and an
/// exact value within the scale bounds, and pans it with the scroll actions.
pub(crate) fn viewer_assistive_actions_zoom_and_pan() {
    let transform = TransformationController::new();
    let mut laid = lay_out(
        InteractiveViewer::new(viewer_alternatives())
            .controller(transform.clone())
            .min_scale(0.5)
            .max_scale(3.0)
            .boundary_margin(EdgeInsets::all(f64::INFINITY))
            .child(SizedBox::new(300.0, 300.0)),
        tight(300.0, 300.0),
    );
    laid.enable_semantics();
    laid.pump();
    let numeric = |laid: &LaidOut| {
        let tree = laid.a11y_tree().expect("semantics are on");
        let node = viewer_node(laid);
        tree.nodes()
            .find(|candidate| candidate.id() == node)
            .and_then(|node| node.raw().numeric_value())
    };
    assert_eq!(numeric(&laid), Some(1.0), "the scale is the node's value");
    request(&laid, Action::Increment, None);
    laid.pump();
    assert_eq!(scale_of(&transform), 2.0, "increment zooms in one step");
    assert_planar(&transform, "increment");
    assert_eq!(numeric(&laid), Some(2.0));
    request(&laid, Action::Increment, None);
    laid.pump();
    assert_eq!(scale_of(&transform), 3.0, "clamped to the maximum");
    request(&laid, Action::Decrement, None);
    laid.pump();
    assert_eq!(scale_of(&transform), 1.5, "decrement zooms out one step");
    request(
        &laid,
        Action::SetValue,
        Some(ActionData::NumericValue(0.75)),
    );
    laid.pump();
    assert!(
        (scale_of(&transform) - 0.75).abs() < 1e-12,
        "a set value is the scale, at {}",
        scale_of(&transform)
    );
    let before = transform.value().translation_component();
    request(&laid, Action::ScrollRight, None);
    laid.pump();
    let after = transform.value().translation_component();
    assert!(
        after.0 < before.0,
        "scroll right reveals the right: {after:?}"
    );
    assert_eq!(after.1, before.1, "on its own axis");
}

fn double_tap(device: Device, point: Offset<f64>) -> PointerScript {
    let tap = Duration::from_millis(30);
    let gap = Duration::from_millis(60);
    match device {
        Device::Touch => TouchScript::new("double tap")
            .down(1, point)
            .wait(tap)
            .up(1)
            .wait(gap)
            .down(2, point)
            .wait(tap)
            .up(2)
            .build(),
        Device::Mouse => MouseScript::new("double click")
            .move_to(point)
            .press(point)
            .wait(tap)
            .release()
            .wait(gap)
            .press(point)
            .wait(tap)
            .release()
            .build(),
    }
    .expect("whole contacts")
}

/// A double tap zooms in one step about the tapped point, through the
/// alternatives' first command; at the maximum it zooms back out to the
/// minimum.
pub(crate) fn viewer_double_tap_zooms_about_the_tap_and_back_out_at_the_maximum() {
    for device in DEVICES {
        let transform = TransformationController::new();
        let mut laid = lay_out(
            InteractiveViewer::new(viewer_alternatives())
                .controller(transform.clone())
                .min_scale(1.0)
                .max_scale(2.0)
                .child(SizedBox::new(300.0, 300.0)),
            tight(300.0, 300.0),
        );
        let tapped = at(90.0, 120.0);
        laid.replay(&double_tap(device, tapped));
        laid.pump_for(Duration::from_millis(400));
        assert_eq!(scale_of(&transform), 2.0, "{device:?}: one step in");
        assert_planar(&transform, &format!("{device:?} double tap"));
        let scene = transform.to_scene(tapped);
        assert!(
            (scene.dx - tapped.dx).abs() < 1e-9 && (scene.dy - tapped.dy).abs() < 1e-9,
            "{device:?}: the tapped point stays under the tap: {scene:?}"
        );
        laid.replay(&double_tap(device, tapped));
        laid.pump_for(Duration::from_millis(400));
        assert_eq!(
            scale_of(&transform),
            1.0,
            "{device:?}: back out at the maximum"
        );
    }
}

/// Degenerate inputs publish no non-finite transform: a zero-size child
/// moves nothing on any path, equal scale bounds fix the scale while the
/// pan still works, and a not-a-number or huge zoom request clamps.
pub(crate) fn viewer_degenerate_geometry_and_bounds_publish_finite_transforms() {
    // A zero-size child.
    let transform = TransformationController::new();
    let mut laid = lay_out(
        InteractiveViewer::new(viewer_alternatives())
            .controller(transform.clone())
            .boundary_margin(EdgeInsets::all(100.0))
            .child(SizedBox::new(0.0, 0.0)),
        crate::common::loose(300.0),
    );
    laid.enable_semantics();
    laid.pump();
    laid.replay(&slow(Device::Touch, at(0.0, 0.0), at(40.0, 40.0)));
    request(&laid, Action::Increment, None);
    laid.pump();
    assert_eq!(transform.value(), Matrix4::IDENTITY, "nothing to move");
    assert_finite(&transform, "zero-size child");

    // Equal bounds: a pinch and a native zoom keep the scale, a pan works.
    let transform = TransformationController::new();
    let mut laid = lay_out(
        InteractiveViewer::new(viewer_alternatives())
            .controller(transform.clone())
            .min_scale(1.0)
            .max_scale(1.0)
            .boundary_margin(EdgeInsets::all(100.0))
            .child(SizedBox::new(300.0, 300.0)),
        tight(300.0, 300.0),
    );
    laid.replay(&PointerScript::pinch(at(150.0, 150.0), 40.0, 120.0, 8));
    settle(&mut laid);
    assert_eq!(
        scale_of(&transform),
        1.0,
        "a pinch comes back to the fixed scale"
    );
    let native = |phase| {
        PointerEvent::PanZoom(PanZoomEvent::new(
            PointerInfo::new(
                PointerId::try_from(9_u64).expect("a nonzero pointer"),
                PointerKind::Trackpad,
            ),
            EventTime::from_nanos(70),
            PointerPosition::try_new(Point::new(150.0, 150.0)).expect("a finite position"),
            phase,
        ))
    };
    laid.dispatch_pointer_event(&native(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&native(PanZoomPhase::Update(
        PanZoomTransform::try_new(1e300, 0.0).expect("a finite scale"),
    )));
    laid.dispatch_pointer_event(&native(PanZoomPhase::End));
    settle(&mut laid);
    assert_eq!(
        scale_of(&transform),
        1.0,
        "a huge native zoom comes back too"
    );
    assert_finite(&transform, "equal bounds");
    laid.replay(&slow(Device::Touch, at(200.0, 150.0), at(150.0, 150.0)));
    settle(&mut laid);
    assert!(
        translation_x(&transform) < 0.0,
        "a pan still moves the fixed-scale scene"
    );

    // Huge and vanishing native scale factors, whose products overflow or
    // underflow, clamp to the bounds; a not-a-number factor is no event.
    assert!(PanZoomTransform::try_new(f64::NAN, 0.0).is_err());
    for (factor, bound) in [
        (1e300, 4.0),
        (f64::MAX, 4.0),
        (1e-300, 1.0),
        (f64::MIN_POSITIVE, 1.0),
    ] {
        let transform = TransformationController::new();
        let mut laid = lay_out(
            InteractiveViewer::new(viewer_alternatives())
                .controller(transform.clone())
                .min_scale(0.5)
                .max_scale(4.0)
                .child(SizedBox::new(300.0, 300.0)),
            tight(300.0, 300.0),
        );
        for _ in 0..2 {
            laid.dispatch_pointer_event(&native(PanZoomPhase::Start));
            laid.dispatch_pointer_event(&native(PanZoomPhase::Update(
                PanZoomTransform::try_new(factor, 0.0).expect("a finite scale"),
            )));
            laid.dispatch_pointer_event(&native(PanZoomPhase::End));
            settle(&mut laid);
        }
        assert_finite(&transform, &format!("factor {factor}"));
        assert_eq!(
            scale_of(&transform),
            bound,
            "factor {factor}: clamped to the bound the boundary admits"
        );
    }
}

/// The viewer under a parent whose state its own update callback changes.
#[derive(Clone)]
struct Rebuilding {
    transform: TransformationController,
    max_scale: Signal<f64>,
    probe: Rc<Probe>,
    updates: Rc<RefCell<u32>>,
}

impl View for Rebuilding {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl flui_view::StatelessView for Rebuilding {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let (signal, updates) = (self.max_scale, Rc::clone(&self.updates));
        NestedScroll::new(
            self.probe.connection(),
            InteractiveViewer::new(viewer_alternatives())
                .controller(self.transform.clone())
                .max_scale(self.max_scale.get(ctx))
                .boundary_margin(EdgeInsets::all(f64::INFINITY))
                .on_interaction_update(move |cx, _details| {
                    *updates.borrow_mut() += 1;
                    signal.set(cx, 2.0 + f64::from(*updates.borrow()))
                })
                .child(SizedBox::new(300.0, 300.0)),
        )
    }
}

/// An update callback that rebuilds the viewer on every move does not cut
/// the gesture: the pan reaches its full distance under one sequence, and
/// each rebuilt configuration applies to the next move.
pub(crate) fn viewer_rebuild_from_its_update_callback_keeps_the_gesture() {
    for device in DEVICES {
        let (transform, probe, vsync) =
            (TransformationController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(SizedBox::shrink().boxed(), vsync);
        let reactive = laid.with_build_owner_mut(|owner| owner.signal_graph().clone());
        let updates = Rc::new(RefCell::new(0));
        laid.pump_widget(Rebuilding {
            transform: transform.clone(),
            max_scale: reactive.signal(2.0),
            probe: Rc::clone(&probe),
            updates: Rc::clone(&updates),
        });
        laid.replay(&slow(device, at(250.0, 150.0), at(100.0, 150.0)));
        settle(&mut laid);
        assert!(
            *updates.borrow() > 3,
            "{device:?}: every move rebuilt the parent"
        );
        let moved = translation_x(&transform);
        assert!(
            (moved + 150.0).abs() < 1e-9,
            "{device:?}: the pan reached the contact's distance, {moved}"
        );
        assert_eq!(
            probe.started().len(),
            1,
            "{device:?}: one sequence across the rebuilds: {:?}",
            probe.seen()
        );
        assert_eq!(
            probe.ended(),
            [(probe.started()[0], ScrollEndReason::Released)],
            "{device:?}: released once, never cut short: {:?}",
            probe.seen()
        );
    }
}

/// A photo holding its contact pulls past its edge through the rubber band —
/// less far than the contact moves — without handing the pager anything,
/// and comes back to the edge after it lifts.
pub(crate) fn viewer_pan_past_its_boundary_stretches_and_snaps_back() {
    for device in DEVICES {
        let (pager, transform, vsync) = (
            PageController::new(),
            TransformationController::new(),
            Vsync::new(),
        );
        transform.set_value(zoomed());
        let mut laid = mounted(
            photo_pager(&pager, photo(&transform), TextDirection::Ltr, &vsync),
            vsync,
        );
        let (mut furthest, mut paged) = (0.0_f64, 0.0_f64);
        laid.replay_with(&slow(device, at(290.0, 150.0), at(-10.0, 150.0)), |_, _| {
            furthest = furthest.min(translation_x(&transform));
            paged = paged.max(page_of(&pager));
        });
        assert!(
            furthest < -PAGE - 30.0 && furthest > -PAGE - 150.0,
            "{device:?}: pulled past the edge with resistance, to {furthest}"
        );
        assert_eq!(
            paged, 0.0,
            "{device:?}: the pager is not handed the stretch"
        );
        settle(&mut laid);
        assert_eq!(
            translation_x(&transform),
            -PAGE,
            "{device:?}: back at the edge after the contact lifts"
        );
        assert_eq!(page_of(&pager), 0.0);
    }
}

/// A pinch past a scale bound shows the scene further with resistance —
/// never half an octave past it — and comes back to the bound on release.
pub(crate) fn viewer_pinch_past_the_scale_bounds_stretches_and_snaps_back() {
    for (from, to, bound) in [(40.0, 200.0, 2.0), (200.0, 40.0, 1.0)] {
        let (transform, vsync) = (TransformationController::new(), Vsync::new());
        let viewer = InteractiveViewer::new(viewer_alternatives())
            .controller(transform.clone())
            .min_scale(1.0)
            .max_scale(2.0)
            .boundary_margin(EdgeInsets::all(f64::INFINITY))
            .child(SizedBox::new(300.0, 300.0));
        let mut laid = mounted(VsyncScope::new(vsync.clone(), viewer).boxed(), vsync);
        let mut furthest = 1.0_f64;
        laid.replay_with(
            &PointerScript::pinch(at(150.0, 150.0), from, to, 8),
            |_, _| {
                let scale = scale_of(&transform);
                if (scale - 1.0).abs() > (furthest - 1.0).abs() {
                    furthest = scale;
                }
            },
        );
        let past = if bound > 1.5 {
            furthest / bound
        } else {
            bound / furthest
        };
        assert!(
            past > 1.05 && past < std::f64::consts::SQRT_2,
            "pinch to {to}: shown at {furthest}, past {bound} with resistance"
        );
        settle(&mut laid);
        assert!(
            (scale_of(&transform) - bound).abs() < 1e-12,
            "pinch to {to}: back at {bound}, at {}",
            scale_of(&transform)
        );
        assert_planar(&transform, &format!("pinch to {to}"));
    }
}

/// Two fingers turning a quarter about the viewport's centre at a fixed
/// spread, in `steps` samples.
fn twist(steps: u32) -> PointerScript {
    let (center, radius) = (at(150.0, 150.0), 60.0);
    let pair = |angle: f64| {
        let (sin, cos) = angle.sin_cos();
        let arm = Offset::new(radius * cos, radius * sin);
        (center - arm, center + arm)
    };
    let sample = Duration::from_millis(8);
    let (first, second) = pair(0.0);
    let mut script = TouchScript::new("twist")
        .down(1, first)
        .wait(sample)
        .down(2, second);
    for step in 1..=steps {
        let (first, second) =
            pair(std::f64::consts::FRAC_PI_2 * f64::from(step) / f64::from(steps));
        script = script.wait(sample).move_to(1, first).move_to(2, second);
    }
    script
        .wait(sample)
        .up(1)
        .up(2)
        .build()
        .expect("BUG: the twist pairs its contacts")
}

/// The angle the published transform turns the plane by.
fn rotation_of(controller: &TransformationController) -> f64 {
    let m = controller.value().to_col_major_array();
    m[1].atan2(m[0])
}

/// A two-finger twist turns the scene about the fingers when rotation is
/// enabled, and leaves it upright, at the same scale, when it is not (the
/// default).
pub(crate) fn viewer_touch_twist_rotates_only_when_rotation_is_enabled() {
    for enabled in [true, false] {
        let (transform, vsync) = (TransformationController::new(), Vsync::new());
        let viewer = InteractiveViewer::new(viewer_alternatives())
            .controller(transform.clone())
            .double_tap_zoom(false)
            .rotation_enabled(enabled)
            .boundary_margin(EdgeInsets::all(f64::INFINITY))
            .child(SizedBox::new(300.0, 300.0));
        let mut laid = mounted(VsyncScope::new(vsync.clone(), viewer).boxed(), vsync);
        laid.replay(&twist(12));
        settle(&mut laid);
        let turned = rotation_of(&transform).abs();
        if enabled {
            assert!(
                turned > std::f64::consts::FRAC_PI_4 && turned < std::f64::consts::FRAC_PI_2 + 1e-6,
                "a quarter twist turns the scene by most of a quarter, by {turned}"
            );
        } else {
            assert_eq!(turned, 0.0, "rotation disabled: the scene stays upright");
        }
        assert!(
            (scale_of(&transform) - 1.0).abs() < 1e-9,
            "rotation enabled {enabled}: a twist at a fixed spread does not zoom, at {}",
            scale_of(&transform)
        );
        assert_planar(&transform, &format!("twist, rotation enabled {enabled}"));
        assert_finite(&transform, &format!("twist, rotation enabled {enabled}"));
    }
}

/// A fling the boundary stops overshoots past it and settles on it, as the
/// last motion of the releasing sequence.
pub(crate) fn viewer_fling_into_its_boundary_overshoots_and_settles() {
    for device in DEVICES {
        let (transform, probe, vsync) =
            (TransformationController::new(), Probe::new(), Vsync::new());
        let viewer = photo(&transform).boundary_margin(EdgeInsets::all(100.0));
        let mut laid = mounted(
            VsyncScope::new(vsync.clone(), NestedScroll::new(probe.connection(), viewer)).boxed(),
            vsync,
        );
        laid.replay(&drag(
            device,
            &line(at(200.0, 150.0), at(150.0, 150.0), 5),
            Duration::from_millis(8),
            Duration::ZERO,
        ));
        assert!(
            translation_x(&transform) > -100.0,
            "{device:?}: released inside"
        );
        let mut furthest = 0.0_f64;
        for _ in 0..120 {
            laid.pump_for(Duration::from_millis(16));
            furthest = furthest.min(translation_x(&transform));
        }
        assert!(
            furthest < -101.0,
            "{device:?}: the fling overshoots its boundary, to {furthest}"
        );
        assert_eq!(
            translation_x(&transform),
            -100.0,
            "{device:?}: and settles on it"
        );
        assert_eq!(
            probe.ended(),
            [(probe.started()[0], ScrollEndReason::Released)],
            "{device:?}: the overshoot ends the sequence, once"
        );
    }
}

/// An arrow held on the viewer whose release goes elsewhere, because focus
/// moved meanwhile, still ends the keyboard sequence the press began.
pub(crate) fn viewer_key_sequence_ends_when_focus_moves_before_the_release() {
    let (transform, probe, vsync) = (TransformationController::new(), Probe::new(), Vsync::new());
    let elsewhere = flui_interaction::focus::FocusNode::with_debug_label("elsewhere");
    let tree = flui_widgets::Column::new(vec![
        SizedBox::new(300.0, 300.0)
            .child(NestedScroll::new(
                probe.connection(),
                photo(&transform).boundary_margin(EdgeInsets::all(100.0)),
            ))
            .boxed(),
        flui_widgets::interaction::Focus::new(SizedBox::new(40.0, 40.0))
            .focus_node(Rc::clone(&elsewhere))
            .boxed(),
    ]);
    let mut laid = mounted(VsyncScope::new(vsync.clone(), tree).boxed(), vsync);
    focus_viewer(&mut laid);
    let manager = laid.focus_manager();
    let right = |state| {
        KeyEvent::new(
            state,
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            EventTime::from_nanos(0),
        )
    };
    assert!(
        manager
            .dispatch_key_event(&right(KeyState::Down))
            .is_handled(),
        "the arrow pans the focused viewer"
    );
    let held = probe.started()[0];
    let _ = elsewhere.request_focus();
    assert!(elsewhere.has_primary_focus());
    let _ = manager.dispatch_key_event(&right(KeyState::Up));
    settle(&mut laid);
    assert_eq!(
        probe.ended(),
        [(held, ScrollEndReason::Released)],
        "the release after focus moved ends the sequence, once"
    );
}

/// A contact on the viewer gives it the keys: an arrow pans it next,
/// without moving focus there first.
pub(crate) fn viewer_contact_gives_it_the_keys() {
    for device in DEVICES {
        let transform = TransformationController::new();
        let mut laid = lay_out(
            InteractiveViewer::new(viewer_alternatives())
                .controller(transform.clone())
                .boundary_margin(EdgeInsets::all(100.0))
                .child(SizedBox::new(300.0, 300.0)),
            tight(300.0, 300.0),
        );
        laid.replay(&slow(device, at(150.0, 150.0), at(150.0, 120.0)));
        settle(&mut laid);
        let before = transform.value().translation_component().0;
        assert!(
            press_key(
                &laid,
                Key::Named(NamedKey::ArrowRight),
                Code::ArrowRight,
                Modifiers::NONE
            ),
            "{device:?}: the arrow reaches the viewer"
        );
        assert_eq!(
            transform.value().translation_component().0,
            before - 53.0,
            "{device:?}: and pans it"
        );
    }
}
