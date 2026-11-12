//! The native Linux hosts' tables: Wayland's and X11's (ADR-0151 §10, §11).
//!
//! Both fill the window, displays, accessibility, preference, lifecycle and
//! storage positions, and keep the session's sources (the Settings portal,
//! logind, the kernel's memory pressure) in the same shape
//! (`owner::linux`). Both fill the input position: Wayland with its seats'
//! input, X11 with XInput 2 and XKB. Wayland fills the clipboard and primary
//! selection positions with its seats' selections, X11 with the `CLIPBOARD`
//! and `PRIMARY` selections on a connection of its own. Wayland fills the
//! text position with its seats' `zwp_text_input_v3` and the drag position
//! with drops on its windows through `wl_data_device`; X11 fills the drag
//! position with XDND drops on its windows and the sessions they start.
//! X11's text input is `Absent` until the host speaks its protocol, and no
//! Wayland window starts a drag yet; a desktop has no haptics and no system
//! bars.

use std::sync::Arc;

use crate::accessibility::linux::UnixAccessibility;
use crate::error::PlatformError;
use crate::executor::NativeExecutor;
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
// The host tables, for the parity table's check against their `Absent` positions.
#[cfg(all(test, wayland_host))]
pub(crate) use wayland::Wayland;
#[cfg(all(test, x11_host))]
pub(crate) use x11::X11;

/// The channel both hosts' notices take to the application.
fn system_notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
    Arc::new(SystemNotices::new(Arc::clone(app)))
}

/// The Settings portal on the session bus, for both hosts.
fn portal(executor: &NativeExecutor, signal: &Arc<OwnerSignal>) -> Arc<dyn LinuxPreferences> {
    PreferenceSource::new(executor, signal)
}

/// logind on the system bus and the kernel's memory pressure, for both hosts.
fn session_notices(executor: &NativeExecutor, signal: &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices> {
    Arc::new(HostNoticeSource::start(executor, signal))
}

#[cfg(wayland_host)]
mod wayland {
    use std::sync::Arc;

    use super::{
        Absent, AppDelivery, Host, HostPlatform, LinuxNotices, LinuxPreferences, NativeExecutor,
        NoticeChannel, OwnerSignal, PlatformError, UnixAccessibility, UserDataDirs, position,
    };
    use crate::clipboard::linux::wayland::SeatClipboard;
    use crate::display::linux::protocol::Environment;
    use crate::display::linux::wayland::WaylandDisplaySource;
    use crate::drag::wayland::SeatDrag;
    use crate::input::linux::wayland::SeatInput;
    use crate::owner::linux::wayland::{
        LastInput, WaylandClipboard, WaylandDisplays, WaylandDrag, WaylandHost, WaylandInput,
        WaylandLoop,
    };
    use crate::text::linux::wayland::{SeatTexts, WaylandText};
    use crate::transfer::linux::wayland::DataDevices;
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
        type Drag = WaylandLoop;
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
        fn clipboard(last_input: LastInput, devices: DataDevices) -> Arc<dyn WaylandClipboard> {
            Arc::new(SeatClipboard::new(last_input, devices))
        }

        /// Drops on the windows, which the seats' data devices hear, and the
        /// sessions the windows start from them.
        fn drag(
            devices: DataDevices,
            last_input: LastInput,
            wake: Box<dyn Fn() + Send + Sync>,
        ) -> Arc<dyn WaylandDrag> {
            SeatDrag::new(&devices, last_input, wake)
        }

        fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
            super::system_notices(app)
        }

        fn preferences(
            executor: &NativeExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxPreferences> {
            super::portal(executor, signal)
        }

        fn host_notices(
            executor: &NativeExecutor,
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
            Self::try_new_with(NativeExecutor::own())
        }

        /// [`try_new`](Self::try_new) on `executor`'s runtime, as
        /// [`CurrentHost::start_with`](crate::CurrentHost::start_with) starts it.
        pub(crate) fn try_new_with(executor: NativeExecutor) -> Result<Self, PlatformError> {
            let socket = Environment::current()
                .wayland_socket_path()
                .ok_or_else(|| PlatformError::Init {
                    message: "Wayland: WAYLAND_DISPLAY names no socket (a bare name needs \
                              XDG_RUNTIME_DIR)"
                        .to_owned(),
                })?;
            Self::try_from_parts(WaylandWindows::new, |_| {
                WaylandLoop::try_new::<Wayland>(&socket, executor)
            })
        }
    }
}

