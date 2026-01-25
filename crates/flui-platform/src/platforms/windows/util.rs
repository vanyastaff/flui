//! Windows utility functions and helpers

use anyhow::{anyhow, Result};
use flui_types::geometry::{device_px, px, DevicePixels, Pixels, Point, Size};
use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::*;

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
pub fn logical_to_device(logical: f32, scale_factor: f32) -> i32 {
    (logical * scale_factor).round() as i32
}

/// Convert device pixels to logical pixels
#[inline]
pub fn device_to_logical(device: i32, scale_factor: f32) -> f32 {
    device as f32 / scale_factor
}

/// Create a Point in logical pixels from device coordinates
#[inline]
pub fn logical_point(x: f32, y: f32, scale_factor: f32) -> Point<Pixels> {
    Point::new(
        px(device_to_logical(x as i32, scale_factor)),
        px(device_to_logical(y as i32, scale_factor)),
    )
}

/// Create a Size in device pixels
#[inline]
pub fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
    Size::new(device_px(width), device_px(height))
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
pub unsafe fn load_cursor_style(style: PCWSTR) -> Result<HCURSOR> {
    LoadCursorW(None, style).map_err(|e| anyhow!("Failed to load cursor: {}", e))
}

/// Check if a key is pressed
#[inline]
pub unsafe fn is_key_pressed(vkey: i32) -> bool {
    (GetAsyncKeyState(vkey) as i32 & 0x8000) != 0
}

/// DPI constants
pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;

/// WM_SIZE wParam values
pub const SIZE_RESTORED: u32 = 0;
pub const SIZE_MINIMIZED: u32 = 1;
pub const SIZE_MAXIMIZED: u32 = 2;
pub const SIZE_MAXSHOW: u32 = 3;
pub const SIZE_MAXHIDE: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lparam_coordinates() {
        // X=100, Y=200 packed into LPARAM
        let lparam = LPARAM(((200 << 16) | 100) as isize);

        assert_eq!(get_x_lparam(lparam), 100);
        assert_eq!(get_y_lparam(lparam), 200);
    }

    #[test]
    fn test_dpi_conversion() {
        let scale_factor = 1.5; // 150% DPI

        let logical = 100.0;
        let device = logical_to_device(logical, scale_factor);
        assert_eq!(device, 150);

        let back_to_logical = device_to_logical(device, scale_factor);
        assert!((back_to_logical - logical).abs() < 0.01);
    }

    #[test]
    fn test_wide_string_conversion() {
        let original = "Hello, 世界! 🦀";
        let wide = to_wide(original);
        let back = from_wide(&wide);

        assert_eq!(original, back);
    }
}
