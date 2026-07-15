//! ## Test parity notes
//!
//! Flutter source (tag `3.44.0`): `packages/flutter/test/gestures/long_press_test.dart`,
//! `packages/flutter/test/gestures/double_tap_test.dart`, and
//! `packages/flutter/test/gestures/arena_test.dart`. These are the timer- and
//! arena-driven cases from the raw recognizer test suites, ported at the
//! `GestureDetector` widget level through [`common::lay_out_with_arena`]'s
//! [`common::LaidOutScoped`] harness — a [`flui_widgets::GestureArenaScope`]
//! over a [`flui_binding::HeadlessBinding`] whose virtual clock drives every
//! gesture deadline deterministically (no `thread::sleep`; `pump(dt)` advances
//! the clock and polls deadlines in the same step Flutter's `tester.async.elapse`
//! plays).
//!
//! Two related integration-test files already exercise long-press/double-tap
//! timing and arena elimination at generous margins (not tied to specific
//! Flutter source lines):
//! `crates/flui-widgets/tests/gesture_detector_advanced.rs` (clock-driven
//! long-press/double-tap firing, tap-vs-long-press and tap-vs-double-tap
//! competition, nested detectors) and
//! `crates/flui-widgets/tests/gesture_detector.rs` (arena elimination via a
//! pan dragged past slop, cited from `arena.dart` inline). The cases below are
//! additive: threshold-epsilon boundaries (this file's vacuity convention —
//! assert the non-fire side and the fire side of every timing edge, not just
//! one), and source-cited scenarios (`Up cancels long press`, `Moving before
//! accept cancels`, `Moving after accept is ok`, `Inter-tap distance cancels
//! double tap`) that neither existing file covers.
//!
//! Divergence carried over from `gesture_detector_test.rs`: FLUI's touch slop
//! is pinned at 18.0 regardless of `PointerType` (Flutter varies it per
//! device); every "past slop" move below uses a delta well clear of 18px so
//! the assertion holds under that pinned value.
//!
//! `kPressTimeout` (`gestures/constants.dart`, tag `3.44.0`) — Flutter's 100ms
//! delay before a tap's `onTapDown` fires "if there's any doubt" — has no
//! timing behavior to port: FLUI's `GestureDetector` has no `on_tap_down`
//! callback (`crates/flui-widgets/src/interaction/gesture_detector.rs`'s
//! `GestureDetector` only exposes `on_tap`, fired on a resolved up, never a
//! down-time highlight signal), so there is no arena deadline gated on this
//! constant to test. Skipped for that reason.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{lay_out_with_arena, tight};
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector};

/// A hit-testable child so the detector's `DeferToChild` listener registers.
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}

// ============================================================================
// Long press — `long_press_test.dart` threshold + cancellation cases.
// ============================================================================

/// Flutter parity: `long_press_test.dart` `'Should recognize long press'`
/// (`down` → 300ms elapse, still nothing → 700ms more, fires). This ports the
/// same shape tightened to the exact `kLongPressTimeout` boundary (500ms) from
/// both sides, per this file's vacuity convention, and additionally checks the
/// fire is a one-shot even under a long hold.
#[test]
fn long_press_boundary_does_not_fire_before_deadline_fires_after() {
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

    scoped.dispatch_pointer_down(50.0, 50.0);

    // 1ms short of the 500ms deadline: must not have fired yet.
    scoped.pump(Duration::from_millis(499));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "499ms of hold is 1ms short of kLongPressTimeout — must not fire yet",
    );

    // Crossing the deadline (total 501ms) fires exactly once.
    scoped.pump(Duration::from_millis(2));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "crossing kLongPressTimeout (total 501ms held) fires on_long_press",
    );

    // Continuing to hold well past the deadline does not fire again.
    scoped.pump(Duration::from_secs(1));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "on_long_press is a one-shot — holding longer must not refire it",
    );

    scoped.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "releasing after the press already fired must not fire it again",
    );
}

