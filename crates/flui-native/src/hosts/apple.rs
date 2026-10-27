//! The Apple host: macOS (AppKit) and iOS (UIKit) share one table, and the
//! target operating system picks the type that fills each position.

use std::sync::Arc;

#[cfg(target_os = "macos")]
use crate::accessibility::apple::MacosAccessibility;
#[cfg(target_os = "ios")]
use crate::clipboard::apple::IOSClipboard;
#[cfg(target_os = "macos")]
use crate::clipboard::apple::MacOSClipboard;
#[cfg(target_os = "ios")]
use crate::display::apple::ios::IosDisplays;
#[cfg(target_os = "macos")]
use crate::display::apple::macos::MacosDisplays;
use crate::error::PlatformError;
#[cfg(target_os = "ios")]
use crate::haptics::apple::IOSHaptics;
#[cfg(target_os = "macos")]
use crate::haptics::apple::MacHaptics;
use crate::host::{Absent, Host, position};
#[cfg(target_os = "ios")]
use crate::input::apple::ios::TouchInputState;
#[cfg(target_os = "macos")]
use crate::input::apple::macos::ViewInputs;
#[cfg(target_os = "macos")]
use crate::lifecycle::apple::macos::SystemNoticeSources;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::app_delivery::AppDelivery;
#[cfg(target_os = "ios")]
use crate::owner::apple::IOSLoop;
#[cfg(target_os = "macos")]
use crate::owner::apple::MacOSLoop;
#[cfg(target_os = "ios")]
use crate::owner::apple::ios::{IosHost, UIKitPreferences, ViewTouches};
#[cfg(target_os = "macos")]
use crate::owner::apple::macos::content_view::ViewHooks;
#[cfg(target_os = "macos")]
use crate::owner::apple::macos::{LoopControl, MacosHost, NoticeSources};
use crate::owner::notices::NoticeChannel;
use crate::owner::signal::OwnerSignal;
#[cfg(target_os = "macos")]
use crate::owner::sources::SystemPreferenceSource;
use crate::platform::HostPlatform;
#[cfg(target_os = "ios")]
use crate::preferences::apple::ios::IOSPreferences;
#[cfg(target_os = "macos")]
use crate::preferences::apple::macos::AppKitPreferences;
use crate::storage::UserDataDirs;
#[cfg(target_os = "ios")]
use crate::text::apple::ios::IosTextSizing;
#[cfg(target_os = "macos")]
use crate::text::apple::macos::{MacOSTextInput, TextView};
#[cfg(target_os = "ios")]
use crate::window::apple::ios::{IOSSystemChrome, IOSWindow, IOSWindows};
#[cfg(target_os = "macos")]
use crate::window::apple::macos::{MacOSWindow, MacOSWindows};
#[cfg(target_os = "macos")]
use crate::window::config::WindowConfiguration;

/// The Apple host's table.
#[derive(Debug)]
pub struct Apple;

impl Host for Apple {
    #[cfg(target_os = "macos")]
    const NAME: &'static str = "macOS (AppKit)";
    #[cfg(target_os = "ios")]
    const NAME: &'static str = "iOS (UIKit)";

    #[cfg(target_os = "macos")]
    type Loop = MacOSLoop;
    #[cfg(target_os = "ios")]
    type Loop = IOSLoop;

    #[cfg(target_os = "macos")]
    type Window = MacOSWindow;
    #[cfg(target_os = "ios")]
    type Window = IOSWindow;

    #[cfg(target_os = "macos")]
    type Displays = MacOSLoop;
    #[cfg(target_os = "ios")]
    type Displays = IOSLoop;

    #[cfg(target_os = "macos")]
    type Input = MacOSLoop;
    #[cfg(target_os = "ios")]
    type Input = IOSLoop;

    #[cfg(target_os = "macos")]
    type Text = MacOSTextInput;
    #[cfg(target_os = "ios")]
    type Text = IosTextSizing;

    // UIKit windows hand out no accessibility yet.
    #[cfg(target_os = "macos")]
    type Accessibility = MacosAccessibility;
    #[cfg(target_os = "ios")]
    type Accessibility = Absent<position::Accessibility>;

    #[cfg(target_os = "macos")]
    type Clipboard = MacOSClipboard;
    #[cfg(target_os = "ios")]
    type Clipboard = IOSClipboard;

    type Drag = Absent<position::Drag>;

    #[cfg(target_os = "macos")]
    type Haptics = MacHaptics;
    #[cfg(target_os = "ios")]
    type Haptics = IOSHaptics;

    #[cfg(target_os = "macos")]
    type Preferences = MacOSLoop;
    #[cfg(target_os = "ios")]
    type Preferences = IOSLoop;

