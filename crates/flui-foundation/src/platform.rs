//! Platform detection and identification
//!
//! This module provides types for identifying the target platform,
//! enabling platform-specific behavior when needed.
//!
//! # Examples
//!
//! ```rust
//! use flui_foundation::TargetPlatform;
//!
//! let platform = TargetPlatform::current();
//! println!("Running on: {:?}", platform);
//!
//! if platform.is_mobile() {
//!     println!("Mobile platform detected");
//! }
//! ```

/// The platform the application is running on.
///
/// This enum represents the major platforms supported by FLUI.
/// Use [`TargetPlatform::current()`] to detect the current platform at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TargetPlatform {
    /// Google Android
    Android,
    /// Apple iOS
    IOS,
    /// Microsoft Windows
    Windows,
    /// Apple macOS
    MacOS,
    /// Linux (any distribution)
    Linux,
    /// Web browser (WebAssembly)
    Web,
    /// Unknown or unsupported platform
    Unknown,
}

impl TargetPlatform {
    /// Detects the current platform at compile time.
    ///
    /// This uses conditional compilation to determine the target platform.
    #[inline]
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "android")]
        {
            Self::Android
        }
        #[cfg(target_os = "ios")]
        {
            Self::IOS
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOS
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::Web
        }
        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_arch = "wasm32"
        )))]
        {
            Self::Unknown
        }
    }

    /// Returns true if this is a mobile platform (Android or iOS).
    #[inline]
    #[must_use]
    pub const fn is_mobile(self) -> bool {
        matches!(self, Self::Android | Self::IOS)
    }

    /// Returns true if this is a desktop platform (Windows, macOS, or Linux).
    #[inline]
    #[must_use]
    pub const fn is_desktop(self) -> bool {
        matches!(self, Self::Windows | Self::MacOS | Self::Linux)
    }

    /// Returns true if this is a web platform (WebAssembly).
    #[inline]
    #[must_use]
    pub const fn is_web(self) -> bool {
        matches!(self, Self::Web)
    }

    /// Returns true if this is an Apple platform (iOS or macOS).
    #[inline]
    #[must_use]
    pub const fn is_apple(self) -> bool {
        matches!(self, Self::IOS | Self::MacOS)
    }

    /// Returns the platform name as a string.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::IOS => "ios",
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::Linux => "linux",
            Self::Web => "web",
            Self::Unknown => "unknown",
        }
    }
}

impl Default for TargetPlatform {
    fn default() -> Self {
        Self::current()
    }
}

impl std::fmt::Display for TargetPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_platform() {
        let platform = TargetPlatform::current();
        // Should return some valid platform
        assert!(matches!(
            platform,
            TargetPlatform::Android
                | TargetPlatform::IOS
                | TargetPlatform::Windows
                | TargetPlatform::MacOS
                | TargetPlatform::Linux
                | TargetPlatform::Web
                | TargetPlatform::Unknown
        ));
    }

    #[test]
    fn test_platform_categories() {
        assert!(TargetPlatform::Android.is_mobile());
        assert!(TargetPlatform::IOS.is_mobile());
        assert!(!TargetPlatform::Windows.is_mobile());

        assert!(TargetPlatform::Windows.is_desktop());
        assert!(TargetPlatform::MacOS.is_desktop());
        assert!(TargetPlatform::Linux.is_desktop());
        assert!(!TargetPlatform::Android.is_desktop());

        assert!(TargetPlatform::Web.is_web());
        assert!(!TargetPlatform::Android.is_web());

        assert!(TargetPlatform::IOS.is_apple());
        assert!(TargetPlatform::MacOS.is_apple());
        assert!(!TargetPlatform::Windows.is_apple());
    }

    #[test]
    fn test_as_str() {
        assert_eq!(TargetPlatform::Android.as_str(), "android");
        assert_eq!(TargetPlatform::IOS.as_str(), "ios");
        assert_eq!(TargetPlatform::Windows.as_str(), "windows");
        assert_eq!(TargetPlatform::MacOS.as_str(), "macos");
        assert_eq!(TargetPlatform::Linux.as_str(), "linux");
        assert_eq!(TargetPlatform::Web.as_str(), "web");
        assert_eq!(TargetPlatform::Unknown.as_str(), "unknown");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", TargetPlatform::Android), "android");
        assert_eq!(format!("{}", TargetPlatform::Windows), "windows");
    }

    #[test]
    fn test_default() {
        let platform = TargetPlatform::default();
        assert_eq!(platform, TargetPlatform::current());
    }
}
