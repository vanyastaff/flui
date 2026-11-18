//! The macOS version the process runs on, and the gates above FLUI's floor
//! (ADR-0244 §4), in the shape of `flui-native`'s `version::gate`, which a
//! capability crate cannot reach (ADR-0154 §1).
//!
//! A feature newer than the floor is a marker implementing [`Gate`],
//! declared next to the code that uses it with a line saying what that code
//! answers without it. [`MacOSVersion::since`] gives a [`Since`] proof or
//! nothing, and the call that needs the feature takes the proof. A gate at
//! or below the floor does not compile: the API is called directly there,
//! and a floor rise turns the gates it makes dead into errors. A version
//! that could not be read unlocks nothing. The floor differs by
//! architecture ([`MACOS_FLOOR`]), so a gate between the two floors exists
//! on x86_64 only.

use std::marker::PhantomData;

use super::floor::MACOS_FLOOR;

/// A macOS version as `NSProcessInfo.operatingSystemVersion` reports it,
/// read when a UI runtime first asks and kept there (never in a `static`).
/// The components are private, so no code compares one with a literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MacOSVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl MacOSVersion {
    /// A version Foundation could not report, which unlocks nothing.
    pub(crate) const UNKNOWN: Self = Self::new(0, 0, 0);

    pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The version from the signed components Foundation reports; one that
    /// is negative or past `u32` reads as zero.
    pub(crate) fn from_components(major: isize, minor: isize, patch: isize) -> Self {
        let component = |value: isize| u32::try_from(value).unwrap_or(0);
        Self::new(component(major), component(minor), component(patch))
    }

    /// The version the process runs on.
    #[cfg(target_os = "macos")]
    pub(crate) fn read() -> Self {
        let version = objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion();
        Self::from_components(
            version.majorVersion,
            version.minorVersion,
            version.patchVersion,
        )
    }

    /// The version as `(major, minor, patch)`, for what the system says it
    /// is (`os::OsVersion`); never compared here.
    pub(crate) const fn components(self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch)
    }

    /// The proof that this version has feature `F`, or `None` when it
    /// predates `F` or is unknown.
    ///
    /// Fails to compile when `F` is not above the floor, where the feature
    /// is always there.
    pub(crate) fn since<F: Gate>(self) -> Option<Since<F>> {
        const {
            assert!(
                rank(F::SINCE) > rank(MACOS_FLOOR),
                "a gate at or below the floor: call the API directly"
            );
        }
        (self != Self::UNKNOWN && rank(self.components()) >= rank(F::SINCE))
            .then_some(Since(PhantomData))
    }
}

/// The position of `(major, minor, patch)` in the order every version
/// agrees with: each component keeps 21 bits, which no release reaches.
const fn rank((major, minor, patch): (u32, u32, u32)) -> u64 {
    const fn component(value: u32) -> u64 {
        const MAX: u64 = (1 << 21) - 1;
        let value = value as u64;
        if value > MAX { MAX } else { value }
    }
    (component(major) << 42) | (component(minor) << 21) | component(patch)
}

/// A feature macOS has from [`SINCE`](Self::SINCE) on.
pub(crate) trait Gate {
    /// The first version that has it, as `(major, minor, patch)`.
    const SINCE: (u32, u32, u32);
}

/// Zero-sized evidence that the running macOS has feature `F`; only
/// [`MacOSVersion::since`] makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Since<F>(PhantomData<fn() -> F>);

#[cfg(test)]
mod tests {
    use super::{Gate, MacOSVersion};

    /// A feature far above any floor.
    enum Distant {}

    impl Gate for Distant {
        const SINCE: (u32, u32, u32) = (13, 0, 0);
    }

    #[test]
    fn a_gate_opens_from_its_version_on_and_never_for_an_unknown_one() {
        let cases = [
            (MacOSVersion::new(12, 7, 6), false),
            (MacOSVersion::new(13, 0, 0), true),
            (MacOSVersion::new(26, 1, 0), true),
            (MacOSVersion::UNKNOWN, false),
            // A component Foundation reported negative reads as zero.
            (MacOSVersion::from_components(-1, 0, 0), false),
        ];
        for (version, opens) in cases {
            assert_eq!(version.since::<Distant>().is_some(), opens, "{version:?}");
        }
    }
}
