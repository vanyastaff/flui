//! The Android API level, and the framework APIs above the floor the backend uses.
//!
//! The level is read in [`android_ndk`](crate::shared::android_ndk), where the NDK functions it
//! comes from are looked up; an NDK function above the floor is a gate of its own there.

use crate::shared::platform_floor::ANDROID_API;

use super::{Gate, OsVersion, Since};

/// The API level of the running system (`Build.VERSION.SDK_INT`); private (see the module
/// above).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AndroidApi(u32);

impl AndroidApi {
    /// A level the system would not report, which unlocks nothing.
    pub(crate) const UNKNOWN: Self = Self(0);

    /// The level the system reported.
    pub(crate) const fn from_level(level: u32) -> Self {
        Self(level)
    }

    /// The proof that this level has feature `F`, or `None`.
    pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
        super::since::<F>(self)
    }
}

impl OsVersion for AndroidApi {
    const FLOOR: u64 = ANDROID_API as u64;

    fn ordinal(self) -> Option<u64> {
        (self != Self::UNKNOWN).then_some(u64::from(self.0))
    }
}

/// Shown as `API N`, for logs and about screens; carries no order.
impl std::fmt::Display for AndroidApi {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == Self::UNKNOWN {
            formatter.write_str("API unknown")
        } else {
            write!(formatter, "API {}", self.0)
        }
    }
}

/// `Context.getDisplay` (API 30). Without it the display comes from the window manager's
/// default display.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextDisplay;

impl Gate for ContextDisplay {
    type Version = AndroidApi;
    const SINCE: u64 = 30;
}

/// `Configuration.fontWeightAdjustment` (API 31). Without it the text-weight preference is
/// reported as unavailable.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextWeight;

impl Gate for TextWeight {
    type Version = AndroidApi;
    const SINCE: u64 = 31;
}

/// `ViewConfiguration.getScaledHorizontalScrollFactor` and its vertical twin (API 26). Without
/// them the scroll factor is read from the theme's `listPreferredItemHeight`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScaledScrollFactor;

impl Gate for ScaledScrollFactor {
    type Version = AndroidApi;
    const SINCE: u64 = 26;
}
