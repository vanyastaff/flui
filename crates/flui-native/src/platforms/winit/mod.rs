//! Winit-based platform implementation

pub(crate) mod events;
#[cfg(target_os = "linux")]
pub(crate) mod linux_unreported;
mod positions;

// The window feature's winit window, at the path applications name it by.
pub use crate::window::winit::WinitWindow;
