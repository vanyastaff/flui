//! `AnchoredDrag` (ADR-0209 §9): rows of the `nested_scroll_channel` table in
//! `contracts.rs`, each pointer row driven by touch and mouse scripts.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::Axis;
use flui_interaction::events::{Code, Key, NamedKey};
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_platform_api::HapticFeedback;
use flui_view::{BoxedView, IntoView, ViewExt};
use flui_widgets::{
    Alternative, AnchorIntent, AnchoredDrag, AnchoredDragController, AnchorsError, ColoredBox,
    ControlPlacement, ControlSpec, Directionality, DraggableAnchors, GestureAlternatives,
    GestureCommand, NestedScroll, ScrollCause, ScrollController, ScrollDevice, Scrollable,
    SizedBox, VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, tight};
use crate::nested_scroll::{DEVICES, Device, Keys, Probe, at, fling, settle, slow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Closed,
    Open,
    Far,
}

const CONTENT: Color = Color::rgb(131, 43, 71);

fn stops(entries: &[(Stop, f64)]) -> DraggableAnchors<Stop> {
    entries
        .iter()
        .fold(DraggableAnchors::builder(), |builder, &(key, offset)| {
            builder.at(key, offset)
        })
        .build()
        .expect("valid anchors")
}

/// A table row: its name, the anchors and what building them gives.
type AnchorRow = (
    &'static str,
    &'static [(Stop, f64)],
    Result<(), AnchorsError>,
);

fn three() -> DraggableAnchors<Stop> {
    stops(&[(Stop::Closed, 0.0), (Stop::Open, 120.0), (Stop::Far, 240.0)])
}

fn controller(anchors: DraggableAnchors<Stop>) -> AnchoredDragController<Stop> {
    AnchoredDragController::new(anchors, Stop::Closed).expect("Closed is an anchor")
}

/// The side of the dragged square and of the area it is dragged in.
const SIDE: f64 = 300.0;

/// How wide the alternatives' two controls are: the touch minimum target
/// each.
fn controls_width() -> f64 {
    2.0 * flui_interaction::GestureSettingsProvider::default()
        .min_target_for(flui_interaction::PointerKind::Touch)
}

/// Alternatives for a drag under test whose alternatives are not what the
/// test is about: two empty controls on the right of the drag in either
/// reading direction, so the drag keeps the area a test drags in.
fn alternatives_in(direction: TextDirection) -> GestureAlternatives {
    let placement = match direction {
        TextDirection::Ltr => ControlPlacement::Trailing,
        TextDirection::Rtl => ControlPlacement::Leading,
    };
    GestureAlternatives::new(Alternative::VisibleControl(
        ControlSpec::new(
            GestureCommand::new("Next anchor", AnchorIntent::Next),
            SizedBox::shrink(),
        )
        .control(
            GestureCommand::new("Previous anchor", AnchorIntent::Previous),
            SizedBox::shrink(),
        )
        .placement(placement),
    ))
    .expect("two labelled commands")
}

/// A 300 × 300 square dragged along `axis`.
fn drag_view(controller: &AnchoredDragController<Stop>, axis: Axis) -> AnchoredDrag<Stop> {
    drag_view_in(controller, axis, TextDirection::Ltr)
}

/// A 300 × 300 square dragged along `axis`, its alternatives' controls on
/// its right under `direction`.
fn drag_view_in(
    controller: &AnchoredDragController<Stop>,
    axis: Axis,
    direction: TextDirection,
) -> AnchoredDrag<Stop> {
    AnchoredDrag::new(
        controller.clone(),
        axis,
        ColoredBox::new(CONTENT).child(SizedBox::new(SIDE, SIDE)),
        alternatives_in(direction),
    )
}

/// `view` laid out with room for the drag's square and, on its right, its
/// alternatives' controls.
fn mount(view: impl IntoView, vsync: &Vsync) -> LaidOut {
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), view).boxed(),
        tight(SIDE + controls_width(), SIDE),
        vsync.clone(),
    );
    laid.tick();
    laid
}

/// Where the square is painted: its left and top edges.
fn painted(laid: &LaidOut) -> (f64, f64) {
    painted_at(laid, CONTENT)
}

