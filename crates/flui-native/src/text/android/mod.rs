//! The Android host's text input: `FluiView`'s editor mirrors the focused
//! field for the input method (ADR-0247, ADR-0090), and the system font scale
//! sizes text natively.
//!
//! - [`ime`]: the decidable half, how the view's mirror and the field's store
//!   stay one document while two threads own the session; built on every
//!   target (see the feature's doc);
//! - `input`: `AndroidTextInput`, the window's `TextStoreHost` over the view,
//!   and `ViewMirror`, the view's copy of the focused field, which the host's
//!   table hands `FluiView` (`owner::android::AndroidHost`);
//! - `sizing`: the system text size captured on the Java main thread, its
//!   acquisition checked on every host.

pub mod ime;
#[cfg(target_os = "android")]
mod input;
#[cfg(any(target_os = "android", test))]
pub(crate) mod sizing;

#[cfg(target_os = "android")]
pub(crate) use input::{AndroidTextInput, ViewMirror};
