//! Platform and environment types
//!
//! This module contains types for representing platform information,
//! similar to Flutter's TargetPlatform.

/// The platform/OS being targeted.
///
/// Similar to Flutter's `TargetPlatform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetPlatform {
    /// Android mobile OS
    Android,
    /// iOS mobile OS
    IOS,
    /// macOS desktop OS
    MacOS,
    /// Windows desktop OS
    Windows,
    /// Linux desktop OS
    Linux,
    /// Web browser
    Web,
    /// Unknown or unsupported platform
    Unknown,
}

impl TargetPlatform {
    /// Get the current platform at runtime.
    pub fn current() -> Self {
        #[cfg(target_os = "android")]
        return TargetPlatform::Android;

        #[cfg(target_os = "ios")]
        return TargetPlatform::IOS;

        #[cfg(target_os = "macos")]
        return TargetPlatform::MacOS;

        #[cfg(target_os = "windows")]
        return TargetPlatform::Windows;

        #[cfg(target_os = "linux")]
        return TargetPlatform::Linux;

        #[cfg(target_arch = "wasm32")]
        return TargetPlatform::Web;

        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_arch = "wasm32"
        )))]
        return TargetPlatform::Unknown;
    }

    /// Check if this is a mobile platform.
    pub fn is_mobile(&self) -> bool {
        matches!(self, TargetPlatform::Android | TargetPlatform::IOS)
    }

    /// Check if this is a desktop platform.
    pub fn is_desktop(&self) -> bool {
        matches!(
            self,
            TargetPlatform::MacOS | TargetPlatform::Windows | TargetPlatform::Linux
        )
    }

    /// Check if this is web platform.
    pub fn is_web(&self) -> bool {
        matches!(self, TargetPlatform::Web)
    }

    /// Check if this platform typically uses touch input.
    pub fn is_touch_primary(&self) -> bool {
        self.is_mobile() || self.is_web()
    }

    /// Check if this platform typically uses mouse/keyboard input.
    pub fn is_pointer_primary(&self) -> bool {
        self.is_desktop()
    }
}

impl Default for TargetPlatform {
    fn default() -> Self {
        Self::current()
    }
}

impl std::fmt::Display for TargetPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetPlatform::Android => write!(f, "Android"),
            TargetPlatform::IOS => write!(f, "iOS"),
            TargetPlatform::MacOS => write!(f, "macOS"),
            TargetPlatform::Windows => write!(f, "Windows"),
            TargetPlatform::Linux => write!(f, "Linux"),
            TargetPlatform::Web => write!(f, "Web"),
            TargetPlatform::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Brightness mode (light/dark theme).
///
/// Similar to Flutter's `Brightness`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PlatformBrightness {
    /// Light theme/mode
    #[default]
    Light,
    /// Dark theme/mode
    Dark,
}

impl PlatformBrightness {
    /// Get the opposite brightness.
    pub fn opposite(&self) -> Self {
        match self {
            PlatformBrightness::Light => PlatformBrightness::Dark,
            PlatformBrightness::Dark => PlatformBrightness::Light,
        }
    }

    /// Check if this is light mode.
    pub fn is_light(&self) -> bool {
        matches!(self, PlatformBrightness::Light)
    }

    /// Check if this is dark mode.
    pub fn is_dark(&self) -> bool {
        matches!(self, PlatformBrightness::Dark)
    }
}

impl std::fmt::Display for PlatformBrightness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformBrightness::Light => write!(f, "Light"),
            PlatformBrightness::Dark => write!(f, "Dark"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_platform_current() {
        let current = TargetPlatform::current();
        // Just ensure it doesn't panic and returns something
        assert_ne!(current, TargetPlatform::Unknown);
    }

    #[test]
    fn test_target_platform_mobile() {
        assert!(TargetPlatform::Android.is_mobile());
        assert!(TargetPlatform::IOS.is_mobile());
        assert!(!TargetPlatform::Windows.is_mobile());
        assert!(!TargetPlatform::Web.is_mobile());
    }

    #[test]
    fn test_target_platform_desktop() {
        assert!(TargetPlatform::Windows.is_desktop());
        assert!(TargetPlatform::MacOS.is_desktop());
        assert!(TargetPlatform::Linux.is_desktop());
        assert!(!TargetPlatform::Android.is_desktop());
        assert!(!TargetPlatform::Web.is_desktop());
    }

    #[test]
    fn test_target_platform_web() {
        assert!(TargetPlatform::Web.is_web());
        assert!(!TargetPlatform::Windows.is_web());
        assert!(!TargetPlatform::Android.is_web());
    }

    #[test]
    fn test_target_platform_input_primary() {
        assert!(TargetPlatform::Android.is_touch_primary());
        assert!(TargetPlatform::IOS.is_touch_primary());
        assert!(TargetPlatform::Web.is_touch_primary());

        assert!(TargetPlatform::Windows.is_pointer_primary());
        assert!(TargetPlatform::MacOS.is_pointer_primary());
        assert!(TargetPlatform::Linux.is_pointer_primary());
    }

    #[test]
    fn test_platform_brightness() {
        let light = PlatformBrightness::Light;
        assert!(light.is_light());
        assert!(!light.is_dark());
        assert_eq!(light.opposite(), PlatformBrightness::Dark);

        let dark = PlatformBrightness::Dark;
        assert!(dark.is_dark());
        assert!(!dark.is_light());
        assert_eq!(dark.opposite(), PlatformBrightness::Light);
    }

    #[test]
    fn test_platform_display() {
        assert_eq!(format!("{}", TargetPlatform::Windows), "Windows");
        assert_eq!(format!("{}", PlatformBrightness::Dark), "Dark");
    }
}