/// Where the first rectangle painted `color` is, through every layer above
/// its picture: its left and top edges.
pub(crate) fn painted_at(laid: &LaidOut, color: Color) -> (f64, f64) {
    use flui_rendering::layer::Layer;
    let tree = laid.layer_tree().expect("a committed scene");
    for (_, node) in tree.iter() {
        let Layer::Picture(picture) = node.layer() else {
            continue;
        };
        for command in picture.picture() {
            let flui_painting::display_list::DrawOp::Rect { rect, paint } = &command.op else {
                continue;
            };
            if paint.color != color {
                continue;
            }
            let (x, y) = command.transform.transform_point(rect.left(), rect.top());
            let mut point = flui_foundation::geometry::Point::new(x, y);
            let mut parent = node.parent();
            while let Some(id) = parent {
                let ancestor = tree.get(id).expect("a scene parent exists");
                match ancestor.layer() {
                    Layer::Transform(layer) => point = layer.transform_point(point),
                    Layer::Offset(layer) => point += layer.offset(),
                    Layer::Opacity(layer) => point += layer.offset(),
                    _ => {}
                }
                parent = ancestor.parent();
            }
            return (point.x, point.y);
        }
    }
    panic!("{color:?} paints");
}

fn near(value: f64, target: f64) -> bool {
    (value - target).abs() < 0.5
}

/// A drag settles on the nearest anchor, a fling on the next one in its
/// direction; the child follows the offset and the drag clamps to the
/// extreme anchors (ADR-0209 §9, W3).
pub(crate) fn anchored_drag_settles_on_anchors_by_release_and_fling() {
    for device in DEVICES {
        let vsync = Vsync::new();
        let anchors = controller(three());
        let mut laid = mount(drag_view(&anchors, Axis::Horizontal), &vsync);
        let mut play = |script| {
            laid.replay(&script);
            settle(&mut laid);
            (anchors.offset(), anchors.current())
        };
        assert_eq!(
            play(slow(device, at(100.0, 150.0), at(150.0, 150.0))),
            (0.0, Stop::Closed),
            "{device:?}: short of halfway the drag returns"
        );
        assert_eq!(
            play(slow(device, at(100.0, 150.0), at(180.0, 150.0))),
            (120.0, Stop::Open),
            "{device:?}: past halfway it rests on the next anchor"
        );
        // Each contact goes down on the square: the strip the drag uncovers
        // is not the drag's.
        assert_eq!(
            play(fling(device, at(200.0, 150.0), at(230.0, 150.0))),
            (240.0, Stop::Far),
            "{device:?}: a fling takes the next anchor in its direction"
        );
        assert_eq!(
            play(slow(device, at(250.0, 150.0), at(299.0, 150.0))),
            (240.0, Stop::Far),
            "{device:?}: the highest anchor bounds the drag"
        );
        assert_eq!(
            play(fling(device, at(290.0, 150.0), at(260.0, 150.0))),
            (120.0, Stop::Open),
            "{device:?}: a fling back takes the anchor before"
        );
        assert!(!anchors.is_moving(), "{device:?}: at rest after the settle");
        let (left, _) = painted(&laid);
        assert!(
            near(left, 120.0),
            "{device:?}: the child is painted at its offset, at {left}"
        );
    }
}

/// Offsets run toward the reading end: under right-to-left text a drag to
/// the left opens the row and the child moves left (A5).
pub(crate) fn anchored_drag_reads_offsets_toward_the_reading_end() {
    for device in DEVICES {
        for direction in [TextDirection::Ltr, TextDirection::Rtl] {
            let vsync = Vsync::new();
            let anchors = controller(three());
            let view = Directionality::new(
                direction,
                drag_view_in(&anchors, Axis::Horizontal, direction),
            );
            let mut laid = mount(view, &vsync);
            let (from, to) = match direction {
                TextDirection::Rtl => (at(200.0, 150.0), at(120.0, 150.0)),
                TextDirection::Ltr => (at(100.0, 150.0), at(180.0, 150.0)),
            };
            laid.replay(&slow(device, from, to));
            settle(&mut laid);
            assert_eq!(
                (anchors.offset(), anchors.current()),
                (120.0, Stop::Open),
                "{device:?} {direction:?}: the drag toward the reading end opens"
            );
            let (left, _) = painted(&laid);
            let expected = if direction == TextDirection::Rtl {
                -120.0
            } else {
                120.0
            };
            assert!(
                near(left, expected),
                "{device:?} {direction:?}: painted at {left}, not {expected}"
            );
            // Toward the reading start there is nothing below Closed.
            laid.replay(&slow(device, to, from));
            laid.replay(&slow(device, to, from));
            settle(&mut laid);
            assert_eq!(
                anchors.offset(),
                0.0,
                "{device:?} {direction:?}: the lowest anchor bounds the drag"
            );
        }
    }
}

