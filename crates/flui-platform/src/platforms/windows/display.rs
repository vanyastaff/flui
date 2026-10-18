//! Windows display source.
//!
//! Samples the monitors with `EnumDisplayMonitors` and turns them into a
//! [`DisplaySnapshot`]. The refresh period is not read yet, so every display
//! reports an unknown period (a consumer falls back to 60 Hz, as it did before
//! the snapshot existed). The monitor key is the GDI device name, a label that
//! the `QueryDisplayConfig` source replaces.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayInfo, DisplayKey, DisplaySnapshot};
use windows::Win32::{
    Foundation::{LPARAM, RECT, TRUE},
    Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW},
    UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
};
// BOOL moved from Win32::Foundation to windows_core in windows 0.62
use windows::core::BOOL;

use crate::error::PlatformError;
use crate::shared::display_source::DisplaySource;

/// Enumerate the monitors now and publish the result through `source`.
///
/// # Errors
/// [`PlatformError::Displays`] when the snapshot revision space is used up.
pub(super) fn sample(source: &DisplaySource) -> Result<DisplaySnapshot, PlatformError> {
    let mut displays = Vec::new();
    for monitor in enumerate_monitors() {
        let Some(id) = source.mint(&DisplayKey::Backend(monitor.device_name.clone())) else {
            continue;
        };
        displays.push(
            DisplayInfo::new(
                id,
                monitor.device_name,
                monitor.bounds,
                monitor.scale_factor,
            )
            .with_usable_bounds(monitor.usable_bounds)
            .with_primary(monitor.is_primary),
        );
    }
    source.publish(displays)
}

/// One monitor as `EnumDisplayMonitors` and `GetMonitorInfoW` describe it.
struct Monitor {
    device_name: String,
    bounds: Bounds<i32>,
    usable_bounds: Bounds<i32>,
    scale_factor: f64,
    is_primary: bool,
}

impl Monitor {
    fn new(hmonitor: HMONITOR) -> Option<Self> {
        // SAFETY: `hmonitor` is a valid monitor handle the OS handed to
        // `EnumDisplayMonitors`'s callback. `MONITORINFOEXW` is zeroed with its
        // `cbSize` set before `GetMonitorInfoW`, which is that API's contract;
        // `GetDpiForMonitor` takes a valid HMONITOR and two valid out-pointers.
        unsafe {
            let mut monitor_info: MONITORINFOEXW = std::mem::zeroed();
            monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(hmonitor, (&raw mut monitor_info.monitorInfo).cast()).as_bool() {
                return None;
            }
            let rc = monitor_info.monitorInfo.rcMonitor;
            let rc_work = monitor_info.monitorInfo.rcWork;

            let mut dpi_x = 96u32;
            let mut dpi_y = 96u32;
            let _ = GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &raw mut dpi_x, &raw mut dpi_y);

            let device_name = String::from_utf16_lossy(
                &monitor_info.szDevice[..]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .copied()
                    .collect::<Vec<u16>>(),
            );
            Some(Self {
                device_name,
                bounds: Bounds {
                    origin: Point::new(rc.left, rc.top),
                    size: Size::new(rc.right - rc.left, rc.bottom - rc.top),
                },
                usable_bounds: Bounds {
                    origin: Point::new(rc_work.left, rc_work.top),
                    size: Size::new(rc_work.right - rc_work.left, rc_work.bottom - rc_work.top),
                },
                scale_factor: f64::from(dpi_x) / 96.0,
                // MONITORINFOF_PRIMARY = 1
                is_primary: (monitor_info.monitorInfo.dwFlags & 1) != 0,
            })
        }
    }
}

fn enumerate_monitors() -> Vec<Monitor> {
    // Callback for EnumDisplayMonitors.
    //
    // # Safety
    // The caller (`EnumDisplayMonitors`) guarantees `hmonitor` is a valid
    // monitor handle and `lparam` carries the pointer passed below: a `*mut
    // Vec<Monitor>` live and unaliased for the whole enumeration.
    unsafe extern "system" fn enum_proc(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        // SAFETY: `lparam.0` is the address of `monitors` below, cast to
        // `isize`. `EnumDisplayMonitors` blocks until every callback returns, so
        // the vector is live and no other reference to it exists concurrently.
        let monitors = unsafe {
            &mut *std::ptr::with_exposed_provenance_mut::<Vec<Monitor>>(lparam.0 as usize)
        };
        if let Some(monitor) = Monitor::new(hmonitor) {
            monitors.push(monitor);
        }
        TRUE
    }

    let mut monitors: Vec<Monitor> = Vec::new();
    // SAFETY: `enum_proc` runs on this thread before `EnumDisplayMonitors`
    // returns, so `monitors` is live and exclusively accessible during all
    // callbacks; the pointer is passed through `LPARAM` and recovered there.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM((&raw mut monitors).expose_provenance() as isize),
        );
    }
    monitors
}
