//! iOS platform implementation (UIKit + Metal).
//!
//! A native iOS backend built on UIKit, `wgpu`'s Metal backend, and Grand
//! Central Dispatch — the same `Platform`/`PlatformWindow` contract the other
//! backends implement, so an application changes nothing but its entry point.
//!
//! # Architecture
//!
//! ```text
//! flui_ios_main()  (called from the Xcode app's Swift/ObjC entry point)
//!   -> IOSPlatform::new()
//!   -> Platform::run()                 [UIApplicationMain]
//!     -> AppDelegate.didFinishLaunching    -> on_ready(): window + GPU + realm
//!     -> didBecomeActive / willResignActive -> active + surface signals
//!     -> didEnterBackground / willEnterForeground -> surface signals
//!     -> CADisplayLink tick                -> dispatch_request_frame()
//! ```
//!
//! # Binding stack
//!
//! `objc2` + `objc2-ui-kit` + `objc2-foundation`, not the `objc` 0.2 /
//! `cocoa` pair the macOS backend still carries: `objc` has not released since
//! 2019 and `cocoa` has no UIKit surface at all, while `objc2` is what every
//! shipping Rust macOS/iOS stack uses today (winit, wgpu, egui, slint, gpui).
//! The versions here are the ones already in the lock via `wgpu-hal` 30.0.1.
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

// `UIScreen.mainScreen` and a handful of UIKit accessors are marked deprecated
// in the multi-scene era (the replacements route through a `UIWindowScene`).
// This backend presents exactly one full-screen window and never adopts
// scenes, so the app-wide accessors remain the honest spelling; `UIScene` is
// a separate, larger feature (multi-window on iPadOS) and is not implemented.
// Adopting scenes would make every `mainScreen` call site scene-relative.
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
mod platform;
mod window;

pub use clipboard::IOSClipboard;
pub use display::IOSDisplay;
pub use executor::IOSExecutor;
pub use platform::IOSPlatform;
pub use window::IOSWindow;
