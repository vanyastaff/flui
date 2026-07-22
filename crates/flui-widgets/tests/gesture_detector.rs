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

/// Flutter parity (tag `3.44.0`): `packages/flutter/lib/src/gestures/arena.dart`
/// `GestureArenaManager` — "The first member to accept or the last member to
/// not reject wins" (line 110). A drag past the slop makes the tap recognizer
/// reject itself, leaving the pan recognizer as the last remaining (and thus
/// winning) member. The upstream Flutter case asserts exactly this
/// arena-elimination behavior, and this test already covers it end to end, so
/// the citation lives here instead of duplicating the case in the parity
/// corpus.
#[test]
fn gesture_detector_recognizes_a_pan_and_suppresses_the_tap() {
    let taps = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let updates = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let (tap_cb, start_cb, update_cb, end_cb) = (
        Arc::clone(&taps),
        Arc::clone(&starts),
        Arc::clone(&updates),
        Arc::clone(&ends),
    );

    // The detector wants BOTH a tap and a pan; the arena must hand a real drag
    // to the pan recognizer and cancel the tap.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_start(move |_details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_update(move |_details| {
                update_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_end(move |_details| {
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // Down, then a move well past the pan slop (60px > 18px) that crosses into a
    // drag, a second move, then up. Every position stays inside the 100×100
    // child so the headless harness (which re-hit-tests each event — no pointer
    // capture) keeps routing to the detector.
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.dispatch_pointer_move(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 90.0);
    laid.dispatch_pointer_up(50.0, 90.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "the drag started exactly once"
    );
    assert!(
        updates.load(Ordering::SeqCst) >= 1,
        "the drag reported at least one update as the pointer moved",
    );
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "the drag ended exactly once on up"
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a drag past the slop cancels the competing tap — they are mutually exclusive",
    );
}

/// Flutter parity (tag `3.44.0`): `packages/flutter/lib/src/gestures/arena.dart`
/// `GestureArenaManager` (line 110, see the citation on
/// `gesture_detector_recognizes_a_pan_and_suppresses_the_tap` above) — with
/// no movement, the tap recognizer is the arena's front (first-added, and
/// here only remaining) member on sweep, so it wins without waiting for the
/// pan recognizer to reject itself.
#[test]
fn gesture_detector_quick_tap_beats_the_pan_recognizer() {
    let taps = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (tap_cb, start_cb) = (Arc::clone(&taps), Arc::clone(&starts));

    // Same dual-gesture detector, but a quick down→up with no movement: the tap
    // is the arena's front member and wins; the pan never starts.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_start(move |_details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a quick down+up fires the tap"
    );
    assert_eq!(
        starts.load(Ordering::SeqCst),
        0,
        "no movement means the pan never starts",
    );
}

