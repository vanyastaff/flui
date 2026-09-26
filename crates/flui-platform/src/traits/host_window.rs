//! The host-side window: a [`PlatformWindow`] plus what only a composition
//! root may reach.
//!
//! [`PlatformWindow`] is the per-window contract the framework programs
//! against, and it names no AccessKit type (ADR-0082 §1). The accessibility
//! bridge speaks AccessKit, so it is reached through this subtrait instead:
//! [`Platform::open_window`](crate::traits::Platform::open_window),
//! [`WindowOpen::Ready`](crate::traits::WindowOpen::Ready) and
//! [`PendingWindow`](crate::traits::PendingWindow) hand the runner an
//! `Arc<dyn HostWindow>`, the runner reads [`HostWindow::accessibility`] once
//! and passes the window on as an `Arc<dyn PlatformWindow>` (an upcast).
//!
//! Every backend fixes its bridge when it builds the window, so reading it
//! once at open time sees the same bridge a later read would.

use std::sync::Arc;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use super::{PlatformAccessibility, PlatformWindow};

/// A window as a backend hands it to the composition root.
///
/// Implemented by every backend window next to its [`PlatformWindow`] impl.
/// A test double that is only ever handed to the framework needs just
/// [`PlatformWindow`]; one returned from `open_window` needs this too.
pub trait HostWindow: PlatformWindow {
    /// This window's accessibility bridge, if the backend exposes one.
    ///
    /// `None` for a backend with no accessibility integration — which is
    /// every backend until its per-OS adapter is wired, and permanently for
    /// one with no such platform API. A composition root that gets `None`
    /// never enables semantics assembly, so the cost is not paid either.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        None
    }
}

// Without these, `Arc<dyn HostWindow>` would not be a raw-handle target, and a
// renderer built straight from an `open_window` result (before the upcast)
// would not compile.
impl HasWindowHandle for dyn HostWindow + '_ {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        PlatformWindow::window_handle(self)
    }
}

impl HasDisplayHandle for dyn HostWindow + '_ {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        PlatformWindow::display_handle(self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

    use super::HostWindow;
    use crate::traits::{PlatformWindow, WindowOptions};

    #[test]
    fn headless_host_window_upcasts_and_keeps_its_accessibility() {
        let platform = crate::headless_platform();
        let host: Arc<dyn HostWindow> = platform
            .open_window(WindowOptions::default())
            .expect("the headless backend opens a window");

        assert!(
            host.accessibility().is_some(),
            "the headless backend exposes its recording accessibility bridge"
        );

        let id = host.id();
        let window: Arc<dyn PlatformWindow> = host;
        assert_eq!(window.id(), id, "the upcast is the same window");
    }

    #[test]
    fn arc_dyn_host_window_is_a_raw_handle_target() {
        fn assert_bounds<T: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static>() {}
        assert_bounds::<Arc<dyn HostWindow>>();
    }
}
