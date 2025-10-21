//! Platform and environment types
//!
//! This module contains types for representing platform information,
//! similar to Flutter's TargetPlatform.

use std::fmt;
use std::ops::Not;
use std::str::FromStr;

/// The platform/OS being targeted.
///
/// Similar to Flutter's `TargetPlatform`.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::TargetPlatform;
///
/// let platform = TargetPlatform::current();
/// if platform.is_desktop() {
///     println!("Running on {}", platform);
/// }
///
/// // Parse from string
/// let android: TargetPlatform = "android".parse().unwrap();
/// assert_eq!(android, TargetPlatform::Android);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TargetPlatform {
    /// Android mobile OS
    Android,
    /// iOS mobile OS
    #[cfg_attr(feature = "serde", serde(rename = "ios"))]
    IOS,
    /// macOS desktop OS
    #[cfg_attr(feature = "serde", serde(rename = "macos"))]
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
    /// Get the current platform at runtime
    ///
    /// Detects the platform based on compile-time configuration.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::TargetPlatform;
    ///
    /// let platform = TargetPlatform::current();
    /// if platform.is_desktop() {
    ///     println!("Running on desktop");
    /// }
    /// ```
    #[must_use]
    pub fn current() -> Self {
        #[cfg(target_os = "android")]
        return Self::Android;

        #[cfg(target_os = "ios")]
        return Self::IOS;

        #[cfg(target_os = "macos")]
        return Self::MacOS;

        #[cfg(target_os = "windows")]
        return Self::Windows;

        #[cfg(target_os = "linux")]
        return Self::Linux;

        #[cfg(target_arch = "wasm32")]
        return Self::Web;

        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_arch = "wasm32"
        )))]
        Self::Unknown
    }

    /// Returns the platform name as a static string
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::TargetPlatform;
    ///
    /// assert_eq!(TargetPlatform::Android.as_str(), "android");
    /// assert_eq!(TargetPlatform::MacOS.as_str(), "macos");
    /// ```
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::IOS => "ios",
            Self::MacOS => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Web => "web",
            Self::Unknown => "unknown",
        }
    }

    /// Check if this is a mobile platform
    ///
    /// Returns `true` for Android and iOS.
    #[must_use]
    #[inline]
    pub const fn is_mobile(&self) -> bool {
        matches!(self, Self::Android | Self::IOS)
    }

    /// Check if this is a desktop platform
    ///
    /// Returns `true` for macOS, Windows, and Linux.
    #[must_use]
    #[inline]
    pub const fn is_desktop(&self) -> bool {
        matches!(self, Self::MacOS | Self::Windows | Self::Linux)
    }

    /// Check if this is web platform
    #[must_use]
    #[inline]
    pub const fn is_web(&self) -> bool {
        matches!(self, Self::Web)
    }

    /// Check if this is unknown platform
    #[must_use]
    #[inline]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Check if this platform typically uses touch input
    ///
    /// Returns `true` for mobile platforms and web.
    #[must_use]
    #[inline]
    pub const fn is_touch_primary(&self) -> bool {
        self.is_mobile() || self.is_web()
    }

    /// Check if this platform typically uses mouse/keyboard input
    ///
    /// Returns `true` for desktop platforms.
    #[must_use]
    #[inline]
    pub const fn is_pointer_primary(&self) -> bool {
        self.is_desktop()
    }
}

impl Default for TargetPlatform {
    #[inline]
    fn default() -> Self {
        Self::current()
    }
}

impl fmt::Display for TargetPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Android => "Android",
            Self::IOS => "iOS",
            Self::MacOS => "macOS",
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::Web => "Web",
            Self::Unknown => "Unknown",
        })
    }
}

impl AsRef<str> for TargetPlatform {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for TargetPlatform {
    type Err = ParsePlatformError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "android" => Ok(Self::Android),
            "ios" => Ok(Self::IOS),
            "macos" | "mac" | "darwin" => Ok(Self::MacOS),
            "windows" | "win" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "web" | "wasm" | "browser" => Ok(Self::Web),
            "unknown" => Ok(Self::Unknown),
            _ => Err(ParsePlatformError(s.to_string())),
        }
    }
}

/// Error type for parsing TargetPlatform from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsePlatformError(String);

impl fmt::Display for ParsePlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid platform: '{}'", self.0)
    }
}

impl std::error::Error for ParsePlatformError {}

/// Brightness mode (light/dark theme).
///
/// Similar to Flutter's `Brightness`.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::PlatformBrightness;
///
/// let light = PlatformBrightness::Light;
/// let dark = !light;  // Using Not operator
/// assert_eq!(dark, PlatformBrightness::Dark);
///
/// // From bool
/// let dark: PlatformBrightness = true.into();
/// assert!(dark.is_dark());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlatformBrightness {
    /// Light theme/mode
    #[default]
    Light,
    /// Dark theme/mode
    Dark,
}

impl PlatformBrightness {
    /// Get the opposite brightness
    ///
    /// Returns `Dark` if `Light`, and `Light` if `Dark`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::PlatformBrightness;
    ///
    /// assert_eq!(
    ///     PlatformBrightness::Light.opposite(),
    ///     PlatformBrightness::Dark
    /// );
    /// ```
    #[must_use]
    #[inline]
    pub const fn opposite(&self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    /// Check if this is light mode
    #[must_use]
    #[inline]
    pub const fn is_light(&self) -> bool {
        matches!(self, Self::Light)
    }

    /// Check if this is dark mode
    #[must_use]
    #[inline]
    pub const fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    /// Returns the brightness as a static string
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Converts to a boolean value
    ///
    /// Returns `true` for Dark, `false` for Light.
    #[must_use]
    #[inline]
    pub const fn as_bool(&self) -> bool {
        matches!(self, Self::Dark)
    }
}

impl fmt::Display for PlatformBrightness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        })
    }
}

