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
//!   ├─ host.rs, hosts/      - A host's table of positions, one per host
//!   ├─ owner/               - Owner-thread capability (ADR-0039), its signal
//!   │                         and slot, the panic boundary, the one call
//!   │                         point for handlers and the handler registry
//!   ├─ display/             - The displays: the registry and snapshot model
//!   │                         beside one file per platform
//!   ├─ text/                - Text input and the system text size: the
//!   │                         model and one module per platform
//!   │
//!   ├─ shared/              - Shared infrastructure
//!   │
//!   └─ platforms/           - Concrete implementations
//!       ├─ winit/           - Cross-platform (Windows/macOS/Linux)
//!       └─ headless/        - Testing implementation
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
//! platform.run(Box::new(|owner| {
//!     println!("Platform ready: {}", owner.shared().name());
//!     Ok(())
//! }))?;
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
//!
//! fn test_window_creation() {
//!     let platform = headless_platform();
//!
//!     let options = WindowOptions::default()
//!         .with_title("Test")
//!         .with_size(Size::new(800.0, 600.0))
//!         .with_visible(true);
//!
//!     let window = platform
//!         .open_window(options)
//!         .expect("Failed to create window");
//!     // Window is a mock, no actual OS resources allocated
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `default` (`desktop`): the native desktop backend for the target OS. It
//!   does not include winit.
//! - `winit-backend`: the winit backend, the Linux windowing path today. It
//!   brings `wayland` and `x11`.
//! - `wayland`, `x11`: on Linux, the clients the display source uses to read the
//!   outputs and their refresh rates from the compositor. On other targets they
//!   add nothing.
//! - `a11y`: the per-OS accessibility bridges.
//!
//! The headless testing platform needs no feature.
//!
//! # Input capabilities
//!
//! There is no per-platform capability constant (ADR-0213). A capability with
//! a handle answers through it (`PlatformWindow::haptics`, `text_input`); the
//! rest arrive per window as `PlatformInput::Capabilities`, first as a
//! snapshot before any other input once the window's input callback is
//! installed, then as changes in order with the input around them.

// Platform FFI is the sanctioned `unsafe` boundary of the workspace (Win32 /
// AppKit / Android NDK / wasm-bindgen backends — `headless` has none). The
// crate no longer opts the whole tree out of `unsafe_code = "warn"`: each
// FFI island (`platforms::windows`, `platforms::macos`, `platforms::web`,
// `platforms::winit`, `platforms::android::memory`, the loops
// `owner::windows`, `owner::apple`, `owner::web`, `owner::winit`, the display
// sources `display::windows::source`, `display::apple::{macos, ios}`, and the
// text services `text::windows::tsf`, `text::apple::macos`, `text::apple::ios`)
// carries its own module-level `expect(unsafe_code)` where it is declared, and
// the two
// small marker-trait impls in `window.rs`/`task.rs` carry item-level allows.
// `platforms::windows` and `owner::windows` are fully SAFETY-documented — every
// `unsafe` block there carries a `// SAFETY:` comment (or, for `unsafe fn`,
// a `# Safety` rustdoc section) stating the invariant that makes it sound.
// `platforms::macos`/`android`/`web` are NOT yet at that bar; see the
// per-crate documentation coverage tracked in the root `Cargo.toml`'s
// `undocumented_unsafe_blocks` note — do not restate "every block" as a
// crate-wide claim until that note says so too.
// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub mod config;
mod display;
pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
// Reachable so a crate outside can show that a table missing a position does
// not compile (`tests/ui/host_table_names_every_position.rs`); not an embedding
// API: nothing outside the crate can build a `HostPlatform`.
#[doc(hidden)]
pub mod host;
mod host_window;
mod hosts;
mod owner;
mod platform;
pub mod platforms;
pub mod shared;
pub mod storage;
#[cfg(test)]
mod table_test;
pub mod task;
// Reachable for the text position's trait, which bounds a host's table, and
// for the backend conformance table, which decodes a composing key with the
// Android and web rules; not an embedding API.
#[doc(hidden)]
pub mod text;
pub use text::sizing::{
    CapturedTextSizing, TextSizingCaptureError, TextSizingCaptureState, capture_text_sizing,
};

// Re-export configuration types
// ==================== Platform Detection ====================

pub use config::{FullscreenMonitor, WindowConfiguration};
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
// Mobile platforms
#[cfg(target_os = "android")]
pub use hosts::AndroidPlatform;
// Re-export platform implementations
pub use platforms::{
    DisplayChange, DisplayChangeError, DisplaySpec, FakeAccessibility, FakeHaptics,
    FakeSystemChrome, FakeTextInput, HeadlessDeferredWindowOpens, HeadlessDisplayControls,
    HeadlessDragAndDrop, HeadlessExitReevaluation, HeadlessHostEvents, HeadlessInputDevices,
    HeadlessOwnerTurns, HeadlessPreferences, HeadlessWindowPolicy, MockWindow,
};
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
// The application's handler set and a window's callbacks (`owner/handlers.rs`).
pub use owner::handlers::{InputCallback, InputWake, WindowCallbacks};
// Re-export task types
pub use task::{Task, TaskLabel};
// The core host's own traits; the contract's names are imported from
// `flui_platform`, the accessibility capability from `flui_semantics::platform`.
// Each has this one path (ADR-0151 §9).
pub use host_window::HostWindow;
pub use platform::{PathPromptOptions, Platform, PlatformExecutor, PlatformReadyCallback};
// The owner-thread capability (ADR-0039 §1): minted only by a backend,
// handed to `on_ready`, never re-exported with a public minting seam.
pub use owner::{
    OpenWindowError, OwnerPlatform, PendingWindow, PlatformProxy, ProxySendError, SharedPlatform,
    WaitError, WakeRegistrationError, WindowOpen,
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
///      - `cfg(target_os = "android")` → `AndroidPlatform`
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
/// - **Android**: Returns `AndroidPlatform` - stub (unimplemented, roadmap
///   available)
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
/// - Platform stub is called (Android, Web)
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
/// platform.run(Box::new(|_owner| {
///     println!("Platform ready!");
///     Ok(())
/// }))?;
/// ```
///
/// # Platform-Specific Code
///
/// For platform-specific features, use cfg guards:
///
/// ```rust,ignore
/// let platform = current_platform()?;
///
/// #[cfg(windows)]
/// {
///     let windows_platform = platform.as_any()
///         .downcast_ref::<WindowsPlatform>()
///         .unwrap();
///     // Use Windows-specific features
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
        // On Android, use AndroidPlatform::new(app) directly from android_main().
        // current_platform() cannot be used because AndroidApp is required.
        Err(PlatformError::Init {
            message: "On Android, use AndroidPlatform::new(app) from android_main() instead of \
                      current_platform()"
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
