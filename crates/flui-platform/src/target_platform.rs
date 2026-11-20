//! The operating systems whose conventions a widget can follow.
//!
//! The type lives in the contract so every crate above names one value; which
//! one a widget follows is `flui_view::environment::PlatformConvention::of`,
//! not the build host (ADR-0028).
//!
//! ```
//! use flui_platform::TargetPlatform;
//!
//! assert_eq!(TargetPlatform::Android.as_str(), "android");
//! ```

/// Target platform identification.
///
/// A widget reads the platform whose conventions it follows from
/// `flui_view::environment::PlatformConvention::of`, which the runtime
/// installs at the root; [`TargetPlatform::current()`] is the compile-time
/// default only the app runner takes (a clippy `disallowed-methods` entry).
/// The `Unknown` variant covers targets that do not match any of the
/// recognised platforms (for example unusual embedded targets) and is the
/// platform-neutral convention a tree outside a runtime follows.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[must_use]
pub enum TargetPlatform {
    /// Apple iOS (iPhone, iPad).
    #[expect(
        non_camel_case_types,
        reason = "`iOS` is Apple's spelling of the platform name"
    )]
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
    /// The platform this code was compiled for (`cfg!(target_os)`): the
    /// target, not the build machine, and `Unknown` on `wasm32`. A widget
    /// follows `PlatformConvention::of` instead.
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

    /// Whether this is iOS or macOS.
    #[inline]
    pub const fn is_apple(self) -> bool {
        matches!(self, Self::iOS | Self::MacOS)
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

impl core::fmt::Display for TargetPlatform {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
