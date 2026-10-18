//! The Linux display source: the outputs of a Wayland compositor or an X
//! server, their refresh rates, and a notice when they change.
//!
//! The first part of a native Linux backend, and the display source of the
//! winit backend today. It connects to the compositor on a connection of its
//! own, apart from any window system's, and reads two kinds of state:
//!
//! - **Wayland**: `wl_output` (version 2 to 4) with `zxdg_output_v1` for the
//!   logical position and size, applied only when the compositor closes a burst
//!   of events with `done` ([`wayland_model`]).
//! - **X11**: RandR 1.5 monitors with the output, CRTC and mode behind each
//!   ([`x11_model`]).
//!
//! Both models are pure: they take the events or state a protocol reports and
//! answer [`OutputView`](view::OutputView)s, and they are checked on every host
//! with no compositor. Thin adapters over `wayland-client` and `x11rb` feed
//! them, behind the `wayland` and `x11` features. [`source::LinuxDisplays`]
//! turns the views into the display snapshot and keeps them current from a
//! thread that waits on the connection.
//!
//! Which protocol is read follows the rule winit uses ([`protocol`]), so the
//! source reads the compositor the windows live on.

// Today only the winit backend reads the source; the native Linux backend that
// will use it directly does not exist yet, so a build with the protocol
// features and no winit backend compiles code nothing calls.
#![cfg_attr(
    not(feature = "winit-backend"),
    allow(
        dead_code,
        reason = "the display source has no user but the winit backend until the native Linux backend exists"
    )
)]

#[cfg(any(
    all(target_os = "linux", any(feature = "wayland", feature = "x11")),
    test
))]
pub(crate) mod view;

pub(crate) mod protocol;

#[cfg(any(all(target_os = "linux", feature = "wayland"), test))]
pub(crate) mod wayland_model;

#[cfg(any(all(target_os = "linux", feature = "x11"), test))]
pub(crate) mod x11_model;

#[cfg(all(target_os = "linux", any(feature = "wayland", feature = "x11")))]
mod session;

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod wayland;

#[cfg(all(target_os = "linux", feature = "x11"))]
mod x11;

#[cfg(all(target_os = "linux", any(feature = "wayland", feature = "x11")))]
pub(crate) mod source;
