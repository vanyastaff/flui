//! Cross-platform embedder abstraction for FLUI
//!
//! This crate provides a unified interface for running FLUI apps across
//! Desktop (Windows, macOS, Linux), Android, iOS, and Web platforms.
//!
//! # Architecture
//!
//! ```text
//! flui-platform
//!   ├─ traits/           - Core trait definitions
//!   │   ├─ embedder.rs   - PlatformEmbedder trait
//!   │   ├─ window.rs     - PlatformWindow trait
//!   │   ├─ lifecycle.rs  - PlatformLifecycle trait
//!   │   └─ capabilities.rs - PlatformCapabilities trait
//!   │
//!   ├─ core/             - Shared implementation
//!   │   ├─ embedder_core.rs - EmbedderCore (90% shared logic)
//!   │   ├─ scene_cache.rs - Type-safe scene caching
//!   │   ├─ pointer_state.rs - Pointer tracking
//!   │   └─ frame_coordinator.rs - Frame rendering
//!   │
//!   ├─ bindings/         - Framework bindings (Flutter-style)
//!   │   ├─ scheduler_binding.rs - Frame scheduling
//!   │   └─ gesture_binding.rs - Safe hit testing
//!   │
//!   └─ platforms/        - Platform-specific embedders
//!       ├─ desktop.rs    - Windows/macOS/Linux
//!       ├─ android.rs    - Android
//!       ├─ ios.rs        - iOS
//!       └─ web.rs        - WebAssembly
//! ```
//!
//! # Key Features
//!
//! - **Zero unsafe code** - Safe hit testing via immutable API
//! - **Maximum code reuse** - 90%+ shared via EmbedderCore
//! - **Type-safe platforms** - Compile-time platform selection
//! - **Easy extensibility** - Add new platforms easily
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_platform::platforms::DesktopEmbedder;
//! use flui_core::binding::AppBinding;
//!
//! async fn run_app() {
//!     let binding = AppBinding::ensure_initialized();
//!     // Embedder created in event loop after Resumed event
//! }
//! ```

pub mod bindings;
pub mod core;
pub mod platforms;
pub mod traits;

// Re-export core types
pub use traits::{
    DesktopCapabilities, MobileCapabilities, PlatformCapabilities, PlatformEmbedder,
    PlatformLifecycle, PlatformWindow, WebCapabilities,
};

pub use core::{EmbedderCore, FrameCoordinator, PointerState, SceneCache};

pub use bindings::{GestureBinding, SchedulerBinding};

// Platform-specific re-exports
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use platforms::desktop::DesktopEmbedder;

#[cfg(target_os = "android")]
pub use platforms::android::AndroidEmbedder;

#[cfg(target_os = "ios")]
pub use platforms::ios::IosEmbedder;

#[cfg(target_arch = "wasm32")]
pub use platforms::web::WebEmbedder;

// Error types
use thiserror::Error;

/// Platform-level errors
#[derive(Error, Debug)]
pub enum PlatformError {
    /// Failed to create platform window
    #[error("Failed to create window: {0}")]
    WindowCreation(String),

    /// Failed to initialize GPU
    #[error("Failed to initialize GPU: {0}")]
    GpuInitialization(String),

    /// Surface error during rendering
    #[error("Surface error: {0}")]
    Surface(String),

    /// Lifecycle state error
    #[error("Lifecycle error: {0}")]
    Lifecycle(String),

    /// Event routing error
    #[error("Event routing error: {0}")]
    EventRouting(String),
}

/// Result type for platform operations
pub type Result<T> = std::result::Result<T, PlatformError>;
