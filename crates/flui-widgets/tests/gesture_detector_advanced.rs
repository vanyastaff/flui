//! Clock-driven gesture recognition: a `GestureDetector`'s `on_long_press` and
//! `on_double_tap` fire deterministically when the detector reads a shared,
//! clock-bound arena from a `GestureArenaScope` and a `HeadlessBinding` drives
//! the arena's deadlines via `pump`. All timing is virtual — no `thread::sleep`.
//!
//! The competition tests exercise the two fixes that make a multi-recognizer
//! detector behave: `LongPressGestureRecognizer::poll_deadline` winning the
//! arena (so a held press rejects the tap), and per-recognizer participation
//! gating (so an unconfigured recognizer never steals a contact).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{lay_out, tight};
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, GestureDetector};

/// A hit-testable child so the detector's `DeferToChild` listener registers.
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

// ============================================================================
// (1) Long press — held past the deadline, driven only by `pump`.
// ============================================================================

pub(crate) fn long_press_fires_when_held_past_the_deadline() {
    let presses = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&presses);

    let mut scoped = lay_out(
        GestureDetector::new()
            .on_long_press(move |_cx| {
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
    scoped.pump_for(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "no long-press before the hold deadline elapses",
    );

    // Crossing 500ms (total 600ms) fires the deadline inside the frame.
    scoped.pump_for(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "a held contact past the deadline fires on_long_press exactly once",
    );
}

// ============================================================================
// (2) Double tap — two quick virtual-clock taps.
// ============================================================================

// ============================================================================
// (3) Competition — one detector with on_tap + on_long_press.
// ============================================================================

// ============================================================================
// (4) Presentation scope is mandatory.
// ============================================================================

// ============================================================================
// (5) on_tap + on_double_tap on the SAME detector (the headline fix).
// ============================================================================

pub(crate) fn double_tap_combined_with_tap_fires_double_tap_once_and_tap_never() {
    let taps = Arc::new(AtomicUsize::new(0));
    let double_taps = Arc::new(AtomicUsize::new(0));
    let (tap_cb, double_cb) = (Arc::clone(&taps), Arc::clone(&double_taps));

    let mut scoped = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_double_tap(move |_cx| {
                double_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // Two quick taps within the window. The double-tap recognizer holds the
    // arena across the inter-tap window, so the binding's first-up sweep is
    // deferred and the tap cannot win early; the second tap completes the
    // double-tap, which rejects BOTH taps. Without the binding-driven lifecycle
    // this fires on_tap TWICE and on_double_tap zero times.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);
    scoped.pump_for(Duration::from_millis(50));
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        1,
        "two quick taps fire on_double_tap exactly once",
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a genuine double tap must NOT fire on_tap at all",
    );
}

// ============================================================================
// (6) Two overlapping detectors compete in one GestureArenaScope.
// ============================================================================
//
// Detector A (outer, on_long_press) wraps detector B (inner, on_tap), both over
// the same hit-testable target so both Listeners sit on the hit path and add
// their recognizers to the SAME arena entry for one contact. The hit path is
// "most specific first", so the INNER detector's recognizer is the arena front
// member. These guard that A's recognizers no longer self-sweep B out (the
// binding owns the sweep): exactly one callback fires per contact.