/// A set without anchors, with a non-finite, out-of-range or coincident
/// offset, or a repeated key is refused; a single anchor holds the child
/// still and hands the whole drag to the scrollable around it.
pub(crate) fn anchored_drag_refuses_degenerate_anchor_sets() {
    let built = |entries: &[(Stop, f64)]| {
        entries
            .iter()
            .fold(DraggableAnchors::builder(), |builder, &(key, offset)| {
                builder.at(key, offset)
            })
            .build()
            .map(|_| ())
    };
    const MAX: f64 = DraggableAnchors::<Stop>::MAX_OFFSET;
    let rows: [AnchorRow; 9] = [
        ("empty", &[], Err(AnchorsError::Empty)),
        (
            "NaN",
            &[(Stop::Closed, 0.0), (Stop::Open, f64::NAN)],
            Err(AnchorsError::NonFinite { index: 1 }),
        ),
        (
            "infinite",
            &[(Stop::Closed, f64::NEG_INFINITY)],
            Err(AnchorsError::NonFinite { index: 0 }),
        ),
        (
            "past the admitted range",
            &[(Stop::Closed, 0.0), (Stop::Open, MAX * 2.0)],
            Err(AnchorsError::OutOfRange { index: 1 }),
        ),
        (
            "at the admitted range",
            &[(Stop::Closed, -MAX), (Stop::Open, MAX)],
            Ok(()),
        ),
        (
            "a repeated key",
            &[(Stop::Closed, 0.0), (Stop::Closed, 80.0)],
            Err(AnchorsError::Duplicate { index: 1 }),
        ),
        (
            "a repeated offset",
            &[(Stop::Closed, 80.0), (Stop::Open, 80.0)],
            Err(AnchorsError::Coincident { index: 1 }),
        ),
        (
            "negative zero is zero",
            &[(Stop::Closed, 0.0), (Stop::Open, -0.0)],
            Err(AnchorsError::Coincident { index: 1 }),
        ),
        ("one anchor", &[(Stop::Closed, 0.0)], Ok(())),
    ];
    for (name, entries, expected) in rows {
        assert_eq!(built(entries), expected, "{name}");
    }
    let one = stops(&[(Stop::Open, 40.0)]);
    assert_eq!(
        AnchoredDragController::new(one.clone(), Stop::Closed).unwrap_err(),
        AnchorsError::Missing,
        "the initial key must be an anchor"
    );
    assert_eq!(one.closest(f64::NAN), Stop::Open, "NaN finds the lowest");
    let anchors = controller(three());
    assert_eq!(anchors.jump_to(Stop::Closed), Ok(()), "a known key jumps");
    assert_eq!(
        anchors.animate_to(
            Stop::Open,
            Duration::ZERO,
            flui_animation::ArcCurve::new(flui_animation::curves::LINEAR)
        ),
        Ok(()),
    );
    assert_eq!(anchors.offset(), 120.0, "a zero-length animation jumps");

    for device in DEVICES {
        let paged = |page: BoxedView| {
            let (vsync, pager) = (Vsync::new(), ScrollController::new());
            let pages = flui_widgets::Row::new(vec![page, SizedBox::new(300.0, 300.0).boxed()]);
            let view = Scrollable::new()
                .controller(pager.clone())
                .scroll_direction(Axis::Horizontal)
                .child(pages);
            let mut laid = mount(view, &vsync);
            laid.replay(&slow(device, at(250.0, 150.0), at(150.0, 150.0)));
            settle(&mut laid);
            pager.pixels()
        };
        let held = AnchoredDragController::new(one.clone(), Stop::Open).expect("an anchor");
        let moved = paged(drag_view(&held, Axis::Horizontal).boxed());
        // A scrollable with no room is the reference: the drag goes up the
        // chain from where the inner contact won it.
        let full = Scrollable::new()
            .scroll_direction(Axis::Horizontal)
            .child(SizedBox::new(300.0, 300.0));
        let reference = paged(SizedBox::new(300.0, 300.0).child(full).boxed());
        assert_eq!(held.offset(), 40.0, "{device:?}: one anchor never moves");
        assert!(
            reference > 50.0 && moved == reference,
            "{device:?}: the pager took the whole drag: {moved}, as with a full list {reference}"
        );
    }
}

