//! Issue #1043's proof program: before this fix, `Renderer::new(&window)`
//! type-checked for a `window: W` local, then `drop(window)` left the
//! surface built from raw handles pointing at a dead window. `Renderer::new`
//! now takes `impl WindowTarget` (`HasWindowHandle + HasDisplayHandle + Send
//! + Sync + 'static`) — an OWNED value, not a borrow — so the escape must no
//! longer compile.
//!
//! `borrow_then_drop`'s body is generic over `W` with no `'static` bound, so
//! `&window` (a reference to a function-local) can never satisfy
//! `WindowTarget`'s `'static` requirement regardless of what a caller
//! instantiates `W` with; the body fails to type-check on its own, the same
//! pattern `raster_backend_requires_send.rs` uses for `RasterBackend: Send`.

#![forbid(unsafe_code)]

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

async fn borrow_then_drop<W>(window: W)
where
    // `Send + Sync` are given explicitly so the ONLY bound `&window` fails
    // to satisfy is `'static` — the one this fixture exists to pin. Without
    // them the compiler reports a missing `Sync` bound on `W` first, which
    // is a real diagnostic but not this issue's diagnostic.
    W: HasWindowHandle + HasDisplayHandle + Send + Sync,
{
    let renderer = flui_engine::wgpu::Renderer::new(&window).await;
    drop(window);
    let _ = renderer;
}

fn main() {
    let _ = borrow_then_drop(LocalWindow);
}
