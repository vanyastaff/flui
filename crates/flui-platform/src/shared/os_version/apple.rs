//! The Apple operating-system version.

use crate::shared::platform_floor::IOS_FLOOR;

use super::{Gate, OsVersion, Since};

/// An operating-system version as `NSProcessInfo.operatingSystemVersion` reports it. Ordered by
/// major, then minor, then patch; the components are private (see the module above).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppleVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl AppleVersion {
    /// A version Foundation could not report, which unlocks nothing.
    pub(crate) const UNKNOWN: Self = Self::new(0, 0, 0);

    pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The version from the signed components Foundation reports; a component that is negative
    /// or too large for `u32` reads as zero.
    pub(crate) fn from_components(major: i64, minor: i64, patch: i64) -> Self {
        let component = |value: i64| u32::try_from(value).unwrap_or(0);
        Self::new(component(major), component(minor), component(patch))
    }

    /// The version's position in the order of [`OsVersion::ordinal`], for a [`Gate::SINCE`].
    ///
    /// Each component keeps 21 bits; a component past that saturates, which no Apple release
    /// reaches.
    pub(crate) const fn rank(self) -> u64 {
        const fn component(value: u32) -> u64 {
            const MAX: u64 = (1 << 21) - 1;
            let value = value as u64;
            if value > MAX { MAX } else { value }
        }
        (component(self.major) << 42) | (component(self.minor) << 21) | component(self.patch)
    }

    /// The proof that this version has feature `F`, or `None`.
    pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
        super::since::<F>(self)
    }
}

impl OsVersion for AppleVersion {
    const FLOOR: u64 = AppleVersion::new(IOS_FLOOR.0, IOS_FLOOR.1, IOS_FLOOR.2).rank();

    fn ordinal(self) -> Option<u64> {
        (self != Self::UNKNOWN).then(|| self.rank())
    }
}

/// Shown as `major.minor.patch`, for logs and about screens; carries no order.
impl std::fmt::Display for AppleVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppleVersion, Gate};

    struct Feature;

    impl Gate for Feature {
        type Version = AppleVersion;
        const SINCE: u64 = AppleVersion::new(15, 2, 0).rank();
    }

    #[test]
    fn a_version_proves_a_feature_from_its_first_release_on() {
        let cases = [
            ((14, 8, 9), false),
            ((15, 1, 99), false),
            ((15, 2, 0), true),
            ((15, 2, 1), true),
            ((16, 0, 0), true),
            ((0, 0, 0), false),
        ];
        for ((major, minor, patch), has) in cases {
            let proof = AppleVersion::new(major, minor, patch).since::<Feature>();
            assert_eq!(proof.is_some(), has, "{major}.{minor}.{patch}");
        }
    }

    #[test]
    fn components_outside_u32_read_as_zero() {
        assert_eq!(
            AppleVersion::from_components(17, 4, 1),
            AppleVersion::new(17, 4, 1)
        );
        assert_eq!(
            AppleVersion::from_components(-1, 4, 1),
            AppleVersion::new(0, 4, 1)
        );
        assert_eq!(
            AppleVersion::from_components(17, i64::MAX, 1),
            AppleVersion::new(17, 0, 1)
        );
    }
}
