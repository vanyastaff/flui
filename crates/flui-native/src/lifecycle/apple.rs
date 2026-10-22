//! The Apple hosts' lifecycle: the AppKit and libdispatch sources of macOS
//! (`macos`). UIKit's memory warning and termination arrive at its application
//! delegate, which is the loop's (`owner::apple::ios`), and reach the
//! application from there through the loop's lifecycle hooks, as AppKit's
//! reopen and termination do on macOS.

#[cfg(any(target_os = "macos", test))]
pub(crate) mod macos;
