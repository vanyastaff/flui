//! A long-press deadline driven through [`HeadlessBinding::pump_frame`].
//!
//! Proves the binding's in-frame ordering with **no wall-clock sleep**: a held
//! pointer fires its long-press deadline purely by pumping virtual frames. It
//! asserts on the observable effect (a fired flag), not `is_ok()`, so it would
//! fail if `pump_frame` did not actually advance the clock and poll deadlines.
//!
//! The recognizer-direct counterpart lives at
//! `crates/flui-interaction/tests/headless_long_press.rs`; here the same deadline
//! is driven through `binding.pump_frame(..)` instead of raw `clock.advance` +
//! `poll_deadlines`, proving the deadline poll happens *inside* the frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_interaction::settings::GestureSettings;
use flui_interaction::{GestureRecognizer, LongPressGestureRecognizer, PointerId};
use flui_types::Offset;
use flui_types::geometry::px;

#[test]
fn long_press_fires_through_pump_frame_without_wall_clock_sleep() {
    let mut binding = HeadlessBinding::new();

    let fired = Arc::new(AtomicBool::new(false));
    let in_callback = Arc::clone(&fired);
    // Built against the binding's shared, clock-bound arena, so the deadline the
    // recognizer captures and checks both read the binding's virtual clock.
    let recognizer = LongPressGestureRecognizer::with_settings(
        binding.arena().clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
    )
    .with_on_long_press_start(move |_details| in_callback.store(true, Ordering::SeqCst));

    // Pointer down captures `down_time` from the VIRTUAL clock (now = base + 0).
    let pointer = PointerId::new(2).expect("nonzero pointer id");
    recognizer.add_pointer(pointer, Offset::new(px(10.0), px(10.0)));

    // Three pumped frames totalling 300ms < 500ms — the deadline must NOT fire.
    for _ in 0..3 {
        binding.pump_frame(Duration::from_millis(100));
    }
    assert!(
        !fired.load(Ordering::SeqCst),
        "long-press must not fire before the 500ms deadline (300ms pumped so far)",
    );

    // One more pumped frame crosses 500ms of virtual time: the deadline fires
    // inside pump_frame's deadline-poll step, deterministically, no sleep.
    binding.pump_frame(Duration::from_millis(200));
    assert!(
        fired.load(Ordering::SeqCst),
        "a held pointer past the 500ms deadline fires inside the pumped frame",
    );
}
