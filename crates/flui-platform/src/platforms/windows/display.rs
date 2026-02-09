//! Windows display implementation

use crate::traits::{DisplayId, PlatformDisplay};
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};
use std::sync::Arc;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::*;

/// Windows display implementation
pub struct WindowsDisplay {
    id: DisplayId,
    name: String,
    bounds: Bounds<DevicePixels>,
    usable_bounds: Bounds<DevicePixels>,
    scale_factor: f64,
    is_primary: bool,
}

impl WindowsDisplay {
    /// Create a new WindowsDisplay from MONITORINFOEXW
    pub fn new(hmonitor: HMONITOR, is_primary: bool) -> Self {
        unsafe {
            let mut monitor_info: MONITORINFOEXW = std::mem::zeroed();
            monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

            let _ = GetMonitorInfoW(hmonitor, &mut monitor_info.monitorInfo as *mut _ as *mut _);

            let rc = monitor_info.monitorInfo.rcMonitor;
            let rc_work = monitor_info.monitorInfo.rcWork;

            // Get DPI for this monitor
            let mut dpi_x = 96u32;
            let mut dpi_y = 96u32;
            let _ = GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale_factor = dpi_x as f64 / 96.0;

            // Convert device name from wide string
            let device_name = String::from_utf16_lossy(
                &monitor_info.szDevice[..]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .copied()
                    .collect::<Vec<u16>>(),
            );

            let id = DisplayId(hmonitor.0 as u64);

            let bounds = Bounds {
                origin: Point::new(
                    flui_types::geometry::device_px(rc.left),
                    flui_types::geometry::device_px(rc.top),
                ),
                size: Size::new(
                    flui_types::geometry::device_px(rc.right - rc.left),
                    flui_types::geometry::device_px(rc.bottom - rc.top),
                ),
            };

            let usable_bounds = Bounds {
                origin: Point::new(
                    flui_types::geometry::device_px(rc_work.left),
                    flui_types::geometry::device_px(rc_work.top),
                ),
                size: Size::new(
                    flui_types::geometry::device_px(rc_work.right - rc_work.left),
                    flui_types::geometry::device_px(rc_work.bottom - rc_work.top),
                ),
            };

            Self {
                id,
                name: device_name,
                bounds,
                usable_bounds,
                scale_factor,
                is_primary,
            }
        }
    }
}

impl PlatformDisplay for WindowsDisplay {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.bounds
    }

    fn usable_bounds(&self) -> Bounds<DevicePixels> {
        self.usable_bounds
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn is_primary(&self) -> bool {
        self.is_primary
    }
}

/// Enumerate all displays
pub fn enumerate_displays() -> Vec<Arc<dyn PlatformDisplay>> {
    unsafe {
        let mut displays: Vec<Arc<dyn PlatformDisplay>> = Vec::new();

        // Callback for EnumDisplayMonitors
        unsafe extern "system" fn enum_proc(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            let displays = &mut *(lparam.0 as *mut Vec<Arc<dyn PlatformDisplay>>);

            let mut monitor_info: MONITORINFOEXW = std::mem::zeroed();
            monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

            if GetMonitorInfoW(hmonitor, &mut monitor_info.monitorInfo as *mut _ as *mut _)
                .as_bool()
            {
                // MONITORINFOF_PRIMARY = 1
                let is_primary = (monitor_info.monitorInfo.dwFlags & 1) != 0;

                let display = Arc::new(WindowsDisplay::new(hmonitor, is_primary));
                displays.push(display);
            }

            TRUE
        }

        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut displays as *mut _ as isize),
        );

        displays
    }
}
