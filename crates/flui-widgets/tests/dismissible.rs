//! Accepted cancellation restores a dragged card instead of dismissing it.
use crate::common::{lay_out_animated, tight};
use flui_animation::Vsync;
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, DismissDirection, Dismissible, GestureDetector, VsyncScope};
use std::cell::Cell;
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