/// A listener that replaces the anchors in the middle of the drag, or of
/// the settle, is honoured at once: the drag goes on within the new
/// extents and settles on the new anchors.
pub(crate) fn anchored_drag_follows_anchors_replaced_mid_drag() {
    for device in DEVICES {
        let vsync = Vsync::new();
        let anchors = controller(stops(&[(Stop::Closed, 0.0), (Stop::Open, 120.0)]));
        let replaced = Rc::new(Cell::new(0));
        let listener = {
            let anchors = anchors.clone();
            let replaced = Rc::clone(&replaced);
            Rc::new(move || {
                if anchors.current() == Stop::Open && replaced.get() == 0 {
                    replaced.set(1);
                    anchors.set_anchors(three());
                    anchors.set_anchors(stops(&[
                        (Stop::Closed, 0.0),
                        (Stop::Open, 120.0),
                        (Stop::Far, 270.0),
                    ]));
                }
            })
        };
        let _id = anchors.listenable().add_listener(listener);
        let mut laid = mount(drag_view(&anchors, Axis::Horizontal), &vsync);
        laid.replay(&slow(device, at(10.0, 150.0), at(290.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            replaced.get(),
            1,
            "{device:?}: crossing Open replaced the anchors"
        );
        assert_eq!(
            (anchors.offset(), anchors.current()),
            (270.0, Stop::Far),
            "{device:?}: the drag went past the old extent and settled on the new anchor"
        );

        // A settle whose target the listener removes lands on the new set.
        let settling = Rc::new(Cell::new(false));
        let _id = {
            let anchors = anchors.clone();
            let settling = Rc::clone(&settling);
            anchors.clone().listenable().add_listener(Rc::new(move || {
                if anchors.current() == Stop::Open && !settling.replace(true) {
                    anchors.set_anchors(stops(&[(Stop::Closed, 0.0), (Stop::Far, 200.0)]));
                }
            }))
        };
        // On the square's last 30 px still in the window.
        laid.replay(&fling(device, at(290.0, 150.0), at(260.0, 150.0)));
        settle(&mut laid);
        assert!(settling.get(), "{device:?}: the settle crossed Open");
        let rest = anchors.offset();
        assert!(
            (rest == 0.0 || rest == 200.0) && !anchors.is_moving(),
            "{device:?}: it rests on an anchor of the new set, at {rest}"
        );
        assert_eq!(
            anchors.anchors().offset(anchors.current()),
            Some(rest),
            "{device:?}: and that anchor is current"
        );
    }
}

/// Unmounting in the middle of a drag leaves the child on the nearest
/// anchor; the controller drives the next drag that shows it.
pub(crate) fn anchored_drag_unmount_mid_drag_rests_on_the_nearest_anchor() {
    let vsync = Vsync::new();
    let anchors = controller(three());
    let mut laid = mount(drag_view(&anchors, Axis::Horizontal), &vsync);
    laid.dispatch_pointer_down(10.0, 150.0);
    for step in 1..=10 {
        laid.dispatch_pointer_move_after(
            10.0 + f64::from(step) * 10.0,
            150.0,
            Duration::from_millis(16),
        );
    }
    assert!(anchors.is_moving(), "the drag holds the child");
    assert!(
        anchors.offset() > 60.0,
        "the drag moved it to {}",
        anchors.offset()
    );
    laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::new(300.0, 300.0)));
    laid.dispatch_pointer_move_after(200.0, 150.0, Duration::from_millis(16));
    laid.dispatch_pointer_up(200.0, 150.0);
    settle(&mut laid);
    assert_eq!(
        (anchors.offset(), anchors.current(), anchors.is_moving()),
        (120.0, Stop::Open, false),
        "the unmounted drag rests on its nearest anchor"
    );
    laid.pump_widget(VsyncScope::new(
        vsync,
        drag_view(&anchors, Axis::Horizontal),
    ));
    laid.tick();
    laid.replay(&slow(Device::Touch, at(150.0, 150.0), at(230.0, 150.0)));
    settle(&mut laid);
    assert_eq!(anchors.current(), Stop::Far, "a remounted drag moves on");
}

