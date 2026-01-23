//! Platform capabilities
//!
//! Describes what each platform supports, allowing the framework
//! to adapt behavior accordingly.

/// Platform capabilities trait
///
/// Describes the features and limitations of each platform.
/// Used by the framework to adapt behavior.
///
/// NOTE: This trait is not `Clone` or `Copy` to remain dyn-safe.
/// Implementations should be cheap to construct.
pub trait PlatformCapabilities: Send + Sync {
    /// Platform name for logging/debugging
    fn platform_name(&self) -> &'static str;

    /// Does this platform have explicit lifecycle management?
    ///
    /// - Mobile: true (foreground/background)
    /// - Desktop: false (simple window open/close)
    /// - Web: true (visibility API)
    fn has_lifecycle_management(&self) -> bool;

    /// Does this platform support multiple windows?
    fn supports_multiple_windows(&self) -> bool;

    /// Should pointer move events be coalesced?
    ///
    /// Desktop has high-frequency mouse events that benefit from coalescing.
    /// Touch events are already lower frequency.
    fn should_coalesce_pointer_moves(&self) -> bool;

    /// Default target frame rate
    fn default_target_fps(&self) -> u32;

    /// Should rendering be suspended when in background?
    ///
    /// Mobile platforms should suspend to save battery.
    fn suspend_rendering_in_background(&self) -> bool;

    /// Does this platform support touch input?
    fn supports_touch(&self) -> bool;

    /// Does this platform support mouse input?
    fn supports_mouse(&self) -> bool;

    /// Does this platform support keyboard input?
    fn supports_keyboard(&self) -> bool;

    /// Does this platform support stylus/pen input?
    fn supports_stylus(&self) -> bool {
        false
    }
}

/// Desktop platform capabilities (Windows, macOS, Linux)
#[derive(Debug, Clone, Copy, Default)]
pub struct DesktopCapabilities;

impl PlatformCapabilities for DesktopCapabilities {
    fn platform_name(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        return "Windows";
        #[cfg(target_os = "macos")]
        return "macOS";
        #[cfg(target_os = "linux")]
        return "Linux";
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return "Desktop";
    }

    fn has_lifecycle_management(&self) -> bool {
        false
    }

    fn supports_multiple_windows(&self) -> bool {
        true
    }

    fn should_coalesce_pointer_moves(&self) -> bool {
        true // High-frequency mouse events
    }

    fn default_target_fps(&self) -> u32 {
        60
    }

    fn suspend_rendering_in_background(&self) -> bool {
        false // Desktop apps continue running
    }

    fn supports_touch(&self) -> bool {
        true // Many desktops have touchscreens
    }

    fn supports_mouse(&self) -> bool {
        true
    }

    fn supports_keyboard(&self) -> bool {
        true
    }

    fn supports_stylus(&self) -> bool {
        true // Surface, Wacom, etc.
    }
}

/// Mobile platform capabilities (Android, iOS)
#[derive(Debug, Clone, Copy, Default)]
pub struct MobileCapabilities {
    /// Whether this is iOS (vs Android)
    pub is_ios: bool,
}

impl MobileCapabilities {
    /// Create Android capabilities
    pub fn android() -> Self {
        Self { is_ios: false }
    }

    /// Create iOS capabilities
    pub fn ios() -> Self {
        Self { is_ios: true }
    }
}

impl PlatformCapabilities for MobileCapabilities {
    fn platform_name(&self) -> &'static str {
        if self.is_ios {
            "iOS"
        } else {
            "Android"
        }
    }

    fn has_lifecycle_management(&self) -> bool {
        true // Explicit foreground/background
    }

    fn supports_multiple_windows(&self) -> bool {
        false // Single window on mobile
    }

    fn should_coalesce_pointer_moves(&self) -> bool {
        false // Touch events already lower frequency
    }

    fn default_target_fps(&self) -> u32 {
        60 // Could be 120 for ProMotion/high-refresh
    }

    fn suspend_rendering_in_background(&self) -> bool {
        true // Save battery
    }

    fn supports_touch(&self) -> bool {
        true
    }

    fn supports_mouse(&self) -> bool {
        false // No mouse on mobile (usually)
    }

    fn supports_keyboard(&self) -> bool {
        true // Virtual keyboard
    }

    fn supports_stylus(&self) -> bool {
        true // Apple Pencil, S Pen
    }
}

/// Web platform capabilities
#[derive(Debug, Clone, Copy, Default)]
pub struct WebCapabilities;

impl PlatformCapabilities for WebCapabilities {
    fn platform_name(&self) -> &'static str {
        "Web"
    }

    fn has_lifecycle_management(&self) -> bool {
        true // Page visibility API
    }

    fn supports_multiple_windows(&self) -> bool {
        false // Single canvas context
    }

    fn should_coalesce_pointer_moves(&self) -> bool {
        true // Browser events can be high-frequency
    }

    fn default_target_fps(&self) -> u32 {
        60 // requestAnimationFrame
    }

    fn suspend_rendering_in_background(&self) -> bool {
        true // Browser pauses RAF anyway
    }

    fn supports_touch(&self) -> bool {
        true // Touch devices
    }

    fn supports_mouse(&self) -> bool {
        true // Desktop browsers
    }

    fn supports_keyboard(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_capabilities() {
        let caps = DesktopCapabilities;
        assert!(caps.supports_mouse());
        assert!(caps.supports_multiple_windows());
        assert!(!caps.suspend_rendering_in_background());
    }

    #[test]
    fn test_mobile_capabilities() {
        let android = MobileCapabilities::android();
        assert_eq!(android.platform_name(), "Android");
        assert!(android.suspend_rendering_in_background());
        assert!(!android.supports_multiple_windows());

        let ios = MobileCapabilities::ios();
        assert_eq!(ios.platform_name(), "iOS");
    }

    #[test]
    fn test_web_capabilities() {
        let caps = WebCapabilities;
        assert!(caps.has_lifecycle_management());
        assert!(caps.supports_touch());
        assert!(caps.supports_mouse());
    }
}
