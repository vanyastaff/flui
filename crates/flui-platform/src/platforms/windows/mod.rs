//! Windows platform implementation using Win32 API
//!
//! This module provides native Windows support without winit,
//! using direct Win32 API calls for maximum control and performance.

mod display;
mod events;
mod platform;
mod util;
mod window;
mod window_ext;

pub use display::{enumerate_displays, WindowsDisplay};
pub use platform::WindowsPlatform;
pub use window::WindowsWindow;
pub use window_ext::{
    TaskbarProgressState, WindowCornerPreference, WindowsBackdrop, WindowsTheme, WindowsWindowExt,
};

// Re-export Windows types for examples
#[cfg(target_os = "windows")]
pub mod win32 {
    pub use windows::Win32::Foundation::HWND;
    pub use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWINDOWATTRIBUTE,
    };
    pub use windows::Win32::UI::Controls::MARGINS;
}
