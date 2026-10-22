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
//! Window, input, drag, haptics and preferences stay in `platforms::android`;
//! the loop reaches them there. The displays and the text input are their
//! features': the loop holds the displays through the hook the host's table
//! fills, and the view serves several features, so what it hands a feature's
//! state is named by the host's table (`AndroidHost`), not by the view.

#[cfg(target_os = "android")]
pub(crate) mod java_view;
#[cfg(target_os = "android")]
mod looper;
pub(crate) mod ndk;
pub(crate) mod poll;

#[cfg(target_os = "android")]
pub(crate) use looper::AndroidLoop;

#[cfg(target_os = "android")]
pub(crate) use hooks::{AndroidHost, HostDisplays};

/// What the host's table fills for the loop.
#[cfg(target_os = "android")]
mod hooks {
    use std::sync::{Arc, Weak};

    use android_activity::AndroidApp;
    use android_activity::ndk::native_window::NativeWindow;
    use flui_platform::{DisplayId, DisplaySnapshot, FrameRateHint};

    use super::java_view::InputMethod;
    use super::ndk::SymbolSource;
    use crate::error::PlatformError;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::watch::Watch;
    use crate::shared::os_version::android::AndroidApi;

    /// What an Android host's table names for its owner thread (ADR-0151
    /// §10): the displays the loop holds and the features' objects `FluiView`
    /// hands what it reports. The owner reaches them through the hooks and
    /// names no feature.
    pub(crate) trait AndroidHost: 'static {
        /// What answers the input method's calls through a new view, one per
        /// view.
        fn input_method() -> Box<dyn InputMethod>;

        /// The displays of the platform on API level `api`, whose NDK
        /// functions `library` looks up and whose changes wake `owner`. The
        /// loop holds them for the table's displays position and hands them to
        /// its window.
        fn displays(
            api: AndroidApi,
            library: Arc<dyn SymbolSource>,
            owner: Weak<OwnerSignal>,
        ) -> Arc<dyn HostDisplays>;
    }

    /// The device's displays as the loop and its window reach them, which the
    /// table makes ([`AndroidHost::displays`]).
    pub(crate) trait HostDisplays: Send + Sync {
        /// The snapshot: the kept one while nothing announced a change, a
        /// fresh sample of `app`'s displays otherwise.
        ///
        /// # Errors
        /// [`PlatformError::Displays`] when the sample cannot be read or the
        /// snapshot revision space is used up.
        fn snapshot(&self, app: &AndroidApp) -> Result<DisplaySnapshot, PlatformError>;

        /// The display state may have changed (configuration change, resume):
        /// read it again at the next owner turn, which this asks for.
        fn invalidate(&self);

        /// The display the Activity was on at the last sample.
        fn activity_display(&self) -> Option<DisplayId>;

        /// Whether the highest rate of the Activity's display changed since
        /// this was last asked; reading clears it.
        fn take_max_rate_change(&self) -> bool;

        /// Ask the system for the refresh rate `hint` names on `window`, if
        /// this system has the request.
        fn request_frame_rate(&self, window: &NativeWindow, hint: FrameRateHint);

        /// Watch the refresh rate where the system reports it, on the thread
        /// whose looper runs the app, until the watch is dropped on that
        /// thread.
        fn observe(&self) -> Watch;
    }
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
