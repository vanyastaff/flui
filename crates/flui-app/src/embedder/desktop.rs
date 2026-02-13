//! Desktop embedder utilities
//!
//! Provides the `PlatformWindowHandle` adapter for GPU surface creation
//! and error types for embedder initialization.

use flui_platform::PlatformWindow;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

// ============================================================================
// PlatformWindowHandle adapter
// ============================================================================

/// Adapter bridging `dyn PlatformWindow` to `HasWindowHandle + HasDisplayHandle`.
///
/// `Renderer::new()` requires `W: HasWindowHandle + HasDisplayHandle`.
/// `PlatformWindow` has the methods but traits can't be implemented on `dyn`.
/// This zero-cost wrapper bridges the gap.
pub(crate) struct PlatformWindowHandle<'a>(pub(crate) &'a dyn PlatformWindow);

impl HasWindowHandle for PlatformWindowHandle<'_> {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for PlatformWindowHandle<'_> {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Embedder error types.
#[derive(Debug, thiserror::Error)]
pub enum EmbedderError {
    /// Failed to create window
    #[error("Failed to create window: {0}")]
    WindowCreation(String),

    /// Failed to initialize GPU
    #[error("Failed to initialize GPU: {0}")]
    GpuInitialization(String),
}
