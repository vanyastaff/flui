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

pub mod platforms;
pub mod shared;
pub mod traits;

// Re-export core traits
pub use traits::{
    Clipboard, DefaultLifecycle, DesktopCapabilities, DisplayId, LifecycleEvent, LifecycleState,
    MobileCapabilities, Platform, PlatformCapabilities, PlatformDisplay, PlatformEmbedder,
    PlatformExecutor, PlatformLifecycle, PlatformTextSystem, PlatformWindow, WebCapabilities,
    WindowEvent, WindowId, WindowOptions,
};

// Re-export platform implementations
pub use platforms::HeadlessPlatform;

// TEMPORARILY DISABLED for Phase 1
// #[cfg(windows)]
// pub use platforms::WindowsPlatform;

// #[cfg(feature = "winit-backend")]
// pub use platforms::WinitPlatform;

// #[cfg(feature = "winit-backend")]
// pub use traits::WinitWindow;

// Re-export shared infrastructure
pub use shared::PlatformHandlers;

use std::sync::Arc;

/// Get the current platform implementation
///
/// This function returns the appropriate platform for the current environment:
///
/// - **Windows**: Returns [`WindowsPlatform`] (native Win32 API)
/// - **Headless/Testing**: Returns [`HeadlessPlatform`] if `FLUI_HEADLESS=1`
///
/// # Example
///
/// ```rust,ignore
/// use flui_platform::current_platform;
///
/// let platform = current_platform();
/// println!("Running on: {}", platform.name());
///
/// platform.run(Box::new(|| {
///     println!("Platform initialized!");
/// }));
/// ```
///
/// # Environment Variables
///
/// - `FLUI_HEADLESS`: Set to `1` to force headless mode (useful for CI)
pub fn current_platform() -> Arc<dyn Platform> {
    // PHASE 1: Only headless platform supported
    tracing::info!("Using headless platform (Phase 1 - foundation layer only)");
    Arc::new(HeadlessPlatform::new())

    // TEMPORARILY DISABLED for Phase 1
    // // Check for headless mode
    // if std::env::var("FLUI_HEADLESS").unwrap_or_default() == "1" {
    //     tracing::info!("Using headless platform (FLUI_HEADLESS=1)");
    //     return Arc::new(HeadlessPlatform::new());
    // }

    // // Windows: Native Win32 platform
    // #[cfg(windows)]
    // {
    //     tracing::info!("Using Windows native platform (Win32 API)");
    //     return Arc::new(WindowsPlatform::new().expect("Failed to create Windows platform"));
    // }

    // // Legacy winit backend (if enabled)
    // #[cfg(all(feature = "winit-backend", not(windows)))]
    // {
    //     tracing::info!("Using legacy winit platform");
    //     return Arc::new(WinitPlatform::new());
    // }

    // // Fallback to headless
    // #[cfg(not(any(windows, feature = "winit-backend")))]
    // {
    //     tracing::warn!("No native platform available, falling back to headless");
    //     Arc::new(HeadlessPlatform::new())
    // }
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
