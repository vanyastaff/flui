//! The Windows build this process runs on, and the proofs it unlocks.
//!
//! The build is read once, when the display source is built, and travels with
//! the platform instance. A flag that only newer builds accept cannot be built
//! without the proof [`WindowsBuild::virtual_mode_aware`] hands out, so no call
//! site compares a bare number.

use windows::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Devices::Display::{
            QDC_ONLY_ACTIVE_PATHS, QDC_VIRTUAL_MODE_AWARE, QUERY_DISPLAY_CONFIG_FLAGS,
        },
        System::SystemInformation::OSVERSIONINFOW,
    },
};

/// The first build that accepts `QDC_VIRTUAL_MODE_AWARE` (Windows 10 1903).
const VIRTUAL_MODE_AWARE_BUILD: u32 = 18362;

/// The OS build number (`dwBuildNumber`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowsBuild(u32);

impl WindowsBuild {
    /// Read the running build with `RtlGetVersion`, which, unlike
    /// `GetVersionExW`, does not report the manifest's compatibility version.
    /// A failed read is build 0, which unlocks nothing.
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
            return Self(0);
        }
        Self(info.dwBuildNumber)
    }

    #[cfg(test)]
    pub(crate) const fn from_number(build: u32) -> Self {
        Self(build)
    }

    /// The proof that `QDC_VIRTUAL_MODE_AWARE` is accepted, or `None` below
    /// build 18362.
    pub(crate) const fn virtual_mode_aware(self) -> Option<VirtualModeAware> {
        if self.0 >= VIRTUAL_MODE_AWARE_BUILD {
            Some(VirtualModeAware(()))
        } else {
            None
        }
    }

    /// The `QueryDisplayConfig` flags for the active paths on this build.
    pub(crate) fn display_config_flags(self) -> QUERY_DISPLAY_CONFIG_FLAGS {
        match self.virtual_mode_aware() {
            Some(proof) => QDC_ONLY_ACTIVE_PATHS | QUERY_DISPLAY_CONFIG_FLAGS::from(proof),
            None => QDC_ONLY_ACTIVE_PATHS,
        }
    }
}

/// Evidence that this build accepts `QDC_VIRTUAL_MODE_AWARE`. Only
/// [`WindowsBuild::virtual_mode_aware`] makes one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VirtualModeAware(());

impl From<VirtualModeAware> for QUERY_DISPLAY_CONFIG_FLAGS {
    fn from(_proof: VirtualModeAware) -> Self {
        QDC_VIRTUAL_MODE_AWARE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_build_below_18362_omits_the_virtual_mode_flag() {
        let old = WindowsBuild::from_number(18361);
        assert!(old.virtual_mode_aware().is_none());
        assert_eq!(old.display_config_flags(), QDC_ONLY_ACTIVE_PATHS);
        assert_eq!(
            WindowsBuild::from_number(0).display_config_flags(),
            QDC_ONLY_ACTIVE_PATHS,
            "an unknown build unlocks nothing"
        );
        let new = WindowsBuild::from_number(18362);
        assert!(new.virtual_mode_aware().is_some());
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
