//! Target platform enum.
//!
//! Canonical home for `TargetPlatform` across the workspace. Identifies the
//! platform the application is running on so platform-specific behaviour can
//! be selected at compile time (`current()`) or branched on at runtime.
//!
//! Per Constitution Principle 2 ("Strict Crate Dependency DAG"), this type
//! lives in `flui-types` (Foundation layer) so any downstream crate can
//! consume it without inverting the dependency graph.
//!
//! # Variants
//!
//! The enum is marked `#[non_exhaustive]` so new variants (for example a
//! future `Web` variant) can be added without breaking external `match`
//! expressions. Intra-crate exhaustive `match` is preserved by the test
//! module below.
//!
//! # Example
//!
//! ```
//! use flui_types::platform::TargetPlatform;
//!
//! let platform = TargetPlatform::current();
//! assert!(!platform.as_str().is_empty());
//!
//! if platform.is_mobile() {
//!     // touch-first input
//! }
//! ```

/// Target platform identification.
///
/// Use [`TargetPlatform::current()`] for compile-time detection of the host
/// platform. The `Unknown` variant covers targets that do not match any of
/// the recognised platforms (for example unusual embedded targets).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[must_use]
pub enum TargetPlatform {
    /// Apple iOS (iPhone, iPad).
    #[allow(non_camel_case_types)]
    iOS,
    /// Google Android.
    Android,
    /// Linux (any distribution).
    Linux,
    /// Apple macOS.
    MacOS,
    /// Microsoft Windows.
    Windows,
    /// Google Fuchsia.
    Fuchsia,
    /// Unknown or unsupported platform.
    ///
    /// Returned by [`TargetPlatform::current()`] for targets that do not
    /// match any of the recognised `target_os` / `target_arch` patterns.
    Unknown,
}

impl TargetPlatform {
    /// Detects the host platform at compile time via `cfg!` evaluation.
    #[inline]
    pub const fn current() -> Self {
        #[cfg(target_os = "android")]
        {
            Self::Android
        }
        #[cfg(target_os = "ios")]
        {
            Self::iOS
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOS
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "fuchsia")]
        {
            Self::Fuchsia
        }
        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "linux",
            target_os = "windows",
            target_os = "fuchsia"
        )))]
        {
            Self::Unknown
        }
    }

    /// Returns true if this is a mobile platform (Android or iOS).
    #[inline]
    pub const fn is_mobile(self) -> bool {
        matches!(self, Self::Android | Self::iOS)
    }

    /// Returns true if this is a desktop platform (Windows, macOS, or Linux).
    #[inline]
    pub const fn is_desktop(self) -> bool {
        matches!(self, Self::Windows | Self::MacOS | Self::Linux)
    }

    /// Returns true if this is an Apple platform (iOS or macOS).
    #[inline]
    pub const fn is_apple(self) -> bool {
        matches!(self, Self::iOS | Self::MacOS)
    }

    /// Returns true if touch is the primary input modality.
    #[inline]
    pub const fn is_touch_primary(self) -> bool {
        self.is_mobile()
    }

    /// Returns the platform name as a static string identifier.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::iOS => "ios",
            Self::Android => "android",
            Self::Linux => "linux",
            Self::MacOS => "macos",
            Self::Windows => "windows",
            Self::Fuchsia => "fuchsia",
            Self::Unknown => "unknown",
        }
    }
}

impl Default for TargetPlatform {
    #[inline]
    fn default() -> Self {
        Self::current()
    }
}

impl core::fmt::Display for TargetPlatform {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::TargetPlatform;

    /// Variant coverage: every variant must be reachable via exhaustive
    /// `match`. Adding a new variant must update this match arm
    /// intentionally (the `#[non_exhaustive]` attribute does not block
    /// intra-crate exhaustive matching).
    #[test]
    fn variant_coverage_exhaustive_match() {
        for variant in [
            TargetPlatform::iOS,
            TargetPlatform::Android,
            TargetPlatform::Linux,
            TargetPlatform::MacOS,
            TargetPlatform::Windows,
            TargetPlatform::Fuchsia,
            TargetPlatform::Unknown,
        ] {
            let tag: u8 = match variant {
                TargetPlatform::iOS => 0,
                TargetPlatform::Android => 1,
                TargetPlatform::Linux => 2,
                TargetPlatform::MacOS => 3,
                TargetPlatform::Windows => 4,
                TargetPlatform::Fuchsia => 5,
                TargetPlatform::Unknown => 6,
            };
            assert!(tag < 7);
        }
    }

    #[test]
    fn current_returns_known_variant() {
        let p = TargetPlatform::current();
        assert!(matches!(
            p,
            TargetPlatform::iOS
                | TargetPlatform::Android
                | TargetPlatform::Linux
                | TargetPlatform::MacOS
                | TargetPlatform::Windows
                | TargetPlatform::Fuchsia
                | TargetPlatform::Unknown
        ));
    }

    #[test]
    fn category_predicates() {
        assert!(TargetPlatform::Android.is_mobile());
        assert!(TargetPlatform::iOS.is_mobile());
        assert!(!TargetPlatform::Windows.is_mobile());

        assert!(TargetPlatform::Windows.is_desktop());
        assert!(TargetPlatform::MacOS.is_desktop());
        assert!(TargetPlatform::Linux.is_desktop());
        assert!(!TargetPlatform::Android.is_desktop());

        assert!(TargetPlatform::iOS.is_apple());
        assert!(TargetPlatform::MacOS.is_apple());
        assert!(!TargetPlatform::Windows.is_apple());

        assert!(TargetPlatform::Android.is_touch_primary());
        assert!(!TargetPlatform::Linux.is_touch_primary());
    }

    #[test]
    fn as_str_round_trip() {
        assert_eq!(TargetPlatform::iOS.as_str(), "ios");
        assert_eq!(TargetPlatform::Android.as_str(), "android");
        assert_eq!(TargetPlatform::Linux.as_str(), "linux");
        assert_eq!(TargetPlatform::MacOS.as_str(), "macos");
        assert_eq!(TargetPlatform::Windows.as_str(), "windows");
        assert_eq!(TargetPlatform::Fuchsia.as_str(), "fuchsia");
        assert_eq!(TargetPlatform::Unknown.as_str(), "unknown");
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", TargetPlatform::Android), "android");
        assert_eq!(format!("{}", TargetPlatform::Windows), "windows");
        assert_eq!(format!("{}", TargetPlatform::Unknown), "unknown");
    }

    #[test]
    fn default_matches_current() {
        assert_eq!(TargetPlatform::default(), TargetPlatform::current());
    }
}
