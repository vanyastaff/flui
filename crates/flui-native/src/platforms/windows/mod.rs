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
mod appearance;
mod clipboard;
mod direct_manipulation;
pub(crate) mod drag;
mod events;
mod haptics;
pub(crate) mod host_events;
pub(crate) mod platform;
pub(crate) mod preferences;
mod text_services;
mod window;
mod window_ext;
mod window_frame;
mod window_limits;
mod window_modes;
mod window_placement;
mod window_visibility;

#[cfg(feature = "a11y")]
pub use accessibility::WindowsAccessibility;
pub use clipboard::WindowsClipboard;
// The types the Windows host's table names: public where they are defined (a
// table names only public types), and nameable only inside the crate.
pub(crate) use crate::display::windows::WindowsDisplays;
pub(crate) use drag::OleTransfer;
pub(crate) use haptics::WindowsHaptics;
pub(crate) use host_events::HostEvents;
pub(crate) use preferences::PreferenceSource;
// The hooks the Windows host registers on FLUI's window class.
pub(crate) use direct_manipulation::Touchpad;
pub(crate) use platform::WindowProc;
pub(crate) use text_services::TextServices;
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
