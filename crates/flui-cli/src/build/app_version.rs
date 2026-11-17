//! The application's version, `flui.toml`'s `[app] version`, and the form
//! each package states it in.
//!
//! The version is SemVer, parsed when `flui.toml` loads. Every package's
//! version comes from it and from nowhere else:
//!
//! - Android's `versionName` is the version as written, and its
//!   `versionCode`, the number a store and `adb install` compare to refuse a
//!   downgrade, is `major * 10000 + minor * 100 + patch`, so each later
//!   release has a larger code ([`AppVersion::android_version_code`]).
//! - Apple's `CFBundleVersion` and `CFBundleShortVersionString` take
//!   `major.minor.patch` ([`AppVersion::apple_bundle_version`]).
//!
//! A pre-release (`1.2.0-beta.1`) and build metadata do not reach a number:
//! `1.2.0-beta.1` and `1.2.0` share a `versionCode` and a `CFBundleVersion`,
//! so a store that already holds the pre-release refuses the release until
//! the patch moves.

use std::fmt;
use std::str::FromStr;

use cargo_metadata::semver::Version;
use serde::{Deserialize, Serialize};

/// The largest `versionCode` Google Play accepts.
const MAX_ANDROID_VERSION_CODE: u64 = 2_100_000_000;

/// The weight of the major and minor components in a `versionCode`, so that
/// a minor or a patch component above [`MAX_CODED_COMPONENT`] would spill
/// into the component above it.
const ANDROID_MAJOR_WEIGHT: u64 = 10_000;
const ANDROID_MINOR_WEIGHT: u64 = 100;
/// The largest minor or patch component a `versionCode` can hold.
const MAX_CODED_COMPONENT: u64 = ANDROID_MINOR_WEIGHT - 1;

/// A SemVer application version, as `flui.toml` states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct AppVersion(Version);

/// Why a version cannot be stated where a package needs it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AppVersionError {
    /// The text is not a SemVer version.
    #[error("`{text}` is not a SemVer version, like 1.2.3: {reason}")]
    NotSemVer { text: String, reason: String },
    /// A minor or patch component is too large to keep its place in the
    /// Android `versionCode`.
    #[error(
        "version {version}: the {component} component {value} is above {MAX_CODED_COMPONENT}, \
         which the Android versionCode (major * 10000 + minor * 100 + patch) cannot hold"
    )]
    ComponentTooLarge {
        version: String,
        component: &'static str,
        value: u64,
    },
    /// The `versionCode` would be above what Google Play accepts.
    #[error(
        "version {version}: the Android versionCode {code} is above \
         {MAX_ANDROID_VERSION_CODE}, the largest Google Play accepts"
    )]
    CodeTooLarge { version: String, code: u64 },
    /// `0.0.0` gives the `versionCode` 0, and Android takes only a positive
    /// one.
    #[error("version {version} gives the Android versionCode 0; Android takes 1 or more")]
    CodeZero { version: String },
}

impl AppVersion {
    /// Parse `text` as SemVer.
    ///
    /// # Errors
    ///
    /// [`AppVersionError::NotSemVer`] naming what the SemVer grammar refused.
    pub(crate) fn try_new(text: &str) -> Result<Self, AppVersionError> {
        Version::parse(text)
            .map(Self)
            .map_err(|error| AppVersionError::NotSemVer {
                text: text.to_owned(),
                reason: error.to_string(),
            })
    }

    /// The Android `versionName`: the version as written.
    #[must_use]
    pub(crate) fn android_version_name(&self) -> String {
        self.0.to_string()
    }

    /// The Android `versionCode`: `major * 10000 + minor * 100 + patch`.
    ///
    /// # Errors
    ///
    /// When a minor or patch component is above 99, so it would reach the
    /// component above it; when the code is above what Google Play accepts;
    /// or for `0.0.0`, whose code is 0.
    pub(crate) fn android_version_code(&self) -> Result<u32, AppVersionError> {
        let version = || self.0.to_string();
        for (component, value) in [("minor", self.0.minor), ("patch", self.0.patch)] {
            if value > MAX_CODED_COMPONENT {
                return Err(AppVersionError::ComponentTooLarge {
                    version: version(),
                    component,
                    value,
                });
            }
        }
        // Saturating: a major component near `u64::MAX` must still be
        // refused as too large, never wrap into a small code.
        let code = self
            .0
            .major
            .saturating_mul(ANDROID_MAJOR_WEIGHT)
            .saturating_add(self.0.minor * ANDROID_MINOR_WEIGHT)
            .saturating_add(self.0.patch);
        let too_large = || AppVersionError::CodeTooLarge {
            version: version(),
            code,
        };
        if code > MAX_ANDROID_VERSION_CODE {
            return Err(too_large());
        }
        match u32::try_from(code) {
            Ok(0) => Err(AppVersionError::CodeZero { version: version() }),
            Ok(code) => Ok(code),
            Err(_) => Err(too_large()),
        }
    }

