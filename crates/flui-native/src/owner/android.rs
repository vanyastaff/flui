//! The Android owner thread: the looper that polls `AndroidApp`, the Java view
//! that reports to it, and the lookup of the NDK functions it calls.
//!
//! - `looper`: `AndroidLoop`, the host's loop over `android-activity`, and the
//!   positions of the host table that read the loop's own state;
//! - `java_view`: `FluiView`, the Java side, and the queue of what it
//!   reported since the loop's last turn (ADR-0247);
//! - `activity`: the Activity reached through JNI, which every feature asks
//!   through the same call;
//! - [`activity_callbacks`]: the Activity's lifecycle callbacks as the looper
//!   hands them to the hook the table registers, and what it delivers for
//!   each;
//! - [`ndk`]: the API level and the NDK functions that exist only from a given
//!   level on, found by name when the platform is built;
//! - [`poll`]: how long one turn of the loop may block (ADR-0249).
//!
//! The window is the window feature's (`window::android`): the loop holds it as
//! an `ActivityWindow`, with the Activity's lifecycle facts (`ActivityFlags`)
//! it records. The input, the displays, the text input, the preferences, the
//! lifecycle, the clipboard and drag and drop are their features': the loop
//! holds what they made through the hooks the host's table fills
//! (`AndroidHost`), and the view serves several features, so what it hands a
//! feature's state is named by the table, not by the view.

#[cfg(target_os = "android")]
pub(crate) mod activity;
pub(crate) mod activity_callbacks;
pub(crate) mod bridge_slot;
#[cfg(target_os = "android")]
pub(crate) mod java_view;
#[cfg(target_os = "android")]
mod looper;
pub(crate) mod ndk;
pub(crate) mod poll;

#[cfg(target_os = "android")]
pub(crate) use looper::{ActivityFlags, ActivityRef, ActivityWindow, AndroidLoop};

#[cfg(target_os = "android")]
pub(crate) use hooks::{ActivityInput, ActivityPreferences, AndroidHost, HostDisplays};

/// What the host's table fills for the loop.
#[cfg(target_os = "android")]
mod hooks {
    use std::sync::{Arc, Weak};

    use android_activity::AndroidApp;
    use android_activity::ndk::native_window::NativeWindow;
    use flui_platform::{DisplayId, DisplaySnapshot, FrameRateHint, PlatformInput};

    use super::activity_callbacks::ActivityCallbacks;
    use super::java_view::{InputMethod, JavaView, ViewTransfer};
    use super::ndk::SymbolSource;
    use crate::error::PlatformError;
    use crate::owner::app_delivery::AppDelivery;
    use crate::owner::notices::NoticeChannel;
    use crate::owner::signal::OwnerSignal;
    use crate::owner::sources::SystemPreferenceSource;
    use crate::owner::watch::Watch;
    use crate::version::gate::android::AndroidApi;

    /// What an Android host's table names for its owner thread (ADR-0151
    /// §10): the displays, the preference source, the notice channel, the
    /// meaning of the Activity's callbacks, the clipboard and the
    /// drag-and-drop transport the loop holds, and the features' objects
    /// `FluiView` hands what it reports. The owner reaches them through the
    /// hooks and names no feature.
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

        /// The clipboard, made once per loop: the loop holds it and answers
        /// the table's clipboard position from it.
        fn clipboard() -> Arc<dyn crate::host::SharedClipboard>;

        /// The drag-and-drop transport of `view`, `FluiView`'s Rust side,
        /// made as the view starts: the view hands it the drag events its
        /// Java side reports, and the loop answers the table's drag position
        /// from it.
        fn transfer(view: Weak<JavaView>) -> Arc<dyn ViewTransfer>;

        /// The system preference source of `app`'s Activity at `api`, for the
        /// table's preference position; `signal` wakes the owner when a read
        /// owes the application a delivery.
        fn preferences(
            app: &AndroidApp,
            api: AndroidApi,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn ActivityPreferences>;

        /// The channel the trim level and the low-memory call take to the
        /// application.
        fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

        /// What each of the Activity's lifecycle callbacks means for `app`.
        fn activity(app: &AndroidApp) -> Box<dyn ActivityCallbacks>;

        /// The input of `app`'s queue: the loop hands it each event it
        /// drains and answers the table's input position.
        fn input() -> Box<dyn ActivityInput>;
    }

    /// The input the Activity's queue carries, which the table makes
    /// ([`AndroidHost::input`]): the loop drains the queue and hands each
    /// event here, dispatches what comes back to its window, and asks for
    /// the contacts a lifecycle step strands to be ended. Nothing here runs
    /// user code.
    pub(crate) trait ActivityInput: Send + Sync {
        /// What `event` means for a window at `scale`, read from `app`.
        fn motion(
            &self,
            app: &AndroidApp,
            event: &android_activity::input::MotionEvent<'_>,
            scale: f64,
        ) -> Vec<PlatformInput>;