/// Assistive technology steps to the next and the previous anchor through
/// the channel, and a focused drag moves the way an arrow key points, in
/// either reading direction (A1's direct path).
pub(crate) fn anchored_drag_assistive_and_key_steps_move_between_anchors() {
    use flui_testing::a11y::{Action, ActionRequest, TreeId};
    let (vsync, probe) = (Vsync::new(), Probe::new());
    let anchors = controller(three());
    let view = NestedScroll::new(probe.connection(), drag_view(&anchors, Axis::Horizontal));
    let mut laid = mount(view, &vsync);
    laid.enable_semantics();
    laid.tick();
    let act = |laid: &mut LaidOut, action: Action| {
        let node = laid
            .a11y_tree()
            .expect("published semantics")
            .nodes()
            .find(|node| node.supports_action(action))
            .map(|node| node.id());
        if let Some(node) = node {
            laid.invoke_semantics_action(ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: node,
                data: None,
            })
            .expect("the request resolves");
        }
        settle(laid);
        node.is_some()
    };
    assert!(!act(&mut laid, Action::Decrement), "nothing below Closed");
    assert!(act(&mut laid, Action::Increment));
    assert_eq!(
        anchors.current(),
        Stop::Open,
        "increment takes the next anchor"
    );
    assert!(act(&mut laid, Action::Increment));
    assert_eq!(anchors.offset(), 240.0);
    assert!(!act(&mut laid, Action::Increment), "nothing above Far");
    assert!(act(&mut laid, Action::Decrement));
    assert_eq!(
        anchors.current(),
        Stop::Open,
        "decrement takes the one before"
    );
    let steps = probe.steps();
    assert!(
        !steps.is_empty()
            && steps.iter().all(|step| step.cause == ScrollCause::UserInput
                && step.device == ScrollDevice::Assistive),
        "assistive steps go through the channel: {steps:?}"
    );

    for direction in [TextDirection::Ltr, TextDirection::Rtl] {
        let vsync = Vsync::new();
        let anchors = controller(three());
        let view = Directionality::new(
            direction,
            drag_view_in(&anchors, Axis::Horizontal, direction),
        );
        let mut laid = mount(view, &vsync);
        let right = Key::Named(NamedKey::ArrowRight);
        let left = Key::Named(NamedKey::ArrowLeft);
        // With nothing focused the arrow can only bring the focus in; it
        // steps no drag.
        let _ = Keys::of(&laid).down(Code::ArrowRight, right.clone());
        Keys::of(&laid).up(Code::ArrowRight, right.clone());
        settle(&mut laid);
        assert_eq!(
            anchors.current(),
            Stop::Closed,
            "{direction:?}: nothing had the focus"
        );
        laid.focus_manager().unfocus();
        // A drag that returns home focuses the drag.
        laid.replay(&slow(Device::Mouse, at(150.0, 150.0), at(160.0, 150.0)));
        settle(&mut laid);
        let (open, close) = match direction {
            TextDirection::Rtl => ((Code::ArrowLeft, left), (Code::ArrowRight, right)),
            TextDirection::Ltr => ((Code::ArrowRight, right), (Code::ArrowLeft, left)),
        };
        let keys = Keys::of(&laid);
        assert!(keys.down(open.0, open.1.clone()), "{direction:?}: handled");
        keys.up(open.0, open.1.clone());
        settle(&mut laid);
        assert_eq!(
            anchors.current(),
            Stop::Open,
            "{direction:?}: toward the reading end"
        );
        assert!(keys.down(close.0, close.1.clone()));
        keys.up(close.0, close.1.clone());
        settle(&mut laid);
        assert_eq!(anchors.current(), Stop::Closed, "{direction:?}: and back");
        // A key the drag cannot act on bubbles past it: nothing handles it,
        // or the focus traversal around the drag takes the focus elsewhere
        // (to its alternatives' controls); the drag stays where it is.
        let bubbles = |laid: &mut LaidOut, code: Code, key: Key| {
            let focused = || laid.focus_manager().primary_focus().map(|node| node.id());
            let before = focused();
            let handled = keys.down(code, key.clone());
            keys.up(code, key);
            let moved_focus = focused() != before;
            settle(laid);
            (!handled || moved_focus) && anchors.current() == Stop::Closed
        };
        assert!(
            bubbles(&mut laid, Code::ArrowDown, Key::Named(NamedKey::ArrowDown)),
            "{direction:?}: keys across the axis bubble"
        );
        // The drag takes the focus again from a drag that returns home.
        laid.replay(&slow(Device::Mouse, at(150.0, 150.0), at(160.0, 150.0)));
        settle(&mut laid);
        assert!(
            bubbles(&mut laid, close.0, close.1.clone()),
            "{direction:?}: at the lowest anchor the key bubbles"
        );
    }
}

/// Every arrival at another anchor, by a drag or its settle, plays one
/// selection haptic with the frame that shows it; settling back where a
/// drag already arrived and programmatic moves play none (ADR-0210).
pub(crate) fn anchored_drag_plays_a_haptic_on_each_new_anchor() {
    for device in DEVICES {
        let vsync = Vsync::new();
        let anchors = controller(three());
        let mut laid = mount(drag_view(&anchors, Axis::Horizontal), &vsync);
        let played = |laid: &LaidOut| {
            laid.window()
                .haptic_actuator()
                .feedback()
                .iter()
                .filter(|kind| **kind == HapticFeedback::SelectionClick)
                .count()
        };
        laid.replay(&slow(device, at(100.0, 150.0), at(150.0, 150.0)));
        settle(&mut laid);
        assert_eq!(played(&laid), 0, "{device:?}: back home, no new anchor");
        laid.replay(&slow(device, at(100.0, 150.0), at(260.0, 150.0)));
        settle(&mut laid);
        assert_eq!(anchors.current(), Stop::Open);
        assert_eq!(
            played(&laid),
            1,
            "{device:?}: crossing Open plays once, settling back onto it no more"
        );
        laid.replay(&fling(device, at(200.0, 150.0), at(230.0, 150.0)));
        settle(&mut laid);
        assert_eq!(anchors.current(), Stop::Far);
        assert_eq!(played(&laid), 2, "{device:?}: the settle onto Far plays");
        anchors.jump_to(Stop::Closed).expect("an anchor");
        settle(&mut laid);
        assert_eq!(anchors.current(), Stop::Closed);
        assert_eq!(
            played(&laid),
            2,
            "{device:?}: a programmatic move plays none"
        );
    }
}