    /// Apple's `CFBundleVersion` and `CFBundleShortVersionString`:
    /// `major.minor.patch`, which is all either key takes.
    #[must_use]
    pub(crate) fn apple_bundle_version(&self) -> String {
        format!("{}.{}.{}", self.0.major, self.0.minor, self.0.patch)
    }
}

impl FromStr for AppVersion {
    type Err = AppVersionError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::try_new(text)
    }
}

impl TryFrom<String> for AppVersion {
    type Error = AppVersionError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::try_new(&text)
    }
}

impl From<AppVersion> for String {
    fn from(version: AppVersion) -> Self {
        version.0.to_string()
    }
}

impl fmt::Display for AppVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppVersion, AppVersionError};

    fn version(text: &str) -> AppVersion {
        AppVersion::try_new(text).expect("a SemVer version")
    }

    /// Each package's form of one version: a later release always has a
    /// larger `versionCode`, a pre-release shares its release's numbers, and
    /// a version whose code would lose its order or leave the store's range
    /// is refused, naming the version.
    #[test]
    fn every_package_states_the_one_version() {
        let mut failures = Vec::new();
        for (text, code, name, apple) in [
            ("0.1.0", 100, "0.1.0", "0.1.0"),
            ("1.2.3", 10_203, "1.2.3", "1.2.3"),
            ("1.10.0", 11_000, "1.10.0", "1.10.0"),
            (
                "2.0.0-beta.1+build.7",
                20_000,
                "2.0.0-beta.1+build.7",
                "2.0.0",
            ),
            (
                "209999.99.99",
                2_099_999_999,
                "209999.99.99",
                "209999.99.99",
            ),
        ] {
            let parsed = version(text);
            let found = (
                parsed.android_version_code(),
                parsed.android_version_name(),
                parsed.apple_bundle_version(),
            );
            if found != (Ok(code), name.to_owned(), apple.to_owned()) {
                failures.push(format!("{text}: {found:?}"));
            }
            // What Apple takes for both keys: at most three period-separated
            // integers, digits and periods only.
            let apple_form = &found.2;
            if apple_form.split('.').count() > 3
                || !apple_form
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            {
                failures.push(format!(
                    "{text}: {apple_form} is not an Apple bundle version"
                ));
            }
        }
        for (text, refusal) in [
            (
                "1.100.0",
                "version 1.100.0: the minor component 100 is above 99, which the Android \
                 versionCode (major * 10000 + minor * 100 + patch) cannot hold",
            ),
            (
                "1.0.100",
                "version 1.0.100: the patch component 100 is above 99, which the Android \
                 versionCode (major * 10000 + minor * 100 + patch) cannot hold",
            ),
            (
                "210000.0.1",
                "version 210000.0.1: the Android versionCode 2100000001 is above 2100000000, \
                 the largest Google Play accepts",
            ),
            (
                "18446744073709551615.0.0",
                "version 18446744073709551615.0.0: the Android versionCode 18446744073709551615 \
                 is above 2100000000, the largest Google Play accepts",
            ),
            (
                "0.0.0",
                "version 0.0.0 gives the Android versionCode 0; Android takes 1 or more",
            ),
        ] {
            match version(text).android_version_code() {
                Err(error) if error.to_string() == refusal => {}
                other => failures.push(format!("{text}: {other:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn a_version_that_is_not_semver_is_refused_when_read() {
        for text in ["1.0", "v1.2.3", "1.2.3.4", ""] {
            assert!(
                matches!(
                    AppVersion::try_new(text),
                    Err(AppVersionError::NotSemVer { text: refused, .. }) if refused == text
                ),
                "{text:?} was accepted"
            );
        }
    }
}
