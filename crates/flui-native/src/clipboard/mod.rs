//! The clipboard: the system's text clipboard on each host (ADR-0151 §10).
//!
//! The model is `lock`, the process-wide lock that every Win32 clipboard
//! session of the Win32 host runs under, and `text`, the forms text takes
//! between the Linux desktops, which the Linux clipboards and the X11 host's
//! drops read. Beside them, one file per platform holds the host's clipboard
//! and fills the clipboard position of its table:
//!
//! - `windows`: `OpenClipboard` with an owner window of its own;
//! - `apple`: `NSPasteboard` on macOS, `UIPasteboard` on iOS;
//! - `android`: `ClipboardManager`, through JNI;
//! - `web`: `navigator.clipboard`, whose read answers when its promise
//!   settles;
//! - `linux`: the Wayland seats' selection through `wl_data_device`, X11's
//!   `CLIPBOARD` selection on a connection of its own;
//! - `headless`: a `MemoryClipboard`, one per loop.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
mod headless;
#[cfg(any(wayland_host, x11_host))]
pub(crate) mod linux;
#[cfg(windows)]
mod lock;
#[cfg(any(wayland_host, x11_host))]
pub(crate) mod text;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
