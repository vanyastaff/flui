//! Platform abstraction layer for FLUI
//!
//! This crate provides a complete platform abstraction for the FLUI framework,
//! enabling cross-platform UI development with a unified API.
//!
//! # Architecture
//!
//! The architecture is inspired by GPUI's platform layer:
//!
//! ```text
//! flui-platform         - Contracts: PlatformWindow, capability traits,
//!                             window/input and data-transfer vocabulary
//!                             (re-exported here)
//!
//! flui-native
//!   ├─ platform.rs          - Central Platform trait and HostPlatform<H>,
//!   │                         its one implementation
//!   ├─ host_window.rs       - HostWindow: PlatformWindow + accessibility
//!   ├─ host.rs              - A host's table: one type per position, and
//!   │                         Absent<F> for a position a platform lacks
//!   ├─ hosts/               - One table per host and its constructor
//!   ├─ owner/               - The owner thread (ADR-0039) and each platform's
//!   │                         loop: the capability, its signal and slot, the
//!   │                         panic boundary, the one call point for handlers
//!   ├─ version/             - The OS version floors and gates (ADR-0244)
//!   │
//!   ├─ window/              - A feature each (ADR-0151 §10): its model beside
//!   ├─ input/                 one file per platform (windows, apple,
//!   ├─ text/                  android, web, linux, winit, headless)
//!   ├─ display/
//!   ├─ clipboard/, drag/
//!   ├─ accessibility/
//!   ├─ haptics/
//!   ├─ preferences/
//!   ├─ lifecycle/
//!   │
//!   └─ storage/             - Where an application keeps its data
//! ```
//!
//! # Key Concepts
//!
//! ## Platform Trait
//!
//! The [`Platform`] trait is the central abstraction that all platform
//! implementations must provide. It covers:
//!
//! - **Lifecycle**: Event loop, quit
//! - **Windows**: Creation, management, events
//! - **Displays**: Monitor enumeration and information
//! - **Executor**: Thread-safe background task execution
//! - **Clipboard**: Read/write operations
//! - **Callbacks**: Event handler registration
//!
//! ## Platform Selection
//!
//! Use [`current_platform()`] to get the appropriate platform for the current
//! environment:
//!
//! ```rust,ignore
//! use flui_native::current_platform;
//!
//! let platform = current_platform()?;
//! // `App` implements `AppHandler`: its `ready` is the bootstrap.
//! platform.run(Rc::new(App::default()))?;
//! ```
//!
//! ## Testing with Headless Mode
//!
//! The [`HeadlessPlatform`] provides a mock implementation perfect for
//! CI/testing without requiring a display server, GPU, or OS windowing system.
//!
//! ### Direct Usage
//!
//! ```rust
//! use flui_native::{Platform, headless_platform};
//!
//! let platform = headless_platform();
//! assert_eq!(platform.name(), "Headless");
//! ```
//!
//! ### Environment Variable (Recommended for CI)
//!
//! Set `FLUI_HEADLESS=1` to force headless mode via [`current_platform()`]:
//!
//! ```bash
//! # Run tests in headless mode
//! FLUI_HEADLESS=1 cargo test
//!
//! # CI configuration
//! - name: Run tests
//!   run: cargo test
//!   env:
//!     FLUI_HEADLESS: 1
//! ```
//!
//! ```rust,ignore
//! use flui_native::current_platform;
//!
//! // Returns HeadlessPlatform when FLUI_HEADLESS=1
//! let platform = current_platform()?;
//! assert_eq!(platform.name(), "Headless");
//! ```
//!
//! ### What Headless Mode Provides
//!
//! - **Mock Windows**: `open_window()` returns mock windows (no OS windows
//!   created)
//! - **In-Memory Clipboard**: Full clipboard API with in-memory storage
//! - **Mock Displays**: Single virtual display at 1920x1080
//! - **Background Executor**: Async task execution with tokio runtime
//! - **Fast Tests**: <100ms overhead, suitable for rapid test iteration
//! - **Parallel Safe**: Thread-safe, no race conditions in parallel test
//!   execution
//!
//! ### Example Test
//!
//! ```rust
//! use flui_native::headless_platform;
//! use flui_platform::WindowOptions;
//! use flui_foundation::geometry::Size;
//! use flui_testing::window_handler::RecordingWindowHandler;
//!
//! fn test_window_creation() {
//!     let platform = headless_platform();
//!
//!     let options = WindowOptions::default()
//!         .with_title("Test")
//!         .with_size(Size::new(800.0, 600.0))
//!         .with_visible(true);
//!
//!     let handler = RecordingWindowHandler::new();
//!     let window = platform
//!         .open_window(options, handler.clone())
//!         .expect("Failed to create window");
//!     // The handler's first input is the window's capability snapshot.
//!     assert!(!handler.inputs().is_empty());
//!     // Window is a mock, no actual OS resources allocated
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `default` (`desktop`): the native desktop backend for the target OS. It
//!   does not include winit.
//! - `winit-backend`: the winit backend, the Linux windowing path today. It
//!   brings `wayland` and `x11`, and on Linux the AT-SPI accessibility bridge.
//! - `wayland`, `x11`: on Linux, the clients the display source uses to read the
//!   outputs and their refresh rates from the compositor. On other targets they
//!   add nothing.
//! - `testing`: the `testing` module, test support for a native host's own
//!   windows (ADR-0243). Not part of the release API.
//!
//! Accessibility has no feature: Win32, AppKit and Linux winit windows always
//! hand out their operating system's AccessKit adapter. The headless testing
//! platform needs no feature.
//!
//! # Input capabilities
//!
//! There is no per-platform capability constant (ADR-0213). A capability with
//! a handle answers through it (`PlatformWindow::haptics`, `text_input`); the
//! rest arrive per window as `PlatformInput::Capabilities`: first as a
//! snapshot, the first input of the handler the window was opened with,
//! delivered before the open returns, then as changes in order with the input
//! around them.

