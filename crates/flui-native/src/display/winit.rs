//! The winit host's displays: the native source of the operating system it
//! runs on, never winit's monitor list, which exists only inside a live event
//! loop and keys monitors by labels that are reused (ADR-0236).
//!
//! - Linux: the compositor's or X server's outputs on a connection of its own
//!   (`display::linux`);
//! - macOS: `NSScreen`, the AppKit host's source;
//! - Windows: the monitors, the Win32 host's source.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::WinitDisplaySource;
#[cfg(target_os = "macos")]
pub(crate) use macos::WinitDisplaySource;
#[cfg(windows)]
pub(crate) use windows::WinitDisplaySource;
