//! One table per host and its constructor, nothing else (ADR-0151 §10).
//!
//! Each file implements [`Host`](crate::host::Host) for its platform, naming
//! the type that fills every position or [`Absent`](crate::host::Absent), and
//! builds the [`HostPlatform`](crate::HostPlatform) over that table. The
//! public platform names are those platforms. Nothing in the crate names this
//! module except the crate root, which picks a host.

pub(crate) mod headless;
#[cfg(windows)]
pub(crate) mod windows;

pub use headless::HeadlessPlatform;
#[cfg(windows)]
pub use windows::WindowsPlatform;
