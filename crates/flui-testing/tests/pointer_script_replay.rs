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

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;
use flui_platform::{GesturePreferences, SystemPreferences};
use flui_testing::replay::PointerScript;
use flui_testing::widgets::{LaidOut, lay_out, tight};
use flui_widgets::{ColoredBox, GestureDetector};

fn at(x: f64, y: f64) -> Offset {
    Offset::new(x, y)
}

/// A long-press detector filling a 100 × 100 window whose long-press timeout
/// is `timeout`, plus the flag its callback sets.
fn long_press_probe(timeout: Duration) -> (LaidOut, Rc<Cell<bool>>) {
    let fired = Rc::new(Cell::new(false));
    let in_callback = Rc::clone(&fired);
    let mut laid = lay_out(
        GestureDetector::new()
            .on_long_press(move |_| in_callback.set(true))
            .child(ColoredBox::new(Color::RED)),
        tight(100.0, 100.0),
    );
    laid.set_system_preferences(
        SystemPreferences::default()
            .with_gestures(GesturePreferences::default().with_long_press_timeout(timeout)),
    );
    laid.pump();
    (laid, fired)
}

pub(crate) fn a_long_press_script_held_past_the_deadline_fires_it() {
    let (mut laid, fired) = long_press_probe(Duration::from_millis(500));

    laid.replay(&PointerScript::long_press(
        at(10.0, 10.0),
        Duration::from_millis(600),
    ));

    assert!(
        fired.get(),
        "600ms of held virtual time must cross the 500ms deadline",
    );
}

pub(crate) fn the_same_script_released_before_the_deadline_does_not() {
    let (mut laid, fired) = long_press_probe(Duration::from_millis(500));

    // Byte-for-byte the same event sequence as the test above — only the hold
    // duration differs. If `replay` stopped advancing the clock, both tests
    // would agree, and one of them would then be wrong.
    laid.replay(&PointerScript::long_press(
        at(10.0, 10.0),
        Duration::from_millis(300),
    ));

    assert!(
        !fired.get(),
        "300ms of held virtual time must not reach the 500ms deadline",
    );
}
