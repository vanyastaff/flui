//! The Android host's input.
//!
//! The loop that polls `AndroidApp`, the Java view and the NDK lookup are the
//! owner thread's (`crate::owner::android`), the window with its safe area
//! and system chrome is the window feature's (`crate::window::android`), and
//! the displays, the text input, the preferences, the lifecycle, the
//! clipboard, drag and drop and haptics are their features'; the input here
//! reads the Activity through the loop.

pub mod input;

pub(crate) use input::AndroidInputState;
