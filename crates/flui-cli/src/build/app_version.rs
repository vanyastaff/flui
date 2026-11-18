//! The application's version, `flui.toml`'s `[app] version` and
//! `[app] build`, and the numbers each package states.
//!
//! The version is SemVer, parsed and checked when `flui.toml` loads, so every
//! packager accepts the same versions. Every package's numbers come from
//! [`AppVersion::package_numbers`] and from nowhere else:
//!
//! - the version as written is Android's `versionName` and Apple's
//!   `FLUIVersion`;
//! - Apple's `CFBundleShortVersionString` is `major.minor.patch`;
//! - the build number, the one a store compares to refuse a build it already
//!   holds, is `[app] build` when the project declares it: Android's
//!   `versionCode` and Apple's `CFBundleVersion`. Without it, the
//!   `versionCode` is `major * 10000 + minor * 100 + patch`, so each later
//!   release has a larger code, and `CFBundleVersion` is `major.minor.patch`.
//!
//! A pre-release (`1.2.0-beta.1`) does not reach a derived number:
//! `1.2.0-beta.1`, `1.2.0-beta.2` and `1.2.0` would share a `versionCode`
//! and a `CFBundleVersion`, and a store that holds one refuses the next. So a
//! release build of a pre-release needs `[app] build`
//! ([`AppVersionError::PreReleaseWithoutBuild`]); a debug build, which no
//! store sees, takes the derived numbers.

use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use cargo_metadata::semver::Version;
use serde::{Deserialize, Serialize};

use crate::build::platform::Profile;

/// The largest `versionCode` Google Play accepts.
const MAX_ANDROID_VERSION_CODE: u64 = 2_100_000_000;

/// The weight of the major and minor components in a derived `versionCode`,
/// so that a minor or a patch component above [`MAX_CODED_COMPONENT`] would
/// spill into the component above it.
const ANDROID_MAJOR_WEIGHT: u64 = 10_000;
const ANDROID_MINOR_WEIGHT: u64 = 100;
/// The largest minor or patch component a derived `versionCode` can hold.
const MAX_CODED_COMPONENT: u64 = ANDROID_MINOR_WEIGHT - 1;

/// A SemVer application version, as `flui.toml` states it, whose derived
/// `versionCode` keeps its order and stays in the store's range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct AppVersion {
    version: Version,
    /// `major * 10000 + minor * 100 + patch`, checked when parsed.
    derived_code: u32,
}

/// The build number a project declares (`[app] build`): a positive integer
/// no larger than the largest `versionCode` Google Play accepts, so one
/// number serves both stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub(crate) struct BuildNumber(NonZeroU32);

/// The numbers one package states, from [`AppVersion::package_numbers`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageNumbers {
    /// The version as written: Android's `versionName`, Apple's `FLUIVersion`.
    pub(crate) as_written: String,
    /// Android's `versionCode`.
    pub(crate) android_version_code: u32,
    /// Apple's `CFBundleShortVersionString`: `major.minor.patch`.
    pub(crate) apple_short_version: String,
    /// Apple's `CFBundleVersion`: the build number, or `major.minor.patch`.
    pub(crate) apple_bundle_version: String,
}

/// Why a version, or a build number, cannot be stated where a package needs it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AppVersionError {
    /// The text is not a SemVer version.
    #[error("`{text}` is not a SemVer version, like 1.2.3: {reason}")]
    NotSemVer { text: String, reason: String },
    /// A minor or patch component is too large to keep its place in the
    /// derived Android `versionCode`.
    #[error(
        "version {version}: the {component} component {value} is above {MAX_CODED_COMPONENT}, \
         which the Android versionCode (major * 10000 + minor * 100 + patch) cannot hold"
    )]
    ComponentTooLarge {
        version: String,
        component: &'static str,
        value: u64,
    },
    /// The derived `versionCode` would be above what Google Play accepts.
    #[error(
        "version {version}: the Android versionCode {code} is above \
         {MAX_ANDROID_VERSION_CODE}, the largest Google Play accepts"
    )]
    CodeTooLarge { version: String, code: u64 },
    /// `0.0.0` gives the `versionCode` 0, and Android takes only a positive
    /// one.
    #[error("version {version} gives the Android versionCode 0; Android takes 1 or more")]
    CodeZero { version: String },
    /// `[app] build` is not a positive integer a store accepts.
    #[error(
        "`[app] build = {build}` is not a build number: a whole number from 1 to \
         {MAX_ANDROID_VERSION_CODE}"
    )]
    BuildOutOfRange { build: u64 },
    /// A release build of a pre-release with no declared build number, which
    /// would share its store numbers with every other pre-release of the
    /// same `major.minor.patch` and with the release.
    #[error(
        "version {version} is a pre-release, which shares the derived build number \
         {apple_bundle_version} with its release and its other pre-releases; a store refuses the \
         second one. Add `build = <n>` under `[app]` in flui.toml, a number larger than every \
         build already uploaded"
    )]
    PreReleaseWithoutBuild {
        version: String,
        apple_bundle_version: String,
    },
}

