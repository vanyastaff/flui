//! The Windows host: Win32 windows, OLE drag and drop, TSF text input and the
//! Win32 message loop.

use std::rc::Rc;
use std::sync::Arc;

use crate::config::WindowConfiguration;
use crate::display::windows::{DisplayBroadcasts, WindowsDisplays};
use crate::error::PlatformError;
use crate::host::{Absent, Host, position};
use crate::owner::windows::broadcast::Subscriber;
use crate::owner::windows::control::OwnerObjects;
use crate::owner::windows::{HostDisplays, WindowHooks, WindowsHost, WindowsLoop};
use crate::platform::HostPlatform;
#[cfg(feature = "a11y")]
use crate::platforms::windows::WindowsAccessibility;
use crate::platforms::windows::{
    HostEvents, OleTransfer, PreferenceSource, TextServices, Touchpad, WindowProc,
    WindowsClipboard, WindowsHaptics, WindowsWindow,
};
use crate::storage::UserDataDirs;

/// The Windows host's table.
#[derive(Debug)]
pub struct Windows;

impl Host for Windows {
    const NAME: &'static str = "Windows";

    type Loop = WindowsLoop;
    type Window = WindowsWindow;
    type Displays = WindowsLoop;
    type Input = WindowsLoop;
    type Text = TextServices;
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
    /// The window's procedure, then the precision touchpad, which only learns
    /// of each window created.
    const WINDOW_HOOKS: WindowHooks = &[&WindowProc, &Touchpad];

    /// The displays (display and work-area changes) and the preferences
    /// (setting changes) only observe, so their order carries no meaning; the
    /// lifecycle (session, power, lock and memory) answers the session-end
    /// query and the power broadcast, which nothing else hears.
    fn broadcast_subscribers(owner: &OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>> {
        vec![
            Rc::new(DisplayBroadcasts::new(owner.displays, owner.signal)),
            Rc::new(owner.preferences.broadcasts()),
            Rc::clone(owner.lifecycle) as Rc<dyn Subscriber>,
        ]
    }

    fn displays() -> Arc<dyn HostDisplays> {
        Arc::new(WindowsDisplays::new())
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
