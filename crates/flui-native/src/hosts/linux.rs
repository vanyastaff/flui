//! The native Linux host: the X11 table and its constructor (ADR-0151 §10,
//! §11).
//!
//! The X11 host fills the window, displays, accessibility, preference,
//! lifecycle and storage positions. Input (XInput2 and XKB), text input,
//! the clipboard and drag and drop come with the host's later parts and are
//! `Absent` until then; X11 has no haptics and no system bars.

use std::sync::Arc;

use crate::accessibility::linux::UnixAccessibility;
use crate::display::linux::source::LinuxDisplays;
use crate::error::PlatformError;
use crate::host::{Absent, Host, position};
use crate::lifecycle::linux::X11Notices;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::linux::x11::{
    HostDisplays, HostNoticeStart, PreferenceStart, X11Connection, X11Host, X11Loop,
};
use crate::owner::notices::NoticeChannel;
use crate::platform::HostPlatform;
use crate::preferences::linux::X11Settings;
use crate::storage::UserDataDirs;
use crate::window::linux::x11::{WindowAtoms, X11Window, X11Windows};

/// The X11 host's table.
#[derive(Debug)]
pub struct X11;

impl Host for X11 {
    const NAME: &'static str = "X11";

    type Loop = X11Loop;
    type Window = X11Window;
    type Displays = X11Loop;
    type Input = Absent<position::Input>;
    type Text = Absent<position::Text>;
    type Accessibility = UnixAccessibility;
    type Clipboard = Absent<position::Clipboard>;
    type Drag = Absent<position::Drag>;
    type Haptics = Absent<position::Haptics>;
    type Preferences = X11Loop;
    type Lifecycle = X11Loop;
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

impl X11Host for X11 {
    /// The X server's outputs, through RandR on a connection of their own.
    fn displays() -> Arc<dyn HostDisplays> {
        LinuxDisplays::new()
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    /// The Settings portal on the session bus.
    fn preferences() -> Box<dyn PreferenceStart> {
        Box::new(X11Settings)
    }

    /// logind on the system bus and the kernel's memory pressure.
    fn host_notices() -> Box<dyn HostNoticeStart> {
        Box::new(X11Notices)
    }
}

/// The native X11 platform: windows on one connection to the X server named by
/// `DISPLAY`, read by a loop on the thread that builds it, with the server's
/// displays, the AT-SPI bridge, the Settings portal and the host's notices.
///
/// Built and run on one thread, which owns its windows. Input, text input,
/// the clipboard and drag and drop are not on it yet; `current_platform`
/// still picks the winit host on Linux.
pub type X11Platform = HostPlatform<X11>;

impl HostPlatform<X11> {
    /// Connect to the X server and build the platform on the calling thread.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when libxcb cannot be loaded, no X server
    /// answers, or the loop's wake descriptor cannot be made.
    pub fn new() -> Result<Self, PlatformError> {
        let connection = X11Connection::connect()?;
        let atoms = WindowAtoms::intern(&connection)?;
        let windows = Arc::clone(&connection);
        Self::try_from_parts(
            |positions| X11Windows::new(positions, windows, atoms),
            |windows| X11Loop::new::<X11>(connection, Arc::clone(windows) as _),
        )
    }
}
