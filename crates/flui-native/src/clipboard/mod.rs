//! The clipboard: the system's text clipboard on each host (ADR-0151 §10).
//!
//! The model is `lock`, the process-wide lock that every Win32 clipboard
//! session runs under, which the Win32 host and winit's `arboard` clipboard on
//! Windows share. Beside it, one file per platform holds the host's clipboard
//! and fills the clipboard position of its table:
//!
//! - `windows`: `OpenClipboard` with an owner window of its own;
//! - `apple`: `NSPasteboard` on macOS, `UIPasteboard` on iOS;
//! - `android`: an in-process stand-in until the system clipboard is reached
//!   through JNI;
//! - `web`: `navigator.clipboard`, read back from what the page wrote;
//! - `winit`: `arboard`;
//! - `headless`: the contract's in-memory clipboard.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
mod headless;
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
