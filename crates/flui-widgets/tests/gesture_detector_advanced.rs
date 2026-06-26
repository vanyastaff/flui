//! Clock-driven gesture recognition: a `GestureDetector`'s `on_long_press` and
//! `on_double_tap` fire deterministically when the detector reads a shared,
//! clock-bound arena from a `GestureArenaScope` and a `HeadlessBinding` drives
//! the arena's deadlines via `pump`. All timing is virtual — no `thread::sleep`.
//!
//! The competition tests exercise the two fixes that make a multi-recognizer
//! detector behave: `LongPressGestureRecognizer::poll_deadline` winning the
//! arena (so a held press rejects the tap), and per-recognizer participation
//! gating (so an unconfigured recognizer never steals a contact).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, lay_out_with_arena, tight};
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector};

/// A hit-testable child so the detector's `DeferToChild` listener registers.
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

// ============================================================================
// (1) Long press — held past the deadline, driven only by `pump`.
// ============================================================================

#[test]
fn long_press_fires_when_held_past_the_deadline() {
    let presses = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&presses);

    let mut scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_long_press(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // Contact down, then hold. Nothing fires before the 500ms deadline.
    scoped.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "no long-press the instant the contact lands",
    );

    // 300ms of virtual time — short of the 500ms hold deadline.
    scoped.pump(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "no long-press before the hold deadline elapses",
    );

    // Crossing 500ms (total 600ms) fires the deadline inside the frame.
    scoped.pump(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "a held contact past the deadline fires on_long_press exactly once",
    );
}

#[test]
fn quick_release_does_not_fire_long_press() {
    let presses = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&presses);

    let scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_long_press(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // Down then up with no virtual time elapsed — the hold deadline never fires.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "a quick release before the deadline is not a long press",
    );
}

// ============================================================================
// (2) Double tap — two quick virtual-clock taps.
// ============================================================================

#[test]
fn double_tap_fires_on_two_quick_taps() {
    let double_taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&double_taps);

    let mut scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_double_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // Two taps 50ms apart — both inside the 300ms double-tap window.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);
    scoped.pump(Duration::from_millis(50));
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        1,
        "two taps within the window fire on_double_tap exactly once",
    );
}

#[test]
fn second_tap_after_the_window_is_not_a_double_tap() {
    let double_taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&double_taps);

    let mut scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_double_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // The second tap lands 400ms after the first — past the 300ms window, so the
    // pair is two singles, not a double tap.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);
    scoped.pump(Duration::from_millis(400));
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        0,
        "a second tap after the window expires is not a double tap",
    );
}

// ============================================================================
// (3) Competition — one detector with on_tap + on_long_press.
// ============================================================================

#[test]
fn quick_tap_beats_long_press_in_the_same_detector() {
    let taps = Arc::new(AtomicUsize::new(0));
    let presses = Arc::new(AtomicUsize::new(0));
    let (tap_cb, press_cb) = (Arc::clone(&taps), Arc::clone(&presses));

    let scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_long_press(move || {
                press_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // Down then up before the hold deadline: the tap is the arena's front member
    // and wins; the long press never fires.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a quick down+up fires the tap",
    );
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "the long press never fires on a quick release",
    );
}

#[test]
fn held_press_beats_tap_in_the_same_detector() {
    let taps = Arc::new(AtomicUsize::new(0));
    let presses = Arc::new(AtomicUsize::new(0));
    let (tap_cb, press_cb) = (Arc::clone(&taps), Arc::clone(&presses));

    let mut scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_long_press(move || {
                press_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // Hold past the deadline so the long press wins the arena (rejecting the
    // tap), THEN release. This is the case that fails without the long-press
    // `poll_deadline` → `accept_tracked` fix — without it, the tap would also
    // fire on release.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.pump(Duration::from_millis(600));
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "the held press fires the long press exactly once",
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "the long press rejected the tap, so the tap must NOT fire on release",
    );
}

// ============================================================================
// (4) Standalone fallback — no GestureArenaScope above the detector.
// ============================================================================

#[test]
fn standalone_detector_still_taps_via_its_private_arena() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    // No scope: the detector falls back to a private arena it closes itself.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the standalone private-arena tap path is non-divergent",
    );
}

#[test]
fn standalone_quick_tap_fires_tap_not_long_press() {
    let taps = Arc::new(AtomicUsize::new(0));
    let presses = Arc::new(AtomicUsize::new(0));
    let (tap_cb, press_cb) = (Arc::clone(&taps), Arc::clone(&presses));

    // A standalone detector wiring BOTH on_tap and on_long_press: the new
    // recognizers must not disturb the private-arena tap path — a quick tap
    // still resolves to the tap (the long press is inert standalone, with no
    // binding to poll its deadline).
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_long_press(move || {
                press_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(taps.load(Ordering::SeqCst), 1, "the quick tap still fires");
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "the long press is inert standalone and does not fire on a quick tap",
    );
}
