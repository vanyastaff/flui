//! The Apple host: macOS (AppKit) and iOS (UIKit) share one table, and the
//! target operating system picks the type that fills each position.

use std::sync::Arc;

#[cfg(target_os = "macos")]
use crate::config::WindowConfiguration;
#[cfg(target_os = "ios")]
use crate::display::apple::ios::IosDisplays;
#[cfg(target_os = "macos")]
use crate::display::apple::macos::MacosDisplays;
use crate::error::PlatformError;
use crate::host::{Absent, Host, position};
#[cfg(target_os = "macos")]
use crate::lifecycle::apple::macos::SystemNoticeSources;
use crate::lifecycle::notice::SystemNotices;
#[cfg(target_os = "ios")]
use crate::owner::apple::IOSLoop;
#[cfg(target_os = "macos")]
use crate::owner::apple::MacOSLoop;
#[cfg(target_os = "ios")]
use crate::owner::apple::ios::{IosHost, UIKitPreferences};
#[cfg(target_os = "macos")]
use crate::owner::apple::macos::{LoopControl, MacosHost, NoticeSources};
use crate::owner::notices::NoticeChannel;
use crate::owner::signal::OwnerSignal;
#[cfg(target_os = "macos")]
use crate::owner::sources::SystemPreferenceSource;
use crate::platform::HostPlatform;
#[cfg(target_os = "ios")]
use crate::platforms::ios::{IOSClipboard, IOSHaptics, IOSSystemChrome, IOSWindow};
#[cfg(all(target_os = "macos", feature = "a11y"))]
use crate::platforms::macos::MacosAccessibility;
#[cfg(target_os = "macos")]
use crate::platforms::macos::{MacHaptics, MacOSClipboard, MacOSWindow};
#[cfg(target_os = "ios")]
use crate::preferences::apple::ios::IOSPreferences;
#[cfg(target_os = "macos")]
use crate::preferences::apple::macos::AppKitPreferences;
use crate::storage::UserDataDirs;
#[cfg(target_os = "macos")]
use crate::text::apple::macos::MacOSTextInput;

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
    type Text = std::sync::Arc<MacOSTextInput>;
    #[cfg(target_os = "ios")]
    type Text = Absent<position::Text>;

    // The AppKit bridge exists with the `a11y` feature; UIKit windows hand out
    // no accessibility.
    #[cfg(all(target_os = "macos", feature = "a11y"))]
    type Accessibility = MacosAccessibility;
    #[cfg(not(all(target_os = "macos", feature = "a11y")))]
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
#[cfg(target_os = "macos")]
pub type MacOSPlatform = HostPlatform<Apple>;

/// The iOS platform: UIKit scenes and `UIApplicationMain`.
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

    fn notices() -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::default())
    }

    fn notice_sources(
        notices: &Arc<dyn NoticeChannel>,
        control: &Arc<LoopControl>,
    ) -> Box<dyn NoticeSources> {
        Box::new(SystemNoticeSources::new(notices, control))
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

    fn notices() -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::default())
    }
}

#[cfg(target_os = "macos")]
impl HostPlatform<Apple> {
    /// Create a new macOS platform with default configuration
    pub fn new() -> Result<Self, PlatformError> {
        Ok(Self::from_loop(MacOSLoop::new::<Apple>()?))
    }

    /// Create a new macOS platform with custom configuration
    pub fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
        Ok(Self::from_loop(MacOSLoop::with_config::<Apple>(config)?))
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
        Ok(Self::from_loop(IOSLoop::new::<Apple>()?))
    }
}