#[cfg(x11_host)]
mod x11 {
    use std::sync::Arc;

    use super::{
        Absent, AppDelivery, Host, HostPlatform, LinuxNotices, LinuxPreferences, NativeExecutor,
        NoticeChannel, OwnerSignal, PlatformError, UnixAccessibility, UserDataDirs, position,
    };
    use crate::clipboard::linux::x11::X11Clipboard;
    use crate::display::linux::source::LinuxDisplays;
    use crate::drag::x11::X11Drag;
    use crate::input::linux::x11::X11Input;
    use crate::owner::linux::x11::{
        Doorbell, HostDisplays, LoopClipboard, LoopDrag, LoopInput, LoopText, X11Connection,
        X11Host, X11Loop,
    };
    use crate::text::linux::x11::{X11Ime, X11Text};
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
        type Text = X11Text;
        type Accessibility = UnixAccessibility;
        type Clipboard = X11Loop;
        type PrimarySelection = X11Loop;
        type Drag = X11Loop;
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
            executor: &NativeExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxPreferences> {
            super::portal(executor, signal)
        }

        fn host_notices(
            executor: &NativeExecutor,
            signal: &Arc<OwnerSignal>,
        ) -> Arc<dyn LinuxNotices> {
            super::session_notices(executor, signal)
        }

        /// XInput 2's pointers, XKB's keyboard and the cursor theme, on the
        /// platform's connection.
        fn input(connection: &Arc<X11Connection>, text: Arc<dyn LoopText>) -> Arc<dyn LoopInput> {
            X11Input::new(connection, text)
        }

        /// The session's input method (IBus or Fcitx5) over D-Bus, reached on
        /// the background executor.
        fn text(
            connection: &Arc<X11Connection>,
            executor: &NativeExecutor,
            doorbell: &Doorbell,
        ) -> Arc<dyn LoopText> {
            X11Ime::start(connection, executor, doorbell)
        }

        /// The `CLIPBOARD` and `PRIMARY` selections on a connection and a
        /// thread of their own, which ring the loop's doorbell when the
        /// application's primary selection is asked for.
        fn clipboard(doorbell: &Doorbell) -> Result<Arc<dyn LoopClipboard>, PlatformError> {
            let doorbell = doorbell.clone();
            let wake = move || {
                if let Err(error) = doorbell.ring() {
                    tracing::error!(%error, "the X11 clipboard cannot wake the loop");
                }
            };
            Ok(Arc::new(X11Clipboard::start(Box::new(wake))?))
        }

        /// XDND drops on the windows, on the platform's connection.
        fn drag(
            connection: &Arc<X11Connection>,
            doorbell: &Doorbell,
        ) -> Result<Arc<dyn LoopDrag>, PlatformError> {
            let doorbell = doorbell.clone();
            let wake = Box::new(move || {
                if let Err(error) = doorbell.ring() {
                    tracing::debug!(%error, "cannot wake the X11 loop for a drop's transfer");
                }
            });
            Ok(X11Drag::try_new(connection, wake)?)
        }
    }

    /// The native X11 platform: windows on one connection to the X server
    /// named by `DISPLAY`, read by a loop on the thread that builds it, with
    /// the server's displays, the AT-SPI bridge, the Settings portal and the
    /// host's notices.
    ///
    /// Built and run on one thread, which owns its windows. `current_platform`
    /// picks it in an X11 session.
    pub type X11Platform = HostPlatform<X11>;

    impl HostPlatform<X11> {
        /// Connect to the X server and build the platform on the calling
        /// thread.
        ///
        /// # Errors
        /// [`PlatformError::Init`] when libxcb cannot be loaded, no X server
        /// answers, or the loop's wake descriptor cannot be made.
        pub fn try_new() -> Result<Self, PlatformError> {
            Self::try_new_with(NativeExecutor::own())
        }

        /// [`try_new`](Self::try_new) on `executor`'s runtime, as
        /// [`CurrentHost::start_with`](crate::CurrentHost::start_with) starts it.
        pub(crate) fn try_new_with(executor: NativeExecutor) -> Result<Self, PlatformError> {
            let connection = X11Connection::connect()?;
            let atoms = WindowAtoms::intern(&connection)?;
            let windows = Arc::clone(&connection);
            Self::try_from_parts(
                |positions| X11Windows::new(positions, windows, atoms),
                |windows| X11Loop::try_new::<X11>(connection, Arc::clone(windows) as _, executor),
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
