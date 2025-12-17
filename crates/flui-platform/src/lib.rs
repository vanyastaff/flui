//! Platform abstraction layer for FLUI
//!
//! This crate provides platform-specific traits and capabilities for FLUI.
//! It defines what each platform supports, allowing the framework to adapt.
//!
//! # Architecture
//!
//! ```text
//! flui-platform
//!   └─ traits/           - Platform abstraction traits
//!       ├─ capabilities.rs - PlatformCapabilities (features per platform)
//!       ├─ lifecycle.rs    - PlatformLifecycle (foreground/background)
//!       ├─ window.rs       - PlatformWindow (window abstraction)
//!       └─ embedder.rs     - PlatformEmbedder (embedder contract)
//! ```
//!
//! # Future Plans
//!
//! This crate will be extended with:
//! - **Accessibility (a11y)** - Screen reader support, semantic tree
//! - **Sensors** - Accelerometer, gyroscope, GPS
//! - **Platform services** - Clipboard, file picker, notifications
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::{PlatformCapabilities, DesktopCapabilities};
//!
//! let caps = DesktopCapabilities;
//! if caps.supports_touch() {
//!     // Enable touch gestures
//! }
//! ```

pub mod traits;

// Re-export core types
pub use traits::{
    DefaultLifecycle, DesktopCapabilities, LifecycleEvent, LifecycleState, MobileCapabilities,
    PlatformCapabilities, PlatformEmbedder, PlatformLifecycle, PlatformWindow, WebCapabilities,
    WinitWindow,
};
