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
#![expect(unsafe_code)]

#[cfg(feature = "a11y")]
mod accessibility;
mod clipboard;
mod display;
mod events;
mod platform;
// Not yet attached to `WindowContext`: the window does not offer a
// `TextStoreHost` yet (ADR-0135 §3 describes the wiring), so
// `HostWindow::text_store_host` answers `None` and only the opt-in probe
// drives this module.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "WindowContext does not hold the text services until the window offers its host"
    )
)]
mod text_services;
mod util;
mod window;
mod window_ext;

#[cfg(feature = "a11y")]
pub use accessibility::WindowsAccessibility;
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
    #![cfg_attr(not(target_os = "windows"), expect(missing_docs))] // straight re-exports; the `windows` crate documents them
    pub use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute},
        UI::Controls::MARGINS,
    };
}

mod owner_control;
