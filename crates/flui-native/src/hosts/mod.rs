//! One table per host and its constructor, nothing else (ADR-0151 §10).
//!
//! Each file implements [`Host`](crate::host::Host) for its platform, naming
//! the type that fills every position or [`Absent`](crate::host::Absent), and
//! builds the [`HostPlatform`](crate::HostPlatform) over that table. The
//! public platform names are those platforms; the Android host has none, since
//! it starts only over the Activity `android_main` is handed, through the
//! crate root's `android_platform`. Nothing in the crate names this module
//! except the crate root, which picks a host.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
pub(crate) mod headless;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

#[cfg(target_os = "ios")]
pub use apple::IOSPlatform;
#[cfg(target_os = "macos")]
pub use apple::MacOSPlatform;
pub use headless::HeadlessPlatform;
#[cfg(target_arch = "wasm32")]
pub use web::WebPlatform;
#[cfg(windows)]
pub use windows::WindowsPlatform;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use winit::WinitPlatform;