impl AppVersion {
    /// Parse `text` as SemVer and check that its derived `versionCode` keeps
    /// its order and stays in the store's range, so every packager accepts
    /// the versions this accepts.
    ///
    /// # Errors
    ///
    /// [`AppVersionError::NotSemVer`] naming what the SemVer grammar refused;
    /// [`AppVersionError::ComponentTooLarge`] for a minor or patch component
    /// above 99, which would reach the component above it;
    /// [`AppVersionError::CodeTooLarge`] when the code is above what Google
    /// Play accepts; [`AppVersionError::CodeZero`] for `0.0.0`.
    pub(crate) fn try_new(text: &str) -> Result<Self, AppVersionError> {
        let version = Version::parse(text).map_err(|error| AppVersionError::NotSemVer {
            text: text.to_owned(),
            reason: error.to_string(),
        })?;
        let derived_code = derived_code(&version)?;
        Ok(Self {
            version,
            derived_code,
        })
    }

    /// The numbers a package built with `profile` states, `build` being the
    /// project's `[app] build`.
    ///
    /// # Errors
    ///
    /// [`AppVersionError::PreReleaseWithoutBuild`] for a release build of a
    /// pre-release with no `build`.
    pub(crate) fn package_numbers(
        &self,
        build: Option<BuildNumber>,
        profile: Profile,
    ) -> Result<PackageNumbers, AppVersionError> {
        let short = format!(
            "{}.{}.{}",
            self.version.major, self.version.minor, self.version.patch
        );
        let (android_version_code, apple_bundle_version) = match build {
            Some(build) => (build.0.get(), build.0.to_string()),
            None if !self.version.pre.is_empty() && profile == Profile::Release => {
                return Err(AppVersionError::PreReleaseWithoutBuild {
                    version: self.version.to_string(),
                    apple_bundle_version: short,
                });
            }
            None => (self.derived_code, short.clone()),
        };
        Ok(PackageNumbers {
            as_written: self.version.to_string(),
            android_version_code,
            apple_short_version: short,
            apple_bundle_version,
        })
    }
}

/// `major * 10000 + minor * 100 + patch`, when it keeps the order of
/// versions and stays in the store's range.
fn derived_code(version: &Version) -> Result<u32, AppVersionError> {
    let text = || version.to_string();
    for (component, value) in [("minor", version.minor), ("patch", version.patch)] {
        if value > MAX_CODED_COMPONENT {
            return Err(AppVersionError::ComponentTooLarge {
                version: text(),
                component,
                value,
            });
        }
    }
    // Saturating: a major component near `u64::MAX` must still be refused
    // as too large, never wrap into a small code.
    let code = version
        .major
        .saturating_mul(ANDROID_MAJOR_WEIGHT)
        .saturating_add(version.minor * ANDROID_MINOR_WEIGHT)
        .saturating_add(version.patch);
    let too_large = || AppVersionError::CodeTooLarge {
        version: text(),
        code,
    };
    if code > MAX_ANDROID_VERSION_CODE {
        return Err(too_large());
    }
    match u32::try_from(code) {
        Ok(0) => Err(AppVersionError::CodeZero { version: text() }),
        Ok(code) => Ok(code),
        Err(_) => Err(too_large()),
    }
}

impl BuildNumber {
    /// `build` as a build number.
    ///
    /// # Errors
    ///
    /// [`AppVersionError::BuildOutOfRange`] for 0 or a number above the
    /// largest `versionCode` Google Play accepts.
    pub(crate) fn try_new(build: u64) -> Result<Self, AppVersionError> {
        if build > MAX_ANDROID_VERSION_CODE {
            return Err(AppVersionError::BuildOutOfRange { build });
        }
        u32::try_from(build)
            .ok()
            .and_then(NonZeroU32::new)
            .map(Self)
            .ok_or(AppVersionError::BuildOutOfRange { build })
    }
}

impl TryFrom<u64> for BuildNumber {
    type Error = AppVersionError;

    fn try_from(build: u64) -> Result<Self, Self::Error> {
        Self::try_new(build)
    }
}

