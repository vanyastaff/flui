//! The Wayland host under test: its loop's turns, run by hand on the thread
//! that made the platform where a test stands in for `Platform::run`, and what
//! the compositor said of a window, read apart from the window's snapshot.

use std::time::Duration;

use crate::error::PlatformError;
use crate::host_window::HostWindow;
use crate::hosts::WaylandPlatform;
use crate::window::linux::wayland::WaylandWindow;

/// Run the loop of `platform` for `span`: read what the compositor sends,
/// let each window carry it out, and deliver what is due, as
/// `Platform::run` does on each turn.
///
/// # Errors
/// What a turn of the loop reports: the connection was lost.
pub fn run_for(platform: &WaylandPlatform, span: Duration) -> Result<(), PlatformError> {
    platform.owner().turn_for(span)
}

/// Draw one blank frame on `window` at its metrics and commit it, as a
/// renderer presents one: a compositor shows a window, and holds it to the
/// state it acknowledged, only once it has a buffer. Call the window's
/// `SurfaceTarget::pre_present_notify` first, as the raster thread does.
/// `false` for a window of another host, or one with nothing to draw on.
#[must_use]
pub fn present(window: &dyn HostWindow) -> bool {
    window
        .as_any()
        .downcast_ref::<WaylandWindow>()
        .is_some_and(WaylandWindow::present_blank)
}

/// What the compositor said of a Wayland window in its configures, which the
/// window acknowledged on its last turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompositorReport {
    /// The size the last configure gave, in logical pixels; `None` where it
    /// left the size to the window.
    pub size: Option<(i32, i32)>,
    /// The compositor maximized the window.
    pub maximized: bool,
    /// The window covers its output.
    pub fullscreen: bool,
    /// The window has the compositor's activation.
    pub activated: bool,
    /// The compositor does not show the window.
    pub suspended: bool,
    /// The window acknowledged a configure.
    pub configured: bool,
    /// The buffer scale the compositor prefers for the window's surface.
    pub preferred_scale: Option<i32>,
    /// The global of the output the surface entered first.
    pub output: Option<u32>,
}

/// What the compositor said of `window`; `None` for a window of another
/// host.
#[must_use]
pub fn compositor_report(window: &dyn HostWindow) -> Option<CompositorReport> {
    let window = window.as_any().downcast_ref::<WaylandWindow>()?;
    let said = window.compositor_said();
    let (width, height) = said.size;
    Some(CompositorReport {
        size: (width > 0 && height > 0).then_some((width, height)),
        maximized: said.states.maximized,
        fullscreen: said.states.fullscreen,
        activated: said.states.activated,
        suspended: said.states.suspended,
        configured: said.configured,
        preferred_scale: said.preferred_scale,
        output: said.output,
    })
}
