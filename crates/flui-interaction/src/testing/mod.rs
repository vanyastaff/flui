//! The doubles of the contracts this crate owns (ADR-0243 D4, ADR-0264), and
//! the builders of synthetic events.
//!
//! [`input`] builds individual synthetic events (pointer, keyboard, modifier
//! chords) for a test that drives a recognizer or a binding directly.
//!
//! [`ScriptedClipboard`] and [`ScriptedPrimarySelection`] are the providers
//! of [`ClipboardCapability`](crate::text_input::ClipboardCapability) and
//! [`PrimarySelectionCapability`](crate::text_input::PrimarySelectionCapability)
//! a headless host registers, built on the scenario primitive
//! ([`flui_platform::capability::scripted`]), which this crate's `testing`
//! feature turns on. [`InMemoryTextStore`] is a complete
//! [`TextStore`](flui_platform::text::store::TextStore) over a `String`,
//! beside the text-input policy it drives.
//!
//! **Scripted gestures and replay live in `flui-testing`**, not here. This
//! module used to carry a `GestureRecorder`/`GesturePlayer`/`GestureBuilder`
//! trio, but a gesture is a shape in time and none of it could replay time: the
//! recorder stamped `Instant::now()` and the player was an
//! `Iterator<Item = PointerEvent>` that advanced no clock, so a recorded long
//! press replayed as an instant tap. It had no consumers as a result. See
//! `flui_testing::replay`, which scripts explicit virtual-time offsets and
//! replays them by advancing a headless host's `ManualClock`.

mod clipboard;
pub mod input;
mod primary_selection;
mod text_store;

pub use clipboard::{
    ClipboardRequest, ClipboardWriteHold, ClipboardWriteRequest, ScriptedClipboard,
};
// Re-export input builders
pub use input::ModifiersBuilder;
pub use primary_selection::{PrimarySelectionRequest, ScriptedPrimarySelection};
pub use text_store::InMemoryTextStore;
