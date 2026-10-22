//! The Windows host's public names: the window and its extensions, at the
//! paths applications name them by, and the raw Win32 types an embedder
//! needs. The window, its input and every other feature live in their
//! features' directories.

// The window feature's public types, at the paths applications name them by.
pub use crate::window::windows::WindowsWindow;
pub use crate::window::windows::backdrop::{
    TaskbarProgressState, WindowCornerPreference, WindowsBackdrop, WindowsTheme, WindowsWindowExt,
};

/// The raw Win32 types an example or embedder needs to talk to a FLUI window
/// directly (DWM frame extension, backdrop attributes, the `HWND` itself).
///
/// Re-exported so consumers do not have to depend on a matching `windows`
/// crate version of their own.
#[cfg(target_os = "windows")]
pub mod win32 {
    #![cfg_attr(not(target_os = "windows"), expect(missing_docs))] // straight re-exports; the `windows` crate documents them
    pub use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute},
        UI::Controls::MARGINS,
    };
}
