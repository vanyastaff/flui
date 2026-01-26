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
//! The [`Platform`] trait is the central abstraction that all platform implementations
//! must provide. It covers:
//!
//! - **Lifecycle**: Event loop, quit, frame requests
//! - **Windows**: Creation, management, events
//! - **Displays**: Monitor enumeration and information
//! - **Executors**: Background and foreground task execution
//! - **Text System**: Font loading and text rendering
//! - **Clipboard**: Read/write operations
//! - **Callbacks**: Event handler registration
//!
//! ## Platform Selection
//!
//! Use [`current_platform()`] to get the appropriate platform for the current environment:
//!
//! ```rust,ignore
//! use flui_platform::current_platform;
//!
//! let platform = current_platform();
//! platform.run(Box::new(|| {
//!     println!("Platform ready: {}", platform.name());
//! }));
//! ```
//!
//! ## Testing
//!
//! The [`HeadlessPlatform`] provides a no-op implementation perfect for unit tests:
//!
//! ```rust
//! use flui_platform::{HeadlessPlatform, Platform};
//!
//! let platform = HeadlessPlatform::new();
//! assert_eq!(platform.name(), "Headless");
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

pub mod config;
pub mod executor;
pub mod platforms;
pub mod shared;
pub mod traits;
pub mod window;

// Re-export configuration types
pub use config::{FullscreenMonitor, WindowConfiguration};

// Re-export executor types
pub use executor::{BackgroundExecutor, ForegroundExecutor};

// Re-export core traits
pub use traits::{
    Clipboard, DefaultLifecycle, DesktopCapabilities, DisplayId, GlyphPosition, LifecycleEvent,
    LifecycleState, MobileCapabilities, Platform, PlatformCapabilities, PlatformDisplay,
    PlatformEmbedder, PlatformExecutor, PlatformLifecycle, PlatformTextSystem, PlatformWindow,
    TextSystemError, WebCapabilities, WindowEvent, WindowId, WindowMode, WindowOptions,
};

// Re-export platform implementations
pub use platforms::HeadlessPlatform;

// Desktop platforms
#[cfg(windows)]
pub use platforms::WindowsPlatform;

#[cfg(target_os = "macos")]
pub use platforms::MacOSPlatform;

#[cfg(target_os = "linux")]
pub use platforms::LinuxPlatform;

// Mobile platforms
#[cfg(target_os = "android")]
pub use platforms::AndroidPlatform;

#[cfg(target_os = "ios")]
pub use platforms::IOSPlatform;

// Web platform
#[cfg(target_arch = "wasm32")]
pub use platforms::WebPlatform;

// Legacy backend
#[cfg(feature = "winit-backend")]
pub use platforms::WinitPlatform;

// Re-export shared infrastructure
pub use shared::PlatformHandlers;

// ==================== Platform Detection ====================

use std::sync::Arc;

/// Get the current platform implementation
///
/// Automatically selects the correct platform based on the target OS at compile time.
/// This is the recommended way to obtain a platform instance in cross-platform code.
///
/// # Platform Selection
///
/// - **Windows**: Returns `WindowsPlatform` - fully implemented with Win32 API
/// - **macOS**: Returns `MacOSPlatform` - stub (unimplemented, roadmap available)
/// - **Linux**: Returns `LinuxPlatform` - stub (unimplemented, roadmap available)
/// - **Android**: Returns `AndroidPlatform` - stub (unimplemented, roadmap available)
/// - **iOS**: Returns `IOSPlatform` - stub (unimplemented, roadmap available)
/// - **Web/WASM**: Returns `WebPlatform` - stub (unimplemented, roadmap available)
///
/// # Platform Status
///
/// | Platform | Status | Quality | Features |
/// |----------|--------|---------|----------|
/// | Windows | ✅ Production | 10/10 | Full featured |
/// | macOS | 📋 Stub | 2/10 | Roadmap complete |
/// | Linux | 📋 Stub | 2/10 | Roadmap complete |
/// | Android | 📋 Stub | 2/10 | Roadmap complete |
/// | iOS | 📋 Stub | 2/10 | Roadmap complete |
/// | Web | 📋 Stub | 2/10 | Roadmap complete |
///
/// # Errors
///
/// Returns an error if:
/// - Platform initialization fails (e.g., COM failure on Windows)
/// - Platform is not supported (should not happen with cfg guards)
/// - Platform stub is called (macOS, Linux, Android, iOS, Web)
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
/// platform.run(Box::new(|| {
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
pub fn current_platform() -> anyhow::Result<Arc<dyn Platform>> {
    #[cfg(windows)]
    {
        Ok(Arc::new(WindowsPlatform::new()?))
    }

    #[cfg(all(target_os = "macos", not(windows)))]
    {
        Ok(Arc::new(MacOSPlatform::new()?))
    }

    #[cfg(all(target_os = "linux", not(any(windows, target_os = "macos"))))]
    {
        Ok(Arc::new(LinuxPlatform::new()?))
    }

    #[cfg(all(
        target_os = "android",
        not(any(windows, target_os = "macos", target_os = "linux"))
    ))]
    {
        Ok(Arc::new(AndroidPlatform::new()?))
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
        Ok(Arc::new(IOSPlatform::new()?))
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
        Ok(Arc::new(WebPlatform::new()?))
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
pub fn headless_platform() -> Arc<dyn Platform> {
    Arc::new(HeadlessPlatform::new())
}
