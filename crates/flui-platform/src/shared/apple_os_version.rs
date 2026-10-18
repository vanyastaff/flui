//! The Apple operating-system version, and the proof a version check hands out.
//!
//! A platform reads the version once, when it is built, and keeps the value
//! (no process-wide state). A feature that exists only from some version on
//! names that version through [`VersionedFeature`]; [`OsVersion::since`] answers
//! with a [`Since`] proof or nothing, and the call that needs the feature takes
//! the proof, so it cannot be written without the check.

use std::marker::PhantomData;

/// An operating-system version as `NSProcessInfo.operatingSystemVersion`
/// reports it. Ordered by major, then minor, then patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OsVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl OsVersion {
    pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The version from the signed components Foundation reports; a component
    /// that is negative or too large for `u32` reads as zero.
    pub(crate) fn from_components(major: i64, minor: i64, patch: i64) -> Self {
        let component = |value: i64| u32::try_from(value).unwrap_or(0);
        Self::new(component(major), component(minor), component(patch))
    }

    /// The proof that this version has feature `F`, or `None` when it predates
    /// it.
    pub(crate) fn since<F: VersionedFeature>(self) -> Option<Since<F>> {
        (self >= F::SINCE).then_some(Since(PhantomData))
    }
}

/// A capability that exists from [`SINCE`](Self::SINCE) on.
pub(crate) trait VersionedFeature {
    /// The first version that has the feature.
    const SINCE: OsVersion;
}

/// Zero-sized evidence that the running version has feature `F`. Only
/// [`OsVersion::since`] makes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Since<F>(PhantomData<fn() -> F>);

#[cfg(test)]
mod tests {
    use super::{OsVersion, Since, VersionedFeature};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Feature;

    impl VersionedFeature for Feature {
        const SINCE: OsVersion = OsVersion::new(15, 2, 0);
    }

    #[test]
    fn a_version_proves_a_feature_from_its_first_release_on() {
        let cases = [
            ((14, 8, 9), false),
            ((15, 1, 99), false),
            ((15, 2, 0), true),
            ((15, 2, 1), true),
            ((16, 0, 0), true),
        ];
        for ((major, minor, patch), has) in cases {
            let proof: Option<Since<Feature>> = OsVersion::new(major, minor, patch).since();
            assert_eq!(proof.is_some(), has, "{major}.{minor}.{patch}");
        }
    }

    #[test]
    fn components_outside_u32_read_as_zero() {
        assert_eq!(OsVersion::from_components(17, 4, 1), OsVersion::new(17, 4, 1));
        assert_eq!(OsVersion::from_components(-1, 4, 1), OsVersion::new(0, 4, 1));
        assert_eq!(
            OsVersion::from_components(17, i64::MAX, 1),
            OsVersion::new(17, 0, 1)
        );
    }
}
