//! Platform abstraction layer for FLUI
//!
//! This crate provides a complete platform abstraction for the FLUI framework,
//! enabling cross-platform UI development with a unified API.
//!
//! # Architecture
//!
//! The architecture is inspired by GPUI and Flutter's platform layer:
//!
//! ```text
//! flui-platform
//!   ├─ traits/              - Core abstractions
//!   │   ├─ platform.rs      - Central Platform trait
//!   │   ├─ window.rs        - Window abstraction
//!   │   ├─ display.rs       - Display/monitor info
//!   │   ├─ capabilities.rs  - Platform capabilities
//!   │   └─ lifecycle.rs     - App lifecycle
//!   │
//!   ├─ shared/              - Shared infrastructure
//!   │   └─ handlers.rs      - Callback registry
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
//! - **Executors**: Background and foreground task execution
//! - **Clipboard**: Read/write operations
//! - **Callbacks**: Event handler registration
//!
//! ## Platform Selection
//!
//! Use [`current_platform()`] to get the appropriate platform for the current
//! environment:
//!
//! ```rust,ignore
//! use flui_platform::current_platform;
//!
//! let platform = current_platform()?;
//! platform.run(Box::new(|platform| {
//!     println!("Platform ready: {}", platform.name());
//! }));
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
//! use flui_platform::{Platform, headless_platform};
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
//! use flui_platform::current_platform;
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
//! - **Foreground Executor**: Channel-based task queue for main thread
//! - **Fast Tests**: <100ms overhead, suitable for rapid test iteration
//! - **Parallel Safe**: Thread-safe, no race conditions in parallel test
//!   execution
//!
//! ### Example Test
//!
//! ```rust
//! use flui_platform::{WindowOptions, headless_platform};
//! use flui_types::geometry::{Size, px};
//!
//! fn test_window_creation() {
//!     let platform = headless_platform();
//!
//!     let options = WindowOptions {
//!         title: "Test".to_string(),
//!         size: Size::new(px(800.0), px(600.0)),
//!         visible: true,
//!         ..Default::default()
//!     };
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
//! - `default`: Includes winit platform
//! - `headless`: Includes headless testing platform
//!
//! # Platform Capabilities
//!
//! Query platform capabilities to adapt behavior:
//!
//! ```rust,ignore
//! use flui_platform::current_platform;
//!
//! let platform = current_platform();
//! let caps = platform.capabilities();
//!
//! if caps.supports_touch() {
//!     // Enable touch gestures
//! }
//!
//! if caps.suspend_rendering_in_background() {
//!     // Implement background suspension
//! }
//! ```

// Platform FFI is the sanctioned `unsafe` boundary of the workspace (Win32 /
// AppKit / headless backends); every block carries a `// SAFETY:` comment.
// Permanent, unlike the tracked debt below.
#![allow(unsafe_code)]
// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub mod config;
pub mod cursor;
#[cfg(not(target_arch = "wasm32"))]
pub mod executor;
pub mod platforms;
pub mod shared;
pub mod task;
pub mod traits;
pub mod window; // PORT-CHECK-OK-SP4: window API surface; binding entry for platform integrators

// Re-export configuration types
// ==================== Platform Detection ====================

