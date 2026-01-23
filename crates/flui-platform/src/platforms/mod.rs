//! Platform implementations
//!
//! Concrete implementations of the Platform trait for different environments.

pub mod headless;

// Windows native platform
#[cfg(windows)]
pub mod windows;

// Legacy winit backend (deprecated, optional)
#[cfg(feature = "winit-backend")]
pub mod winit;

pub use headless::HeadlessPlatform;

#[cfg(windows)]
pub use windows::WindowsPlatform;

#[cfg(feature = "winit-backend")]
pub use winit::WinitPlatform;
