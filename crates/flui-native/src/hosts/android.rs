//! The Android host: NativeActivity through `android-activity`, and the
//! `FluiView` it installs (ADR-0247).

use std::sync::{Arc, Weak};

use android_activity::AndroidApp;

use crate::clipboard::android::MockClipboard;
use crate::display::android::AndroidDisplays;
use crate::drag::android::AndroidTransfer;
use crate::haptics::android::AndroidHaptics;
use crate::host::{Absent, Host, position};
use crate::lifecycle::android::ActivityRule;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::android::activity_callbacks::ActivityCallbacks;
use crate::owner::android::java_view::{InputMethod, JavaView, ViewTransfer};
use crate::owner::android::ndk::SymbolSource;
use crate::owner::android::{ActivityPreferences, AndroidHost, AndroidLoop, HostDisplays};
use crate::owner::notices::NoticeChannel;
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;
use crate::platforms::android::{AndroidInputState, AndroidSystemChrome, AndroidWindow};
use crate::preferences::android::AndroidPreferences;
use crate::shared::os_version::android::AndroidApi;
use crate::text::android::{AndroidTextInput, ViewInputMethod};

/// The Android host's table.
#[derive(Debug)]
pub struct Android;

impl Host for Android {
    const NAME: &'static str = "Android";

    type Loop = AndroidLoop;
    type Window = AndroidWindow;
    type Displays = AndroidLoop;
    type Input = AndroidInputState;
    type Text = std::rc::Rc<AndroidTextInput>;
    type Accessibility = Absent<position::Accessibility>;
    // The loop holds the clipboard and the transport `AndroidHost` makes.
    type Clipboard = AndroidLoop;
    type Drag = AndroidLoop;
    type Haptics = AndroidHaptics;
    type Preferences = AndroidLoop;
    type Lifecycle = AndroidLoop;
    type Chrome = AndroidSystemChrome;
    type Storage = AndroidLoop;
}

impl AndroidHost for Android {
    /// The text feature's answer to the input method's calls.
    fn input_method() -> Box<dyn InputMethod> {
        Box::new(ViewInputMethod::default())
    }

    /// An in-process stand-in until the system clipboard is reached through
    /// JNI.
    fn clipboard() -> Arc<dyn flui_platform::Clipboard> {
        Arc::new(MockClipboard::new())
    }

    /// `FluiView`'s drag and drop.
    fn transfer(view: Weak<JavaView>) -> Arc<dyn ViewTransfer> {
        AndroidTransfer::for_view(view)
    }

    fn displays(
        api: AndroidApi,
        library: Arc<dyn SymbolSource>,
        owner: Weak<OwnerSignal>,
    ) -> Arc<dyn HostDisplays> {
        Arc::new(AndroidDisplays::new(api, library, owner))
    }

    fn preferences(
        app: &AndroidApp,
        api: AndroidApi,
        signal: &Arc<OwnerSignal>,
    ) -> Box<dyn ActivityPreferences> {
        Box::new(AndroidPreferences::new(app, api, signal))
    }

    fn notices() -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::default())
    }

    /// The lifecycle feature's reading of the Activity's callbacks.
    fn activity(app: &AndroidApp) -> Box<dyn ActivityCallbacks> {
        Box::new(ActivityRule::new(app))
    }
}

/// Android platform implementation using `android-activity`
///
/// Wraps the `AndroidApp` provided by `android_main()` and implements the
/// `Platform` trait for integration with the FLUI framework.
///
/// # Usage
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     let platform = AndroidPlatform::new(app);
///     let _ = platform.run(Box::new(|owner| {
///         // Platform ready (first Resume) — create window and renderer
///         let _ = owner;
///         Ok(())
///     }));
/// }
/// ```
pub type AndroidPlatform = HostPlatform<Android>;

impl HostPlatform<Android> {
    /// Create a new Android platform from the `AndroidApp` provided by
    /// `android_main()`
    pub fn new(app: AndroidApp) -> Self {
        Self::from_loop(AndroidLoop::new::<Android>(app))
    }
}
