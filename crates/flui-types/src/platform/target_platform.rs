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
    #[expect(non_camel_case_types)]
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
    use super::TargetPlatform::{self, *};

    /// `(name, mobile, desktop, apple)` for every variant; the match keeps
    /// the table exhaustive when a variant is added.
    #[test]
    fn every_variant() {
        for p in [iOS, Android, Linux, MacOS, Windows, Fuchsia, Unknown] {
            let expected = match p {
                iOS => ("ios", true, false, true),
                Android => ("android", true, false, false),
                Linux => ("linux", false, true, false),
                MacOS => ("macos", false, true, true),
                Windows => ("windows", false, true, false),
                Fuchsia => ("fuchsia", false, false, false),
                Unknown => ("unknown", false, false, false),
            };
            assert_eq!(
                (p.as_str(), p.is_mobile(), p.is_desktop(), p.is_apple()),
                expected
            );
            assert_eq!(p.is_touch_primary(), p.is_mobile());
            assert_eq!(p.to_string(), p.as_str());
        }
    }

    #[test]
    fn current_is_the_compile_target() {
        let expected = if cfg!(target_os = "android") {
            Android
        } else if cfg!(target_os = "ios") {
            iOS
        } else if cfg!(target_os = "macos") {
            MacOS
        } else if cfg!(target_os = "linux") {
            Linux
        } else if cfg!(target_os = "windows") {
            Windows
        } else if cfg!(target_os = "fuchsia") {
            Fuchsia
        } else {
            Unknown
        };
        assert_eq!(TargetPlatform::current(), expected);
        assert_eq!(TargetPlatform::default(), expected);
    }
}
