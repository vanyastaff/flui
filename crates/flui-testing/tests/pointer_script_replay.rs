//! Scripted pointer input replayed on the virtual clock.
//!
//! The property under test is the one the predecessor infrastructure
//! (`flui_interaction::testing::recording`) could not express: replaying a
//! gesture replays its *timing*. Its `GesturePlayer` was an
//! `Iterator<Item = PointerEvent>` — nothing advanced a clock between events,
//! so a long-press recording arrived as an instant tap and every
//! deadline-driven recognizer saw the wrong gesture.
//!
//! Each test here is written so it would fail if `replay` stopped advancing
//! virtual time: the verdict flips on the script's timing alone, with the same
//! event sequence on both sides.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::settings::GestureSettings;
use flui_interaction::{GestureRecognizer, LongPressGestureRecognizer, PointerId};
use flui_testing::HeadlessBinding;
use flui_testing::replay::PointerScript;

fn at(x: f64, y: f64) -> Offset {
    Offset::new(x, y)
}

/// A long-press recognizer on `binding`'s clock-bound arena, plus the flag its
/// callback sets.
fn long_press_probe(
    binding: &HeadlessBinding,
    timeout: Duration,
) -> (Arc<LongPressGestureRecognizer>, Arc<AtomicBool>) {
    let fired = Arc::new(AtomicBool::new(false));
    let in_callback = Arc::clone(&fired);
    let recognizer = LongPressGestureRecognizer::with_settings(
        binding.arena().clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(timeout),
    )
    .with_on_long_press_start(move |_details| in_callback.store(true, Ordering::SeqCst));
    (recognizer, fired)
}

/// Replay `script`, enrolling `recognizer` for the contact when the replay
/// hit-tests its Down.
///
/// The hit test is where a contact acquires its targets in production too, so
/// this is the same seam a mounted tree fills — a bare recognizer just answers
/// it directly instead of through a render-tree walk. The route is captured
/// once per contact, so the enrollment happens on the Down and the rest of the
/// contact's events follow the captured route.
fn replay_against(
    binding: &mut HeadlessBinding,
    recognizer: &Arc<LongPressGestureRecognizer>,
    script: &PointerScript,
) {
    let recognizer = Arc::clone(recognizer);
    binding.replay_with(script, move |_, position| {
        recognizer.add_pointer(PointerId::PRIMARY, position, position);
        flui_interaction::HitTestResult::new()
    });
}

pub(crate) fn a_long_press_script_held_past_the_deadline_fires_it() {
    let mut binding = HeadlessBinding::new();
    let (recognizer, fired) = long_press_probe(&binding, Duration::from_millis(500));

    replay_against(
        &mut binding,
        &recognizer,
        &PointerScript::long_press(at(10.0, 10.0), Duration::from_millis(600)),
    );

    assert!(
        fired.load(Ordering::SeqCst),
        "600ms of held virtual time must cross the 500ms deadline",
    );
}

pub(crate) fn the_same_script_released_before_the_deadline_does_not() {
    let mut binding = HeadlessBinding::new();
    let (recognizer, fired) = long_press_probe(&binding, Duration::from_millis(500));

    // Byte-for-byte the same event sequence as the test above — only the hold
    // duration differs. If `replay` stopped advancing the clock, both tests
    // would agree, and one of them would then be wrong.
    replay_against(
        &mut binding,
        &recognizer,
        &PointerScript::long_press(at(10.0, 10.0), Duration::from_millis(300)),
    );

    assert!(
        !fired.load(Ordering::SeqCst),
        "300ms of held virtual time must not reach the 500ms deadline",
    );
}
