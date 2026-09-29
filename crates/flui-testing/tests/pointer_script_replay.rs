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
use flui_interaction::events::PointerType;
use flui_interaction::settings::GestureSettings;
use flui_interaction::{GestureRecognizer, LongPressGestureRecognizer, PointerId};
use flui_testing::HeadlessBinding;
use flui_testing::replay::{GestureRecorder, PointerPhase, PointerScript};

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

#[test]
fn a_long_press_script_held_past_the_deadline_fires_it() {
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

#[test]
fn the_same_script_released_before_the_deadline_does_not() {
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

#[test]
fn a_recording_taken_on_the_virtual_clock_round_trips_to_the_same_timing() {
    let mut binding = HeadlessBinding::new();
    let mut recorder = GestureRecorder::new("captured");

    // Capture three events with known virtual gaps between them. The offsets
    // come from the binding's clock, so this recording is reproducible — the
    // predecessor stamped `Instant::now()` and could not be.
    recorder.record(
        &binding,
        PointerId::PRIMARY,
        PointerPhase::Down,
        at(0.0, 0.0),
        PointerType::Touch,
    );
    binding.pump_frame(Duration::from_millis(120));
    recorder.record(
        &binding,
        PointerId::PRIMARY,
        PointerPhase::Move,
        at(40.0, 0.0),
        PointerType::Touch,
    );
    binding.pump_frame(Duration::from_millis(80));
    recorder.record(
        &binding,
        PointerId::PRIMARY,
        PointerPhase::Up,
        at(80.0, 0.0),
        PointerType::Touch,
    );

    let script = recorder.finish();
    assert_eq!(script.len(), 3);
    assert_eq!(script.events()[0].at, Duration::ZERO);
    assert_eq!(script.events()[1].at, Duration::from_millis(120));
    assert_eq!(script.events()[2].at, Duration::from_millis(200));
    assert_eq!(script.duration(), Duration::from_millis(200));

    // Replaying it spends the same virtual time it was captured over.
    let mut replay_binding = HeadlessBinding::new();
    replay_binding.replay(&script);
    assert_eq!(replay_binding.clock().elapsed(), Duration::from_millis(200));
}

#[test]
#[should_panic(expected = "resampling to be disabled")]
fn replay_refuses_to_run_with_pointer_resampling_enabled() {
    // With resampling on, the emitted move samples come from the resampler's
    // wall-clock queue and a wall-clock sampling window, neither of which the
    // script's offsets reach. A replay that ran anyway would return a number
    // that looks deterministic and is not, so it refuses.
    let mut binding = HeadlessBinding::new();
    binding
        .gestures()
        .set_resampling_enabled(true)
        .expect("no active pointers yet, so the mode is still settable");

    binding.replay(&PointerScript::tap(at(5.0, 5.0)));
}
