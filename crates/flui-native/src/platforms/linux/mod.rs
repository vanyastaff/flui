//! Linux code that runs today under the winit host.
//!
//! Linux windows, input and the event loop come from the winit fallback host
//! (`WinitPlatform`) until the native Linux host (Wayland and X11) replaces it
//! (ADR-0151 section 11). What is native already lives here, and the winit host
//! uses it (the outputs of a Wayland compositor or an X server are the display
//! feature's, `display::linux`):
//!
//! - `lifecycle_model`: the pure model behind the X11 probe;
//! - `x11_window_probe`: whether a window is minimized, and its mode, as the X
//!   server and the window manager report them;
//! - `accessibility`: the AT-SPI adapter over `accesskit_unix`.
//!
//! Sleep, shutdown, the screen lock and memory pressure are the lifecycle's
//! (`lifecycle::linux`); the Settings portal is the preferences'
//! (`preferences::linux`).

#[cfg(all(target_os = "linux", feature = "a11y"))]
mod accessibility;
#[cfg(feature = "winit-backend")]
pub(crate) mod lifecycle_model;
#[cfg(feature = "winit-backend")]
pub(crate) mod x11_window_probe;

#[cfg(all(target_os = "linux", feature = "a11y"))]
pub use accessibility::UnixAccessibility;
