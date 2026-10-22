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
//! Window, input, display, haptics and preferences stay in `platforms::android`;
//! the loop reaches them there. The view serves several features, so what it
//! hands a feature's state is named by the host's table ([`AndroidHost`]), not
//! by the view; the clipboard and the drag-and-drop transport the loop holds
//! are made there too.

#[cfg(target_os = "android")]
pub(crate) mod java_view;
#[cfg(target_os = "android")]
mod looper;
pub(crate) mod ndk;

#[cfg(target_os = "android")]
pub(crate) use looper::AndroidLoop;

/// What an Android host's table names for its owner thread (ADR-0151 §10):
/// the features' objects `FluiView` hands what it reports, and the clipboard
/// and drag-and-drop transport the loop holds. The owner reaches them through
/// these hooks and names no feature.
#[cfg(target_os = "android")]
pub(crate) trait AndroidHost: 'static {
    /// What answers the input method's calls through a new view, one per
    /// view.
    fn input_method() -> Box<dyn java_view::InputMethod>;

    /// The clipboard, made once per loop: the loop holds it and answers the
    /// table's clipboard position from it.
    fn clipboard() -> std::sync::Arc<dyn flui_platform::Clipboard>;

    /// The drag-and-drop transport of `view`, `FluiView`'s Rust side, made as
    /// the view starts: the view hands it the drag events its Java side
    /// reports, and the loop answers the table's drag position from it.
    fn transfer(
        view: std::sync::Weak<java_view::JavaView>,
    ) -> std::sync::Arc<dyn java_view::ViewTransfer>;
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
