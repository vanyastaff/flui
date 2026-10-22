//! The Android owner thread: the looper that polls `AndroidApp`, the Java view
//! that reports to it, and the lookup of the NDK functions it calls.
//!
//! - `looper`: `AndroidLoop`, the host's loop over `android-activity`, and the
//!   positions of the host table that read the loop's own state;
//! - `java_view`: `FluiView`, the Java side, and the queue of what it
//!   reported since the loop's last turn (ADR-0247);
//! - [`ndk`]: the API level and the NDK functions that exist only from a given
//!   level on, found by name when the platform is built;
//! - [`poll`]: how long one turn of the loop may block (ADR-0249).
//!
//! Window, input, text, drag, haptics and preferences stay in
//! `platforms::android`; the loop reaches them there. The displays are the
//! `display` feature's, which the loop holds through the hook the host's table
//! fills.

#[cfg(target_os = "android")]
pub(crate) mod java_view;
#[cfg(target_os = "android")]
mod looper;
pub(crate) mod ndk;
pub(crate) mod poll;

#[cfg(target_os = "android")]
pub(crate) use looper::AndroidLoop;
