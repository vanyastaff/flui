//! The native Linux hosts' tables: Wayland's and X11's (ADR-0151 §10, §11).
//!
//! Both fill the window, displays, accessibility, preference, lifecycle and
//! storage positions, and keep the session's sources (the Settings portal,
//! logind, the kernel's memory pressure) in the same shape
//! (`owner::linux`). Both fill the input position: Wayland with its seats'
//! input, X11 with XInput 2 and XKB. Wayland fills the clipboard position
//! with its seats' selection, X11 with the `CLIPBOARD` selection on a
//! connection of its own. Wayland fills the text position with its seats'
//! `zwp_text_input_v3`. X11's text input, its drag and drop and its primary
//! selection are `Absent` until it speaks their protocols; a desktop has no
//! haptics and no system bars.

use std::sync::Arc;

use crate::accessibility::linux::UnixAccessibility;
use crate::error::PlatformError;
use crate::executor::BackgroundExecutor;
use crate::host::{Absent, Host, position};
use crate::lifecycle::linux::HostNoticeSource;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::linux::{LinuxNotices, LinuxPreferences};
use crate::owner::notices::NoticeChannel;
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;
use crate::preferences::linux::PreferenceSource;
use crate::storage::UserDataDirs;

#[cfg(wayland_host)]
pub use wayland::WaylandPlatform;
#[cfg(x11_host)]
pub use x11::X11Platform;

/// The channel both hosts' notices take to the application.
fn system_notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
    Arc::new(SystemNotices::new(Arc::clone(app)))
}

/// The Settings portal on the session bus, for both hosts.
fn portal(executor: &BackgroundExecutor, signal: &Arc<OwnerSignal>) -> Arc<dyn LinuxPreferences> {
    PreferenceSource::new(executor, signal)
}

/// logind on the system bus and the kernel's memory pressure, for both hosts.
fn session_notices(
    executor: &BackgroundExecutor,
    signal: &Arc<OwnerSignal>,
) -> Arc<dyn LinuxNotices> {
    Arc::new(HostNoticeSource::start(executor, signal))
}

#[cfg(wayland_host)]
mod wayland {
    use std::sync::Arc;

    use super::{
        Absent, AppDelivery, BackgroundExecutor, Host, HostPlatform, LinuxNotices,
        LinuxPreferences, NoticeChannel, OwnerSignal, PlatformError, UnixAccessibility,
        UserDataDirs, position,
    };
    use crate::clipboard::linux::wayland::SeatClipboard;
    use crate::display::linux::protocol::Environment;
    use crate::display::linux::wayland::WaylandDisplaySource;
    use crate::input::linux::wayland::SeatInput;
    use crate::owner::linux::wayland::{
        LastInput, WaylandClipboard, WaylandDisplays, WaylandHost, WaylandInput, WaylandLoop,
    };
    use crate::text::linux::wayland::{SeatTexts, WaylandText};
    use crate::window::linux::wayland::{WaylandWindow, WaylandWindows};

    /// The Wayland host's table.
    #[derive(Debug)]
    pub struct Wayland;

    impl Host for Wayland {
        const NAME: &'static str = "Wayland";

        type Loop = WaylandLoop;
        type Window = WaylandWindow;
        type Displays = WaylandLoop;
        type Input = WaylandLoop;
        type Text = WaylandText;
        type Accessibility = UnixAccessibility;
        type Clipboard = WaylandLoop;
        type PrimarySelection = WaylandLoop;
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

        /// The keyboards, pointers, touchscreens, tablet tools and touchpad
        /// pinches of every seat, with the cursor each window shows, and the
        /// text inputs that reach each window's text.
        fn input(last_input: LastInput) -> Arc<dyn WaylandInput> {
            Arc::new(SeatInput::new(last_input, Arc::new(SeatTexts::new())))
        }

        /// The seats' selection, through `wl_data_device`.
        fn clipboard(last_input: LastInput) -> Arc<dyn WaylandClipboard> {
            Arc::new(SeatClipboard::new(last_input))
        }

        fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
            super::system_notices(app)
        }

