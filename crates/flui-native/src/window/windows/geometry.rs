//! Where a Win32 window is (ADR-0363): the display a sample of it is measured
//! on, and the geometry its sampled rectangles publish, converted through
//! each display's own origin and scale ([`DisplayFrame`]). Pure apart from
//! the monitor reads it is handed, so `geometry_tests` checks it on desktops
//! this machine does not have.

#![forbid(unsafe_code)]

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use flui_platform::{WindowExtent, WindowGeometry};
use windows::Win32::Foundation::{HWND, RECT};

use crate::owner::windows::HostDisplays;
use crate::window::desktop::{DisplayFrame, display_of_rect};
use crate::window::state::SampledGeometry;

/// The display `hwnd` is on and the displays its position is measured on.
///
/// A window on a monitor the published snapshot lacks (one plugged in since
/// it was taken) has the monitors read again first: its sample would
/// otherwise publish no position while its host observes positions, and
/// nothing samples the window again when the monitors are read later. The
/// miss invalidates the snapshot only once per revision
/// ([`HostDisplays::display_of_window`]), so a monitor no sample can name (one
/// with no display target) costs one read, not one per sample.
pub(super) fn window_display(
    displays: &dyn HostDisplays,
    hwnd: HWND,
) -> (
    Option<flui_platform::DisplayId>,
    flui_platform::DisplaySnapshot,
) {
    if let Some(display) = displays.display_of_window(hwnd) {
        return (Some(display), displays.published());
    }
    if displays.nearest_monitor(hwnd).is_some()
        && let Ok(desktop) = displays.snapshot()
    {
        return (displays.display_of_window(hwnd), desktop);
    }
    (None, displays.published())
}

/// What one read of a Win32 window found of its rectangles, in the desktop's
/// device pixels.
#[derive(Debug, Clone, Copy)]
pub(super) struct DeviceRects {
    /// `GetWindowRect`; `None` when the read failed or the window is
    /// minimized, which Win32 reports at a parking place.
    pub(super) window: Option<RECT>,
    /// `GetClientRect`; `None` when the read failed.
    pub(super) client: Option<RECT>,
    /// The rectangle the window returns to from a mode other than normal.
    pub(super) restore: Option<Bounds<i32>>,
    /// The content size last known, in logical pixels: what a minimized
    /// window, or one whose client area could not be read, reports.
    pub(super) cached: Size<f64>,
    /// Whether the window is minimized.
    pub(super) minimized: bool,
    /// The window's own device pixels per logical pixel.
    pub(super) scale: DevicePixelRatio,
}

/// The geometry a sample of a window publishes: the outer position measured
/// on `display`, the display the window is on, through that display's origin
/// and scale; the outer and content extents over the window's own scale; and
/// the rectangle it returns to on the display that holds most of it, measured
/// and sized at that display's scale, which is the scale the window takes when
/// it returns there.
///
/// A window on a display `desktop` lacks has no position. Every difference of
/// two edges is taken in `f64`, where it is exact, so an inverted or
/// overflowing rectangle is refused as a negative extent, never saturated.
pub(super) fn sampled_geometry(
    rects: DeviceRects,
    display: Option<flui_platform::DisplayId>,
    desktop: &flui_platform::DisplaySnapshot,
) -> Result<SampledGeometry, flui_platform::InvalidGeometry> {
    let scale = rects.scale;
    let extent = |left: i32, top: i32, right: i32, bottom: i32, scale: DevicePixelRatio| {
        Size::new(
            scale.to_logical(f64::from(right) - f64::from(left)),
            scale.to_logical(f64::from(bottom) - f64::from(top)),
        )
    };
    let content = match (rects.minimized, rects.client) {
        (false, Some(client)) => {
            extent(client.left, client.top, client.right, client.bottom, scale)
        }
        _ => rects.cached,
    };
    let content = WindowExtent::try_new(content)?;
    let outer = match rects.window {
        Some(window) => WindowGeometry::try_new(
            display
                .and_then(|id| DisplayFrame::find(desktop, id))
                .and_then(|frame| frame.position(Point::new(window.left, window.top))),
            extent(window.left, window.top, window.right, window.bottom, scale),
        )?,
        None => WindowGeometry::unplaced(content),
    };
    let normal = rects
        .restore
        .map(|bounds| {
            let frame = display_of_rect(desktop, bounds);
            let at = frame.map_or(scale, DisplayFrame::scale);
            WindowGeometry::try_new(
                frame.and_then(|frame| frame.position(bounds.origin)),
                Size::new(
                    at.to_logical(f64::from(bounds.size.width)),
                    at.to_logical(f64::from(bounds.size.height)),
                ),
            )
        })
        .transpose()?;
    Ok(SampledGeometry {
        outer,
        content,
        normal,
    })
}
