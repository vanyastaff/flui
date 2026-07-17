//! Platform implementations
//!
//! Concrete implementations of the Platform trait for different environments.

pub mod headless;

// Desktop platforms
#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

// Mobile platforms
#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "ios")]
pub mod ios;

// Web platform
#[cfg(target_arch = "wasm32")]
pub mod web;

// winit fallback backend — primary on Linux until native Wayland/X11 lands
// (roadmap Cross.P); optional on Windows/macOS behind the `winit-backend`
// feature.
#[cfg(feature = "winit-backend")]
pub mod winit;

// Re-exports
#[cfg(target_os = "android")]
pub use android::AndroidPlatform;
pub use headless::{FakeHaptics, FakeTextInput, HeadlessPlatform};
#[cfg(target_os = "ios")]
pub use ios::IOSPlatform;
#[cfg(target_os = "linux")]
pub use linux::LinuxPlatform;
#[cfg(target_os = "macos")]
pub use macos::MacOSPlatform;
#[cfg(target_arch = "wasm32")]
pub use web::WebPlatform;
#[cfg(windows)]
pub use windows::WindowsPlatform;
#[cfg(feature = "winit-backend")]
pub use winit::WinitPlatform; // winit fallback backend — see `platforms::winit` docs
