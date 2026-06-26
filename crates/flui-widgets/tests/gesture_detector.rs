//! End-to-end gesture recognition: a `GestureDetector`'s `on_tap` fires when the
//! user taps (pointer down then up) its child. Drives the real recognizer +
//! per-detector arena through the hit-test + dispatch path (the detector closes
//! the arena itself, so no global GestureBinding is needed).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector, SizedBox};

#[test]
fn gesture_detector_fires_on_tap_for_a_down_up_on_the_child() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            // A hit-testable child so the DeferToChild Listener registers.
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(taps.load(Ordering::SeqCst), 0, "no tap before any pointer");

    // A tap = down then up at the same place.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on the child fires on_tap exactly once",
    );
}

#[test]
fn gesture_detector_does_not_fire_without_a_hittable_target() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    // DeferToChild over a childless SizedBox (hit-tests false) → nothing is hit,
    // so no pointer reaches the recognizer.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "no tap when nothing under the detector is hit",
    );
}

#[test]
fn gesture_detector_does_not_fire_when_the_pointer_moves_past_slop() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // Down, then a move well past the (mouse) touch slop, then up: the tap is
    // cancelled by the drag, so on_tap must NOT fire.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_move(50.0, 90.0);
    laid.dispatch_pointer_up(50.0, 90.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a pointer that drags past slop does not tap",
    );
}

#[test]
fn gesture_detector_cancel_aborts_the_tap_without_wedging_the_detector() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // A cancelled contact must NOT tap...
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_cancel(50.0, 50.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a cancelled contact does not tap"
    );

    // ...and must not leave the recognizer wedged: a fresh tap still works.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap after a cancel still fires (the cancel swept the arena entry)",
    );
}
