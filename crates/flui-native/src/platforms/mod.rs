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

// The Linux display and lifecycle models are pure, so every host checks them;
// the rest of the Linux backend exists on Linux only.
#[cfg(all(test, not(target_os = "linux")))]
#[path = "linux/display/mod.rs"]
mod linux_display_models;

#[cfg(all(test, not(target_os = "linux")))]
#[path = "linux/lifecycle_model/mod.rs"]
mod linux_lifecycle_models;

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
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub mod winit;

// Re-exports
#[cfg(target_os = "android")]
pub use android::AndroidPlatform;
pub use headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, FakeAccessibility, FakeHaptics,
    FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens, HeadlessDisplayControls,
    HeadlessDragAndDrop, HeadlessExitReevaluation, HeadlessHostEvents, HeadlessInputDevices,
    HeadlessOwnerTurns, HeadlessPlatform, HeadlessWindowPolicy, MockWindow,
};
#[cfg(target_os = "ios")]
pub use ios::IOSPlatform;
#[cfg(target_os = "macos")]
pub use macos::MacOSPlatform;
#[cfg(target_arch = "wasm32")]
pub use web::WebPlatform;
#[cfg(windows)]
pub use windows::WindowsPlatform;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use winit::WinitPlatform; // winit fallback backend — see `platforms::winit` docs