pub use config::{FullscreenMonitor, WindowConfiguration};
// Re-export cursor types
pub use cursor::CursorStyle;
// Re-export executor types
#[cfg(not(target_arch = "wasm32"))]
pub use executor::{BackgroundExecutor, ForegroundExecutor};
// Mobile platforms
#[cfg(target_os = "android")]
pub use platforms::AndroidPlatform;
// Re-export platform implementations
#[cfg(target_os = "ios")]
pub use platforms::IOSPlatform;
#[cfg(target_os = "linux")]
pub use platforms::LinuxPlatform;
#[cfg(target_os = "macos")]
pub use platforms::MacOSPlatform;
pub use platforms::{FakeHaptics, FakeTextInput, HeadlessPlatform};
// Web platform
#[cfg(target_arch = "wasm32")]
pub use platforms::WebPlatform;
// Desktop platforms
#[cfg(windows)]
pub use platforms::WindowsPlatform;
// winit fallback backend — primary on Linux until native Wayland/X11 lands
// (roadmap Cross.P)
#[cfg(feature = "winit-backend")]
pub use platforms::WinitPlatform;
// Re-export shared infrastructure
pub use shared::{PlatformHandlers, WindowCallbacks};
// Re-export task types
pub use task::{Priority, Task, TaskLabel};
// Re-export core traits
pub use traits::{
    Clipboard, ClipboardItem, DefaultLifecycle, DesktopCapabilities, DispatchEventResult,
    DisplayId, LifecycleEvent, LifecycleState, MobileCapabilities, PathPromptOptions, Platform,
    PlatformCapabilities, PlatformDisplay, PlatformEmbedder, PlatformExecutor, PlatformHaptics,
    PlatformLifecycle, PlatformReadyCallback, PlatformTextInput, PlatformWindow, WebCapabilities,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowEvent, WindowId, WindowMode,
    WindowOptions,
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
///      - `cfg(target_os = "linux")` → `LinuxPlatform`
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
/// - **macOS**: Returns `MacOSPlatform` - stub (unimplemented, roadmap
///   available)
/// - **Linux**: Returns `WinitPlatform` if the `winit-backend` feature is
///   enabled (native Wayland/X11 — `LinuxPlatform` — is not implemented yet,
///   roadmap Cross.P); otherwise returns an error. `flui-app` enables
///   `winit-backend` for Linux builds.
/// - **Android**: Returns `AndroidPlatform` - stub (unimplemented, roadmap
///   available)
/// - **iOS**: Returns `IOSPlatform` - stub (unimplemented, roadmap available)
/// - **Web/WASM**: Returns `WebPlatform` - stub (unimplemented, roadmap
///   available)
///
/// # Platform Status
///
/// | Platform | Status | Quality | Features |
/// |----------|--------|---------|----------|
/// | Windows | ✅ Production | 10/10 | Full featured |
/// | macOS | 📋 Stub | 2/10 | Roadmap complete |
/// | Linux | 🪟 winit fallback (`winit-backend`) | 5/10 | Windowing + input; native Wayland/X11 still a stub |
/// | Android | 📋 Stub | 2/10 | Roadmap complete |
/// | iOS | 📋 Stub | 2/10 | Roadmap complete |
/// | Web | 📋 Stub | 2/10 | Roadmap complete |
///
/// # Errors
///
/// Returns an error if:
/// - Platform initialization fails (e.g., COM failure on Windows)
/// - Platform is not supported (should not happen with cfg guards)
/// - Platform stub is called (macOS, Android, iOS, Web)
/// - Linux is reached without the `winit-backend` feature enabled
///
/// # Examples
///
/// ```rust,ignore
/// use flui_platform::current_platform;
///
/// // Get platform and run event loop
/// let platform = current_platform()?;
/// println!("Running on: {}", platform.name());
///
/// platform.run(Box::new(|_platform| {
///     println!("Platform ready!");
/// }));
/// ```
///
/// ```rust,ignore
/// // Check platform capabilities
/// let platform = current_platform()?;
/// let caps = platform.capabilities();
///
/// if caps.supports_multiple_windows() {
///     // Open multiple windows
/// }
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
pub fn current_platform() -> anyhow::Result<Box<dyn Platform>> {
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
        // Native Wayland/X11 is not implemented yet (`LinuxPlatform` is a
        // stub — roadmap Cross.P); the winit fallback backend is the Linux
        // path until then. It is opt-in via `winit-backend` because it pulls
        // in `arboard` (clipboard); `flui-app` enables it for Linux builds.
        #[cfg(feature = "winit-backend")]
        {
            Ok(Box::new(WinitPlatform::new()))
        }

        #[cfg(not(feature = "winit-backend"))]
        {
            Err(anyhow::anyhow!(
                "no Linux windowing backend enabled — flui-app enables `winit-backend` on Linux"
            ))
        }
    }

    #[cfg(all(
        target_os = "android",
        not(any(windows, target_os = "macos", target_os = "linux"))
    ))]
    {
        // On Android, use AndroidPlatform::new(app) directly from android_main().
        // current_platform() cannot be used because AndroidApp is required.
        anyhow::bail!(
            "On Android, use AndroidPlatform::new(app) from android_main() instead of current_platform()"
        )
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
        Err(anyhow::anyhow!(
            "Unsupported platform - no platform implementation available for this target"
        ))
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
/// use flui_platform::headless_platform;
///
/// let platform = headless_platform();
/// assert_eq!(platform.name(), "Headless");
/// ```
pub fn headless_platform() -> Box<dyn Platform> {
    Box::new(HeadlessPlatform::new())
}
