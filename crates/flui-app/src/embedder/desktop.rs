//! Desktop embedder utilities
//!
//! Provides the `PlatformWindowHandle` adapter for GPU surface creation.

use flui_platform::PlatformWindow;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

// ============================================================================
// PlatformWindowHandle adapter
// ============================================================================

/// Adapter bridging `dyn PlatformWindow` to `HasWindowHandle +
/// HasDisplayHandle`.
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