// Platform FFI is the sanctioned `unsafe` boundary of the workspace (Win32 /
// AppKit / UIKit / Android NDK / wasm-bindgen — `headless` has none). The
// crate does not opt the whole tree out of `unsafe_code = "warn"`: each FFI
// island carries its own `expect(unsafe_code)` where it is declared — the
// loops `owner::windows`, `owner::apple`, `owner::web`, `owner::winit` and
// `owner::android::java_view`, and the platform files of the features that
// call the OS directly (`window::windows`, `input::windows`,
// `text::windows::tsf`, `display::apple::{macos, ios}`, `preferences::windows`
// and the like) — and the reading of the Windows build in `version::gate` is
// one item-level `expect`. A platform file's pure half forbids it. The Win32
// files are fully SAFETY-documented — every `unsafe` block there carries a
// `// SAFETY:` comment (or, for `unsafe fn`, a `# Safety` rustdoc section)
// stating the invariant that makes it sound. The AppKit, Android and web files
// are NOT yet at that bar; see the per-crate documentation coverage tracked in
// the root `Cargo.toml`'s `undocumented_unsafe_blocks` note — do not restate
// "every block" as a crate-wide claim until that note says so too.
// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub(crate) mod accessibility;
pub(crate) mod clipboard;
mod display;
pub(crate) mod drag;
pub mod error;
// Reachable for the backend conformance suite, which reads every backend's
// haptics degradation table (`tests/backend_conformance.rs`); not API.
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
#[doc(hidden)]
pub mod haptics;
// Reachable so a crate outside can show that a table missing a position does
// not compile (`tests/ui/host_table_names_every_position.rs`); not an embedding
// API: nothing outside the crate can build a `HostPlatform`.
#[doc(hidden)]
pub mod host;
mod host_window;
mod hosts;
// Reachable for the backend conformance table and the native recordings in
// `tests/`, which decode through the pointer assembly and the key rules, and
// for the allocation test, its own binary; not an embedding API. The event
// timeline, which the composition root stamps frames on, is named at the root.
#[doc(hidden)]
pub mod input;
pub use input::event_timestamp_ns;
mod lifecycle;
pub mod os;
mod owner;
mod platform;
mod preferences;
pub mod storage;
#[cfg(test)]
mod table_test;
pub mod task;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
// Reachable for the text position's trait, which bounds a host's table; not
// an embedding API.
#[doc(hidden)]
pub mod text;
pub use text::sizing::{
    CapturedTextSizing, TextSizingCaptureError, TextSizingCaptureState, capture_text_sizing,
};
mod version;
mod window;