impl From<BuildNumber> for u64 {
    fn from(build: BuildNumber) -> Self {
        u64::from(build.0.get())
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
        version.version.to_string()
    }
}

impl fmt::Display for AppVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.version.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppVersion, AppVersionError, BuildNumber, PackageNumbers};
    use crate::build::platform::Profile;

    fn version(text: &str) -> AppVersion {
        AppVersion::try_new(text).expect("a SemVer version")
    }

    fn numbers(code: u32, written: &str, short: &str, bundle: &str) -> PackageNumbers {
        PackageNumbers {
            as_written: written.to_owned(),
            android_version_code: code,
            apple_short_version: short.to_owned(),
            apple_bundle_version: bundle.to_owned(),
        }
    }

    /// Each package's numbers for one version: a later release always has a
    /// larger derived `versionCode`; a declared build number is both stores'
    /// build number; Apple's forms are at most three dot-separated integers.
    #[test]
    fn every_package_states_the_one_version() {
        let mut failures = Vec::new();
        let build = |n: u64| Some(BuildNumber::try_new(n).expect("a build number"));
        for (text, declared, profile, expected) in [
            (
                "0.1.0",
                None,
                Profile::Release,
                numbers(100, "0.1.0", "0.1.0", "0.1.0"),
            ),
            (
                "1.2.3",
                None,
                Profile::Release,
                numbers(10_203, "1.2.3", "1.2.3", "1.2.3"),
            ),
            (
                "1.10.0",
                None,
                Profile::Release,
                numbers(11_000, "1.10.0", "1.10.0", "1.10.0"),
            ),
            (
                "209999.99.99",
                None,
                Profile::Release,
                numbers(
                    2_099_999_999,
                    "209999.99.99",
                    "209999.99.99",
                    "209999.99.99",
                ),
            ),
            (
                "2.0.0+build.7",
                None,
                Profile::Release,
                numbers(20_000, "2.0.0+build.7", "2.0.0", "2.0.0"),
            ),
            (
                "2.0.0-beta.1",
                None,
                Profile::Debug,
                numbers(20_000, "2.0.0-beta.1", "2.0.0", "2.0.0"),
            ),
            (
                "2.0.0-beta.1",
                build(41),
                Profile::Release,
                numbers(41, "2.0.0-beta.1", "2.0.0", "41"),
            ),
            (
                "2.0.0",
                build(42),
                Profile::Debug,
                numbers(42, "2.0.0", "2.0.0", "42"),
            ),
        ] {
            match version(text).package_numbers(declared, profile) {
                Ok(found) if found == expected => {
                    for apple in [&found.apple_short_version, &found.apple_bundle_version] {
                        if apple.split('.').count() > 3
                            || !apple.split('.').all(|part| {
                                !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
                            })
                        {
                            failures.push(format!("{text}: {apple} is not an Apple version"));
                        }
                    }
                }
                other => failures.push(format!("{text} {declared:?} {profile}: {other:?}")),
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// A release build of a pre-release names the key to add; a debug build
    /// of it, or a release build with a build number, is packaged.
    #[test]
    fn a_release_of_a_pre_release_needs_a_build_number() {
        let refused = version("1.4.2-beta.2")
            .package_numbers(None, Profile::Release)
            .expect_err("no build number");
        assert_eq!(
            refused,
            AppVersionError::PreReleaseWithoutBuild {
                version: "1.4.2-beta.2".to_owned(),
                apple_bundle_version: "1.4.2".to_owned(),
            }
        );
        assert!(
            refused
                .to_string()
                .contains("Add `build = <n>` under `[app]`")
        );
        for (build, accepted) in [
            (0, false),
            (1, true),
            (2_100_000_000, true),
            (2_100_000_001, false),
            (u64::MAX, false),
        ] {
            assert_eq!(BuildNumber::try_new(build).is_ok(), accepted, "{build}");
        }
    }

    /// Every limit a packager has is checked when the version is read, so a
    /// version one packager refuses never loads: a component that would spill
    /// into the one above, a code past the store's range, `0.0.0`, and text
    /// that is not SemVer.
    #[test]
    fn a_version_no_packager_takes_is_refused_when_read() {
        for (text, refusal) in [
            (
                "1.100.0",
                "version 1.100.0: the minor component 100 is above 99, which the Android \
                 versionCode (major * 10000 + minor * 100 + patch) cannot hold",
            ),
            (
                "1.200.0",
                "version 1.200.0: the minor component 200 is above 99, which the Android \
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
            match AppVersion::try_new(text) {
                Err(error) if error.to_string() == refusal => {}
                other => panic!("{text}: {other:?}"),
            }
        }
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
