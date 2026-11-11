//! ADR-0305 §2: a surface outlives any one call, so `SurfaceFactory::create`
//! takes a target the surface co-owns. A window borrowed from a local could
//! drop while its surface still presents, so the borrow does not type-check.

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
    fn pre_present_notify(&self, _statistics: SwapChainStatistics) {}
}

fn borrow_then_drop(factory: &flui_engine::SurfaceFactory) {
    let window = LocalWindow;
    let surface = factory.create(Arc::new(&window));
    drop(window);
    let _ = surface;
}

fn main() {}
