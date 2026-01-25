//! Windows platform implementation using Win32 API
//!
//! This module provides native Windows support without winit,
//! using direct Win32 API calls for maximum control and performance.

mod events;
mod platform;
mod util;
mod window;

pub use platform::WindowsPlatform;
pub use window::WindowsWindow;
