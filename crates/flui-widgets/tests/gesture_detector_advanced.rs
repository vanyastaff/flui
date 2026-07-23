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

use common::{lay_out, tight};
use flui_types::Color;
use flui_view::{BuildOwner, ElementTree, View};
use flui_widgets::{ColoredBox, Draggable, GestureDetector};

/// A hit-testable child so the detector's `DeferToChild` listener registers.
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

/// Deliberately bypass the canonical presentation wrapper so invariant tests
/// can prove gesture consumers reject missing ownership.
fn mount_without_presentation_scope(root: impl View) {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let root_id = tree.mount_root(&root, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
}

// ============================================================================
// (1) Long press — held past the deadline, driven only by `pump`.
// ============================================================================

#[test]
fn long_press_fires_when_held_past_the_deadline() {
    let presses = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&presses);

    let mut scoped = lay_out(
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

#[test]
fn quick_release_does_not_fire_long_press() {
    let presses = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&presses);

    let scoped = lay_out(
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

    let mut scoped = lay_out(
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
    scoped.pump_for(Duration::from_millis(50));
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

    let mut scoped = lay_out(
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
    scoped.pump_for(Duration::from_millis(400));
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

    let scoped = lay_out(
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

    let mut scoped = lay_out(
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
    scoped.pump_for(Duration::from_millis(600));
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
// (4) Presentation scope is mandatory.
// ============================================================================

#[test]
#[should_panic(expected = "GestureArenaScope")]
fn gesture_detector_without_a_presentation_arena_fails_during_mount() {
    mount_without_presentation_scope(GestureDetector::new().on_tap(|| {}).child(target()));
}

#[test]
#[should_panic(expected = "GestureArenaScope")]
fn draggable_without_a_presentation_arena_fails_during_mount() {
    mount_without_presentation_scope(Draggable::<i32>::new(target()));
}

// ============================================================================
// (5) on_tap + on_double_tap on the SAME detector (the headline fix).
// ============================================================================

#[test]
fn double_tap_combined_with_tap_fires_double_tap_once_and_tap_never() {
    let taps = Arc::new(AtomicUsize::new(0));
    let double_taps = Arc::new(AtomicUsize::new(0));
    let (tap_cb, double_cb) = (Arc::clone(&taps), Arc::clone(&double_taps));

    let mut scoped = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_double_tap(move || {
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

#[test]
fn lone_tap_is_held_until_the_double_tap_window_closes_then_fires_tap() {
    let taps = Arc::new(AtomicUsize::new(0));
    let double_taps = Arc::new(AtomicUsize::new(0));
    let (tap_cb, double_cb) = (Arc::clone(&taps), Arc::clone(&double_taps));

    let mut scoped = lay_out(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_double_tap(move || {
                double_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    // One tap: the double-tap recognizer holds the arena, so the tap is deferred
    // — it must NOT have fired yet.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "the lone tap is held until the double-tap window closes",
    );

    // Cross the 300ms window with no second contact: the double-tap gives up,
    // withdraws itself, and the lone tap finally wins and fires once.
    scoped.pump_for(Duration::from_millis(350));
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "after the window closes the held tap fires exactly once",
    );
    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        0,
        "a single tap is not a double tap",
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

fn nested_tap_over_long_press(
    tap_count: Arc<AtomicUsize>,
    press_count: Arc<AtomicUsize>,
) -> GestureDetector {
    GestureDetector::new()
        .on_long_press(move || {
            press_count.fetch_add(1, Ordering::SeqCst);
        })
        .child(
            GestureDetector::new()
                .on_tap(move || {
                    tap_count.fetch_add(1, Ordering::SeqCst);
                })
                .child(target()),
        )
}

#[test]
fn overlapping_detectors_quick_tap_resolves_to_the_inner_tap() {
    let taps = Arc::new(AtomicUsize::new(0));
    let presses = Arc::new(AtomicUsize::new(0));

    let scoped = lay_out(
        nested_tap_over_long_press(Arc::clone(&taps), Arc::clone(&presses)),
        tight(100.0, 100.0),
    );

    // Quick down+up: the inner tap is the front member and wins on the binding's
    // sweep; the outer long press never fires.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the inner tap wins a quick contact",
    );
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "the outer long press does not fire on a quick contact",
    );
}

#[test]
fn overlapping_detectors_held_press_resolves_to_the_outer_long_press() {
    let taps = Arc::new(AtomicUsize::new(0));
    let presses = Arc::new(AtomicUsize::new(0));

    let mut scoped = lay_out(
        nested_tap_over_long_press(Arc::clone(&taps), Arc::clone(&presses)),
        tight(100.0, 100.0),
    );

    // Hold past the deadline: the outer long press wins the shared arena
    // (rejecting the inner tap), then release. The inner tap must NOT fire — the
    // case that regresses if the inner tap's own up self-sweeps the arena.
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.pump_for(Duration::from_millis(600));
    scoped.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "the held press fires the outer long press exactly once",
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "the long press rejected the inner tap, so the tap must NOT fire",
    );
}