/// Flutter parity: `long_press_test.dart` `'Up cancels long press'` — a
/// release before the deadline elapses cancels the gesture, and it must never
/// fire later even if virtual time keeps advancing (the recognizer stopped
/// tracking the contact on the early up).
#[test]
fn long_press_release_before_deadline_cancels_permanently() {
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

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.pump(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "300ms of hold is short of the 500ms deadline",
    );

    scoped.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "releasing before the deadline cancels — must not fire on release",
    );

    // Advancing virtual time past where the (now-cancelled) deadline would
    // have fired must not resurrect it.
    scoped.pump(Duration::from_secs(1));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "a cancelled long press must never fire, however much time passes",
    );
}

/// Flutter parity: `long_press_test.dart` `'Moving before accept cancels'` —
/// a move past the touch slop while the deadline is still pending cancels the
/// gesture; it must not fire even if the contact is then held past the
/// deadline and released.
#[test]
fn long_press_move_past_slop_before_deadline_cancels() {
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

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.pump(Duration::from_millis(300));

    // 40px vertical move — well past the pinned 18px slop.
    scoped.dispatch_pointer_move(50.0, 90.0);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "a move past slop cancels before the deadline fires",
    );

    // Even holding the (now off-slop) contact past where the deadline would
    // have fired, then releasing, must not fire the cancelled gesture.
    scoped.pump(Duration::from_secs(1));
    scoped.dispatch_pointer_up(50.0, 90.0);
    scoped.pump(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "a long press cancelled by motion must never fire afterward",
    );
}

/// Flutter parity: `long_press_test.dart` `'Moving after accept is ok'` — once
/// the deadline has fired and the gesture is accepted, further movement (even
/// past the touch slop) does not cancel it; the fire already happened and
/// must remain the only one.
#[test]
fn long_press_move_after_deadline_does_not_cancel_already_fired_press() {
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

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.pump(Duration::from_millis(600));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "holding past the deadline fires the long press",
    );

    // A large move after acceptance — must not cancel or refire.
    scoped.dispatch_pointer_move(50.0, 95.0);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "moving after the gesture already started is ok — no cancel, no refire",
    );

    scoped.dispatch_pointer_up(50.0, 95.0);
    scoped.pump(Duration::from_millis(300));
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "release after an accepted long press does not fire it again",
    );
}

// ============================================================================
// Double tap — `double_tap_test.dart` window + distance cases.
// ============================================================================

/// Flutter parity: the `kDoubleTapTimeout` constant (`gestures/constants.dart`,
/// 300ms) and `double_tap_test.dart` `'Inter-tap delay cancels double tap'`
/// (which uses a 5000ms margin, well past the window). This ports the same
/// window-expiry shape but tightens both sides to the exact 300ms boundary,
/// per this file's vacuity convention, and — wiring `on_tap` alongside
/// `on_double_tap` (neither existing test file does both at this precision) —
/// asserts what each side actually resolves to: inside the window, one
/// `on_double_tap` and no `on_tap`; outside the window, two standalone
/// `on_tap`s (the double-tap recognizer's `arena.hold`/`release` on each lone
/// tap — `arena_test.dart`'s hold/release/sweep ordering — defers, then
/// releases, each tap once its own window lapses) and no `on_double_tap`.
#[test]
fn double_tap_window_boundary_second_tap_inside_fires_double_outside_fires_two_singles() {
    // Inside the window: second tap at 299ms (1ms short of the 300ms limit).
    let taps = Arc::new(AtomicUsize::new(0));
    let double_taps = Arc::new(AtomicUsize::new(0));
    let (tap_cb, double_cb) = (Arc::clone(&taps), Arc::clone(&double_taps));

    let mut inside = lay_out_with_arena(
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

    inside.dispatch_pointer_down(50.0, 50.0);
    inside.dispatch_pointer_up(50.0, 50.0);
    inside.pump(Duration::from_millis(299));
    inside.dispatch_pointer_down(50.0, 50.0);
    inside.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        1,
        "a second tap 1ms inside the 300ms window fires on_double_tap",
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a genuine double tap must not also fire on_tap",
    );

    // Outside the window: second tap at 301ms (1ms past the 300ms limit) — a
    // fresh detector, since the first one's arena is now in a post-double-tap
    // state.
    let taps = Arc::new(AtomicUsize::new(0));
    let double_taps = Arc::new(AtomicUsize::new(0));
    let (tap_cb, double_cb) = (Arc::clone(&taps), Arc::clone(&double_taps));

    let mut outside = lay_out_with_arena(
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

    outside.dispatch_pointer_down(50.0, 50.0);
    outside.dispatch_pointer_up(50.0, 50.0);
    // The window elapses with no second contact: the held first tap gives up
    // and fires standalone (arena_test.dart's hold -> release ordering).
    outside.pump(Duration::from_millis(301));
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the first tap's own 300ms window lapsing releases its hold and fires on_tap",
    );

    outside.dispatch_pointer_down(50.0, 50.0);
    outside.dispatch_pointer_up(50.0, 50.0);
    // The second tap starts its own inter-tap window (nothing follows it), so
    // it too must lapse before it fires standalone.
    outside.pump(Duration::from_millis(301));
    assert_eq!(
        taps.load(Ordering::SeqCst),
        2,
        "a second tap 1ms outside the window is a standalone tap, not a double tap",
    );
    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        0,
        "two taps separated by more than the window are never a double tap",
    );
}

