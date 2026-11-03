//! The window feature (ADR-0151 §10): what every host keeps of a window, and
//! one file per platform.
//!
//! The model is what two or more platforms share: the published state, its
//! staleness and the mode decided from a window system's flags ([`state`]),
//! what the X11 host decides about a window before it opens (`plan`), and the
//! acknowledgement a host waits for before it publishes a window it attached
//! ([`installation`]); beside it is the window
//! configuration the Win32 host is built with (`config`). Each platform file
//! implements the window position of its host's table, and its system chrome
//! (the `Chrome` position) beside it.
//!
//! A loop holds its windows without naming them: each `owner/<os>` declares
//! what it asks of the window feature, a platform file here implements it, and
//! `hosts/<os>.rs` hands the loop the window feature's object.

#[cfg(windows)]
pub(crate) mod config;
pub(crate) mod installation;
#[cfg(x11_host)]
pub(crate) mod plan;
pub(crate) mod state;

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
#[cfg(any(target_os = "android", target_os = "ios", target_os = "macos", test))]
pub(crate) mod capabilities;
pub(crate) mod headless;
#[cfg(any(target_os = "ios", target_arch = "wasm32", test))]
pub(crate) mod keyboard;
#[cfg(any(target_os = "android", test))]
pub(crate) mod keyboard_motion;
#[cfg(any(linux_host, test))]
pub(crate) mod linux {
    //! The Linux windows: the native Wayland host's (`wayland`) and the native
    //! X11 host's (`x11`).

    #[cfg(any(wayland_host, test))]
    pub(crate) mod wayland;
    #[cfg(any(x11_host, test))]
    pub(crate) mod x11;
}
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