/// As a leaf the drag takes its axis first and offers the rest up its
/// chain; as an ancestor it takes what a list inside leaves at its edge and
/// the list's release settles it (ADR-0209 §9, the ownership table's
/// anchored rows).
pub(crate) fn anchored_drag_is_a_bounded_leaf_and_a_participant() {
    for device in DEVICES {
        let (vsync, probe) = (Vsync::new(), Probe::new());
        let anchors = controller(stops(&[(Stop::Closed, 0.0), (Stop::Open, 120.0)]));
        let view = NestedScroll::new(probe.connection(), drag_view(&anchors, Axis::Horizontal));
        let mut laid = mount(view, &vsync);
        let remainder = |probe: &Probe| -> f64 {
            probe
                .take()
                .iter()
                .filter_map(|seen| match seen {
                    crate::nested_scroll::Seen::Post(step) => Some(step.available.x),
                    _ => None,
                })
                .sum()
        };
        laid.replay(&slow(device, at(10.0, 150.0), at(210.0, 150.0)));
        settle(&mut laid);
        assert_eq!(anchors.offset(), 120.0, "{device:?}");
        let offered: f64 = probe
            .steps()
            .iter()
            .filter(|step| step.cause == ScrollCause::UserInput)
            .map(|step| step.available.x)
            .sum();
        assert!(
            offered < -120.0,
            "{device:?}: every user step of the drag went through the chain: {offered}"
        );
        assert_eq!(
            remainder(&probe),
            0.0,
            "{device:?}: the drag that moved the leaf keeps its latch past the extent"
        );
        laid.replay(&slow(device, at(130.0, 150.0), at(230.0, 150.0)));
        settle(&mut laid);
        let past = remainder(&probe);
        assert!(
            past < -50.0 && anchors.offset() == 120.0,
            "{device:?}: from the extent the next drag goes up the chain: {past}"
        );

        // A swipe row in a vertical list: each drag goes to the member of
        // its axis.
        let vsync = Vsync::new();
        let row = controller(stops(&[(Stop::Closed, 0.0), (Stop::Open, 120.0)]));
        let list = ScrollController::new();
        let rows = flui_widgets::Column::new(vec![
            SizedBox::new(300.0, 100.0)
                .child(drag_view(&row, Axis::Horizontal))
                .boxed(),
            SizedBox::new(300.0, 1000.0).boxed(),
        ]);
        let view = Scrollable::new().controller(list.clone()).child(rows);
        let mut laid = mount(view, &vsync);
        laid.replay(&slow(device, at(100.0, 50.0), at(200.0, 50.0)));
        settle(&mut laid);
        assert_eq!(
            (row.current(), list.pixels()),
            (Stop::Open, 0.0),
            "{device:?}: a horizontal drag swipes the row"
        );
        laid.replay(&slow(device, at(100.0, 80.0), at(100.0, 10.0)));
        settle(&mut laid);
        assert!(
            row.offset() == 120.0 && list.pixels() > 20.0,
            "{device:?}: a vertical drag on the row scrolls the list, at {}",
            list.pixels()
        );

        // A sheet with a list: the list at its top hands the drag down to
        // the sheet, and the list's release settles the sheet.
        let vsync = Vsync::new();
        let sheet = controller(stops(&[(Stop::Closed, 0.0), (Stop::Open, 200.0)]));
        let list = ScrollController::new();
        let content = Scrollable::new()
            .controller(list.clone())
            .child(SizedBox::new(300.0, 1000.0));
        let view = AnchoredDrag::new(
            sheet.clone(),
            Axis::Vertical,
            content,
            alternatives_in(TextDirection::Ltr),
        );
        let mut laid = mount(view, &vsync);
        laid.replay(&slow(device, at(150.0, 20.0), at(150.0, 150.0)));
        settle(&mut laid);
        assert_eq!(list.pixels(), 0.0, "{device:?}: the list stays at its top");
        assert_eq!(
            (sheet.offset(), sheet.current(), sheet.is_moving()),
            (200.0, Stop::Open, false),
            "{device:?}: the sheet took the drag and settled past halfway"
        );
        sheet.jump_to(Stop::Closed).expect("an anchor");
        laid.replay(&fling(device, at(150.0, 20.0), at(150.0, 70.0)));
        settle(&mut laid);
        assert_eq!(
            (list.pixels(), sheet.current()),
            (0.0, Stop::Open),
            "{device:?}: the list's release flings the sheet on to the next anchor"
        );
        if device == Device::Touch {
            sheet.jump_to(Stop::Closed).expect("an anchor");
            let mut script = flui_testing::TouchScript::new("cancelled").down(1, at(150.0, 20.0));
            for point in crate::nested_scroll::line(at(150.0, 20.0), at(150.0, 110.0), 6) {
                script = script.wait(Duration::from_millis(16)).move_to(1, point);
            }
            let script = script.cancel(1).build().expect("one whole contact");
            laid.replay(&script);
            settle(&mut laid);
            let rest = sheet.offset();
            assert!(
                (rest == 0.0 || rest == 200.0) && !sheet.is_moving(),
                "a cancelled list drag settles the sheet on an anchor, at {rest}"
            );
        }
        sheet.jump_to(Stop::Closed).expect("an anchor");
        list.jump_to(300.0);
        laid.replay(&slow(device, at(150.0, 20.0), at(150.0, 150.0)));
        settle(&mut laid);
        let scrolled = list.pixels();
        assert!(
            sheet.offset() == 0.0 && scrolled < 300.0 && scrolled > 100.0,
            "{device:?}: a list away from its top keeps the drag: the sheet at {}, the list at {scrolled}",
            sheet.offset()
        );
    }

    // A wheel does not drag to an anchor: it scrolls what encloses the drag.
    let vsync = Vsync::new();
    let sheet = controller(stops(&[(Stop::Closed, 0.0), (Stop::Open, 200.0)]));
    let outer = ScrollController::new();
    let content = flui_widgets::Column::new(vec![
        SizedBox::new(300.0, 300.0)
            .child(drag_view(&sheet, Axis::Vertical))
            .boxed(),
        SizedBox::new(300.0, 1000.0).boxed(),
    ]);
    let mut laid = mount(
        Scrollable::new().controller(outer.clone()).child(content),
        &vsync,
    );
    outer.jump_to(100.0);
    laid.tick();
    // A notch up over the sheet: room for the list to go back, and for the
    // sheet to move down, were it a scroll view.
    let wheel = flui_testing::MouseScript::new("wheel")
        .move_to(at(150.0, 100.0))
        .wheel(-1.0)
        .wait(Duration::from_millis(300))
        .frame()
        .build()
        .expect("a wheel notch");
    laid.replay(&wheel);
    settle(&mut laid);
    assert!(
        sheet.offset() == 0.0 && outer.pixels() < 100.0,
        "the wheel scrolled the list, not the sheet: sheet {}, list {}",
        sheet.offset(),
        outer.pixels()
    );
}