/// Flutter parity: `double_tap_test.dart` `'Inter-tap distance cancels double
/// tap'` — a second tap within the time window but farther than
/// `kDoubleTapSlop` (100 logical px) from the first is not part of a double
/// tap (matches the upstream assertions exactly: no `on_double_tap`; this
/// file wires only `on_double_tap`, not `on_tap` — see
/// `overlapping_contact_replacing_the_held_primary_pointer_drops_its_late_win`
/// below for why combining both here would assert past this case's own
/// behavior — an out-of-slop second contact replaces the held first tap as
/// the shared recognizer's primary pointer rather than coexisting with it).
#[test]
fn double_tap_second_tap_far_from_first_is_not_a_double_tap() {
    let double_taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&double_taps);

    // A larger canvas so the second tap can land >100px from the first while
    // staying inside the hit-testable child.
    let scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_double_tap(move || {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(300.0, 300.0),
    );

    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    // (250, 250) is ~283px from (50, 50) — well past the 100px double-tap
    // slop — and lands well within the 300x300 window.
    scoped.dispatch_pointer_down(250.0, 250.0);
    scoped.dispatch_pointer_up(250.0, 250.0);
    assert_eq!(
        double_taps.load(Ordering::SeqCst),
        0,
        "a second tap outside the 100px double-tap slop does not combine with the first",
    );
}

