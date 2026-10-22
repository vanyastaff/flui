//! The window feature (ADR-0151 §10): what every host keeps of a window, and
//! one file per platform.
//!
//! The model is what two or more platforms share: the published state and its
//! staleness ([`state`]), the hidden-surface rules of the Win32 and AppKit
//! windows (`visibility`), the window configuration a desktop host is built
//! with ([`config`]) and the acknowledgement a host waits for before it
//! publishes a window it attached ([`installation`]). Each platform file
//! implements the window position of its host's table, and its system chrome
//! (the `Chrome` position) beside it.
//!
//! A loop holds its windows without naming them: each `owner/<os>` declares
//! what it asks of the window feature, a platform file here implements it, and
//! `hosts/<os>.rs` hands the loop the window feature's object.

pub mod config;
pub(crate) mod installation;
pub(crate) mod state;
#[cfg(any(windows, target_os = "macos", test))]
pub(crate) mod visibility;

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
pub(crate) mod headless;
#[cfg(any(all(target_os = "linux", feature = "winit-backend"), test))]
pub(crate) mod linux {
    //! The live layer of the Linux windows, which the winit host uses until
    //! the native Wayland and X11 hosts replace it.

    pub(crate) mod x11;
}
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

/// What a window manager says of a window that the windowing toolkit does not
/// report: whether it is minimized, its mode and its frame (on X11,
/// `WM_STATE`, `_NET_WM_STATE` and `_NET_FRAME_EXTENTS`), and which windows it
/// changed.
///
/// The winit window reads it; the X11 probe (`linux::x11`) answers it, and the
/// winit host connects the two, so neither platform file names the other.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
pub(crate) trait WindowManagerView: Send + Sync {
    /// Starts hearing of changes to `window`'s mode.
    fn watch(&self, window: u32);

    /// Writes `mode` into the properties of `window`, which is not mapped
    /// yet: a window manager reads the mode from them when it maps a window.
    fn set_mode_hint(&self, window: u32, mode: flui_platform::WindowMode);

    /// The frame the window manager put around `window`, waiting at most
    /// `wait` for it to say.
    fn frame_extents(
        &self,
        window: u32,
        wait: std::time::Duration,
    ) -> Option<flui_foundation::geometry::Size<i32>>;

    /// `window`'s mode as the window manager states it.
    fn mode(&self, window: u32) -> Option<flui_platform::WindowMode>;

    /// The watched windows whose mode changed since the last call.
    fn take_changed(&self) -> Vec<u32>;
}

/// A [`WindowManagerView`] could not be had, or a request to the window
/// manager failed: the reason, as the server or the system gave it.
#[cfg(all(target_os = "linux", feature = "winit-backend"))]
#[derive(Debug, thiserror::Error)]
#[error("the window manager's view: {0}")]
pub(crate) struct WindowManagerError(String);

#[cfg(all(target_os = "linux", feature = "winit-backend"))]
impl WindowManagerError {
    /// The error `error` describes.
    pub(crate) fn from_display(error: impl std::fmt::Display) -> Self {
        Self(error.to_string())
    }
}
