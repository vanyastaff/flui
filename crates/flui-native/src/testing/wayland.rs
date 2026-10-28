//! The Wayland host under test: its loop's turns, run by hand on the thread
//! that made the platform where a test stands in for `Platform::run`, and what
//! the compositor said of a window, read apart from the window's snapshot.

use std::time::Duration;

use crate::error::PlatformError;
use crate::host::Host;
use crate::host_window::HostWindow;
use crate::owner::linux::wayland::WaylandLoop;
use crate::platform::HostPlatform;
use crate::window::linux::wayland::WaylandWindow;

/// Run the loop of `platform`, a `WaylandPlatform`, for `span`: read what the
/// compositor sends, let each window carry it out, and deliver what is due,
/// as `Platform::run` does on each turn.
///
/// # Errors
/// What a turn of the loop reports: the connection was lost.
pub fn run_for<H: Host<Loop = WaylandLoop>>(
    platform: &HostPlatform<H>,
    span: Duration,
) -> Result<(), PlatformError> {
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

/// What the compositor sent a Wayland window, as the protocol carried it:
/// nothing here went through the window's own decoding, so a test that reads
/// it checks the window's snapshot against the compositor, not against
/// itself.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompositorReport {
    /// The size the last toplevel configure gave, in logical pixels; `0` on
    /// an axis it left to the window.
    pub size: (i32, i32),
    /// The last toplevel configure's `xdg_toplevel.state` values, as numbered
    /// by xdg-shell.
    pub states: Vec<u32>,
    /// How many `xdg_surface.configure` events came.
    pub configures: u32,
    /// The buffer scale the compositor prefers for the window's surface.
    pub preferred_scale: Option<i32>,
    /// The globals of the outputs the surface entered and has not left.
    pub outputs: Vec<u32>,
}

/// The cursor the seats' input last showed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CursorReport {
    /// A `cursor-shape` value's number, or the theme cursor's name.
    pub name: String,
    /// The buffer scale a theme cursor was drawn at; `0` for a shape.
    pub scale: i32,
}

/// The cursor the seats of `platform`, a `WaylandPlatform`, last showed;
/// `None` before one was shown.
#[must_use]
pub fn cursor_report<H: Host<Loop = WaylandLoop>>(
    platform: &HostPlatform<H>,
) -> Option<CursorReport> {
    platform.owner().shown_cursor().map(|shown| CursorReport {
        name: shown.name,
        scale: shown.scale,
    })
}

/// What the compositor sent `window`; `None` for a window of another host.
#[must_use]
pub fn compositor_report(window: &dyn HostWindow) -> Option<CompositorReport> {
    let window = window.as_any().downcast_ref::<WaylandWindow>()?;
    let said = window.compositor_said();
    Some(CompositorReport {
        size: said.size,
        states: said
            .raw_states
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_ne_bytes)
            .collect(),
        configures: said.configures,
        preferred_scale: said.preferred_scale,
        outputs: said.outputs,
    })
}