/// Verified Flutter parity, not a bug: a second, unrelated contact that
/// lands on the same shared `TapGestureRecognizer` while the first tap is
/// still held (`DoubleTapGestureRecognizer`'s inter-tap window) *replaces*
/// the first as the tracked primary pointer — the first tap's late win is
/// then silently dropped, exactly as it is in Flutter.
///
/// `crates/flui-widgets/src/interaction/gesture_detector.rs` shares ONE
/// `TapGestureRecognizer` instance across every contact a detector ever
/// sees, matching Flutter's `RawGestureDetectorState`, which also builds its
/// recognizers once and reuses them. This test was originally filed as a
/// "single-slot state clobber" bug expecting the held first tap to still
/// fire once its window lapsed (`taps == 2`). Tracing Flutter 3.44.0's own
/// state machine shows that expectation does not hold:
///
/// - `PrimaryPointerGestureRecognizer.didStopTrackingLastPointer`
///   (`recognizer.dart:768-774`) resets `_state` to `ready` the instant the
///   tracked pointer count reaches zero — i.e. synchronously on the first
///   tap's up event, *regardless* of whether its arena entry is still held
///   open by `DoubleTapGestureRecognizer._registerFirstTap`'s
///   `gestureArena.hold` (`multitap.dart:328-330`).
/// - `BaseTapGestureRecognizer.addAllowedPointer` (`tap.dart:276-299`): when
///   a new pointer's down arrives with `state == ready` and a stale,
///   unresolved `_down`/`_up` pair is still recorded, it calls `_reset()`
///   and adopts the new pointer as `_primaryPointer` — abandoning the held
///   pointer's sequence outright (comment there: "If there is no result in
///   the previous gesture arena, we ignore them and prepare to accept a new
///   pointer").
/// - `BaseTapGestureRecognizer.acceptGesture` / `.rejectGesture`
///   (`tap.dart:348-368`) both guard on `pointer == primaryPointer` before
///   touching `_down`/`_up`/`_wonArenaForPrimaryPointer`; the abandoned
///   pointer's late win therefore arrives, finds `pointer != primaryPointer`,
///   and is silently ignored.
///
/// So a genuinely new, unrelated contact on a shared tap recognizer does not
/// coexist with a still-held earlier one — it evicts it. FLUI's
/// `TapGestureRecognizer` now ports this contract explicitly via a
/// `sequence_pointer` field compared in `accept_gesture`/`reject_gesture`/
/// `resolve_pointer` (`crates/flui-interaction/src/recognizers/tap.rs`),
/// rather than relying on it as an accidental side effect of an unconditional
/// state reset. The non-overlapping case — no second contact arrives before
/// the window lapses — is unaffected and still fires normally:
/// `lone_tap_is_held_until_the_double_tap_window_closes_then_fires_tap` in
/// `gesture_detector_advanced.rs`.
#[test]
fn overlapping_contact_replacing_the_held_primary_pointer_drops_its_late_win() {
    let taps = Arc::new(AtomicUsize::new(0));
    let double_taps = Arc::new(AtomicUsize::new(0));
    let (tap_cb, double_cb) = (Arc::clone(&taps), Arc::clone(&double_taps));

    let mut scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_double_tap(move || {
                double_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(300.0, 300.0),
    );

    // First tap: held pending the double-tap window — its on_tap is deferred,
    // not dropped, in the non-overlapping case (see the doc comment above).
    scoped.dispatch_pointer_down(50.0, 50.0);
    scoped.dispatch_pointer_up(50.0, 50.0);

    // A second, unrelated contact far enough away to never combine into a
    // double tap: it becomes the new tracked primary pointer and resolves as
    // its own standalone tap while the first is still held.
    scoped.dispatch_pointer_down(250.0, 250.0);
    scoped.dispatch_pointer_up(250.0, 250.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the second, unrelated tap fires"
    );

    // The first tap's window lapses. Its arena entry is now the sole
    // remaining member and resolves as accepted, but `TapGestureRecognizer`
    // already abandoned that pointer's sequence when the second contact took
    // over — Flutter parity: the held pointer's late win is dropped, not
    // fired as a second `on_tap`.
    scoped.pump(Duration::from_millis(301));
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the held first tap's late win is dropped once a newer contact has \
         taken over the shared recognizer's primary pointer — Flutter parity",
    );
    assert_eq!(double_taps.load(Ordering::SeqCst), 0);
}

// ============================================================================
// Arena — `arena.dart` rejection-cascade / last-member-standing semantics.
// ============================================================================

/// Flutter parity: `arena.dart`'s `GestureArenaManager` — "The first member to
/// accept or the last member to not reject wins" (line 110, same rule already
/// cited in `tests/gesture_detector.rs` for a two-way tap-vs-pan
/// competition). This extends that to a three-way arena (tap + pan +
/// long-press all wired on one detector): a drag past the pan slop rejects
/// the tap (front member, moved off-slop) while the pan itself becomes the
/// sole remaining, and thus winning, member — before the long-press deadline
/// has even elapsed and without waiting for the pointer to lift. The
/// long-press must never fire afterward, however long the contact is held.
#[test]
fn arena_rejection_cascade_pan_wins_over_tap_and_long_press() {
    let taps = Arc::new(AtomicUsize::new(0));
    let presses = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (tap_cb, press_cb, start_cb) =
        (Arc::clone(&taps), Arc::clone(&presses), Arc::clone(&starts));

    let mut scoped = lay_out_with_arena(
        GestureDetector::new()
            .on_tap(move || {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_long_press(move || {
                press_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_start(move |_details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(target()),
        tight(100.0, 100.0),
    );

    scoped.dispatch_pointer_down(50.0, 20.0);
    // 60px, well past both the 18px touch slop and the 18px pan slop, and
    // well before the 500ms long-press deadline.
    scoped.dispatch_pointer_move(50.0, 80.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "the pan starts as soon as the move crosses the slop, rejecting the tap",
    );

    // Hold well past the long-press deadline, then release: the long press
    // lost the arena to the pan and must never fire, no matter how long the
    // contact is held afterward.
    scoped.pump(Duration::from_millis(600));
    scoped.dispatch_pointer_up(50.0, 80.0);

    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "the long press lost the arena to the pan and must never fire",
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "the tap was rejected by the move past slop and must never fire",
    );
}