        /// Whether `event` is a hover of TalkBack's explore by touch: its
        /// `MotionEvent` action code and position in pixels, which the loop
        /// offers the window's accessibility bridge before it reads the event
        /// as input.
        fn touch_exploration(
            &self,
            event: &android_activity::input::MotionEvent<'_>,
        ) -> Option<(i32, f32, f32)>;

        /// What `event` means; `composing` says whether the input method
        /// composes now, so a key it passed on is still its own.
        fn key(
            &self,
            app: &AndroidApp,
            event: &android_activity::input::KeyEvent<'_>,
            composing: bool,
        ) -> Option<PlatformInput>;

        /// What the decoding held that is due by the end of a drain.
        fn finish_drain(&self) -> Vec<PlatformInput>;

        /// End every contact: the Activity paused, lost focus or is
        /// destroyed.
        fn focus_lost(&self) -> Vec<PlatformInput>;

        /// End every contact: the native window it was drawn on is gone.
        fn capture_lost(&self) -> Vec<PlatformInput>;

        /// Read the system's scroll factors again for `app` at `api`: a
        /// setting or the configuration changed, or the Activity resumed.
        fn refresh_scroll_factors(&self, app: &AndroidApp, api: AndroidApi);
    }

    /// The system preference source the Android loop keeps (ADR-0172): read
    /// again on a configuration change, on `Resume` and when `FluiView`
    /// reports a changed setting, and by the table's preference position.
    pub(crate) trait ActivityPreferences: SystemPreferenceSource + Send + Sync {
        /// When a read that failed is due again; the looper polls no longer
        /// than that.
        fn retry_deadline(&self) -> Option<web_time::Instant>;

        /// Whether the Activity's configuration is in night mode now (the
        /// system dark theme, or the app's override of it), as the source
        /// reads it; the window reports it as its appearance.
        ///
        /// # Errors
        /// The Java read failed.
        fn night_mode(&self) -> Result<bool, flui_platform::PreferenceQueryError>;

        /// Device pixels per logical pixel, from the Activity's display
        /// metrics, as the source reads them.
        ///
        /// # Errors
        /// The Java read failed or reported no usable density.
        fn pixel_ratio(
            &self,
        ) -> Result<flui_foundation::geometry::DevicePixelRatio, flui_platform::PreferenceQueryError>;

        /// The touch gesture geometry the system's view configuration
        /// reports now, which the window answers its presentation's query
        /// with.
        ///
        /// # Errors
        /// The Java read failed or reported an invalid geometry.
        fn gesture_geometry(
            &self,
        ) -> Result<flui_platform::GestureGeometry, flui_platform::PreferenceQueryError>;
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

        /// The window's `frame_rate_hint` capability: `Native` exactly where
        /// this system takes a refresh-rate request (`ANativeWindow_setFrameRate`,
        /// API 30).
        fn frame_rate_hint(&self) -> flui_platform::Availability;

        /// `WindowManager.isCrossWindowBlurEnabled` at the last sample: the
        /// device blurs behind windows now. False below API 31 and before the
        /// first sample.
        fn cross_window_blur(&self) -> bool;

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

/// The identity of one window of the Activity: the window the Activity's next
/// surface brings never takes an earlier one's, so what a feature keeps of a
/// window, or a reply made for it, cannot reach the window after it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ActivityWindowId(std::num::NonZeroU64);

/// The identities the loop issues, in increasing order, without wrapping:
/// once the last is issued, no window opens.
#[derive(Debug, Default)]
pub(crate) struct ActivityWindowIds(std::sync::atomic::AtomicU64);

impl ActivityWindowIds {
    /// The next identity; `None` once the space is used up.
    pub(crate) fn next(&self) -> Option<ActivityWindowId> {
        use std::sync::atomic::Ordering;
        let previous = self
            .0
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |issued| {
                issued.checked_add(1)
            })
            .ok()?;
        let issued = previous.checked_add(1)?;
        std::num::NonZeroU64::new(issued).map(ActivityWindowId)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use super::ActivityWindowIds;

    /// Each window of the Activity gets an identity no other had, and the
    /// last one is the last: the space is refused, never wrapped onto a
    /// window a reply or a feature's state still names.
    #[test]
    fn activity_window_identities_never_repeat_or_wrap() {
        let ids = ActivityWindowIds::default();
        let first = ids.next().expect("the first identity");
        let second = ids.next().expect("the second identity");
        assert_ne!(first, second);

        let near_the_end = ActivityWindowIds(AtomicU64::new(u64::MAX - 1));
        assert!(near_the_end.next().is_some(), "the last identity is issued");
        assert_eq!(near_the_end.next(), None, "and none after it");
        assert_eq!(near_the_end.next(), None, "ever");
    }
}
