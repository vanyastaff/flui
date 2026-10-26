//! A precision-touchpad pan reaches the same owners a touch drag would: a
//! pan-only `GestureDetector`, or the `Scrollable` the ownership table picks
//! (ADR-0251).

use std::{cell::RefCell, rc::Rc};

use crate::common::{LaidOut, lay_out, tight};
use flui_foundation::geometry::Point;
use flui_platform::{
    EventTime,
    pointer::{
        DeviceId, PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta,
        ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit,
    },
};
use flui_widgets::{DragSlot, GestureDetector, ScrollController, Scrollable, SizedBox};

fn packet(phase: ScrollPhase, dx: f64, dy: f64, nanos: u64) -> PointerEvent {
    let pointer = PointerInfo::new(
        PointerId::try_from(1_u64).expect("pointer"),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(1_u64).expect("device"));
    PointerEvent::Scroll(
        ScrollEvent::new(
            pointer,
            EventTime::from_nanos(nanos),
            PointerPosition::try_new(Point::new(150.0, 150.0)).expect("focal point"),
            ScrollDelta::try_new(ScrollUnit::Pixels, dx, dy).expect("scroll"),
        )
        .with_precision(ScrollPrecision::Precise)
        .with_phase(phase),
    )
}

/// A pan of the given steps (scroll deltas), from `Began` to `Ended`.
fn pan(laid: &LaidOut, steps: &[(f64, f64)]) {
    let mut nanos = 1_u64;
    laid.dispatch_pointer_event(&packet(ScrollPhase::Began, 0.0, 0.0, nanos));
    for (dx, dy) in steps {
        nanos += 8_000_000;
        laid.dispatch_pointer_event(&packet(ScrollPhase::Changed, *dx, *dy, nanos));
    }
    laid.dispatch_pointer_event(&packet(ScrollPhase::Ended, 0.0, 0.0, nanos + 8_000_000));
}

fn list(controller: &ScrollController, height: f64) -> Scrollable {
    Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, height))
}

pub(crate) fn a_touchpad_pan_drives_a_pan_only_gesture_detector() {
    let starts = Rc::new(RefCell::new(0_u32));
    let updates = Rc::new(RefCell::new(0_u32));
    let (started, updated) = (Rc::clone(&starts), Rc::clone(&updates));
    let laid = lay_out(
        GestureDetector::new()
            .behavior(flui_widgets::HitTestBehavior::Opaque)
            .drag(
                DragSlot::pan()
                    .on_start(move |_, _| *started.borrow_mut() += 1)
                    .on_update(move |_, _| *updated.borrow_mut() += 1),
            )
            .child(SizedBox::new(300.0, 300.0)),
        tight(300.0, 300.0),
    );
    pan(&laid, &[(0.0, 40.0), (0.0, 40.0), (0.0, 40.0)]);
    assert_eq!(*starts.borrow(), 1, "the pan began once");
    assert!(*updates.borrow() >= 1, "the pan moved the detector");
}

pub(crate) fn a_scrollable_inside_a_horizontal_drag_wins_a_vertical_pan() {
    let controller = ScrollController::new();
    let drags = Rc::new(RefCell::new(0_u32));
    let started = Rc::clone(&drags);
    let laid = lay_out(
        GestureDetector::new()
            .behavior(flui_widgets::HitTestBehavior::Opaque)
            .drag(DragSlot::horizontal().on_start(move |_, _| *started.borrow_mut() += 1))
            .child(list(&controller, 1000.0)),
        tight(300.0, 300.0),
    );
    pan(&laid, &[(0.0, 40.0), (0.0, 40.0), (0.0, 40.0)]);
    assert!(
        controller.pixels() > 0.0,
        "the list scrolled with the pan: {}",
        controller.pixels()
    );
    assert_eq!(*drags.borrow(), 0, "the horizontal drag did not start");
}

pub(crate) fn a_horizontal_pan_beside_a_vertical_list_goes_to_the_drag() {
    let controller = ScrollController::new();
    let drags = Rc::new(RefCell::new(0_u32));
    let started = Rc::clone(&drags);
    let laid = lay_out(
        GestureDetector::new()
            .behavior(flui_widgets::HitTestBehavior::Opaque)
            .drag(DragSlot::horizontal().on_start(move |_, _| *started.borrow_mut() += 1))
            .child(list(&controller, 1000.0)),
        tight(300.0, 300.0),
    );
    pan(&laid, &[(40.0, 0.0), (40.0, 0.0), (40.0, 0.0)]);
    assert_eq!(*drags.borrow(), 1, "the horizontal drag took the pan");
    assert_eq!(controller.pixels(), 0.0, "the list stayed put");
}

pub(crate) fn a_list_with_nowhere_to_go_leaves_the_pan_to_the_drag() {
    let controller = ScrollController::new();
    let drags = Rc::new(RefCell::new(0_u32));
    let started = Rc::clone(&drags);
    let laid = lay_out(
        GestureDetector::new()
            .behavior(flui_widgets::HitTestBehavior::Opaque)
            .drag(DragSlot::pan().on_start(move |_, _| *started.borrow_mut() += 1))
            .child(list(&controller, 100.0)),
        tight(300.0, 300.0),
    );
    pan(&laid, &[(0.0, 40.0), (0.0, 40.0), (0.0, 40.0)]);
    assert_eq!(*drags.borrow(), 1, "the list sat the contest out");
}

pub(crate) fn a_plain_list_scrolls_with_a_touchpad_pan_as_before() {
    let controller = ScrollController::new();
    let laid = lay_out(list(&controller, 1000.0), tight(300.0, 300.0));
    pan(&laid, &[(0.0, 40.0), (0.0, 40.0)]);
    assert_eq!(controller.pixels(), 80.0);
}
