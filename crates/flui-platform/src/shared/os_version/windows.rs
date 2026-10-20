//! The Windows build this process runs on.

#![expect(
    unsafe_code,
    reason = "reads the build with RtlGetVersion; the block names its contract"
)]

use windows::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Devices::Display::{
            QDC_ONLY_ACTIVE_PATHS, QDC_VIRTUAL_MODE_AWARE, QUERY_DISPLAY_CONFIG_FLAGS,
        },
        System::SystemInformation::OSVERSIONINFOW,
    },
};

use crate::shared::platform_floor::WINDOWS_BUILD;

use super::{Gate, OsVersion, Since};

/// The OS build number (`dwBuildNumber`); private (see the module above).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowsBuild(u32);

impl WindowsBuild {
    /// A build `RtlGetVersion` could not report, which unlocks nothing.
    pub(crate) const UNKNOWN: Self = Self(0);

    /// Read the running build with `RtlGetVersion`, which, unlike `GetVersionExW`, does not
    /// report the manifest's compatibility version. A failed read is [`UNKNOWN`](Self::UNKNOWN).
    pub(crate) fn read() -> Self {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is an `OSVERSIONINFOW` whose size field holds its own
        // size, which is what `RtlGetVersion` requires; it outlives the call.
        let status = unsafe { RtlGetVersion(&raw mut info) };
        if status.is_err() {
            tracing::warn!(
                ?status,
                "the Windows build is unknown; gated flags stay off"
            );
            return Self::UNKNOWN;
        }
        Self(info.dwBuildNumber)
    }

    #[cfg(test)]
    pub(crate) const fn from_number(build: u32) -> Self {
        Self(build)
    }

    /// The proof that this build has feature `F`, or `None`.
    pub(crate) fn since<F: Gate<Version = Self>>(self) -> Option<Since<F>> {
        super::since::<F>(self)
    }

    /// The `QueryDisplayConfig` flags for the active paths on this build.
    pub(crate) fn display_config_flags(self) -> QUERY_DISPLAY_CONFIG_FLAGS {
        match self.since::<QdcVirtualModeAware>() {
            Some(proof) => QDC_ONLY_ACTIVE_PATHS | QUERY_DISPLAY_CONFIG_FLAGS::from(proof),
            None => QDC_ONLY_ACTIVE_PATHS,
        }
    }
}

impl OsVersion for WindowsBuild {
    const FLOOR: u64 = WINDOWS_BUILD as u64;

    fn ordinal(self) -> Option<u64> {
        (self != Self::UNKNOWN).then_some(u64::from(self.0))
    }
}

/// Shown as `build N`, for logs and about screens; carries no order.
impl std::fmt::Display for WindowsBuild {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "build {}", self.0)
    }
}

/// `QDC_VIRTUAL_MODE_AWARE`, which `QueryDisplayConfig` accepts from Windows 10 1903 (build
/// 18362). Without it the active paths are queried without virtual-mode awareness.
#[derive(Debug, Clone, Copy)]
pub(crate) struct QdcVirtualModeAware;

impl Gate for QdcVirtualModeAware {
    type Version = WindowsBuild;
    const SINCE: u64 = 18362;
}

impl From<Since<QdcVirtualModeAware>> for QUERY_DISPLAY_CONFIG_FLAGS {
    fn from(_proof: Since<QdcVirtualModeAware>) -> Self {
        QDC_VIRTUAL_MODE_AWARE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_build_below_18362_omits_the_virtual_mode_flag() {
        let old = WindowsBuild::from_number(18361);
        assert!(old.since::<QdcVirtualModeAware>().is_none());
        assert_eq!(old.display_config_flags(), QDC_ONLY_ACTIVE_PATHS);
        assert_eq!(
            WindowsBuild::UNKNOWN.display_config_flags(),
            QDC_ONLY_ACTIVE_PATHS,
            "an unknown build unlocks nothing"
        );
        let new = WindowsBuild::from_number(18362);
        assert!(new.since::<QdcVirtualModeAware>().is_some());
        assert_eq!(
            new.display_config_flags(),
            QDC_ONLY_ACTIVE_PATHS | QDC_VIRTUAL_MODE_AWARE
        );
    }

    fn the_running_build_is_read() {
        assert!(
            WindowsBuild::read().0 >= 10_000,
            "RtlGetVersion reports the real build, not a manifest-compatible one"
        );
    }

    #[test]
    fn os_capability_table() {
        crate::table_test::run_table(
            "os_capability_table",
            &[
                (
                    "windows_build_below_18362_omits_the_virtual_mode_flag",
                    windows_build_below_18362_omits_the_virtual_mode_flag as fn(),
                ),
                (
                    "the_running_build_is_read",
                    the_running_build_is_read as fn(),
                ),
            ],
        );
    }
}
