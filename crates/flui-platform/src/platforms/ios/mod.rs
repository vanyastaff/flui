//! iOS platform implementation (UIKit + Metal).
//!
//! A native iOS backend built on UIKit, `wgpu`'s Metal backend, and Grand
//! Central Dispatch — the same `Platform`/`PlatformWindow` contract the other
//! backends implement, so an application changes nothing but its entry point.
//!
//! # Architecture
//!
//! ```text
//! Rust main() / flui::run_app()
//!   -> IOSPlatform::new()
//!   -> Platform::run()                 [UIApplicationMain]
//!     -> AppDelegate.didFinishLaunching    -> on_ready(): process services / scene registration
//!     -> SceneDelegate connection          -> session realm / native attachment
//!     -> scene active / inactive            -> focus observations
//!     -> scene background / foreground      -> execution, visibility, surface
//!     -> scene disconnect                   -> retained realm, detached native attachment
//!     -> CADisplayLink tick                -> dispatch_request_frame()
//! ```
//!
//! # Binding stack
//!
//! `objc2` + `objc2-ui-kit` + `objc2-foundation`, sharing the modern Objective-C
//! binding stack with the native AppKit backend. UIKit object ownership stays on
//! the main thread.
//!
//! # Threading
//!
//! UIKit is main-thread-only, and `UIApplicationMain` owns the main thread for
//! the process's life. Every window, view, and pasteboard access therefore
//! happens on that thread; the only background work is GCD's
//! ([`executor::IOSExecutor`]). The delegate's session state is thread-local
//! for the same reason — see `platform.rs`'s `DELEGATE_STATE`.
//!
//! # iOS versions
//!
//! Target: iOS 13+. `objc2`'s bindings span iOS 10–26, so nothing here needs
//! an availability gate above 13 — a claim to re-check the moment a call is
//! added that the SDK marks newer.

// Some screen/geometry APIs remain deprecated in SDK 26.2. Scene ownership is
// implemented; this allowance does not exempt new code from scene lifetime rules.
#![expect(deprecated)]
// This module (and its submodules) is the workspace's sanctioned `unsafe` FFI
// island for UIKit — direct Objective-C calls have no safe wrapper. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (the same shape `macos/mod.rs`
// uses).
#![expect(unsafe_code)]

mod clipboard;
mod display;
mod events;
mod executor;
mod native_owner;
mod platform;
mod scene;
mod window;

pub use clipboard::IOSClipboard;
pub use display::IOSDisplay;
pub use executor::IOSExecutor;
pub use platform::IOSPlatform;
pub use scene::{IOSSceneAttachmentId, IOSSceneEvent, IOSSceneSessionId};
pub use window::IOSWindow;