impl Not for PlatformBrightness {
    type Output = Self;

    #[inline]
    fn not(self) -> Self::Output {
        self.opposite()
    }
}

impl From<bool> for PlatformBrightness {
    #[inline]
    fn from(is_dark: bool) -> Self {
        if is_dark {
            Self::Dark
        } else {
            Self::Light
        }
    }
}

impl From<PlatformBrightness> for bool {
    #[inline]
    fn from(brightness: PlatformBrightness) -> bool {
        brightness.as_bool()
    }
}

impl AsRef<str> for PlatformBrightness {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for PlatformBrightness {
    type Err = ParseBrightnessError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(ParseBrightnessError(s.to_string())),
        }
    }
}

/// Error type for parsing PlatformBrightness from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseBrightnessError(String);

impl fmt::Display for ParseBrightnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid brightness: '{}', expected 'light' or 'dark'", self.0)
    }
}

impl std::error::Error for ParseBrightnessError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_platform_current() {
        let current = TargetPlatform::current();
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
    fn test_target_platform_as_str() {
        assert_eq!(TargetPlatform::Android.as_str(), "android");
        assert_eq!(TargetPlatform::IOS.as_str(), "ios");
        assert_eq!(TargetPlatform::MacOS.as_str(), "macos");
        assert_eq!(TargetPlatform::Windows.as_str(), "windows");
        assert_eq!(TargetPlatform::Web.as_str(), "web");
    }

    #[test]
    fn test_target_platform_as_ref() {
        let platform = TargetPlatform::Android;
        let s: &str = platform.as_ref();
        assert_eq!(s, "android");
    }

    #[test]
    fn test_target_platform_from_str() {
        assert_eq!("android".parse::<TargetPlatform>().unwrap(), TargetPlatform::Android);
        assert_eq!("ANDROID".parse::<TargetPlatform>().unwrap(), TargetPlatform::Android);
        assert_eq!("ios".parse::<TargetPlatform>().unwrap(), TargetPlatform::IOS);
        assert_eq!("macos".parse::<TargetPlatform>().unwrap(), TargetPlatform::MacOS);
        assert_eq!("mac".parse::<TargetPlatform>().unwrap(), TargetPlatform::MacOS);
        assert_eq!("windows".parse::<TargetPlatform>().unwrap(), TargetPlatform::Windows);
        assert_eq!("win".parse::<TargetPlatform>().unwrap(), TargetPlatform::Windows);
        assert_eq!("linux".parse::<TargetPlatform>().unwrap(), TargetPlatform::Linux);
        assert_eq!("web".parse::<TargetPlatform>().unwrap(), TargetPlatform::Web);
        assert_eq!("wasm".parse::<TargetPlatform>().unwrap(), TargetPlatform::Web);

        assert!("invalid".parse::<TargetPlatform>().is_err());
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
    fn test_platform_brightness_not() {
        let light = PlatformBrightness::Light;
        let dark = !light;
        assert_eq!(dark, PlatformBrightness::Dark);

        let light_again = !dark;
        assert_eq!(light_again, PlatformBrightness::Light);
    }

    #[test]
    fn test_platform_brightness_from_bool() {
        let dark: PlatformBrightness = true.into();
        assert_eq!(dark, PlatformBrightness::Dark);

        let light: PlatformBrightness = false.into();
        assert_eq!(light, PlatformBrightness::Light);
    }

    #[test]
    fn test_platform_brightness_to_bool() {
        let dark = PlatformBrightness::Dark;
        let is_dark: bool = dark.into();
        assert!(is_dark);

        let light = PlatformBrightness::Light;
        let is_dark: bool = light.into();
        assert!(!is_dark);
    }

    #[test]
    fn test_platform_brightness_as_str() {
        assert_eq!(PlatformBrightness::Light.as_str(), "light");
        assert_eq!(PlatformBrightness::Dark.as_str(), "dark");
    }

    #[test]
    fn test_platform_brightness_from_str() {
        assert_eq!("light".parse::<PlatformBrightness>().unwrap(), PlatformBrightness::Light);
        assert_eq!("LIGHT".parse::<PlatformBrightness>().unwrap(), PlatformBrightness::Light);
        assert_eq!("dark".parse::<PlatformBrightness>().unwrap(), PlatformBrightness::Dark);
        assert_eq!("DARK".parse::<PlatformBrightness>().unwrap(), PlatformBrightness::Dark);

        assert!("invalid".parse::<PlatformBrightness>().is_err());
    }

    #[test]
    fn test_platform_brightness_ord() {
        assert!(PlatformBrightness::Light < PlatformBrightness::Dark);
        
        let mut vec = vec![
            PlatformBrightness::Dark,
            PlatformBrightness::Light,
            PlatformBrightness::Dark,
        ];
        vec.sort();
        assert_eq!(vec, vec![
            PlatformBrightness::Light,
            PlatformBrightness::Dark,
            PlatformBrightness::Dark,
        ]);
    }

    #[test]
    fn test_platform_display() {
        assert_eq!(format!("{}", TargetPlatform::Windows), "Windows");
        assert_eq!(format!("{}", PlatformBrightness::Dark), "Dark");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_target_platform_serde() {
        let platform = TargetPlatform::Android;
        let json = serde_json::to_string(&platform).unwrap();
        let deserialized: TargetPlatform = serde_json::from_str(&json).unwrap();
        assert_eq!(platform, deserialized);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_platform_brightness_serde() {
        let brightness = PlatformBrightness::Dark;
        let json = serde_json::to_string(&brightness).unwrap();
        let deserialized: PlatformBrightness = serde_json::from_str(&json).unwrap();
        assert_eq!(brightness, deserialized);
    }
}