// The acknowledgement a host waits for before it publishes a window it
// attached (a UIKit scene's), which the composition root completes.
pub use window::installation::{
    InstallationCancelled, PendingWindowInstallation, WindowInstallation,
};
// The typed lifecycle/service error taxonomy — `anyhow` is retired from
// this crate's public API (the window-open family lives with its
// capability types in `owner`).
pub use error::{BootstrapError, PlatformError, PlatformResult};
// Re-export executor types
#[cfg(not(target_arch = "wasm32"))]
pub use executor::BackgroundExecutor;
// Each host's platform: the one implementation of `Platform` over its table.
pub use hosts::HeadlessPlatform;
#[cfg(target_os = "ios")]
pub use hosts::IOSPlatform;
#[cfg(target_os = "macos")]
pub use hosts::MacOSPlatform;
pub use platform::HostPlatform;
// Web platform
#[cfg(target_arch = "wasm32")]
pub use hosts::WebPlatform;
// Desktop platforms
#[cfg(windows)]
pub use hosts::WindowsPlatform;
// winit fallback backend — primary on Linux until native Wayland/X11 lands
// (roadmap Cross.P)
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use hosts::WinitPlatform;
// The headless host's test controls and doubles, each its feature's.
pub use accessibility::headless::FakeAccessibility;
pub use display::headless::{
    DisplayChange, DisplayChangeError, DisplaySpec, HeadlessDisplayControls,
};
pub use drag::headless::HeadlessDragAndDrop;
pub use input::headless::HeadlessInputDevices;
pub use lifecycle::headless::HeadlessHostEvents;
pub use owner::headless::{
    HeadlessDeferredWindowOpens, HeadlessExitReevaluation, HeadlessOwnerTurns,
};
pub use preferences::headless::HeadlessPreferences;
pub use text::headless::FakeTextInput;
pub use window::headless::{
    HeadlessWindowPolicy, HeadlessWindowSystem, MockWindow, UnknownDisplay,
};
// What a UIKit scene tells the composition root.
#[cfg(target_os = "ios")]
pub use owner::apple::ios::{IOSSceneAttachmentId, IOSSceneSessionId};
#[cfg(target_os = "ios")]
pub use window::apple::ios::IOSSceneEvent;
// The frame request a host stamps on the event clock (`owner/handlers.rs`).
pub use owner::handlers::{frame_request_aged, frame_request_now, frame_request_timed};
// Re-export task types
pub use task::{Task, TaskLabel};
// The core host's own traits; the contract's names are imported from
// `flui_platform`, the accessibility capability from `flui_semantics::platform`.
// Each has this one path (ADR-0151 §9).
pub use host_window::HostWindow;
pub use platform::{AppHandler, ExitAnswer, PathPromptOptions, Platform, PlatformExecutor};
// The owner-thread capability (ADR-0039 §1): minted only by a backend,
// handed to `on_ready`, never re-exported with a public minting seam.
pub use owner::{
    OpenWindowError, OwnerPlatform, PendingWindow, PlatformProxy, ProxySendError, SharedPlatform,
    WaitError, WakeRegistrationError, WindowHandlerFactory, WindowOpen, WindowRequest,
};

