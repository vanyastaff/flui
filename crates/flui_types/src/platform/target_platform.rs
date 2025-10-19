//! Target platform enum

/// The platform that the app is running on
///
/// Similar to Flutter's `TargetPlatform`. Used to determine platform-specific
/// behavior and styling.
///
/// # Examples
///
/// ```
/// use flui_types::platform::TargetPlatform;
///
/// let platform = TargetPlatform::Android;
/// assert!(platform.is_mobile());
/// assert!(!platform.is_desktop());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TargetPlatform {
    /// Android mobile platform
    Android,

    /// iOS mobile platform
    #[allow(non_camel_case_types)]
    iOS,

    /// macOS desktop platform
    MacOS,

    /// Linux desktop platform
    Linux,

    /// Windows desktop platform
    Windows,

    /// Fuchsia platform
    Fuchsia,

    /// Web platform
    Web,
}

impl TargetPlatform {
    /// Returns true if this is a mobile platform
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::TargetPlatform;
    ///
    /// assert!(TargetPlatform::Android.is_mobile());
    /// assert!(TargetPlatform::iOS.is_mobile());
    /// assert!(!TargetPlatform::Windows.is_mobile());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_mobile(&self) -> bool {
        matches!(self, Self::Android | Self::iOS | Self::Fuchsia)
    }

    /// Returns true if this is a desktop platform
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::TargetPlatform;
    ///
    /// assert!(TargetPlatform::Windows.is_desktop());
    /// assert!(TargetPlatform::MacOS.is_desktop());
    /// assert!(!TargetPlatform::Android.is_desktop());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_desktop(&self) -> bool {
        matches!(self, Self::MacOS | Self::Linux | Self::Windows)
    }

    /// Returns true if this is the web platform
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::TargetPlatform;
    ///
    /// assert!(TargetPlatform::Web.is_web());
    /// assert!(!TargetPlatform::Android.is_web());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_web(&self) -> bool {
        matches!(self, Self::Web)
    }

    /// Returns true if this is an Apple platform (iOS or macOS)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::TargetPlatform;
    ///
    /// assert!(TargetPlatform::iOS.is_apple());
    /// assert!(TargetPlatform::MacOS.is_apple());
    /// assert!(!TargetPlatform::Android.is_apple());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_apple(&self) -> bool {
        matches!(self, Self::iOS | Self::MacOS)
    }

    /// Returns true if touch input is primary for this platform
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::TargetPlatform;
    ///
    /// assert!(TargetPlatform::Android.is_touch_primary());
    /// assert!(TargetPlatform::iOS.is_touch_primary());
    /// assert!(!TargetPlatform::Windows.is_touch_primary());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_touch_primary(&self) -> bool {
        self.is_mobile() || matches!(self, Self::Web)
    }

    /// Returns a string representation
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::platform::TargetPlatform;
    ///
    /// assert_eq!(TargetPlatform::Android.as_str(), "android");
    /// assert_eq!(TargetPlatform::iOS.as_str(), "ios");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::iOS => "ios",
            Self::MacOS => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Fuchsia => "fuchsia",
            Self::Web => "web",
        }
    }
}

impl Default for TargetPlatform {
    fn default() -> Self {
        // Detect the current platform at compile time
        #[cfg(target_os = "android")]
        return Self::Android;

        #[cfg(target_os = "ios")]
        return Self::iOS;

        #[cfg(target_os = "macos")]
        return Self::MacOS;

        #[cfg(target_os = "linux")]
        return Self::Linux;

        #[cfg(target_os = "windows")]
        return Self::Windows;

        #[cfg(target_os = "fuchsia")]
        return Self::Fuchsia;

        #[cfg(target_arch = "wasm32")]
        return Self::Web;

        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "linux",
            target_os = "windows",
            target_os = "fuchsia",
            target_arch = "wasm32"
        )))]
        return Self::Linux; // fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_platform_is_mobile() {
        assert!(TargetPlatform::Android.is_mobile());
        assert!(TargetPlatform::iOS.is_mobile());
        assert!(!TargetPlatform::Windows.is_mobile());
        assert!(!TargetPlatform::MacOS.is_mobile());
        assert!(!TargetPlatform::Linux.is_mobile());
    }

    #[test]
    fn test_target_platform_is_desktop() {
        assert!(TargetPlatform::Windows.is_desktop());
        assert!(TargetPlatform::MacOS.is_desktop());
        assert!(TargetPlatform::Linux.is_desktop());
        assert!(!TargetPlatform::Android.is_desktop());
        assert!(!TargetPlatform::iOS.is_desktop());
    }

    #[test]
    fn test_target_platform_is_web() {
        assert!(TargetPlatform::Web.is_web());
        assert!(!TargetPlatform::Android.is_web());
        assert!(!TargetPlatform::Windows.is_web());
    }

    #[test]
    fn test_target_platform_default() {
        let platform = TargetPlatform::default();
        // The default depends on the compile target
        // Just verify it returns something
        let _ = platform;
    }
}
