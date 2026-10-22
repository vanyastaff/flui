//! Platform implementations
//!
//! Concrete implementations of the Platform trait for different environments.

pub mod headless;

// Desktop platforms
#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "ios")]
pub mod ios;

// winit fallback backend — primary on Linux until native Wayland/X11 lands
// (roadmap Cross.P); optional on Windows/macOS behind the `winit-backend`
// feature.
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub mod winit;

// Re-exports
pub use headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, FakeSystemChrome, FakeTextInput,
    HeadlessDeferredWindowOpens, HeadlessDisplayControls, HeadlessExitReevaluation,
    HeadlessHostEvents, HeadlessInputDevices, HeadlessOwnerTurns, HeadlessPreferences,
    HeadlessWindowPolicy, MockWindow,
};