#[test]
fn secondary_tap_fires_on_secondary_down_up() {
    let primary_taps = Arc::new(AtomicUsize::new(0));
    let secondary_taps = Arc::new(AtomicUsize::new(0));
    let (primary_cb, secondary_cb) = (Arc::clone(&primary_taps), Arc::clone(&secondary_taps));

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                primary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_secondary_tap(move || {
                secondary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(
        secondary_taps.load(Ordering::SeqCst),
        0,
        "no tap before any pointer"
    );

    // A secondary-button (right-click) down + up fires on_secondary_tap.
    laid.dispatch_secondary_down(50.0, 50.0);
    laid.dispatch_secondary_up(50.0, 50.0);

    assert_eq!(
        secondary_taps.load(Ordering::SeqCst),
        1,
        "a secondary down+up fires on_secondary_tap exactly once",
    );
    assert_eq!(
        primary_taps.load(Ordering::SeqCst),
        0,
        "a secondary tap must NOT fire on_tap",
    );
}

/// Flutter parity (tag `3.44.0`): `widgets/gesture_detector.dart`'s
/// `onHorizontalDrag*` family — an axis-constrained recognizer distinct from
/// `onPan*`, exercised end to end (down/start/update/end) here for the first
/// time in this crate. `DrawerController`'s own `_handleDragDown`/`_move`/
/// `_settle` (`material/drawer.dart`) is the parity seam this family exists
/// for.
///
/// Red-check: swap `DragAxis::Horizontal` for `DragAxis::Vertical` in
/// `GestureDetectorState::init_state`'s `horizontal_drag` recognizer — this
/// test's horizontal move no longer crosses the (now-vertical) slop, and
/// `starts`/`updates`/`ends` all stay `0`.
#[test]
fn horizontal_drag_fires_down_start_update_end_for_horizontal_motion() {
    let downs = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let updates = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let (down_cb, start_cb, update_cb, end_cb) = (
        Arc::clone(&downs),
        Arc::clone(&starts),
        Arc::clone(&updates),
        Arc::clone(&ends),
    );

    let laid = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_down(move |_details| {
                down_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_start(move |_details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_update(move |_details| {
                update_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_end(move |_details| {
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    assert_eq!(
        downs.load(Ordering::SeqCst),
        0,
        "no down before any pointer"
    );

    // Down, then a horizontal move well past the drag slop (60px > 18px),
    // a second move, then up.
    laid.dispatch_pointer_down(20.0, 100.0);
    assert_eq!(
        downs.load(Ordering::SeqCst),
        1,
        "on_horizontal_drag_down fires immediately on contact",
    );

    laid.dispatch_pointer_move(80.0, 100.0);
    laid.dispatch_pointer_move(150.0, 100.0);
    laid.dispatch_pointer_up(150.0, 100.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "the horizontal drag started exactly once"
    );
    assert!(
        updates.load(Ordering::SeqCst) >= 1,
        "the horizontal drag reported at least one update",
    );
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "the horizontal drag ended exactly once on up"
    );
}

/// A purely vertical move must not cross the horizontal recognizer's slop —
/// `DragGestureRecognizer::calculate_primary_delta` projects onto the
/// horizontal axis only (`crates/flui-interaction/src/recognizers/drag.rs`).
///
/// Red-check: change the recognizer's axis to `DragAxis::Free` — a vertical
/// move now crosses its (any-direction) slop and `starts` becomes `1`.
#[test]
fn horizontal_drag_does_not_fire_for_purely_vertical_motion() {
    let starts = Arc::new(AtomicUsize::new(0));
    let start_cb = Arc::clone(&starts);

    let laid = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_start(move |_details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    laid.dispatch_pointer_down(100.0, 20.0);
    laid.dispatch_pointer_move(100.0, 90.0);
    laid.dispatch_pointer_up(100.0, 90.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        0,
        "a purely vertical move must not start a horizontal drag",
    );
}

/// Flutter parity: a cancelled contact (`onHorizontalDragCancel`) must not
/// leave the recognizer wedged — the next contact still drags normally.
/// Mirrors `gesture_detector_cancel_aborts_the_tap_without_wedging_the_detector`
/// for the horizontal-drag family.
#[test]
fn horizontal_drag_cancel_fires_and_does_not_wedge_the_detector() {
    let cancels = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (cancel_cb, start_cb) = (Arc::clone(&cancels), Arc::clone(&starts));

    let laid = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_start(move |_details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_cancel(move || {
                cancel_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_move(80.0, 100.0);
    assert_eq!(starts.load(Ordering::SeqCst), 1, "the drag started");

    laid.dispatch_pointer_cancel(80.0, 100.0);
    assert_eq!(
        cancels.load(Ordering::SeqCst),
        1,
        "a cancel mid-drag fires on_horizontal_drag_cancel"
    );

    // A fresh contact afterward still drags normally.
    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_move(80.0, 100.0);
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "a drag after a cancel still starts (the cancel did not wedge the recognizer)",
    );
}

#[test]
fn primary_tap_does_not_fire_on_secondary_tap() {
    let primary_taps = Arc::new(AtomicUsize::new(0));
    let secondary_taps = Arc::new(AtomicUsize::new(0));
    let (primary_cb, secondary_cb) = (Arc::clone(&primary_taps), Arc::clone(&secondary_taps));

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                primary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_secondary_tap(move || {
                secondary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // A primary-button tap fires on_tap and must NOT fire on_secondary_tap.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        primary_taps.load(Ordering::SeqCst),
        1,
        "a primary down+up fires on_tap exactly once",
    );
    assert_eq!(
        secondary_taps.load(Ordering::SeqCst),
        0,
        "a primary tap must NOT fire on_secondary_tap",
    );
}