/// A drag and drop session hovering near the drag's edges scrolls nothing:
/// an anchored drag is no autoscroll target (ADR-0211 §8).
pub(crate) fn anchored_drag_is_no_drop_autoscroll_target() {
    use std::sync::Arc;

    use flui_foundation::geometry::Point;
    use flui_foundation::{PresentationId, UiRuntimeId};
    use flui_interaction::drag::{DragOwner, ProcessNonce};
    use flui_platform_api::DragDropEvent;
    use flui_platform_api::EventTime;
    use flui_platform_api::data_transfer::{
        DataTransferOffer, OfferRecord, OfferTable, RepresentationDescriptor, TransferActions,
        TransferFormat,
    };
    use flui_platform_api::pointer::PointerKind;

    let vsync = Vsync::new();
    let sheet = controller(stops(&[(Stop::Closed, 0.0), (Stop::Open, 200.0)]));
    let laid = mount(drag_view(&sheet, Axis::Vertical), &vsync);
    let owner = DragOwner::new(
        ProcessNonce::from_raw(std::num::NonZeroU64::new(7).expect("nonzero")),
        UiRuntimeId::new(7),
        Arc::new(flui_platform_api::NullDataTransferSource),
    );
    let representations: Arc<[RepresentationDescriptor]> =
        Arc::from(vec![RepresentationDescriptor {
            format: TransferFormat::Text,
            declared_len: None,
        }]);
    let mut offers = OfferTable::new();
    let id = offers.mint(OfferRecord::new(Arc::clone(&representations)));
    let offer = DataTransferOffer::new(id, representations);
    let window = PresentationId::new(1);
    let time = |ms: u64| EventTime::from_nanos(1_000_000_000 + ms * 1_000_000);
    let path_at = |y: f64| {
        laid.hit_test_pointer(at(150.0, y), PointerKind::Mouse)
            .path()
            .to_vec()
    };
    let (top, bottom) = (path_at(10.0), path_at(290.0));
    laid.enter_owner_scope(|| {
        owner.deliver(
            window,
            DragDropEvent::Entered {
                offer,
                allowed: TransferActions::COPY,
                position: Some(Point::new(150.0, 10.0)),
            },
            &mut |_, _| Some(top.clone()),
        );
        owner.tick(time(0));
        owner.tick(time(100));
    });
    assert_eq!(
        sheet.offset(),
        0.0,
        "hovering at the top edge moved nothing"
    );
    laid.enter_owner_scope(|| {
        owner.deliver(
            window,
            DragDropEvent::Moved {
                id,
                allowed: TransferActions::COPY,
                position: Point::new(150.0, 290.0),
            },
            &mut |_, _| Some(bottom.clone()),
        );
        owner.tick(time(200));
        owner.tick(time(300));
    });
    assert_eq!(
        (sheet.offset(), sheet.current()),
        (0.0, Stop::Closed),
        "hovering at either edge left the drag where it rests"
    );
    laid.enter_owner_scope(|| {
        owner.deliver(window, DragDropEvent::Exited { id }, &mut |_, _| None);
    });
}

