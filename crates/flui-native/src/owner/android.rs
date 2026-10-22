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
    /// What answers the input method's calls through a new view, one per
    /// view.
    fn input_method() -> Box<dyn java_view::InputMethod>;
}

/// What `FluiView` reports of its editor after an input method edit
/// (`nativeImeState`): the text it holds and the raw offsets of `Selection`
/// and the composing span (`-1` for none), as Java counts them, in UTF-16 code
/// units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorState {
    /// The view's text.
    pub(crate) text: String,
    /// `Selection.getSelectionStart`: the selection's anchor.
    pub(crate) selection_start: i32,
    /// `Selection.getSelectionEnd`: the selection's moving end.
    pub(crate) selection_end: i32,
    /// `BaseInputConnection.getComposingSpanStart`.
    pub(crate) composition_start: i32,
    /// `BaseInputConnection.getComposingSpanEnd`.
    pub(crate) composition_end: i32,
}
