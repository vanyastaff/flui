//! The Android host's features: window, input, text, display, drag, haptics,
//! system chrome, safe area and preferences.
//!
//! The loop that polls `AndroidApp`, the Java view and the NDK lookup are the
//! owner thread's (`crate::owner::android`); the features here read the
//! Activity through it.

mod clipboard;
pub(crate) mod drag;
mod gesture_exclusion;
mod haptics;
pub mod input;
pub(crate) mod memory;
pub(crate) mod preferences;
mod safe_area;
mod system_chrome;
mod text_input;
pub(crate) mod text_sizing;
pub mod window;

pub use clipboard::MockClipboard;
pub(crate) use drag::AndroidTransfer;
pub(crate) use haptics::AndroidHaptics;
pub(crate) use input::AndroidInputState;
pub(crate) use system_chrome::AndroidSystemChrome;
pub(crate) use text_input::AndroidTextInput;
pub use window::AndroidWindow;