/// Get the current platform implementation
///
/// Automatically selects the correct platform based on the target OS at compile
/// time. This is the recommended way to obtain a platform instance in
/// cross-platform code.
///
/// # Detection Logic
///
/// The platform selection follows a two-stage process:
///
/// 1. **Runtime Environment Check** (executed first):
///    - Checks `FLUI_HEADLESS` environment variable
///    - If set (any value), returns `HeadlessPlatform` immediately
///    - Bypasses all compile-time OS detection
///    - Used for CI/testing without GPU or display server
///
/// 2. **Compile-Time OS Detection** (if not headless):
///    - Uses Rust's `#[cfg]` attributes to select platform at compile time
///    - Selection order (first match wins):
///      - `cfg(windows)` → `WindowsPlatform`
///      - `cfg(target_os = "macos")` → `MacOSPlatform`
///      - `cfg(target_os = "linux")` → `WinitPlatform` (the `winit-backend`
///        feature)
///      - `cfg(target_os = "android")` → an error: the Android host starts
///        over the Activity `android_main` is handed (`android_platform`)
///      - `cfg(target_os = "ios")` → `IOSPlatform`
///      - `cfg(target_arch = "wasm32")` → `WebPlatform`
///    - Conditional guards prevent multiple platforms being compiled
///    - Results in zero runtime overhead (selection happens at compile time)
///
/// # Environment Variables
///
/// - **FLUI_HEADLESS=1**: Forces headless mode for CI/testing (overrides OS
///   detection)
///
/// # Platform Selection
///
/// - **Headless** (if `FLUI_HEADLESS=1`): Returns `HeadlessPlatform` - testing
///   mode
/// - **Windows**: Returns `WindowsPlatform` - fully implemented with Win32 API
/// - **macOS**: Returns `MacOSPlatform` - the native AppKit backend: windows,
///   input, clipboard, IME, displays, and a vsync-paced frame loop
/// - **Linux**: Returns `WinitPlatform` if the `winit-backend` feature is
///   enabled (the native Wayland and X11 host replaces it, ADR-0151
///   section 11); otherwise returns an error. `flui-app` enables
///   `winit-backend` for Linux builds.
/// - **Android**: Returns an error: the host needs the Activity, which the
///   system hands only to `android_main`; `android_platform(app)` starts it
/// - **iOS**: Returns `IOSPlatform` - the native UIKit backend: windows, touch
///   input, clipboard, displays, a `CADisplayLink` frame source, and the
///   background/foreground lifecycle (verified on a real simulator)
/// - **Web/WASM**: Returns `WebPlatform` - stub (unimplemented, roadmap
///   available)
///
/// # Platform Status
///
/// | Platform | Status | Quality | Features |
/// |----------|--------|---------|----------|
/// | Windows | ✅ Production | 10/10 | Full featured; no executing coverage in CI (lint-only via `cross-typecheck`) |
/// | macOS | ✅ Native AppKit | 9/10 | Windowing, input, clipboard, IME, displays, vsync-paced frame loop; lint-only + bundled probes locally, no CI test job |
/// | Linux | 🪟 winit fallback (`winit-backend`) | 5/10 | Windowing + input; native Wayland/X11 still a stub |
/// | Android | 📋 Stub | 2/10 | Roadmap complete |
/// | iOS | ✅ Native UIKit | 7/10 | Windowing, touch, clipboard, displays, `CADisplayLink` frame source; lint-only in CI (simulator verified) |
/// | Web | 📋 Stub | 2/10 | Roadmap complete |
///
/// # Errors
///
/// Returns [`PlatformError::Init`] if:
/// - Platform initialization fails (e.g., COM failure on Windows)
/// - Platform is not supported (should not happen with cfg guards)
/// - It is called on Android, whose host starts over the Activity
///   (`android_platform`)
/// - Linux is reached without the `winit-backend` feature enabled
///
/// # Examples
///
/// ```rust,ignore
/// use flui_native::current_platform;
///
/// // Get platform and run event loop
/// let platform = current_platform()?;
/// println!("Running on: {}", platform.name());
///
/// // `App` implements `AppHandler`: its `ready` is the bootstrap.
/// platform.run(Rc::new(App::default()))?;
/// ```
///
/// # Platform-Specific Code
///
/// What one OS offers beyond the window contract is an extension of
/// [`HostWindow`], which answers `None` for a window of another host:
///
/// ```rust,ignore
/// let platform = current_platform()?;
/// let window = platform.open_window(WindowOptions::default(), handler)?;
///
/// #[cfg(windows)]
/// {
///     use flui_native::os::windows::{WindowsBackdrop, WindowsWindowExt};
///     // A Win32 window's view; `None` under `FLUI_HEADLESS`.
///     if let Some(win32) = window.win32() {
///         win32.set_backdrop(WindowsBackdrop::Mica)?;
///     }
/// }
/// ```
pub fn current_platform() -> Result<Box<dyn Platform>, PlatformError> {
    // Check for headless mode via environment variable (CI/testing)
    if std::env::var("FLUI_HEADLESS").is_ok() {
        tracing::info!("FLUI_HEADLESS detected, using headless platform");
        return Ok(Box::new(HeadlessPlatform::new()));
    }

    #[cfg(windows)]
    {
        Ok(Box::new(WindowsPlatform::new()?))
    }

    #[cfg(all(target_os = "macos", not(windows)))]
    {
        Ok(Box::new(MacOSPlatform::new()?))
    }

    #[cfg(all(target_os = "linux", not(any(windows, target_os = "macos"))))]
    {
        // The winit fallback host is the Linux path until the native Wayland
        // and X11 host replaces it (ADR-0151 section 11). It is opt-in via
        // `winit-backend` because it pulls in `arboard` (clipboard); `flui-app`
        // enables it for Linux builds.
        #[cfg(feature = "winit-backend")]
        {
            Ok(Box::new(WinitPlatform::new()))
        }

        #[cfg(not(feature = "winit-backend"))]
        {
            Err(PlatformError::Init {
                message: "no Linux windowing backend enabled — flui-app enables `winit-backend` \
                          on Linux"
                    .to_string(),
            })
        }
    }

    #[cfg(all(
        target_os = "android",
        not(any(windows, target_os = "macos", target_os = "linux"))
    ))]
    {
        // The Android host needs the Activity, which the system hands only to
        // `android_main`: `android_platform(app)` starts it.
        Err(PlatformError::Init {
            message: "On Android, start the platform with `flui_native::android_platform(app)` \
                      from android_main() instead of current_platform()"
                .to_string(),
        })
    }

    #[cfg(all(
        target_os = "ios",
        not(any(
            windows,
            target_os = "macos",
            target_os = "linux",
            target_os = "android"
        ))
    ))]
    {
        Ok(Box::new(IOSPlatform::new()?))
    }

    #[cfg(all(
        target_arch = "wasm32",
        not(any(
            windows,
            target_os = "macos",
            target_os = "linux",
            target_os = "android",
            target_os = "ios"
        ))
    ))]
    {
        Ok(Box::new(WebPlatform::new()?))
    }

    #[cfg(not(any(
        windows,
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32"
    )))]
    {
        Err(PlatformError::Init {
            message: "Unsupported platform - no platform implementation available for this target"
                .to_string(),
        })
    }
}

