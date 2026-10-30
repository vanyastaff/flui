//! The clipboard: the system's text clipboard on each host (ADR-0151 §10).
//!
//! The model is `lock`, the process-wide lock that every Win32 clipboard
//! session runs under, which the Win32 host and winit's `arboard` clipboard on
//! Windows share. Beside it, one file per platform holds the host's clipboard
//! and fills the clipboard position of its table:
//!
//! - `windows`: `OpenClipboard` with an owner window of its own;
//! - `apple`: `NSPasteboard` on macOS, `UIPasteboard` on iOS;
//! - `android`: `ClipboardManager`, through JNI;
//! - `web`: `navigator.clipboard`, whose read answers when its promise
//!   settles;
//! - `linux`: the Wayland seats' selection through `wl_data_device`, and
//!   the text forms the Linux desktops exchange;
//! - `winit`: `arboard`;
//! - `headless`: a `MemoryClipboard`, one per loop.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
mod headless;
#[cfg(wayland_host)]
pub(crate) mod linux;
#[cfg(windows)]
mod lock;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;
