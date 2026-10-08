//! Windows utility functions and helpers
#![expect(dead_code)]

use flui_foundation::geometry::{Point, Size};
use windows::{
    Win32::Foundation::LPARAM,
    core::{PCWSTR, w},
};

use crate::traits::CursorError;

/// Windows platform window class name (shared across platform.rs and window.rs)
pub const WINDOW_CLASS_NAME: PCWSTR = w!("FluiWindowClass");
use windows::Win32::UI::WindowsAndMessaging::{HCURSOR, LoadCursorW};

/// Convert LPARAM to X coordinate
#[inline]
pub fn get_x_lparam(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as i16 as i32
}

/// Convert LPARAM to Y coordinate
#[inline]
pub fn get_y_lparam(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
}

/// Get high word from u32
#[inline]
pub fn hiword(value: u32) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

/// Get low word from u32
#[inline]
pub fn loword(value: u32) -> u16 {
    (value & 0xFFFF) as u16
}

/// Convert logical pixels to device pixels
#[inline]
pub fn logical_to_device(logical: f64, scale_factor: f64) -> i32 {
    (logical * scale_factor).round() as i32
}

/// Convert device pixels to logical pixels
#[inline]
pub fn device_to_logical(device: i32, scale_factor: f64) -> f64 {
    device as f64 / scale_factor
}

/// Create a Point in logical pixels from device coordinates
#[inline]
pub fn logical_point(x: f64, y: f64, scale_factor: f64) -> Point<f64> {
    Point::new(
        device_to_logical(x as i32, scale_factor),
        device_to_logical(y as i32, scale_factor),
    )
}

/// Create a Size in device pixels
#[inline]
pub fn device_size(width: i32, height: i32) -> Size<i32> {
    Size::new(width, height)
}

/// Convert UTF-16 wide string to String
pub fn from_wide(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

/// Convert String to UTF-16 wide string
pub fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Load a cursor by style
///
/// # Safety
///
/// `style` must be a value `LoadCursorW` accepts as its resource-name
/// argument: either one of the predefined `IDC_*` constants (an integer
/// resource ordinal packed into a pointer-sized value via
/// `MAKEINTRESOURCEW`, which must never be dereferenced as an actual
/// pointer) or a genuine NUL-terminated UTF-16 string pointer that stays
/// valid for the duration of this call. Every call site in this crate
/// passes a predefined `IDC_*` constant.
pub unsafe fn load_cursor_style(style: PCWSTR) -> Result<HCURSOR, CursorError> {
    // SAFETY: per the `# Safety` contract above; `None` for `hinstance` is
    // required and documented for loading a predefined `IDC_*` cursor.
    unsafe {
        LoadCursorW(None, style)
            .map_err(|e| CursorError::Backend(format!("Failed to load cursor: {e}")))
    }
}

/// DPI constants
pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;

/// WM_SIZE wParam values — single source of truth in the cfg-free (and
/// host-tested) `shared::visibility` rule module; re-exported here so the
/// backend keeps its historical import path.
/// (Only the three the backend consumes are re-exported; `SIZE_MAXSHOW`/
/// `SIZE_MAXHIDE` stay reachable at their defining path.)
pub use crate::shared::visibility::{SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED};
