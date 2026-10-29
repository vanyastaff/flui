//! The Apple owner thread: AppKit's run loop and delegate on macOS,
//! `UIApplicationMain` with its application and scene delegates on iOS.
//!
//! - `macos`: the `NSApplication` loop, the owner lane every thread-affine
//!   AppKit call travels through, and the wake pump;
//! - `terminate_gate`: how the macOS loop answers `applicationShouldTerminate:`
//!   while the user's session is ending, as a pure rule every host checks;
//! - `ios`: the UIKit loop, its delegates and the scene sessions they connect;
//! - `native_owner`: a value only the main thread may touch or release, held
//!   by a loop that other threads share.
//!
//! On both, the owner thread is the process's main thread, which the operating
//! system's application loop runs on.

// This module (and its submodules) is one of the workspace's sanctioned
// `unsafe` FFI islands — direct AppKit and UIKit Objective-C calls have no
// safe wrapper. The workspace lint `unsafe_code = "warn"` is opted out here, at
// the module boundary, rather than for the whole crate (see `lib.rs`). On
// another host only the pure rule is compiled, for its tests.
#![cfg_attr(any(target_os = "macos", target_os = "ios"), expect(unsafe_code))]

#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod metal_layer;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod native_owner;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod terminate_gate;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use native_owner::NativeOwner;

#[cfg(target_os = "ios")]
pub(crate) use ios::IOSLoop;
#[cfg(target_os = "macos")]
pub(crate) use macos::MacOSLoop;
