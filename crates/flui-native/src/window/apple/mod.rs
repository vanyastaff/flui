//! The Apple windows: the AppKit window ([`macos`]) and the UIKit scene's
//! window ([`ios`]), over the pure rules both ask ([`geometry`]).
//!
//! The macOS window keeps its AppKit features beyond the cross-platform window
//! (`MacOSWindowExt`) and its Liquid Glass backdrop ([`liquid_glass`]) beside
//! it.

pub(crate) mod geometry;
#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod liquid_glass;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
