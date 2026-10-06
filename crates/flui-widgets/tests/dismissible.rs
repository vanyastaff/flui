//! Dismissible release: cancellation restores a dragged card, a fling keeps
//! the finger's speed.
use crate::common::{lay_out_animated, tight};
use flui_animation::Vsync;
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;
use flui_widgets::{
    ColoredBox, DismissDirection, DismissUpdateDetails, Dismissible, GestureDetector, VsyncScope,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

fn cancelled_card(direction: DismissDirection, end: f64) {
    let dismissed = Rc::new(Cell::new(0));
    let taps = Rc::new(Cell::new(0));
    let on_dismiss = Rc::clone(&dismissed);
    let on_tap = Rc::clone(&taps);
    let vsync = Vsync::new();
    let child = GestureDetector::new()
        .on_tap(move |_| on_tap.set(on_tap.get() + 1))
        .child(ColoredBox::new(Color::rgb(10, 20, 30)));
    let card = Dismissible::new(child)
        .direction(direction)
        .resize_duration(None)
        .on_dismissed(move |_, _| on_dismiss.set(on_dismiss.get() + 1));
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), card),
        tight(200.0, 200.0),
        vsync,
    );
    let horizontal = direction == DismissDirection::Horizontal;
    let (x, y) = if horizontal { (end, 20.0) } else { (20.0, end) };
    let card_target = laid.find_by_render_type("RenderDecoratedBox");
    assert!(
        laid.hit_test_pointer(Offset::new(20.0, 20.0))
            .path()
            .iter()
            .any(|entry| entry.target == card_target)
    );
    let (slop_x, slop_y) = if horizontal {
        (40.0, 20.0)
    } else {
        (20.0, 40.0)
    };
    laid.dispatch_pointer_down(20.0, 20.0);
    // The child's tap competes with the drag. The first move wins the arena;
    // Start behavior reanchors there, so a second move must update the card.
    laid.dispatch_pointer_move(slop_x, slop_y);
    laid.dispatch_pointer_move(x, y);
    laid.pump();
    assert!(
        !laid
            .hit_test_pointer(Offset::new(20.0, 20.0))
            .path()
            .iter()
            .any(|entry| entry.target == card_target),
        "the admitted drag visibly moved the card before cancellation"
    );
    if end > 200.0 {
        assert!(
            !laid
                .hit_test_pointer(Offset::new(199.0, 20.0))
                .path()
                .iter()
                .any(|entry| entry.target == card_target),
            "the fully slid horizontal card is outside the viewport"
        );
    }
    laid.dispatch_pointer_cancel();
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        dismissed.get(),
        0,
        "cancel beyond threshold never dismisses"
    );
    // The original hit location becomes reachable again only after the slide
    // returns to zero; suppressing callbacks without restoring motion fails.
    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);
    assert_eq!(
        taps.get(),
        1,
        "cancelled card returned to its original hit location"
    );
    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_move(slop_x, slop_y);
    laid.dispatch_pointer_move(x, y);
    laid.dispatch_pointer_up(x, y);
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(dismissed.get(), 1, "next completed gesture still dismisses");
}

pub(crate) fn a_cancelled_horizontal_dismiss_restores_the_card() {
    cancelled_card(DismissDirection::Horizontal, 150.0);
}

pub(crate) fn a_cancelled_vertical_dismiss_restores_the_card() {
    cancelled_card(DismissDirection::Vertical, 150.0);
}

pub(crate) fn cancelling_a_fully_slid_card_restores_it_without_dismissal() {
    cancelled_card(DismissDirection::Horizontal, 250.0);
}

/// The card's speed, in px/s, just after a release at 1500 px/s on a card
/// `width` px wide.
fn release_speed(width: f64) -> f64 {
    let progress = Rc::new(RefCell::new(Vec::new()));
    let recorder = Rc::clone(&progress);
    let vsync = Vsync::new();
    let card = Dismissible::new(ColoredBox::new(Color::rgb(10, 20, 30)))
        .resize_duration(None)
        .on_update(move |_, details: DismissUpdateDetails| {
            recorder.borrow_mut().push(details.progress);
        });
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), card),
        tight(width, 100.0),
        vsync,
    );
    laid.dispatch_pointer_down(10.0, 50.0);
    // 15 px every 10 ms: 1500 px/s to the right.
    let mut x = 10.0;
    for _ in 0..5 {
        x += 15.0;
        laid.dispatch_pointer_move_after(x, 50.0, Duration::from_millis(10));
    }
    laid.dispatch_pointer_up(x, 50.0);
    laid.pump();
    progress.borrow_mut().clear();
    // A frame anchors the fling, then a 0.1 ms step samples its start: the
    // settle spring's acceleration changes the speed by under 5 % that soon.
    let step = Duration::from_micros(100);
    for _ in 0..3 {
        laid.pump_for(step);
    }
    let samples = progress.borrow();
    let moving: Vec<_> = samples.windows(2).filter(|pair| pair[1] != pair[0]).collect();
    let pair = moving.first().expect("the released card moves");
    (pair[1] - pair[0]) * width / step.as_secs_f64()
}

/// A fling hands the finger's speed to the settle animation whatever the
/// card's width: the gesture's px/s become controller units per second by
/// dividing by the width, not by a fixed scale.
pub(crate) fn a_dismissible_release_keeps_finger_speed_on_any_width() {
    for width in [150.0, 1200.0] {
        let speed = release_speed(width);
        assert!(
            (speed - 1500.0).abs() <= 0.1 * 1500.0,
            "a {width} px card left at {speed} px/s after a 1500 px/s release"
        );
    }
}
