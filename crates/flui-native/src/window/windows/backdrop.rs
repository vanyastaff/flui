//! What the desktop window manager draws around and behind a Win32 window:
//! the system backdrop material, the corners, the title bar's colours and
//! theme, and the light or dark chrome that follows the user's app theme.
//!
//! [`WindowsWindowExt`] is the extension trait a Windows application reaches
//! these through; [`system_appearance`] and [`apply_chrome`] are what the
//! window procedure applies when the theme changes.
//!
//! ```rust,ignore
//! use flui_native::{WindowsBackdrop, WindowsWindowExt};
//!
//! window.set_backdrop(WindowsBackdrop::Mica);
//! window.enable_snap_layouts();
//! ```

use flui_foundation::geometry::Point;
use flui_platform::{Brightness, WindowAppearance};
use windows::Win32::Foundation::{FALSE, HWND, TRUE};
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::HRGN;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_MAXIMIZEBOX,
};
use windows::core::w;

use super::WindowsWindow;

// ============================================================================
// Windows Window Extension Trait
// ============================================================================

/// Windows-specific window extensions.
///
/// This trait provides access to Windows-specific features that are not part
/// of the cross-platform `Window` trait.
///
/// # Platform Availability
///
/// - **Mica/Acrylic Materials:** Windows 11 Build 22000+
/// - **Snap Layouts:** Windows 11 Build 22000+
/// - **Rounded Corners:** Windows 11 Build 22000+
/// - **DWM Composition:** Windows Vista+
/// - **Taskbar Integration:** Windows 7+
#[cfg(target_os = "windows")]
pub trait WindowsWindowExt {
    /// Set the window backdrop material.
    ///
    /// Backdrop materials provide modern translucent effects in Windows 11.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 11 Build 22000+ for Mica/Acrylic
    /// - Windows 10+ for basic transparency
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.set_backdrop(WindowsBackdrop::Mica);
    /// ```
    fn set_backdrop(&mut self, backdrop: WindowsBackdrop);

    /// Clear backdrop material and restore opaque window.
    fn clear_backdrop(&mut self);

    /// Get the current backdrop material.
    fn backdrop(&self) -> WindowsBackdrop;

    /// Enable Snap Layouts integration.
    ///
    /// Snap Layouts is Windows 11's enhanced window snapping system that shows
    /// layout options when hovering over the maximize button.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 11 Build 22000+
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.enable_snap_layouts();
    /// ```
    fn enable_snap_layouts(&mut self);

    /// Disable Snap Layouts integration.
    fn disable_snap_layouts(&mut self);

    /// Check if Snap Layouts is enabled.
    fn is_snap_layouts_enabled(&self) -> bool;

    /// Set window corner preference.
    ///
    /// Controls whether the window has rounded or square corners.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 11 Build 22000+
    fn set_corner_preference(&mut self, preference: WindowCornerPreference);

    /// Get the window corner preference.
    fn corner_preference(&self) -> WindowCornerPreference;

    /// Enable DWM blur behind the window.
    ///
    /// This creates a blur effect for transparent areas of the window.
    ///
    /// # Platform Requirements
    ///
    /// - Windows Vista+
    fn enable_blur_behind(&mut self, enable: bool);

    /// Set the window's taskbar progress state.
    ///
    /// Shows progress indication in the taskbar button.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 7+
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.set_taskbar_progress(TaskbarProgressState::Normal, 50); // 50%
    /// ```
    fn set_taskbar_progress(&mut self, state: TaskbarProgressState, progress: u32);

    /// Clear taskbar progress indication.
    fn clear_taskbar_progress(&mut self);

    /// Set whether the window should use immersive dark mode.
    ///
    /// This affects the title bar and window chrome appearance.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 10 Build 17763+ (October 2018 Update)
    fn set_dark_mode(&mut self, dark_mode: bool);

    /// Check if dark mode is enabled.
    fn is_dark_mode(&self) -> bool;

    /// Set the window's theme.
    ///
    /// Controls the appearance of window chrome and title bar.
    fn set_theme(&mut self, theme: WindowsTheme);

