//! Deterministic long-press: a held pointer fires the long-press deadline on
//! pumped virtual-clock frames, with **no wall-clock sleep**.
//!
//! This is the keystone the headless frame driver builds on. A [`ManualClock`]
//! drives the arena's `now()`, so a recognizer's captured down-time and its
//! deadline check both read the *virtual* timeline; advancing the clock and
//! calling `poll_deadlines()` resolves the deadline at virtual time.
//!
//! Red→green guard: before the clock-on-arena change the recognizer read
//! `Instant::now()` directly, so the virtual clock had no effect — sleep-free,
//! the deadline never fired and this test failed. After the change it fires
//! deterministically. (The existing wall-clock `long_press.rs` unit tests keep
//! exercising the production `SystemClock` path with real `thread::sleep`.)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_interaction::arena::GestureArena;
use flui_interaction::settings::GestureSettings;
use flui_interaction::{GestureRecognizer, LongPressGestureRecognizer, ManualClock, PointerId};
use flui_types::Offset;
use flui_types::geometry::px;

#[test]
fn long_press_fires_on_pumped_virtual_frames_without_sleeping() {
    let clock = ManualClock::new();
    let arena = GestureArena::with_clock(Arc::new(clock.clone()));

    let fired = Arc::new(AtomicBool::new(false));
    let in_cb = Arc::clone(&fired);
    let recognizer = LongPressGestureRecognizer::with_settings(
        arena.clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
    )
    .with_on_long_press_start(move |_details| in_cb.store(true, Ordering::SeqCst));

    // Pointer down captures `down_time` from the VIRTUAL clock (now = base + 0).
    let pointer = PointerId::new(2).expect("nonzero pointer id");
    recognizer.add_pointer(pointer, Offset::new(px(10.0), px(10.0)));

    // Hold still; pump virtual frames totalling < 500ms — must NOT fire.
    for _ in 0..3 {
        clock.advance(Duration::from_millis(100));
        arena.poll_deadlines();
    }
    assert!(
        !fired.load(Ordering::SeqCst),
        "long-press must not fire before the deadline elapses (300ms < 500ms)",
    );

    // One more virtual frame crosses 500ms — fires deterministically, no sleep.
    clock.advance(Duration::from_millis(200));
    arena.poll_deadlines();
    assert!(
        fired.load(Ordering::SeqCst),
        "a held pointer past the 500ms deadline fires on the pumped frame",
    );
}
