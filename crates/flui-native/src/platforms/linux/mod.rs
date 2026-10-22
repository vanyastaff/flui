//! Linux code that runs today under the winit host.
//!
//! Linux windows, input and the event loop come from the winit fallback host
//! (`WinitPlatform`) until the native Linux host (Wayland and X11) replaces it
//! (ADR-0151 section 11). What is native already lives here, and the winit host
//! uses it:
//!
//! - `display`: the outputs of a Wayland compositor or an X server, with their
//!   refresh rates and a notice when they change, read on a thread of its own;
//! - `host_notices`: sleep and wake, shutdown, the screen lock and memory
//!   pressure, from the login manager and the kernel;
//! - `lifecycle_model`: the pure models behind those adapters and the X11 probe;
//! - `x11_window_probe`: whether a window is minimized, and its mode, as the X
//!   server and the window manager report them.
//!
//! The AT-SPI adapter is the accessibility feature's (`accessibility::linux`).

pub(crate) mod display;
#[cfg(feature = "winit-backend")]
pub(crate) mod host_notices;
#[cfg(feature = "winit-backend")]
pub(crate) mod lifecycle_model;
#[cfg(feature = "winit-backend")]
pub(crate) mod x11_window_probe;
