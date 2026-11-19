//! Windows a provider's test acquires its handles in, on every host: a
//! [`WindowRef`] over a [`MockWindow`] whose surface is the target a test
//! gives it, open until its [`WindowCloser`] closes it.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{WindowCloser, WindowRef};
use flui_platform::testing::{MockWindow, NoNativeSurface};
use flui_platform::{PlatformWindow, SurfaceTarget};

/// A window with no native handle, open until its closer closes it.
pub(crate) fn window() -> (WindowRef, WindowCloser) {
    open(Arc::new(NoNativeSurface))
}

/// A window whose surface is `target`, open until its closer closes it.
pub(crate) fn open(target: Arc<dyn SurfaceTarget>) -> (WindowRef, WindowCloser) {
    let (closer, lifetime) = WindowCloser::new();
    let window: Rc<dyn PlatformWindow> = Rc::new(MockWindow::new().with_surface_target(target));
    (WindowRef::new(&window, lifetime), closer)
}
