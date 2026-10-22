//! The winit host: windows, input and the event loop from winit, on Windows,
//! macOS and Linux.

use std::sync::Arc;

use flui_platform::Clipboard;

#[cfg(all(target_os = "linux", feature = "a11y"))]
use crate::accessibility::linux::UnixAccessibility;
use crate::clipboard::winit::ArboardClipboard;
#[cfg(target_os = "macos")]
use crate::display::apple::macos::MacosDisplays;
#[cfg(windows)]
use crate::display::windows::WindowsDisplays;
#[cfg(target_os = "linux")]
use crate::display::winit::WinitDisplaySource;
use crate::drag::winit::WinitDataTransfer;
use crate::host::{Absent, Host, position};
#[cfg(target_os = "linux")]
use crate::lifecycle::linux::WinitNotices;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::notices::NoticeChannel;
#[cfg(target_os = "linux")]
use crate::owner::winit::HostNoticeStart;
use crate::owner::winit::{FileDrops, HostDisplays, PreferenceStart, WinitHost, WinitLoop};
use crate::platform::HostPlatform;
#[cfg(target_os = "macos")]
use crate::preferences::apple::macos::WinitSettings;
#[cfg(target_os = "linux")]
use crate::preferences::linux::WinitSettings;
#[cfg(windows)]
use crate::preferences::windows::WinitSettings;
use crate::storage::UserDataDirs;
use crate::text::winit::WinitTextInput;
#[cfg(target_os = "linux")]
use crate::window::linux::x11::connect_window_manager;
use crate::window::winit::{WinitWindow, WinitWindows};

/// The winit host's table.
#[derive(Debug)]
pub struct Winit;

impl Host for Winit {
    #[cfg(target_os = "windows")]
    const NAME: &'static str = "Winit (Windows)";
    #[cfg(target_os = "macos")]
    const NAME: &'static str = "Winit (macOS)";
    #[cfg(target_os = "linux")]
    const NAME: &'static str = "Winit (Linux)";

    type Loop = WinitLoop;
    type Window = WinitWindow;
    type Displays = WinitLoop;
    type Input = WinitLoop;
    type Text = std::sync::Arc<WinitTextInput>;
    // The AT-SPI bridge exists on Linux with the `a11y` feature.
    #[cfg(all(target_os = "linux", feature = "a11y"))]
    type Accessibility = UnixAccessibility;
    #[cfg(not(all(target_os = "linux", feature = "a11y")))]
    type Accessibility = Absent<position::Accessibility>;
    type Clipboard = WinitLoop;
    type Drag = WinitLoop;
    // Desktop winit targets have no haptic hardware to drive.
    type Haptics = Absent<position::Haptics>;
    // winit reports no preferences: the loop reads the source of the system
    // it runs on, which the table registers.
    type Preferences = WinitLoop;
    // Only Linux's login manager and kernel report lifecycle events through
    // this host; on Windows and macOS winit reports none.
    #[cfg(target_os = "linux")]
    type Lifecycle = WinitLoop;
    #[cfg(not(target_os = "linux"))]
    type Lifecycle = Absent<position::Lifecycle>;
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

/// The winit host's display source: the native one of the operating system,
/// the Win32 host's monitors, the AppKit host's screens, or the compositor's
/// outputs on Linux.
#[cfg(windows)]
type NativeDisplays = WindowsDisplays;
#[cfg(target_os = "macos")]
type NativeDisplays = MacosDisplays;
#[cfg(target_os = "linux")]
type NativeDisplays = WinitDisplaySource;

impl WinitHost for Winit {
    fn displays() -> Arc<dyn HostDisplays> {
        Arc::new(NativeDisplays::new())
    }

    fn notices() -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::default())
    }

    /// The source of the system the host runs on.
    fn preferences() -> Box<dyn PreferenceStart> {
        Box::new(WinitSettings)
    }

    #[cfg(target_os = "linux")]
    fn host_notices() -> Box<dyn HostNoticeStart> {
        Box::new(WinitNotices)
    }

    /// The system clipboard through `arboard`, or an inert one where it
    /// cannot open (a headless session).
    fn clipboard() -> Arc<dyn Clipboard> {
        ArboardClipboard::or_inert()
    }

    /// The files winit reports dragged and dropped.
    fn drag() -> Arc<dyn FileDrops> {
        Arc::new(WinitDataTransfer::new())
    }
}

/// The winit platform: the Linux host until the native Wayland and X11 host
/// replaces it, and an opt-in alternative to the native hosts on Windows and
/// macOS, with winit's windows and event loop.
///
/// # Usage
///
/// ```rust,ignore
/// let platform = WinitPlatform::new();
/// platform.run(Box::new(|owner| {
///     // `open_window` is guaranteed `Ready` here — see the module-level
///     // doc on same-thread window creation.
///     let _window = owner.open_window(Default::default())?.try_ready()?;
///     Ok(())
/// }))?;
/// ```
pub type WinitPlatform = HostPlatform<Winit>;

impl HostPlatform<Winit> {
    /// Create a new winit platform
    pub fn new() -> Self {
        // On Linux the X11 probe reads what winit does not report of a window.
        let windows = WinitWindows::new(
            #[cfg(target_os = "linux")]
            connect_window_manager,
        );
        Self::from_parts(windows, |windows| {
            WinitLoop::new::<Winit>(Arc::clone(windows) as _)
        })
    }
}

impl Default for HostPlatform<Winit> {
    fn default() -> Self {
        Self::new()
    }
}
