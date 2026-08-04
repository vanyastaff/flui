//! Windows platform implementation using Win32 API
//!
//! This module provides native Windows support without winit,
//! using direct Win32 API calls for maximum control and performance.

// This module (and its submodules) is one of the workspace's sanctioned
// `unsafe` FFI islands — direct Win32 calls have no safe wrapper. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`); every `unsafe`
// block still carries its own `// SAFETY:` comment stating the invariant
// that makes it sound.
#![allow(unsafe_code)]

mod clipboard;
mod display;
mod events;
mod platform;
mod util;
mod window;
mod window_ext;

pub use clipboard::WindowsClipboard;
pub use display::{WindowsDisplay, enumerate_displays};
pub use platform::WindowsPlatform;
pub use window::WindowsWindow;
pub use window_ext::{
    TaskbarProgressState, WindowCornerPreference, WindowsBackdrop, WindowsTheme, WindowsWindowExt,
};

/// The raw Win32 types an example or embedder needs to talk to a FLUI window
/// directly (DWM frame extension, backdrop attributes, the `HWND` itself).
///
/// Re-exported so consumers do not have to depend on a matching `windows`
/// crate version of their own.
#[cfg(target_os = "windows")]
pub mod win32 {
    #![allow(missing_docs)] // straight re-exports; the `windows` crate documents them
    pub use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute},
        UI::Controls::MARGINS,
    };
}
