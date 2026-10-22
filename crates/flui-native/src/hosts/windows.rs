//! The Windows host: Win32 windows, OLE drag and drop, TSF text input and the
//! Win32 message loop.

use std::rc::Rc;

use crate::config::WindowConfiguration;
use crate::error::PlatformError;
use crate::host::{Absent, Host, position};
use crate::owner::windows::broadcast::Subscriber;
use crate::owner::windows::control::OwnerObjects;
use crate::owner::windows::{WindowHooks, WindowsHost, WindowsLoop};
use crate::platform::HostPlatform;
#[cfg(feature = "a11y")]
use crate::platforms::windows::WindowsAccessibility;
use crate::platforms::windows::{
    DropTargetRegistration, HostEvents, OleTransfer, PreferenceSource, Touchpad, WindowProc,
    WindowsClipboard, WindowsDisplays, WindowsHaptics, WindowsWindow,
};
use crate::storage::UserDataDirs;
use crate::text::windows::{TextServices, Tsf};

/// The Windows host's table.
#[derive(Debug)]
pub struct Windows;

impl Host for Windows {
    const NAME: &'static str = "Windows";

    type Loop = WindowsLoop;
    type Window = WindowsWindow;
    type Displays = WindowsDisplays;
    type Input = WindowsLoop;
    type Text = Rc<TextServices>;
    #[cfg(feature = "a11y")]
    type Accessibility = WindowsAccessibility;
    #[cfg(not(feature = "a11y"))]
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = WindowsClipboard;
    type Drag = OleTransfer;
    type Haptics = WindowsHaptics;
    type Preferences = PreferenceSource;
    type Lifecycle = HostEvents;
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

impl WindowsHost for Windows {
    /// The text services, which hear a window's moves before its procedure
    /// answers them; the window's procedure; then the drop target and the
    /// precision touchpad, which only learn of each window created. A created
    /// window activates TSF, then registers its drop target, then gets its
    /// touchpad viewport.
    const WINDOW_HOOKS: WindowHooks = &[&Tsf, &WindowProc, &DropTargetRegistration, &Touchpad];

    /// The preferences (setting and display changes), then the lifecycle
    /// (session, power, lock and memory).
    fn broadcast_subscribers(owner: &OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>> {
        vec![
            Rc::new(owner.preferences.broadcasts()),
            Rc::clone(owner.lifecycle) as Rc<dyn Subscriber>,
        ]
    }
}

/// The native Windows platform: Win32 windows and message loop, OLE drag and
/// drop, TSF text input and the system's clipboard, displays and preferences.
pub type WindowsPlatform = HostPlatform<Windows>;

impl HostPlatform<Windows> {
    /// Create a new Windows platform instance with default configuration
    pub fn new() -> Result<Self, PlatformError> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new Windows platform instance with custom configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Window configuration (hotkeys, debouncing, fullscreen
    ///   behavior)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_native::{WindowsPlatform, WindowConfiguration, FullscreenMonitor};
    ///
    /// // Disable F11 hotkey
    /// let config = WindowConfiguration::no_hotkey();
    /// let platform = WindowsPlatform::with_config(config)?;
    ///
    /// // Use primary monitor for fullscreen
    /// let config = WindowConfiguration {
    ///     fullscreen_monitor: FullscreenMonitor::Primary,
    ///     ..Default::default()
    /// };
    /// let platform = WindowsPlatform::with_config(config)?;
    /// ```
    pub fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
        Ok(Self::from_loop(WindowsLoop::with_config::<Windows>(
            config,
        )?))
    }
}