    #[cfg(target_os = "macos")]
    type Lifecycle = MacOSLoop;
    #[cfg(target_os = "ios")]
    type Lifecycle = IOSLoop;

    #[cfg(target_os = "macos")]
    type Chrome = Absent<position::Chrome>;
    #[cfg(target_os = "ios")]
    type Chrome = IOSSystemChrome;

    type Storage = UserDataDirs;
}

/// The macOS platform: AppKit windows, the `NSPasteboard` clipboard and the
/// `NSApplication` run loop.
///
/// The windows are AppKit's (`window::apple::macos`); their input reaches each
/// window's content view (`owner::apple::macos::content_view`), which hands
/// its events to the input (`input::apple::macos`) and its
/// `NSTextInputClient` methods to the text input (`text::apple::macos`,
/// ADR-0069): `keyDown:` is a gate, so one press reaches the application
/// exactly once, either as a composition or commit or as a key event. The displays are read
/// from `NSScreen.screens`, each keyed by its UUID with the refresh period of
/// its current mode, and read again after
/// `NSApplicationDidChangeScreenParametersNotification`. AppKit has no
/// `ControlFlow::WaitUntil`, so the loop's wake pump actuates the registered
/// wake deadline (`owner::apple::macos`).
///
/// ```rust,ignore
/// use flui_native::{MacOSPlatform, Platform};
///
/// let platform = MacOSPlatform::new()?;
/// Box::new(platform).run(Box::new(|_owner| Ok(())))?;
/// ```
#[cfg(target_os = "macos")]
pub type MacOSPlatform = HostPlatform<Apple>;

/// The iOS platform: UIKit scenes and `UIApplicationMain`.
///
/// `UIApplicationMain` owns the main thread for the process's life, and UIKit
/// is main-thread-only, so every window, view and pasteboard access happens on
/// that thread; the only background work is Grand Central Dispatch's (the
/// loop's executor). The loop runs the bootstrap from the
/// application delegate and attaches each scene's window from the scene
/// delegate (`owner::apple::ios`); a `CADisplayLink` paces the frames.
///
/// The target is iOS 13. `objc2`'s bindings span later releases, so a call
/// the SDK marks newer than 13 takes the proof a version gate hands out
/// (`version::gate`, ADR-0244): `CADisplayLink.preferredFrameRateRange` (iOS
/// 15) is one, and before iOS 15 the link gets `preferredFramesPerSecond`.
#[cfg(target_os = "ios")]
pub type IOSPlatform = HostPlatform<Apple>;

#[cfg(target_os = "macos")]
impl MacosHost for Apple {
    fn displays() -> Arc<dyn crate::owner::apple::macos::HostDisplays> {
        Arc::new(MacosDisplays::new())
    }

    fn preferences(signal: &Arc<OwnerSignal>) -> Box<dyn SystemPreferenceSource> {
        Box::new(AppKitPreferences::new(signal))
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    fn notice_sources(
        notices: &Arc<dyn NoticeChannel>,
        control: &Arc<LoopControl>,
    ) -> Box<dyn NoticeSources> {
        Box::new(SystemNoticeSources::new(notices, control))
    }

    fn view_hooks() -> ViewHooks {
        ViewHooks {
            input: || Box::new(ViewInputs::default()),
            text: || Box::new(TextView::default()),
        }
    }
}

#[cfg(target_os = "ios")]
impl IosHost for Apple {
    fn displays(
        marker: objc2::MainThreadMarker,
    ) -> Arc<dyn crate::owner::apple::ios::HostDisplays> {
        Arc::new(IosDisplays::new(marker))
    }

    fn preferences(
        signal: &Arc<OwnerSignal>,
        marker: objc2::MainThreadMarker,
    ) -> Box<dyn UIKitPreferences> {
        Box::new(IOSPreferences::new(signal, marker))
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    fn view_touches() -> Box<dyn ViewTouches> {
        Box::new(TouchInputState::default())
    }
}

#[cfg(target_os = "macos")]
impl HostPlatform<Apple> {
    /// Create a new macOS platform.
    pub fn new() -> Result<Self, PlatformError> {
        // The loop keeps the open windows itself; what they share is the
        // configuration they open with.
        Self::try_from_parts(
            |positions| MacOSWindows::new(WindowConfiguration::default(), positions),
            |_| MacOSLoop::new::<Apple>(),
        )
    }
}

#[cfg(target_os = "ios")]
impl HostPlatform<Apple> {
    /// Create a new iOS platform.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Init`] if called off the main thread, which is
    /// where UIKit requires every window and view operation to run.
    pub fn new() -> Result<Self, PlatformError> {
        Self::try_from_parts(IOSWindows::new, |windows| {
            IOSLoop::new::<Apple>(Arc::clone(windows) as _)
        })
    }
}
