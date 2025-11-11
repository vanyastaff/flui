//! Platform embedder module
//!
//! This module provides the platform integration layer for FLUI apps.
//! It handles window creation, event loop, and GPU rendering.

mod wgpu;

// Re-exports
pub use self::wgpu::WgpuEmbedder;

/// Platform embedder trait
///
/// Defines the interface for platform-specific implementations.
/// Currently we only have WgpuEmbedder, but this allows for future platforms.
pub trait PlatformEmbedder {
    /// Run the embedder's event loop
    ///
    /// This method blocks until the app exits.
    fn run(self) -> !;
}
