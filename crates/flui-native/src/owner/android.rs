//! The Android owner thread: the looper that polls `AndroidApp`, the Java view
//! that reports to it, and the lookup of the NDK functions it calls.
//!
//! - `looper`: `AndroidLoop`, the host's loop over `android-activity`, and the
//!   positions of the host table that read the loop's own state;
//! - `java_view`: `FluiView`, the Java side, and the queue of what it
//!   reported since the loop's last turn (ADR-0247);
//! - [`ndk`]: the API level and the NDK functions that exist only from a given
//!   level on, found by name when the platform is built.
//!
//! Window, input, display, drag, haptics and preferences stay in
//! `platforms::android`; the loop reaches them there. The view serves several
//! features, so what it hands a feature's state is named by the host's table
//! ([`AndroidHost`]), not by the view.

#[cfg(target_os = "android")]
pub(crate) mod java_view;
#[cfg(target_os = "android")]
mod looper;
pub(crate) mod ndk;

#[cfg(target_os = "android")]
pub(crate) use looper::AndroidLoop;

/// What an Android host's table names for its owner thread (ADR-0151 §10):
/// the features' objects `FluiView` hands what it reports.
#[cfg(target_os = "android")]
pub(crate) trait AndroidHost: 'static {
    /// A new mirror of the field the view edits, one per view.
    fn field_mirror() -> std::sync::Arc<dyn java_view::FieldMirror>;
}
