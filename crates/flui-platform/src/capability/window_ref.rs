//! The window a provider sees: its id, its scale, its raw handles, whether it
//! is still open, and how a desktop portal names it.

use std::{fmt, rc::Rc, rc::Weak, sync::Arc};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use super::{PortalParent, Resolver, WindowLifetime};
use crate::{PlatformWindow, SurfaceTarget, WindowId};

/// The acquiring widget's window, as a provider sees it: its id, its scale,
/// its raw window and display handles, its [`WindowLifetime`], and the
/// [`PortalParent`] a desktop portal names it by. No other method of the
/// window is reachable, so no window method becomes surface that capability
/// crates depend on (ADR-0152 §2, item 6; ADR-0363).
///
/// The raw handles are what owner-attached dialogs need; they are reached
/// through [`HasWindowHandle`] and [`HasDisplayHandle`], the upstream traits
/// ADR-0089 §2 allows in a stable signature.
///
/// Owner-thread (`!Send`), like the provider context it comes from.
#[derive(Clone)]
pub struct WindowRef {
    id: WindowId,
    scale: f64,
    target: Arc<dyn SurfaceTarget>,
    lifetime: WindowLifetime,
    /// Asked for the portal parent; the reference does not keep the window
    /// alive.
    window: Weak<dyn PlatformWindow>,
}

impl WindowRef {
    /// `window` as its providers see it, open for as long as `lifetime` says:
    /// its id, its scale now (`1.0` before it has metrics) and its surface's
    /// handles.
    #[must_use]
    pub fn new(window: &Rc<dyn PlatformWindow>, lifetime: WindowLifetime) -> Self {
        let scale = window
            .state()
            .metrics
            .map_or(1.0, |metrics| metrics.scale().get());
        Self {
            id: window.id(),
            scale,
            target: window.surface_target(),
            lifetime,
            window: Rc::downgrade(window),
        }
    }

    /// The window's id.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Device pixels per logical pixel when the provider was asked.
    #[must_use]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Whether the window is still open, and the notice when it closes: what
    /// a provider that answers the window later ends its work by.
    #[must_use]
    pub fn lifetime(&self) -> &WindowLifetime {
        &self.lifetime
    }

    /// Ask how a desktop portal names this window, for one request: `answer`
    /// receives the [`PortalParent`] the request passes as `parent_window`
    /// and holds until it ends.
    ///
    /// A closed window, or one its host has let go, answers
    /// [`PortalParent::none`] at once: its id may name another window by
    /// then. An open window answers as its host does
    /// ([`PlatformWindow::portal_parent`]): an X11 window at once, a Wayland
    /// window once the compositor exported it. Owner thread only.
    pub fn portal_parent(&self, answer: Resolver<PortalParent>) {
        match self.window.upgrade() {
            Some(window) if !self.lifetime.is_closed() => window.portal_parent(answer),
            // The owner retiring first leaves nothing waiting for the answer.
            _ => {
                let _ = answer.resolve(PortalParent::none());
            }
        }
    }
}

impl HasWindowHandle for WindowRef {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.target.window_handle()
    }
}

impl HasDisplayHandle for WindowRef {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.target.display_handle()
    }
}

impl fmt::Debug for WindowRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRef")
            .field("id", &self.id)
            .field("scale", &self.scale)
            .field("closed", &self.lifetime.is_closed())
            .finish_non_exhaustive()
    }
}
