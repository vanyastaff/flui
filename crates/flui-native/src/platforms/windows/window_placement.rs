//! Where a Win32 window is and returns to, read from and asked of the OS:
//! its placement, the work-area origin placements are measured from, the
//! outer size a content size needs at a DPI, and the spot an opening window
//! is created at.

use flui_foundation::geometry::{Bounds, Point, Size};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT},
};

use super::window_frame::to_screen;
use flui_platform::WindowOptions;

/// DPI constants
pub(super) const USER_DEFAULT_SCREEN_DPI: u32 = 96;

/// Convert logical pixels to device pixels
#[inline]
pub(super) fn logical_to_device(logical: f64, scale_factor: f64) -> i32 {
    (logical * scale_factor).round() as i32
}

/// The largest device coordinate or extent a window command may name. It
/// leaves room below `i32::MAX` for the frame `AdjustWindowRectExForDpi` adds
/// and for an origin plus a size.
pub(super) const DEVICE_COORDINATE_LIMIT: i32 = 0x3FFF_0000;

/// Convert logical pixels to device pixels, or `None` when the result is not
/// a finite device value within [`DEVICE_COORDINATE_LIMIT`]: geometry Win32
/// would otherwise receive saturated.
#[inline]
pub(super) fn checked_logical_to_device(logical: f64, scale_factor: f64) -> Option<i32> {
    let device = (logical * scale_factor).round();
    let limit = f64::from(DEVICE_COORDINATE_LIMIT);
    (device.is_finite() && (-limit..=limit).contains(&device)).then_some(device as i32)
}

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
    target: Option<&flui_platform::DisplayInfo>,
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
    rect.right
        .checked_sub(rect.left)
        .zip(rect.bottom.checked_sub(rect.top))
        .ok_or_else(|| windows::Win32::Foundation::E_INVALIDARG.into())
}

/// `extent` at a scale `ratio` times the one it was measured at, or `None`
/// when that is not a device value a window command may name.
fn rescaled(extent: i32, ratio: f64) -> Option<i32> {
    checked_logical_to_device(f64::from(extent), ratio)
}

impl super::platform::WindowContext {
    /// Ask for a frame of `hwnd` after a callback ran, unless that callback
    /// closed the window: the handle may by then name a window opened in its
    /// place, which this context's slot no longer is.
    pub(super) fn redraw_if_still_owned(&self, hwnd: HWND) {
        if self.still_owns(hwnd) {
            super::window::WindowsWindow::redraw_native(hwnd);
        }
    }

    /// Whether `hwnd` is still this context's window: a callback that closed
    /// it may have let a window opened in its place take the handle.
    fn still_owns(&self, hwnd: HWND) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GWLP_USERDATA, GetWindowLongPtrW};

        // SAFETY: a pointer-sized read of the window's userdata slot; nothing
        // is dereferenced.
        let slot = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
        slot == std::ptr::from_ref(self) as isize
    }

    /// `WM_DPICHANGED`: the window is now at `dpi`, and the system suggests
    /// the rectangle `suggested` for it.
    pub(super) fn change_dpi(&self, hwnd: HWND, dpi: u16, suggested: Option<RECT>) {
        use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

        let new_scale = f64::from(dpi) / f64::from(USER_DEFAULT_SCREEN_DPI);
        tracing::info!(dpi, new_scale, "DPI changed");
        let old_scale = self.scale_factor.replace(new_scale);
        // The display's scale changed, so its snapshot entry did.
        self.displays.invalidate();
        self.pointer_registry
            .borrow_mut()
            .set_click_settings(super::events::click_settings(new_scale));
        // The scale and the rectangle change together: a subscriber sees the
        // window once, at its new size.
        let hold = self.hold_publishing();
        self.update_window_state(|state| state.scale_factor = new_scale);
        self.dispatch_event(flui_platform::WindowEvent::ScaleFactorChanged {
            window_id: self.window_id,
            scale_factor: new_scale,
        });
        // The handler may have closed the window: its rectangle, resize and
        // frame are then not for whatever window has the handle now.
        if !self.still_owns(hwnd) {
            return;
        }
        self.apply_dpi_rectangle(hwnd, suggested, new_scale / old_scale);
        // So may the resize callback the rectangle's `WM_SIZE` ran.
        if !self.still_owns(hwnd) {
            return;
        }
        drop(hold);
        self.publish_window_state(hwnd);

        // The runtime takes a new scale from a resize. When the rectangle kept
        // the client size in device pixels, or the move failed, no `WM_SIZE`
        // brought it.
        let mut client = RECT::default();
        // SAFETY: `client` is a stack local out-parameter.
        let measured = unsafe { GetClientRect(hwnd, &raw mut client) }.is_ok();
        if measured && !self.mode.get().is_minimized() {
            let size = Size::new(
                (client.right - client.left).max(1),
                (client.bottom - client.top).max(1),
            );
            if self.last_resize.get() != Some((size, new_scale)) {
                self.update_window_state(|state| {
                    state.bounds.size = Size::new(
                        f64::from(size.width) / new_scale,
                        f64::from(size.height) / new_scale,
                    );
                });
                self.publish_window_state(hwnd);
                self.dispatch_resize(size);
                self.redraw_if_still_owned(hwnd);
            }
        }
        // The field's screen rectangles follow the new scale and rectangle.
        self.text_layout_moved();
        // After the state above is current: the hint wakes the owner, which
        // samples the snapshot and re-derives.
        self.callbacks.dispatch_display_changed();
    }

    /// Give the window the rectangle its new DPI asks for; `ratio` is the new
    /// scale over the old.
    ///
    /// A window that is not fullscreen takes the rectangle the system
    /// suggests. A fullscreen window covers its monitor again instead, which
    /// the suggestion, scaled from its old size, need not do; the rectangle
    /// it returns to is rescaled with it, as the system would have scaled it.
    pub(super) fn apply_dpi_rectangle(&self, hwnd: HWND, suggested: Option<RECT>, ratio: f64) {
        use windows::Win32::{
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
            },
            UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos},
        };

        let mode = self.mode.get();
        if let Some(bounds) = mode.fullscreen_bounds()
            && let (Some(width), Some(height)) = (
                rescaled(bounds.size.width, ratio),
                rescaled(bounds.size.height, ratio),
            )
        {
            self.mode.set(mode.with_fullscreen_bounds(Bounds {
                origin: bounds.origin,
                size: Size::new(width, height),
            }));
        }
        let target = if mode.is_fullscreen() {
            let mut monitor = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            // SAFETY: `monitor` is a stack local with `cbSize` set; the
            // monitor handle comes from the live window.
            let read = unsafe {
                GetMonitorInfoW(
                    MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                    &raw mut monitor,
                )
            };
            read.as_bool().then_some(monitor.rcMonitor)
        } else {
            suggested
        };
        let Some(rect) = target else {
            return;
        };
        let (Some(width), Some(height)) = (
            rect.right.checked_sub(rect.left),
            rect.bottom.checked_sub(rect.top),
        ) else {
            return;
        };
        // A fullscreen window covers its monitor whatever its limits say.
        self.limits_waived.set(mode.is_fullscreen());
        // SAFETY: plain integer and flag arguments on the live window, with
        // `None` as the documented no-op z-order handle.
        let moved = unsafe {
            SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        self.limits_waived.set(false);
        if let Err(error) = moved {
            tracing::warn!(?hwnd, ?error, "SetWindowPos (DPI change) failed");
        }
    }
}
