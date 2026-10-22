//! Winit-based platform implementation

#[cfg(target_os = "linux")]
pub(crate) mod linux_unreported;

// The window feature's winit window, at the path applications name it by.
pub use crate::window::winit::WinitWindow;
