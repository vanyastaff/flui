//! The version of the Apple system the process runs on, and the gates above
//! FLUI's floor (ADR-0244 §4), in the shape of `flui-native`'s
//! `version::gate`, which a capability crate cannot reach (ADR-0154 §1).
//!
//! A feature newer than the floor is a marker implementing [`Gate`] for one
//! system, declared next to the code that uses it with a line saying what
//! that code answers without it. [`Version::since`] gives a [`Since`] proof
//! or nothing, and the call that needs the feature takes the proof. A gate
//! at or below its system's floor does not compile: the API is called
//! directly there, and a floor rise turns the gates it makes dead into
//! errors. A version that could not be read unlocks nothing.
//!
//! Each system has its own version type (`MacOSVersion`, `IOSVersion`),
//! so a gate written for one is never asked of the other. The macOS floor
//! differs by architecture (`MACOS_FLOOR`), so a gate between its two
//! floors exists on x86_64 only.

use std::fmt;
use std::marker::PhantomData;

/// An Apple system FLUI supports: the floor its gates compile against.
pub(crate) trait AppleSystem {
    /// The lowest version FLUI supports, as `(major, minor, patch)`.
    const FLOOR: (u32, u32, u32);
}

/// macOS.
#[cfg(any(target_os = "macos", test))]
pub(crate) enum OnMacOS {}

#[cfg(any(target_os = "macos", test))]
impl AppleSystem for OnMacOS {
    const FLOOR: (u32, u32, u32) = super::floor::MACOS_FLOOR;
}

/// iOS and iPadOS, which share their versions.
#[cfg(any(target_os = "ios", test))]
pub(crate) enum OnIOS {}

#[cfg(any(target_os = "ios", test))]
impl AppleSystem for OnIOS {
    const FLOOR: (u32, u32, u32) = super::floor::IOS_FLOOR;
}

/// A version of the system `S` as `NSProcessInfo.operatingSystemVersion`
/// reports it, read when a UI runtime first asks and kept there (never in a
/// `static`). The components are private, so no code compares one with a
/// literal.
pub(crate) struct Version<S> {
    major: u32,
    minor: u32,
    patch: u32,
    system: PhantomData<fn() -> S>,
}

/// A macOS version.
#[cfg(any(target_os = "macos", test))]
pub(crate) type MacOSVersion = Version<OnMacOS>;

/// An iOS or iPadOS version.
#[cfg(any(target_os = "ios", test))]
pub(crate) type IOSVersion = Version<OnIOS>;

impl<S> Clone for Version<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Version<S> {}

impl<S> PartialEq for Version<S> {
    fn eq(&self, other: &Self) -> bool {
        self.components() == other.components()
    }
}

impl<S> Eq for Version<S> {}

impl<S> fmt::Debug for Version<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl<S> Version<S> {
    /// A version Foundation could not report, which unlocks nothing.
    pub(crate) const UNKNOWN: Self = Self::new(0, 0, 0);

    pub(crate) const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            system: PhantomData,
        }
    }

    /// The version from the signed components Foundation reports; one that
    /// is negative or past `u32` reads as zero.
    pub(crate) fn from_components(major: isize, minor: isize, patch: isize) -> Self {
        let component = |value: isize| u32::try_from(value).unwrap_or(0);
        Self::new(component(major), component(minor), component(patch))
    }

    /// The version the process runs on.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
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
}

impl<S: AppleSystem> Version<S> {
    /// The proof that this version has feature `F`, or `None` when it
    /// predates `F` or is unknown.
    ///
    /// Fails to compile when `F` is not above the floor of `S`, where the
    /// feature is always there.
    pub(crate) fn since<F: Gate<System = S>>(self) -> Option<Since<F>> {
        const {
            assert!(
                rank(F::SINCE) > rank(S::FLOOR),
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

/// The version of the system `S`, read when a UI runtime first asks for it
/// (`ProviderContext::runtime_local`) and kept there.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) struct RunningVersion<S>(pub(crate) Version<S>);

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl<S> Default for RunningVersion<S> {
    fn default() -> Self {
        Self(Version::read())
    }
}

/// A feature [`System`](Self::System) has from [`SINCE`](Self::SINCE) on.
pub(crate) trait Gate {
    /// The system the feature is gated on.
    type System: AppleSystem;
    /// The first version that has it, as `(major, minor, patch)`.
    const SINCE: (u32, u32, u32);
}

/// Zero-sized evidence that the running system has feature `F`; only
/// [`Version::since`] makes one.
pub(crate) struct Since<F>(PhantomData<fn() -> F>);

impl<F> Clone for Since<F> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<F> Copy for Since<F> {}

impl<F> fmt::Debug for Since<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Since")
    }
}

#[cfg(test)]
mod tests {
    use super::{Gate, IOSVersion, MacOSVersion, OnIOS, OnMacOS};

    /// A macOS feature far above any floor.
    enum DistantMac {}

    impl Gate for DistantMac {
        type System = OnMacOS;
        const SINCE: (u32, u32, u32) = (13, 0, 0);
    }

    /// An iOS feature above the floor.
    enum DistantPhone {}

    impl Gate for DistantPhone {
        type System = OnIOS;
        const SINCE: (u32, u32, u32) = (16, 4, 0);
    }

    #[test]
    fn a_gate_opens_from_its_version_on_and_never_for_an_unknown_one() {
        let mac = [
            (MacOSVersion::new(12, 7, 6), false),
            (MacOSVersion::new(13, 0, 0), true),
            (MacOSVersion::new(26, 1, 0), true),
            (MacOSVersion::UNKNOWN, false),
            // A component Foundation reported negative reads as zero.
            (MacOSVersion::from_components(-1, 0, 0), false),
        ];
        for (version, opens) in mac {
            assert_eq!(
                version.since::<DistantMac>().is_some(),
                opens,
                "{version:?}"
            );
        }
        let phone = [
            (IOSVersion::new(16, 3, 1), false),
            (IOSVersion::new(16, 4, 0), true),
            (IOSVersion::new(26, 0, 1), true),
            (IOSVersion::UNKNOWN, false),
            (IOSVersion::from_components(17, -2, 0), true),
        ];
        for (version, opens) in phone {
            assert_eq!(
                version.since::<DistantPhone>().is_some(),
                opens,
                "{version:?}"
            );
        }
    }
}