    /// Get the current window theme.
    fn theme(&self) -> WindowsTheme;

    /// Enable/disable window drop shadow.
    ///
    /// # Platform Requirements
    ///
    /// - Windows XP+
    fn set_has_shadow(&mut self, has_shadow: bool);

    /// Set custom title bar color.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 10 Build 17763+
    fn set_title_bar_color(&mut self, color: Option<(u8, u8, u8)>);

    /// Set custom caption/border color.
    ///
    /// # Platform Requirements
    ///
    /// - Windows 11 Build 22000+
    fn set_caption_color(&mut self, color: Option<(u8, u8, u8)>);

    /// Enable/disable window animations.
    fn set_animations_enabled(&mut self, enabled: bool);

    /// Get the window's DPI.
    fn dpi(&self) -> u32;

    /// Convert point from device (pixel) coordinates to logical coordinates.
    fn convert_point_from_device(
        &self,
        point: flui_foundation::geometry::Point<i32>,
    ) -> flui_foundation::geometry::Point<f64>;

    /// Convert point from logical coordinates to device (pixel) coordinates.
    fn convert_point_to_device(
        &self,
        point: flui_foundation::geometry::Point<f64>,
    ) -> flui_foundation::geometry::Point<i32>;
}

// ============================================================================
// Windows Backdrop Material
// ============================================================================

/// Windows backdrop material types.
///
/// Provides modern translucent effects for Windows 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsBackdrop {
    /// No backdrop effect (opaque window).
    None,

    /// Mica material - subtle translucent effect (Windows 11).
    ///
    /// Mica is a subtle, semi-transparent material that shows the desktop
    /// wallpaper. Best for app backgrounds.
    Mica,

    /// Mica Alt material - darker variant (Windows 11).
    ///
    /// Similar to Mica but with darker tinting for better contrast.
    MicaAlt,

    /// Acrylic material - blurred translucent effect (Windows 10/11).
    ///
    /// Acrylic provides a frosted-glass effect with blur and noise texture.
    /// Best for surfaces like sidebars and panels.
    Acrylic,

    /// Tabbed backdrop (Windows 11 22H2+).
    ///
    /// Optimized for tabbed interfaces.
    Tabbed,
}

impl WindowsBackdrop {
    /// Convert to DWM_SYSTEMBACKDROP_TYPE value.
    ///
    /// Maps to the Windows 11 DwmSetWindowAttribute DWMWA_SYSTEMBACKDROP_TYPE
    /// values.
    pub fn to_dwm_value(self) -> i32 {
        match self {
            WindowsBackdrop::None => 1, // DWMSBT_NONE
            WindowsBackdrop::Mica => 2, // DWMSBT_MAINWINDOW (Mica)
            // DWMSBT_TABBEDWINDOW backs both Mica Alt and Tabbed.
            WindowsBackdrop::MicaAlt | WindowsBackdrop::Tabbed => 4,
            WindowsBackdrop::Acrylic => 3, // DWMSBT_TRANSIENTWINDOW (Acrylic)
        }
    }

    /// Check if this backdrop requires Windows 11.
    pub fn requires_windows_11(self) -> bool {
        !matches!(self, WindowsBackdrop::None | WindowsBackdrop::Acrylic)
    }
}

// ============================================================================
// Window Corner Preference
// ============================================================================

/// Window corner rounding preference.
///
/// Controls the corner style for Windows 11 windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowCornerPreference {
    /// Default corner style (usually rounded on Windows 11).
    Default,

    /// Do not round corners.
    DoNotRound,

    /// Force rounded corners.
    Round,

    /// Small rounded corners.
    RoundSmall,
}

impl WindowCornerPreference {
    /// Convert to DWM_WINDOW_CORNER_PREFERENCE value.
    pub fn to_dwm_value(self) -> i32 {
        match self {
            WindowCornerPreference::Default => 0,    // DWMWCP_DEFAULT
            WindowCornerPreference::DoNotRound => 1, // DWMWCP_DONOTROUND
            WindowCornerPreference::Round => 2,      // DWMWCP_ROUND
            WindowCornerPreference::RoundSmall => 3, // DWMWCP_ROUNDSMALL
        }
    }
}

