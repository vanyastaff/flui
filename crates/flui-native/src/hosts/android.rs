//! The Android host: NativeActivity through `android-activity`, and the
//! `FluiView` it installs (ADR-0247).

use std::sync::{Arc, Weak};

use android_activity::AndroidApp;

use crate::accessibility::android::AndroidAccessibility;
use crate::clipboard::{MemoryClipboard, SharedClipboard};
use crate::display::android::AndroidDisplays;
use crate::drag::android::AndroidTransfer;
use crate::haptics::android::AndroidHaptics;
use crate::host::Host;
use crate::input::android::AndroidInput;
use crate::lifecycle::android::ActivityRule;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::android::activity_callbacks::ActivityCallbacks;
use crate::owner::android::java_view::{InputMethod, JavaView, ViewTransfer};
use crate::owner::android::ndk::SymbolSource;
use crate::owner::android::{
    ActivityInput, ActivityPreferences, AndroidHost, AndroidLoop, HostDisplays,
};
use crate::owner::app_delivery::AppDelivery;
use crate::owner::notices::NoticeChannel;
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;
use crate::preferences::android::AndroidPreferences;
use crate::text::android::ViewInputMethod;
use crate::version::gate::android::AndroidApi;
use crate::window::android::{AndroidWindow, system_chrome::AndroidSystemChrome};

/// The Android host's table.
#[derive(Debug)]
pub struct Android;

impl Host for Android {
    const NAME: &'static str = "Android";

    type Loop = AndroidLoop;
    type Window = AndroidWindow;
    type Displays = AndroidLoop;
    type Input = AndroidLoop;
    type Text = ViewInputMethod;
    type Accessibility = AndroidAccessibility;
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
    fn clipboard() -> Arc<dyn SharedClipboard> {
        Arc::new(MemoryClipboard::new())
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
    ) -> Arc<dyn ActivityPreferences> {
        Arc::new(AndroidPreferences::new(app, api, signal))
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    /// The lifecycle feature's reading of the Activity's callbacks.
    fn activity(app: &AndroidApp) -> Box<dyn ActivityCallbacks> {
        Box::new(ActivityRule::new(app))
    }

    /// The input feature's translation of the Activity's queue.
    fn input() -> Box<dyn ActivityInput> {
        Box::new(AndroidInput::default())
    }
}

impl HostPlatform<Android> {
    /// The Android platform over the Activity `android_main` was handed; the
    /// crate root's `android_platform` is its one caller.
    pub(crate) fn new(app: AndroidApp) -> Self {
        Self::from_parts(|positions| positions, |_| AndroidLoop::new::<Android>(app))
    }
}