/// The drag is a stop of focus traversal, named or not: Tab reaches the row
/// of its gesture alternatives, where the arrow keys move it. Named, its
/// slider reads its label and its current anchor's description; unnamed, the
/// slider has neither.
pub(crate) fn anchored_drag_is_a_tab_stop_and_a_named_slider() {
    use flui_testing::a11y::Role;
    let tab = || Key::Named(NamedKey::Tab);
    let right = Key::Named(NamedKey::ArrowRight);
    for named in [true, false] {
        let vsync = Vsync::new();
        let anchors = controller(three());
        let view = drag_view(&anchors, Axis::Horizontal);
        let view = if named {
            view.semantics("Drawer", |stop| format!("{stop:?}"))
        } else {
            view
        };
        let mut laid = mount(view, &vsync);
        laid.enable_semantics();
        laid.tick();
        let slider = |laid: &LaidOut| {
            let tree = laid.a11y_tree().expect("published semantics");
            tree.nodes()
                .find(|node| node.role() == Role::Slider)
                .map(|node| {
                    (
                        node.label().map(str::to_owned),
                        node.value().map(str::to_owned),
                    )
                })
        };
        let keys = Keys::of(&laid);
        keys.down(Code::Tab, tab());
        keys.up(Code::Tab, tab());
        laid.tick();
        let moved = keys.down(Code::ArrowRight, right.clone());
        keys.up(Code::ArrowRight, right.clone());
        settle(&mut laid);
        assert!(moved, "named {named}: Tab focused the drag's row");
        assert_eq!(
            anchors.current(),
            Stop::Open,
            "named {named}: and the arrow moved it"
        );
        let expected = if named {
            Some((Some("Drawer".to_owned()), Some("Open".to_owned())))
        } else {
            Some((None, None))
        };
        assert_eq!(
            slider(&laid),
            expected,
            "named {named}: the slider reads its label and its anchor"
        );
    }
}

/// The offset's listenable hears every move as it happens: each step of a
/// drag before any frame, and a programmatic move of a drag no longer shown.
pub(crate) fn anchored_drag_offset_listenable_follows_every_move() {
    let vsync = Vsync::new();
    let anchors = controller(three());
    let heard = Rc::new(Cell::new(0_u32));
    let _id = {
        let heard = Rc::clone(&heard);
        anchors
            .offset_listenable()
            .add_listener(Rc::new(move || heard.set(heard.get() + 1)))
    };
    let mut laid = mount(drag_view(&anchors, Axis::Horizontal), &vsync);
    laid.dispatch_pointer_down(100.0, 150.0);
    laid.dispatch_pointer_move_after(130.0, 150.0, Duration::from_millis(16));
    let claimed = heard.get();
    for step in 1..=5 {
        laid.dispatch_pointer_move_after(
            130.0 + f64::from(step) * 10.0,
            150.0,
            Duration::from_millis(16),
        );
        assert_eq!(
            heard.get(),
            claimed + step,
            "each step of the drag is heard as it moves, before a frame"
        );
    }
    laid.dispatch_pointer_up(180.0, 150.0);
    settle(&mut laid);
    laid.pump_widget(VsyncScope::new(vsync, SizedBox::new(300.0, 300.0)));
    let before = heard.get();
    anchors.jump_to(Stop::Far).expect("an anchor");
    assert_eq!(
        heard.get(),
        before + 1,
        "a jump of an unmounted drag is heard"
    );
    anchors.jump_to(Stop::Far).expect("an anchor");
    assert_eq!(heard.get(), before + 1, "a jump that moves nothing is not");
}