/// Create a headless platform for testing
///
/// This is a convenience function for tests. Prefer using this over
/// `current_platform()` in test code for clarity.
///
/// # Example
///
/// ```rust
/// use flui_native::headless_platform;
///
/// let platform = headless_platform();
/// assert_eq!(platform.name(), "Headless");
/// ```
pub fn headless_platform() -> Box<dyn Platform> {
    Box::new(HeadlessPlatform::new())
}

/// The Android platform over the Activity the system handed `android_main`.
///
/// The one public item of the core host that names an `android-activity`
/// type: the system hands the Activity only to the application's
/// `android_main`, and the composition root passes it on here, as the
/// facade's `run_app_android` takes it (ADR-0151 §8). Nothing the platform
/// hands back names it.
///
/// ```rust,ignore
/// #[unsafe(no_mangle)]
/// fn android_main(app: android_activity::AndroidApp) {
///     let platform = flui_native::android_platform(app);
///     // `App` implements `AppHandler`: its `ready` is the bootstrap.
///     let _ = platform.run(Rc::new(App::default()));
/// }
/// ```
#[cfg(target_os = "android")]
#[must_use]
pub fn android_platform(app: android_activity::AndroidApp) -> Box<dyn Platform> {
    Box::new(HostPlatform::<hosts::android::Android>::new(app))
}
