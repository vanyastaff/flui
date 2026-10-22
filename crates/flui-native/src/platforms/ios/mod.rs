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
//!     -> SceneDelegate connection          -> session UI runtime / native attachment
//!     -> scene active / inactive            -> focus observations
//!     -> scene background / foreground      -> execution, visibility, surface
//!     -> scene disconnect                   -> retained UI runtime, detached native attachment
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
//! ([`IOSExecutor`]). The delegate's session state is thread-local for the
//! same reason — see `DELEGATE_STATE` in `owner/apple/ios.rs`.
//!
//! # iOS versions
//!
//! Target: iOS 13+. `objc2`'s bindings span iOS 10–26, so a call the SDK marks
//! newer than 13 needs an availability gate. The one today is
//! `CADisplayLink.preferredFrameRateRange` (iOS 15): `os_version` reads
//! `NSProcessInfo.operatingSystemVersion` once, the platform keeps it, and the
//! call takes the proof a version check hands out (`shared::os_version`).
//! Before iOS 15 the link gets `preferredFramesPerSecond` instead.

pub(crate) mod os_version;

pub use crate::owner::apple::ios::{IOSExecutor, IOSSceneAttachmentId, IOSSceneSessionId};
// The window and the scene event that hands it over live in the window
// feature; their public names stay here.
pub use crate::window::apple::ios::{IOSSceneEvent, IOSWindow};
