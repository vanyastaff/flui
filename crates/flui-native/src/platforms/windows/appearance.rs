//! The user's light/dark app theme and the window chrome that follows it.

use flui_platform::{Brightness, WindowAppearance};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

/// `DWMWA_USE_IMMERSIVE_DARK_MODE`, honoured from Windows 10 20H1.
const USE_IMMERSIVE_DARK_MODE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(20);

/// The app theme chosen in Settings › Personalization › Colors.
///
/// Windows exposes it only as the `AppsUseLightTheme` registry value; the
/// window's own DWM dark-mode flag is chrome this backend sets, not an
/// observation. A missing value (before Windows 10 1809, or a profile that
/// never chose) means light.
pub(super) fn system_appearance() -> WindowAppearance {
    let mut light: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated literals from `w!`.
    // `RRF_RT_REG_DWORD` admits only a 4-byte value, `size` states the 4-byte
    // capacity of the stack-local `light` the data pointer addresses, and
    // `light` is read only when the call succeeded.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut light).cast()),
            Some(&raw mut size),
        )
    };
    if status.is_ok() && light == 0 {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }
}

/// Draw `hwnd`'s title bar and borders dark exactly when `appearance` is.
pub(super) fn apply_chrome(hwnd: HWND, appearance: WindowAppearance) {
    let dark = i32::from(Brightness::from(appearance).is_dark());
    // SAFETY: the attribute takes a 4-byte BOOL; the pointer addresses the
    // stack-local `i32` for the call and the size states its width. A failure
    // (an older Windows, or a handle already destroyed) leaves the chrome as
    // it was, which is cosmetic.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            size_of::<i32>() as u32,
        )
    };
}
