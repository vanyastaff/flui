//! The Android host: NativeActivity through `android-activity`, and the
//! `FluiView` it installs (ADR-0247).

use std::sync::{Arc, Weak};

use android_activity::AndroidApp;

use crate::display::android::AndroidDisplays;
use crate::host::{Absent, Host, position};
use crate::owner::android::ndk::SymbolSource;
use crate::owner::android::{AndroidHost, AndroidLoop, HostDisplays};
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;
use crate::platforms::android::{
    AndroidHaptics, AndroidInputState, AndroidSystemChrome, AndroidTextInput, AndroidTransfer,
    AndroidWindow, MockClipboard,
};
use crate::shared::os_version::android::AndroidApi;

/// The Android host's table.
#[derive(Debug)]
pub struct Android;

impl Host for Android {
    const NAME: &'static str = "Android";

    type Loop = AndroidLoop;
    type Window = AndroidWindow;
    type Displays = AndroidLoop;
    type Input = AndroidInputState;
    type Text = AndroidTextInput;
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = MockClipboard;
    type Drag = AndroidTransfer;
    type Haptics = AndroidHaptics;
    type Preferences = AndroidLoop;
    type Lifecycle = AndroidLoop;
    type Chrome = AndroidSystemChrome;
    type Storage = AndroidLoop;
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

impl AndroidHost for Android {
    fn displays(
        api: AndroidApi,
        library: Arc<dyn SymbolSource>,
        owner: Weak<OwnerSignal>,
    ) -> Arc<dyn HostDisplays> {
        Arc::new(AndroidDisplays::new(api, library, owner))
    }
}

impl HostPlatform<Android> {
    /// Create a new Android platform from the `AndroidApp` provided by
    /// `android_main()`
    pub fn new(app: AndroidApp) -> Self {
        Self::from_loop(AndroidLoop::new::<Android>(app))
    }
}
