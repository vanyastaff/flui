//! Testing utilities for gesture and event handling.
//!
//! [`input`] builds individual synthetic events (pointer, keyboard, modifier
//! chords) for a test that drives a recognizer or a binding directly.
//!
//! **Scripted gestures and replay live in `flui-testing`**, not here. This
//! module used to carry a `GestureRecorder`/`GesturePlayer`/`GestureBuilder`
//! trio, but a gesture is a shape in time and none of it could replay time: the
//! recorder stamped `Instant::now()` and the player was an
//! `Iterator<Item = PointerEvent>` that advanced no clock, so a recorded long
//! press replayed as an instant tap. It had no consumers as a result. See
//! `flui_testing::replay`, which scripts explicit virtual-time offsets and
//! replays them by advancing a `HeadlessBinding`'s `ManualClock`.

pub mod input;

// Re-export input builders
pub use input::ModifiersBuilder;