// ============================================================================
// Taskbar Progress State
// ============================================================================

/// Taskbar progress indicator state.
///
/// Controls the appearance of the progress bar in the taskbar button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskbarProgressState {
    /// No progress indicator.
    NoProgress,

    /// Normal progress (green on Windows 7-10, accent color on Windows 11).
    Normal,

    /// Indeterminate progress (animated, no specific percentage).
    Indeterminate,

    /// Error state (red).
    Error,

    /// Paused state (yellow).
    Paused,
}

impl TaskbarProgressState {
    /// Convert to TBPF_* flags.
    pub fn to_tbpf_value(self) -> u32 {
        match self {
            TaskbarProgressState::NoProgress => 0x0, // TBPF_NOPROGRESS
            TaskbarProgressState::Normal => 0x2,     // TBPF_NORMAL
            TaskbarProgressState::Indeterminate => 0x1, // TBPF_INDETERMINATE
            TaskbarProgressState::Error => 0x4,      // TBPF_ERROR
            TaskbarProgressState::Paused => 0x8,     // TBPF_PAUSED
        }
    }
}

// ============================================================================
// Windows Theme
// ============================================================================

/// Windows window theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsTheme {
    /// Light theme (default on most systems).
    Light,

    /// Dark theme.
    Dark,

    /// Follow system theme.
    System,
}

impl WindowsTheme {
    /// Convert to DWMWA_USE_IMMERSIVE_DARK_MODE value.
    pub fn to_dark_mode_value(self) -> Option<bool> {
        match self {
            WindowsTheme::Light => Some(false),
            WindowsTheme::Dark => Some(true),
            WindowsTheme::System => None, // Use system default
        }
    }
}

// ============================================================================
// DWM Attributes
// ============================================================================

/// DWM (Desktop Window Manager) attribute constants.
///
/// These correspond to DWMWINDOWATTRIBUTE values from dwmapi.h.
#[expect(dead_code)]
pub(crate) mod dwm_attributes {
    /// Use immersive dark mode.
    pub const DWMWA_USE_IMMERSIVE_DARK_MODE: i32 = 20;

    /// Window corner preference.
    pub const DWMWA_WINDOW_CORNER_PREFERENCE: i32 = 33;

    /// System backdrop type.
    pub const DWMWA_SYSTEMBACKDROP_TYPE: i32 = 38;

    /// Caption color.
    pub const DWMWA_CAPTION_COLOR: i32 = 35;

    /// Border color.
    pub const DWMWA_BORDER_COLOR: i32 = 34;

    /// Enable blur behind window.
    pub const DWMWA_ENABLE_BLUR_BEHIND: i32 = 10;
}

// ============================================================================
// The app theme and the chrome that follows it
// ============================================================================

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
impl WindowsWindow {
    /// Set DWM window attribute
    ///
    /// # Safety
    ///
    /// `value` must be a valid, initialized `T` whose size and layout match
    /// what `attribute` (a `DWMWINDOWATTRIBUTE` ordinal) expects DWM to read
    /// — the byte count passed to `DwmSetWindowAttribute` is derived from
    /// `size_of::<T>()`, so calling this with the wrong `T` for a given
    /// `attribute` has DWM read past `value`'s bytes.
    unsafe fn set_dwm_attribute<T>(&self, attribute: i32, value: &T) -> windows::core::Result<()> {
        // SAFETY: per the `# Safety` contract above, the caller guarantees
        // `T` matches `attribute`'s expected layout; `value` is a live `&T`
        // for the duration of this call, and `size_of::<T>()` is the
        // correct byte count for the pointer `std::ptr::from_ref` produces
        // from it.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

            DwmSetWindowAttribute(
                self.hwnd(),
                DWMWINDOWATTRIBUTE(attribute),
                std::ptr::from_ref::<T>(value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<T>() as u32,
            )
        }
    }

