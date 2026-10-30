//! The environments FLUI has a floor for, and how a refusal names a release (ADR-0244).
//!
//! A backend that starts below its environment's floor, or without something the floor is made
//! of, refuses with these: the environment, and the releases as text. A release is shown, never
//! compared: what an application may branch on is a capability, not a version (ADR-0244 §11).

use std::fmt;

/// An environment with a floor of its own in `platform-support.toml`.
///
/// Unlike [`TargetPlatform`](crate::TargetPlatform), which picks a look and feel, this names
/// what a floor is stated for: the web is one, and Fuchsia, which FLUI does not run on, is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Environment {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    MacOS,
    /// Apple iOS and iPadOS.
    #[expect(
        non_camel_case_types,
        reason = "spelled as Apple spells it, as `TargetPlatform::iOS` is"
    )]
    iOS,
    /// Google Android.
    Android,
    /// Linux, on Wayland or X11.
    Linux,
    /// A browser.
    Web,
}

impl fmt::Display for Environment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Windows => "Windows",
            Self::MacOS => "macOS",
            Self::iOS => "iOS",
            Self::Android => "Android",
            Self::Linux => "Linux",
            Self::Web => "the web",
        })
    }
}

/// A release of an operating system as its environment writes it (`build 10240`, `13.0.0`,
/// `API 23`), for a message, a log or an about screen.
///
/// It has no order and no components: a release is shown, never compared (ADR-0244 §11).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OsRelease(String);

impl OsRelease {
    /// The release as `release` displays.
    pub fn from_display(release: impl fmt::Display) -> Self {
        Self(release.to_string())
    }
}

impl fmt::Display for OsRelease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
