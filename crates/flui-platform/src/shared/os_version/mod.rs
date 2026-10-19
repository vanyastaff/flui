//! The running OS version of each environment, and the gates above its floor (ADR-0244).
//!
//! Each environment has one version type, read once when its platform is built and kept there
//! (never in a `static`): [`WindowsBuild`](windows::WindowsBuild),
//! [`AppleVersion`](apple::AppleVersion), [`AndroidApi`](android::AndroidApi). The number inside
//! is private to the type, so no code outside this module compares a version with a literal.
//!
//! A feature newer than the floor is a marker type implementing [`Gate`], declared next to the
//! code that uses it, with a doc line saying what that code does without it. The version's
//! `since::<F>()` answers with a [`Since<F>`] proof or nothing, and the call that needs the
//! feature takes the proof. A gate at or below the floor does not compile: the API is then called
//! directly, and a floor rise turns the gates it makes dead into compile errors. A version that
//! could not be read unlocks nothing.

use std::marker::PhantomData;

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
#[cfg(any(target_os = "ios", test))]
pub(crate) mod apple;
#[cfg(windows)]
pub(crate) mod windows;

/// An environment's version type: its floor, and an order over its values.
pub(crate) trait OsVersion: Copy {
    /// The floor's position in [`ordinal`](Self::ordinal)'s order.
    const FLOOR: u64;

    /// The version's position in an order every version of this type agrees with; `None` when
    /// the version could not be read.
    fn ordinal(self) -> Option<u64>;
}

/// A feature that exists from [`SINCE`](Self::SINCE) on, in the versions of
/// [`Version`](Self::Version).
pub(crate) trait Gate {
    /// The version type the feature is gated on.
    type Version: OsVersion;
    /// The first version that has the feature, as an [`OsVersion::ordinal`].
    const SINCE: u64;
}

/// Zero-sized evidence that the running version has feature `F`. Only [`since`] makes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Since<F>(PhantomData<fn() -> F>);

/// The proof that `version` has feature `F`, or `None` when it predates `F` or is unknown.
///
/// Fails to compile when `F` is not above the floor, where the feature is always there.
pub(crate) fn since<F: Gate>(version: F::Version) -> Option<Since<F>> {
    const {
        assert!(
            F::SINCE > <F::Version as OsVersion>::FLOOR,
            "a gate at or below the floor: call the API directly"
        );
    }
    version
        .ordinal()
        .filter(|ordinal| *ordinal >= F::SINCE)
        .map(|_| Since(PhantomData))
}

#[cfg(test)]
mod tests {
    use super::android::{
        AndroidApi, ContextDisplay, DisplayCutout, InsetsByType, InsetsController,
        ScaledScrollFactor, TextWeight,
    };
    use super::apple::AppleVersion;
    use super::{Gate, since};
    use crate::shared::android_ndk::{
        ChoreographerInstance, RegisterRefreshRate, SetFrameRate, UnregisterRefreshRate,
    };
    use crate::shared::display_link_rate::FrameRateRangeProperty;

    /// The failures of one marker's row: the version just before `SINCE` gets no proof, `SINCE`
    /// gets one, and an unknown version gets none.
    fn row<F: Gate>(
        name: &str,
        before: F::Version,
        at: F::Version,
        unknown: F::Version,
    ) -> Vec<String> {
        let mut failures = Vec::new();
        if since::<F>(before).is_some() {
            failures.push(format!("{name}: the version before SINCE got a proof"));
        }
        if since::<F>(at).is_none() {
            failures.push(format!("{name}: SINCE got no proof"));
        }
        if since::<F>(unknown).is_some() {
            failures.push(format!("{name}: an unknown version got a proof"));
        }
        failures
    }

    #[test]
    fn os_version_gate_table() {
        let api = AndroidApi::from_level;
        let unknown = AndroidApi::UNKNOWN;
        let mut failures = Vec::new();
        failures.extend(row::<SetFrameRate>(
            "SetFrameRate",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<ChoreographerInstance>(
            "ChoreographerInstance",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<RegisterRefreshRate>(
            "RegisterRefreshRate",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<UnregisterRefreshRate>(
            "UnregisterRefreshRate",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<ContextDisplay>(
            "ContextDisplay",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<TextWeight>("TextWeight", api(30), api(31), unknown));
        failures.extend(row::<ScaledScrollFactor>(
            "ScaledScrollFactor",
            api(25),
            api(26),
            unknown,
        ));
        failures.extend(row::<InsetsByType>(
            "InsetsByType",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<DisplayCutout>(
            "DisplayCutout",
            api(27),
            api(28),
            unknown,
        ));
        failures.extend(row::<InsetsController>(
            "InsetsController",
            api(29),
            api(30),
            unknown,
        ));
        failures.extend(row::<FrameRateRangeProperty>(
            "FrameRateRangeProperty",
            AppleVersion::new(14, 99, 99),
            AppleVersion::new(15, 0, 0),
            AppleVersion::UNKNOWN,
        ));
        #[cfg(windows)]
        failures.extend(row::<super::windows::QdcVirtualModeAware>(
            "QdcVirtualModeAware",
            super::windows::WindowsBuild::from_number(18361),
            super::windows::WindowsBuild::from_number(18362),
            super::windows::WindowsBuild::UNKNOWN,
        ));
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