    /// Get DWM window attribute
    ///
    /// # Safety
    ///
    /// `T` must match the layout DWM writes for `attribute` (a
    /// `DWMWINDOWATTRIBUTE` ordinal) — `size_of::<T>()` is the byte count
    /// passed to `DwmGetWindowAttribute`, so the wrong `T` for a given
    /// `attribute` has DWM write past `value`'s bytes.
    unsafe fn get_dwm_attribute<T: Default>(&self, attribute: i32) -> windows::core::Result<T> {
        // SAFETY: per the `# Safety` contract above, `T::default()` seeds a
        // valid, fully-initialized `value` before `&raw mut value` is handed
        // to DWM, so even if the call fails without writing anything, `value`
        // is never read uninitialized; the caller guarantees `T` matches
        // `attribute`'s expected layout and `size_of::<T>()`.
        unsafe {
            use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};

            let mut value = T::default();
            DwmGetWindowAttribute(
                self.hwnd(),
                DWMWINDOWATTRIBUTE(attribute),
                (&raw mut value).cast::<std::ffi::c_void>(),
                std::mem::size_of::<T>() as u32,
            )?;
            Ok(value)
        }
    }
}

// ============================================================================
// Windows Window Extension Trait Implementation
// ============================================================================

impl WindowsWindowExt for WindowsWindow {
    fn set_backdrop(&mut self, backdrop: WindowsBackdrop) {
        // SAFETY: `backdrop_value` is `i32`, matching `DWMWA_SYSTEMBACKDROP_TYPE`'s
        // expected layout, satisfying `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let backdrop_value = backdrop.to_dwm_value();
            if let Err(e) =
                self.set_dwm_attribute(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE, &backdrop_value)
            {
                tracing::warn!("Failed to set backdrop material: {:?}", e);
            } else {
                tracing::debug!("Set window backdrop to {:?}", backdrop);
            }
        }
    }

    fn clear_backdrop(&mut self) {
        self.set_backdrop(WindowsBackdrop::None);
    }

    fn backdrop(&self) -> WindowsBackdrop {
        // SAFETY: `i32` matches `DWMWA_SYSTEMBACKDROP_TYPE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_SYSTEMBACKDROP_TYPE) {
                // Ok(1) is DWMSBT_NONE — covered by the fallback arm.
                Ok(2) => WindowsBackdrop::Mica,
                Ok(3) => WindowsBackdrop::Acrylic,
                Ok(4) => WindowsBackdrop::MicaAlt,
                _ => WindowsBackdrop::None,
            }
        }
    }

    fn enable_snap_layouts(&mut self) {
        // Snap Layouts are automatically enabled on Windows 11 if the window has
        // a standard maximize button. No explicit API call needed.
        // We just need to ensure WS_MAXIMIZEBOX is set
        //
        // SAFETY: see `PlatformWindow::is_resizable`/`set_resizable` above —
        // same `GetWindowLongPtrW`/`SetWindowLongPtrW`/`SetWindowPos` shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd(), GWL_STYLE) as u32;
            style |= WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd(), GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd(),
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "SetWindowPos (enable snap layouts style refresh) failed");
            }

            tracing::debug!("Snap Layouts enabled (via WS_MAXIMIZEBOX)");
        }
    }

    fn disable_snap_layouts(&mut self) {
        // Disable by removing WS_MAXIMIZEBOX
        //
        // SAFETY: see `enable_snap_layouts` above — same call shape.
        unsafe {
            let mut style = GetWindowLongPtrW(self.hwnd(), GWL_STYLE) as u32;
            style &= !WS_MAXIMIZEBOX.0;
            SetWindowLongPtrW(self.hwnd(), GWL_STYLE, style as isize);
            if let Err(error) = SetWindowPos(
                self.hwnd(),
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                tracing::warn!(hwnd = ?self.hwnd(), ?error, "SetWindowPos (disable snap layouts style refresh) failed");
            }

            tracing::debug!("Snap Layouts disabled");
        }
    }

    fn is_snap_layouts_enabled(&self) -> bool {
        // SAFETY: see `PlatformWindow::is_resizable` above — same call shape.
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd(), GWL_STYLE) as u32;
            (style & WS_MAXIMIZEBOX.0) != 0
        }
    }

    fn set_corner_preference(&mut self, preference: WindowCornerPreference) {
        // SAFETY: `corner_value` is `i32`, matching
        // `DWMWA_WINDOW_CORNER_PREFERENCE`'s expected layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let corner_value = preference.to_dwm_value();
            if let Err(e) = self.set_dwm_attribute(
                dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_value,
            ) {
                tracing::warn!("Failed to set corner preference: {:?}", e);
            } else {
                tracing::debug!("Set corner preference to {:?}", preference);
            }
        }
    }

    fn corner_preference(&self) -> WindowCornerPreference {
        // SAFETY: `i32` matches `DWMWA_WINDOW_CORNER_PREFERENCE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            match self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_WINDOW_CORNER_PREFERENCE) {
                // Ok(0) is DWMCP_DEFAULT — covered by the fallback arm.
                Ok(1) => WindowCornerPreference::DoNotRound,
                Ok(2) => WindowCornerPreference::Round,
                Ok(3) => WindowCornerPreference::RoundSmall,
                _ => WindowCornerPreference::Default,
            }
        }
    }

    fn enable_blur_behind(&mut self, enable: bool) {
        use windows::Win32::Graphics::Dwm::{
            DWM_BB_ENABLE, DWM_BLURBEHIND, DwmEnableBlurBehindWindow,
        };

        // SAFETY: `bb` is a stack-local `DWM_BLURBEHIND`, fully initialized
        // above; `&raw const bb` gives `DwmEnableBlurBehindWindow` a valid
        // pointer to it — the API takes a typed struct pointer, not a
        // `(pointer, size)` pair, so there is no separate size argument to
        // get wrong.
        unsafe {
            let bb = DWM_BLURBEHIND {
                dwFlags: DWM_BB_ENABLE,
                fEnable: if enable { TRUE } else { FALSE },
                hRgnBlur: HRGN::default(),
                fTransitionOnMaximized: FALSE,
            };

            if let Err(e) = DwmEnableBlurBehindWindow(self.hwnd(), &raw const bb) {
                tracing::warn!("Failed to enable blur behind: {:?}", e);
            } else {
                tracing::debug!("Blur behind: {}", enable);
            }
        }
    }

    fn set_taskbar_progress(&mut self, state: TaskbarProgressState, progress: u32) {
        // This requires ITaskbarList3 COM interface
        // For now, just log - full implementation would need COM integration
        tracing::debug!("Set taskbar progress: {:?} {}%", state, progress);

        // TODO: Implement ITaskbarList3::SetProgressState and SetProgressValue
        // This requires:
        // 1. CoCreateInstance for ITaskbarList3
        // 2. Call SetProgressState(hwnd, state)
        // 3. Call SetProgressValue(hwnd, progress, 100)
    }

    fn clear_taskbar_progress(&mut self) {
        self.set_taskbar_progress(TaskbarProgressState::NoProgress, 0);
    }

    fn set_dark_mode(&mut self, dark_mode: bool) {
        // An explicit choice outlives later system theme changes.
        super::proc::with_window_context(self.hwnd(), "set_dark_mode", |ctx| {
            ctx.chrome_follows_system.set(false);
        });
        // SAFETY: `dark_mode_value` is `i32`, matching
        // `DWMWA_USE_IMMERSIVE_DARK_MODE`'s expected layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            let dark_mode_value: i32 = i32::from(dark_mode);
            if let Err(e) = self.set_dwm_attribute(
                dwm_attributes::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode_value,
            ) {
                tracing::warn!("Failed to set dark mode: {:?}", e);
            } else {
                tracing::debug!("Set dark mode: {}", dark_mode);
            }
        }
    }

    fn is_dark_mode(&self) -> bool {
        // SAFETY: `i32` matches `DWMWA_USE_IMMERSIVE_DARK_MODE`'s expected
        // layout, satisfying `get_dwm_attribute`'s `# Safety` contract.
        unsafe {
            self.get_dwm_attribute::<i32>(dwm_attributes::DWMWA_USE_IMMERSIVE_DARK_MODE)
                .unwrap_or(0)
                != 0
        }
    }

    fn set_theme(&mut self, theme: WindowsTheme) {
        if let Some(dark_mode) = theme.to_dark_mode_value() {
            self.set_dark_mode(dark_mode);
        } else {
            // Back to the system theme, now and on each later change.
            super::proc::with_window_context(self.hwnd(), "set_theme", |ctx| {
                ctx.chrome_follows_system.set(true);
            });
            apply_chrome(self.hwnd(), system_appearance());
        }
    }

    fn theme(&self) -> WindowsTheme {
        if self.is_dark_mode() {
            WindowsTheme::Dark
        } else {
            WindowsTheme::Light
        }
    }

    fn set_has_shadow(&mut self, has_shadow: bool) {
        // Windows doesn't have a direct API to disable shadows
        // Shadows are controlled by DWM composition
        // We can try extended window styles, but this is limited
        tracing::debug!("set_has_shadow: {} (limited support)", has_shadow);
    }

    fn set_title_bar_color(&mut self, color: Option<(u8, u8, u8)>) {
        // SAFETY: both `colorref` and `default_color` are `u32`, matching
        // `DWMWA_CAPTION_COLOR`'s expected `COLORREF` layout, satisfying
        // `set_dwm_attribute`'s `# Safety` contract.
        unsafe {
            if let Some((r, g, b)) = color {
                // Windows expects COLORREF format: 0x00BBGGRR
                let colorref: u32 = ((b as u32) << 16) | ((g as u32) << 8) | (r as u32);

                if let Err(e) =
                    self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &colorref)
                {
                    tracing::warn!("Failed to set title bar color: {:?}", e);
                } else {
                    tracing::debug!("Set title bar color: RGB({}, {}, {})", r, g, b);
                }
            } else {
                // Reset to default (0xFFFFFFFF means use default)
                let default_color: u32 = 0xFFFF_FFFF;
                if let Err(error) =
                    self.set_dwm_attribute(dwm_attributes::DWMWA_CAPTION_COLOR, &default_color)
                {
                    tracing::warn!(
                        hwnd = ?self.hwnd(),
                        ?error,
                        "failed to reset title bar color to default"
                    );
                }
            }
        }
    }

    fn set_caption_color(&mut self, color: Option<(u8, u8, u8)>) {
        // Caption color is the same as title bar color in Windows 11
        self.set_title_bar_color(color);
    }

    fn set_animations_enabled(&mut self, enabled: bool) {
        // Windows animations are typically controlled system-wide
        // Per-window animation control is limited
        tracing::debug!("set_animations_enabled: {} (system-wide setting)", enabled);
    }

    fn dpi(&self) -> u32 {
        // SAFETY: `GetDpiForWindow` takes `self.hwnd()` by value, no pointer
        // arguments — a stale or invalid `hwnd` cannot cause UB here, it is
        // just an ordinary FFI call either way. NOT a "safe fallback",
        // though: per its documented contract, an invalid `hwnd` makes this
        // return a literal `0`, not some usable default DPI — a caller that
        // divides by this result (e.g. computing a scale factor) would get
        // infinity or NaN, not graceful degradation. No current caller in
        // this crate divides by `dpi()`'s result, but a future one should
        // not assume `0` means "use 96 instead".
        unsafe { GetDpiForWindow(self.hwnd()) }
    }

    fn convert_point_from_device(&self, point: Point<i32>) -> Point<f64> {
        let scale = self.scale_factor();
        Point::new(point.x as f64 / scale, point.y as f64 / scale)
    }

    fn convert_point_to_device(&self, point: Point<f64>) -> Point<i32> {
        let scale = self.scale_factor();
        Point::new(
            (point.x * scale).round() as i32,
            (point.y * scale).round() as i32,
        )
    }
}
