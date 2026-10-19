//! Where a Win32 window is and returns to, read from and asked of the OS:
//! its placement, the work-area origin placements are measured from, the
//! outer size a content size needs at a DPI, and the spot an opening window
//! is created at.

use flui_foundation::geometry::{Bounds, Point, Size};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT},
};

use super::util::{USER_DEFAULT_SCREEN_DPI, logical_to_device};
use super::window_frame::to_screen;
use crate::traits::WindowOptions;

/// The window's placement: its show state and the rectangles it returns to.
pub(super) fn placement_of(hwnd: HWND) -> windows::core::Result<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: `placement` is a stack local with `length` set; a stale `hwnd`
    // only makes the call fail.
    unsafe { GetWindowPlacement(hwnd, &raw mut placement) }?;
    Ok(placement)
}

/// How far the work area of `hwnd`'s monitor starts from the monitor's own
/// origin, in device pixels: a taskbar on the top or left edge moves it.
/// `WINDOWPLACEMENT::rcNormalPosition` is measured from the work area, screen
/// coordinates from the monitor.
pub(super) fn workspace_origin(hwnd: HWND) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };

    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a stack local with `cbSize` set; a stale `hwnd` makes
    // the monitor lookup fall back to the nearest monitor and never faults.
    let found = unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &raw mut info).as_bool()
    };
    if found {
        (
            info.rcWork.left - info.rcMonitor.left,
            info.rcWork.top - info.rcMonitor.top,
        )
    } else {
        (0, 0)
    }
}

/// The outer rectangle `hwnd` returns to from a maximized or minimized state
/// (`rcNormalPosition`), in device pixels of the virtual screen. Unlike
/// `GetWindowRect` it is the normal rectangle whatever state the window is in
/// now.
pub(super) fn restore_rectangle(hwnd: HWND) -> Option<Bounds<i32>> {
    let placement = placement_of(hwnd).ok()?;
    let screen = to_screen(placement.rcNormalPosition, workspace_origin(hwnd));
    Some(Bounds {
        origin: Point::new(screen.left, screen.top),
        size: Size::new(screen.right - screen.left, screen.bottom - screen.top),
    })
}

/// Where Win32 is asked to put a window that opens, in physical pixels of the
/// virtual screen.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct OpenSpot {
    /// The scale the sizes were computed at: the target display's, else the
    /// system's.
    pub(super) scale: f64,
    /// The outer size the window is created with.
    pub(super) outer: (i32, i32),
    /// The outer top-left; `None` leaves the place to the system.
    pub(super) origin: Option<(i32, i32)>,
    /// The outer rectangle (left, top, right, bottom) a window that opens
    /// maximized or fullscreen returns to; `None` leaves it to the system.
    pub(super) normal: Option<(i32, i32, i32, i32)>,
}

/// Turn the options of an opening window into physical pixels.
///
/// `options.size` is the content size, so `outer_of` (the window frame at a
/// DPI) turns it into the outer size. `options.position` and
/// `options.normal_geometry` are relative to the origin of `target`'s usable
/// area in that display's logical pixels and convert through that display's own
/// scale; with no `target` they are global logical pixels at `system_scale`.
pub(super) fn open_spot(
    options: &WindowOptions,
    target: Option<&flui_platform_api::DisplayInfo>,
    system_scale: f64,
    outer_of: impl Fn(u32, (i32, i32)) -> (i32, i32),
) -> OpenSpot {
    let scale = target
        .map(|display| display.scale_factor)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(system_scale);
    let dpi = ((scale * f64::from(USER_DEFAULT_SCREEN_DPI)).round() as u32).max(1);
    let content = (
        logical_to_device(options.size.width, scale),
        logical_to_device(options.size.height, scale),
    );
    let outer = outer_of(dpi, content);
    let anchor = target.map_or((0, 0), |display| {
        (
            display.usable_bounds.origin.x,
            display.usable_bounds.origin.y,
        )
    });
    let place = |at: Point<f64>| {
        (
            anchor.0.saturating_add(logical_to_device(at.x, scale)),
            anchor.1.saturating_add(logical_to_device(at.y, scale)),
        )
    };
    let origin = options.position.map(place);
    let normal = options.normal_geometry.and_then(|geometry| {
        let (left, top) = geometry.position.map(place).or(origin)?;
        Some((
            left,
            top,
            left.saturating_add(logical_to_device(geometry.size.width, scale)),
            top.saturating_add(logical_to_device(geometry.size.height, scale)),
        ))
    });
    OpenSpot {
        scale,
        outer,
        origin,
        normal,
    }
}

/// The outer size, in physical pixels, of a window of `style` that has a
/// content area of `content` at `dpi`: the content plus its title bar and
/// borders (`AdjustWindowRectExForDpi`), measured for the style given and not
/// for the window's current rectangle, which a maximized or minimized window
/// does not keep.
pub(super) fn outer_size(
    style: u32,
    ex_style: u32,
    dpi: u32,
    content: (i32, i32),
) -> windows::core::Result<(i32, i32)> {
    use windows::Win32::UI::{
        HiDpi::AdjustWindowRectExForDpi,
        WindowsAndMessaging::{WINDOW_EX_STYLE, WINDOW_STYLE},
    };

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: content.0,
        bottom: content.1,
    };
    // SAFETY: `rect` is a stack local passed as the documented in/out
    // parameter; the style and DPI are plain values.
    unsafe {
        AdjustWindowRectExForDpi(
            &raw mut rect,
            WINDOW_STYLE(style),
            false,
            WINDOW_EX_STYLE(ex_style),
            dpi,
        )?;
    }
    Ok((rect.right - rect.left, rect.bottom - rect.top))
}
