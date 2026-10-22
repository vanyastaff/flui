//! The Android host's features: window, input, system chrome and safe area.
//!
//! The loop that polls `AndroidApp`, the Java view and the NDK lookup are the
//! owner thread's (`crate::owner::android`); the features here read the
//! Activity through it.

mod gesture_exclusion;
pub mod input;
mod safe_area;
mod system_chrome;
pub mod window;

pub(crate) use input::AndroidInputState;
pub(crate) use system_chrome::AndroidSystemChrome;
pub use window::AndroidWindow;
