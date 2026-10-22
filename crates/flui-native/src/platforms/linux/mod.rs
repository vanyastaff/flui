//! Linux code that runs today under the winit host.
//!
//! Linux windows, input and the event loop come from the winit fallback host
//! (`WinitPlatform`) until the native Linux host (Wayland and X11) replaces it
//! (ADR-0151 section 11). What is native already lives here, and the winit host
//! uses it (the outputs of a Wayland compositor or an X server are the display
//! feature's, `display::linux`):
//!
//! - `host_notices`: sleep and wake, shutdown, the screen lock and memory
//!   pressure, from the login manager and the kernel;
//! - `lifecycle_model`: the pure models behind those adapters and the X11 probe;
//! - `x11_window_probe`: whether a window is minimized, and its mode, as the X
//!   server and the window manager report them;
//! - `accessibility`: the AT-SPI adapter over `accesskit_unix`.

#[cfg(all(target_os = "linux", feature = "a11y"))]
mod accessibility;
#[cfg(feature = "winit-backend")]
pub(crate) mod host_notices;
#[cfg(feature = "winit-backend")]
pub(crate) mod lifecycle_model;
#[cfg(feature = "winit-backend")]
pub(crate) mod x11_window_probe;

#[cfg(all(target_os = "linux", feature = "a11y"))]
pub use accessibility::UnixAccessibility;
