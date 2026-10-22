//! The Android host: NativeActivity through `android-activity`, and the
//! `FluiView` it installs (ADR-0247).

use std::sync::{Arc, Weak};

use android_activity::AndroidApp;

use crate::clipboard::android::MockClipboard;
use crate::drag::android::AndroidTransfer;
use crate::haptics::android::AndroidHaptics;
use crate::host::{Absent, Host, position};
use crate::owner::android::java_view::{InputMethod, JavaView, ViewTransfer};
use crate::owner::android::{AndroidHost, AndroidLoop};
use crate::platform::HostPlatform;
use crate::platforms::android::{
    AndroidDisplays, AndroidInputState, AndroidSystemChrome, AndroidWindow,
};
use crate::text::android::{AndroidTextInput, ViewInputMethod};

/// The Android host's table.
#[derive(Debug)]
pub struct Android;

impl Host for Android {
    const NAME: &'static str = "Android";

    type Loop = AndroidLoop;
    type Window = AndroidWindow;
    type Displays = AndroidDisplays;
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
