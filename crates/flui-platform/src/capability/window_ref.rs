//! The window a provider sees: its id, its scale, its raw handles and
//! whether it is still open.

use std::{fmt, sync::Arc};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use super::WindowLifetime;
use crate::{SurfaceTarget, WindowId};

/// The acquiring widget's window, as a provider sees it: its id, its scale,
/// its raw window and display handles, and its [`WindowLifetime`]. No other
/// method of the window is reachable, so no window method becomes surface
/// that capability crates depend on (ADR-0152 §2, item 6).
///
/// The raw handles are what portals and owner-attached dialogs need; they are
/// reached through [`HasWindowHandle`] and [`HasDisplayHandle`], the upstream
/// traits ADR-0089 §2 allows in a stable signature.
///
/// Owner-thread (`!Send`), like the provider context it comes from.
#[derive(Clone)]
pub struct WindowRef {
    id: WindowId,
    scale: f64,
    target: Arc<dyn SurfaceTarget>,
    lifetime: WindowLifetime,
}

impl WindowRef {
    /// The window `id` at `scale`, with the handles of `target`, open for as
    /// long as `lifetime` says.
    #[must_use]
    pub fn new(
        id: WindowId,
        scale: f64,
        target: Arc<dyn SurfaceTarget>,
        lifetime: WindowLifetime,
    ) -> Self {
        Self {
            id,
            scale,
            target,
            lifetime,
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