        fn preferences(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxPreferences> {
            super::portal(executor, signal)
        }

        fn host_notices(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxNotices> {
            super::session_notices(executor, signal)
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
        pub fn try_new() -> Result<Self, PlatformError> {
            let socket = Environment::current()
                .wayland_socket_path()
                .ok_or_else(|| PlatformError::Init {
                    message: "Wayland: WAYLAND_DISPLAY names no socket (a bare name needs \
                              XDG_RUNTIME_DIR)"
                        .to_owned(),
                })?;
            Self::try_from_parts(WaylandWindows::new, |_| {
                WaylandLoop::try_new::<Wayland>(&socket)
            })
        }
    }
}

#[cfg(x11_host)]
mod x11 {
    use std::sync::Arc;

    use super::{
        Absent, AppDelivery, BackgroundExecutor, Host, HostPlatform, LinuxNotices,
        LinuxPreferences, NoticeChannel, OwnerSignal, PlatformError, UnixAccessibility,
        UserDataDirs, position,
    };
    use crate::clipboard::linux::x11::X11Clipboard;
    use crate::display::linux::source::LinuxDisplays;
    use crate::host::SharedClipboard;
    use crate::input::linux::x11::X11Input;
    use crate::owner::linux::x11::{HostDisplays, LoopInput, X11Connection, X11Host, X11Loop};
    use crate::window::linux::x11::{WindowAtoms, X11Window, X11Windows};

    /// The X11 host's table.
    #[derive(Debug)]
    pub struct X11;

    impl Host for X11 {
        const NAME: &'static str = "X11";

        type Loop = X11Loop;
        type Window = X11Window;
        type Displays = X11Loop;
        type Input = X11Loop;
        type Text = Absent<position::Text>;
        type Accessibility = UnixAccessibility;
        type Clipboard = X11Loop;
        type PrimarySelection = Absent<position::PrimarySelection>;
        type Drag = Absent<position::Drag>;
        // A desktop has no haptic hardware to drive.
        type Haptics = Absent<position::Haptics>;
        type Preferences = X11Loop;
        type Lifecycle = X11Loop;
        // A desktop window styles no system bars.
        type Chrome = Absent<position::Chrome>;
        type Storage = UserDataDirs;
    }

    impl X11Host for X11 {
        /// The X server's outputs, through RandR on a connection of their own.
        fn displays() -> Arc<dyn HostDisplays> {
            LinuxDisplays::new()
        }

        fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
            super::system_notices(app)
        }

        fn preferences(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxPreferences> {
            super::portal(executor, signal)
        }

        fn host_notices(
            executor: &BackgroundExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxNotices> {
            super::session_notices(executor, signal)
        }

        /// XInput 2's pointers, XKB's keyboard and the cursor theme, on the
        /// platform's connection.
        fn input(connection: &Arc<X11Connection>) -> Arc<dyn LoopInput> {
            X11Input::new(connection)
        }

        /// The `CLIPBOARD` selection on a connection and a thread of its own.
        fn clipboard() -> Result<Arc<dyn SharedClipboard>, PlatformError> {
            Ok(Arc::new(X11Clipboard::start()?))
        }
    }

    /// The native X11 platform: windows on one connection to the X server
    /// named by `DISPLAY`, read by a loop on the thread that builds it, with
    /// the server's displays, the AT-SPI bridge, the Settings portal and the
    /// host's notices.
    ///
    /// Built and run on one thread, which owns its windows. `current_platform`
    /// picks it in an X11 session when the winit host is not built.
    pub type X11Platform = HostPlatform<X11>;

    impl HostPlatform<X11> {
        /// Connect to the X server and build the platform on the calling
        /// thread.
        ///
        /// # Errors
        /// [`PlatformError::Init`] when libxcb cannot be loaded, no X server
        /// answers, or the loop's wake descriptor cannot be made.
        pub fn try_new() -> Result<Self, PlatformError> {
            let connection = X11Connection::connect()?;
            let atoms = WindowAtoms::intern(&connection)?;
            let windows = Arc::clone(&connection);
            Self::try_from_parts(
                |positions| X11Windows::new(positions, windows, atoms),
                |windows| X11Loop::try_new::<X11>(connection, Arc::clone(windows) as _),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use crate::accessibility::linux::UnixAccessibility;
    use crate::host::Host;

    /// Every native Linux window hands out the AT-SPI bridge, with no feature
    /// of its own (ADR-0151 §15): each table names the adapter, not `Absent`,
    /// so a window a screen reader cannot see does not compile into a host.
    #[test]
    fn linux_windows_hand_out_the_at_spi_bridge() {
        #[cfg(wayland_host)]
        assert_eq!(
            TypeId::of::<<super::wayland::Wayland as Host>::Accessibility>(),
            TypeId::of::<UnixAccessibility>()
        );
        #[cfg(x11_host)]
        assert_eq!(
            TypeId::of::<<super::x11::X11 as Host>::Accessibility>(),
            TypeId::of::<UnixAccessibility>()
        );
    }
}
