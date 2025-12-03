//! Platform embedder module
//!
//! This module re-exports platform-specific embedders from `flui-platform`.
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
//! All embedders are thin wrappers around `flui_platform::EmbedderCore`,
//! which contains 90%+ of the shared logic.
//!
//! ```text
//! flui_platform::EmbedderCore (shared logic)
//!   ├─ Pipeline coordination
//!   ├─ Safe hit testing (GestureBinding)
//!   ├─ Frame scheduling (SchedulerBinding)
//!   └─ Scene caching
//!
//! Platform Embedders (thin wrappers)
//!   ├─ DesktopEmbedder - winit window + GPU
//!   └─ AndroidEmbedder - Android lifecycle
//! ```

// Re-export from flui-platform
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_platform::DesktopEmbedder;

#[cfg(target_os = "android")]
pub use flui_platform::AndroidEmbedder;

// Backward compatibility: WgpuEmbedder type alias
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub type WgpuEmbedder = DesktopEmbedder;

#[cfg(target_os = "android")]
pub type WgpuEmbedder = AndroidEmbedder;
