//! Windows-specific window extensions
//!
//! This module provides Windows-specific features that extend the core `Window` trait.
//! These features are only available on Windows and use Microsoft's Win32/WinRT APIs.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::Window;
//! use flui_platform::windows::WindowsWindowExt;
//!
//! // Use cross-platform API
//! window.set_title("My App");
//!
//! // Use Windows-specific extensions
//! window.set_backdrop(WindowsBackdrop::Mica);
//! window.enable_snap_layouts();
//! ```

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
        point: flui_types::geometry::Point<flui_types::geometry::DevicePixels>,
    ) -> flui_types::geometry::Point<flui_types::geometry::Pixels>;

    /// Convert point from logical coordinates to device (pixel) coordinates.
    fn convert_point_to_device(
        &self,
        point: flui_types::geometry::Point<flui_types::geometry::Pixels>,
    ) -> flui_types::geometry::Point<flui_types::geometry::DevicePixels>;
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
    /// Mica is a subtle, semi-transparent material that shows the desktop wallpaper.
    /// Best for app backgrounds.
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
    /// Maps to the Windows 11 DwmSetWindowAttribute DWMWA_SYSTEMBACKDROP_TYPE values.
    pub fn to_dwm_value(self) -> i32 {
        match self {
            WindowsBackdrop::None => 1,      // DWMSBT_NONE
            WindowsBackdrop::Mica => 2,      // DWMSBT_MAINWINDOW (Mica)
            WindowsBackdrop::MicaAlt => 4,   // DWMSBT_TABBEDWINDOW (Mica Alt)
            WindowsBackdrop::Acrylic => 3,   // DWMSBT_TRANSIENTWINDOW (Acrylic)
            WindowsBackdrop::Tabbed => 4,    // DWMSBT_TABBEDWINDOW
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
            WindowCornerPreference::Default => 0,       // DWMWCP_DEFAULT
            WindowCornerPreference::DoNotRound => 1,    // DWMWCP_DONOTROUND
            WindowCornerPreference::Round => 2,         // DWMWCP_ROUND
            WindowCornerPreference::RoundSmall => 3,    // DWMWCP_ROUNDSMALL
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
            TaskbarProgressState::NoProgress => 0x0,        // TBPF_NOPROGRESS
            TaskbarProgressState::Normal => 0x2,            // TBPF_NORMAL
            TaskbarProgressState::Indeterminate => 0x1,     // TBPF_INDETERMINATE
            TaskbarProgressState::Error => 0x4,             // TBPF_ERROR
            TaskbarProgressState::Paused => 0x8,            // TBPF_PAUSED
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
#[allow(dead_code)]
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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backdrop_dwm_values() {
        assert_eq!(WindowsBackdrop::None.to_dwm_value(), 1);
        assert_eq!(WindowsBackdrop::Mica.to_dwm_value(), 2);
        assert_eq!(WindowsBackdrop::Acrylic.to_dwm_value(), 3);
        assert_eq!(WindowsBackdrop::MicaAlt.to_dwm_value(), 4);
    }

    #[test]
    fn test_backdrop_version_requirements() {
        assert!(!WindowsBackdrop::None.requires_windows_11());
        assert!(!WindowsBackdrop::Acrylic.requires_windows_11());
        assert!(WindowsBackdrop::Mica.requires_windows_11());
        assert!(WindowsBackdrop::MicaAlt.requires_windows_11());
    }

    #[test]
    fn test_corner_preference_values() {
        assert_eq!(WindowCornerPreference::Default.to_dwm_value(), 0);
        assert_eq!(WindowCornerPreference::DoNotRound.to_dwm_value(), 1);
        assert_eq!(WindowCornerPreference::Round.to_dwm_value(), 2);
        assert_eq!(WindowCornerPreference::RoundSmall.to_dwm_value(), 3);
    }

    #[test]
    fn test_taskbar_progress_values() {
        assert_eq!(TaskbarProgressState::NoProgress.to_tbpf_value(), 0x0);
        assert_eq!(TaskbarProgressState::Normal.to_tbpf_value(), 0x2);
        assert_eq!(TaskbarProgressState::Indeterminate.to_tbpf_value(), 0x1);
        assert_eq!(TaskbarProgressState::Error.to_tbpf_value(), 0x4);
        assert_eq!(TaskbarProgressState::Paused.to_tbpf_value(), 0x8);
    }

    #[test]
    fn test_theme_dark_mode_conversion() {
        assert_eq!(WindowsTheme::Light.to_dark_mode_value(), Some(false));
        assert_eq!(WindowsTheme::Dark.to_dark_mode_value(), Some(true));
        assert_eq!(WindowsTheme::System.to_dark_mode_value(), None);
    }
}
