//! Platform embedder module
//!
//! This module provides platform-specific integration layers for FLUI apps.
//! Each platform has its own embedder optimized for its lifecycle and requirements.
//!
//! # Platform Selection
//!
//! The appropriate embedder is automatically selected based on the target platform:
//! - **Desktop** (Windows, macOS, Linux): `DesktopEmbedder`
//! - **Android**: `AndroidEmbedder`
//!
//! # Architecture
//!
//! ```text
//! Platform Embedder (desktop.rs / android.rs)
//!   ├─ Window Management (winit)
//!   ├─ GPU Rendering (flui_engine::GpuRenderer)
//!   ├─ Framework Integration (AppBinding)
//!   └─ Platform Lifecycle (desktop: simple / android: suspend/resume)
//! ```

// Platform-specific modules
#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
mod android;

// Platform-specific re-exports
#[cfg(not(target_os = "android"))]
pub use desktop::DesktopEmbedder;

#[cfg(target_os = "android")]
pub use android::AndroidEmbedder;

// Backward compatibility: WgpuEmbedder type alias
// This allows existing code to continue working without changes
#[cfg(not(target_os = "android"))]
pub type WgpuEmbedder = DesktopEmbedder;

#[cfg(target_os = "android")]
pub type WgpuEmbedder = AndroidEmbedder;
