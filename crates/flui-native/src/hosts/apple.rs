//! The Apple host: macOS (AppKit) and iOS (UIKit) share one table, and the
//! target operating system picks the type that fills each position.

#[cfg(target_os = "macos")]
use crate::config::WindowConfiguration;
use crate::error::PlatformError;
use crate::host::{Absent, Host, position};
#[cfg(target_os = "ios")]
use crate::owner::apple::IOSLoop;
#[cfg(target_os = "macos")]
use crate::owner::apple::MacOSLoop;
use crate::platform::HostPlatform;
#[cfg(target_os = "ios")]
use crate::platforms::ios::{
    IOSClipboard, IOSHaptics, IOSPreferences, IOSSystemChrome, IOSWindow, IosDisplays,
};
#[cfg(all(target_os = "macos", feature = "a11y"))]
use crate::platforms::macos::MacosAccessibility;
#[cfg(target_os = "macos")]
use crate::platforms::macos::{
    MacHaptics, MacOSClipboard, MacOSWindow, MacosDisplays, PreferenceSource,
};
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
    type Displays = MacosDisplays;
    #[cfg(target_os = "ios")]
    type Displays = IosDisplays;

    #[cfg(target_os = "macos")]
    type Input = MacOSLoop;
    #[cfg(target_os = "ios")]
    type Input = IOSLoop;

    #[cfg(target_os = "macos")]
    type Text = MacOSTextInput;
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
    type Preferences = PreferenceSource;
    #[cfg(target_os = "ios")]
    type Preferences = IOSPreferences;

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
impl HostPlatform<Apple> {
    /// Create a new macOS platform with default configuration
    pub fn new() -> Result<Self, PlatformError> {
        Ok(Self::from_loop(MacOSLoop::new()?))
    }

    /// Create a new macOS platform with custom configuration
    pub fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
        Ok(Self::from_loop(MacOSLoop::with_config(config)?))
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
        Ok(Self::from_loop(IOSLoop::new()?))
    }
}
