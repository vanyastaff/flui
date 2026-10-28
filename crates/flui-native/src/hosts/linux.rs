//! The native Linux hosts' tables: Wayland's.

use std::sync::Arc;

use crate::accessibility::linux::UnixAccessibility;
use crate::display::linux::protocol::Environment;
use crate::display::linux::wayland::WaylandDisplaySource;
use crate::error::PlatformError;
use crate::executor::BackgroundExecutor;
use crate::host::{Absent, Host, position};
use crate::lifecycle::linux::HostNoticeSource;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::linux::wayland::{
    WaylandDisplays, WaylandHost, WaylandLoop, WaylandNotices, WaylandPreferences,
};
use crate::owner::notices::NoticeChannel;
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;
use crate::preferences::linux::PreferenceSource;
use crate::storage::UserDataDirs;
use crate::window::linux::wayland::{WaylandWindow, WaylandWindows};

/// The Wayland host's table.
///
/// What the host carries yet: windows and their state and commands, the
/// displays, the preferences, the lifecycle, accessibility and data
/// directories. Input, text input, the clipboard and drag and drop are
/// `Absent` until the host speaks their protocols.
#[derive(Debug)]
pub struct Wayland;

impl Host for Wayland {
    const NAME: &'static str = "Wayland";

    type Loop = WaylandLoop;
    type Window = WaylandWindow;
    type Displays = WaylandLoop;
    type Input = Absent<position::Input>;
    type Text = Absent<position::Text>;
    type Accessibility = UnixAccessibility;
    type Clipboard = Absent<position::Clipboard>;
    type Drag = Absent<position::Drag>;
    // A desktop compositor has no haptic hardware to drive.
    type Haptics = Absent<position::Haptics>;
    type Preferences = WaylandLoop;
    type Lifecycle = WaylandLoop;
    // A desktop window styles no system bars.
    type Chrome = Absent<position::Chrome>;
    type Storage = UserDataDirs;
}

impl WaylandHost for Wayland {
    fn displays() -> Arc<dyn WaylandDisplays> {
        Arc::new(WaylandDisplaySource::new())
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    fn preferences(
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn WaylandPreferences> {
        PreferenceSource::new(executor, signal)
    }

    fn host_notices(
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn WaylandNotices> {
        Arc::new(HostNoticeSource::start(executor, signal))
    }
}

/// The native Wayland platform: windows on the compositor `WAYLAND_DISPLAY`
/// names, over libwayland.
pub type WaylandPlatform = HostPlatform<Wayland>;

impl HostPlatform<Wayland> {
    /// Connect to the compositor whose socket `WAYLAND_DISPLAY` names, the
    /// one the display source reads too. The calling thread owns the
    /// platform: it opens the windows and runs the loop.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when `WAYLAND_DISPLAY` names no socket, no
    /// compositor answers on it, libwayland cannot be loaded, or the
    /// compositor lacks the globals a window needs.
    pub fn new() -> Result<Self, PlatformError> {
        let socket = Environment::current()
            .wayland_socket_path()
            .ok_or_else(|| PlatformError::Init {
                message: "Wayland: WAYLAND_DISPLAY names no socket (a bare name needs \
                          XDG_RUNTIME_DIR)"
                    .to_owned(),
            })?;
        Self::try_from_parts(WaylandWindows::new, |_| {
            WaylandLoop::new::<Wayland>(&socket)
        })
    }
}
