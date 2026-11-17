//! Issue #1043's proof program: before this fix, `Renderer::new(&window)`
//! type-checked for a `window: W` local, then `drop(window)` left the
//! surface built from raw handles pointing at a dead window. `Renderer::new`
//! now takes the window's `Arc<dyn SurfaceTarget>`, whose trait object is
//! `'static`, so a target that borrows a local window cannot be handed over
//! and the escape no longer compiles.

#![forbid(unsafe_code)]

use std::sync::Arc;

use flui_platform::{SurfaceTarget, SwapChainStatistics};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

struct LocalWindow;

impl HasWindowHandle for LocalWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for LocalWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for &'_ LocalWindow {
    fn pre_present_notify(&self, _frame: Option<flui_foundation::FrameStamp>, _statistics: SwapChainStatistics) {}
}

async fn borrow_then_drop() {
    let window = LocalWindow;
    let renderer = flui_engine::Renderer::new(Arc::new(&window)).await;
    drop(window);
    let _ = renderer;
}

fn main() {
    let _ = borrow_then_drop();
}
