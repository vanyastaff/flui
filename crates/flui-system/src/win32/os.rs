//! What the system is, over `RtlGetVersion`, which reports the real build
//! whatever the executable's manifest declares, and `GetNativeSystemInfo`,
//! which reports the machine's architecture, not an emulated process's.
//! Neither reads a file, so both answer on the owner thread.

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::System::SystemInformation::{
    GetNativeSystemInfo, OSVERSIONINFOW, PROCESSOR_ARCHITECTURE, PROCESSOR_ARCHITECTURE_AMD64,
    PROCESSOR_ARCHITECTURE_ARM, PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_INTEL,
    SYSTEM_INFO,
};

use crate::os::{Architecture, OsBackend, OsInfo, OsVersion};

/// The system's name for its version: Windows 11 from build 22000.
pub(super) fn os_name(version: OsVersion) -> &'static str {
    if version.major == 10 && version.patch >= 22_000 {
        "Windows 11"
    } else if version.major == 10 {
        "Windows 10"
    } else {
        "Windows"
    }
}

/// The architecture `GetNativeSystemInfo` names.
pub(super) fn architecture(named: PROCESSOR_ARCHITECTURE) -> Architecture {
    match named {
        PROCESSOR_ARCHITECTURE_AMD64 => Architecture::X86_64,
        PROCESSOR_ARCHITECTURE_ARM64 => Architecture::Aarch64,
        PROCESSOR_ARCHITECTURE_INTEL => Architecture::X86,
        PROCESSOR_ARCHITECTURE_ARM => Architecture::Arm,
        _ => Architecture::Other,
    }
}

/// [`OsBackend`] over the version and system information calls.
pub(crate) struct WindowsOs;

impl OsBackend for WindowsOs {
    fn info(&self) -> OsInfo {
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: u32::try_from(std::mem::size_of::<OSVERSIONINFOW>())
                .expect("BUG: OSVERSIONINFOW is a few hundred bytes"),
            ..Default::default()
        };
        // SAFETY: `version` is a live out-parameter whose size field matches
        // its type; the call fills it and reads nothing else of ours.
        let _ = unsafe { RtlGetVersion(&raw mut version) };
        let version = OsVersion {
            major: version.dwMajorVersion,
            minor: version.dwMinorVersion,
            patch: version.dwBuildNumber,
        };
        let mut system = SYSTEM_INFO::default();
        // SAFETY: `system` is a live out-parameter the call fills.
        unsafe { GetNativeSystemInfo(&raw mut system) };
        // SAFETY: the union's processor-architecture view is the one
        // `GetNativeSystemInfo` fills.
        let named = unsafe { system.Anonymous.Anonymous.wProcessorArchitecture };
        OsInfo::new(os_name(version), version, architecture(named))
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::SystemInformation::{
        PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_IA64,
    };

    use super::{WindowsOs, architecture, os_name};
    use crate::os::{Architecture, OsBackend, OsVersion};

    #[test]
    fn the_system_names_its_version_by_build_and_its_machine_by_architecture() {
        let windows = |patch| OsVersion {
            major: 10,
            minor: 0,
            patch,
        };
        assert_eq!(os_name(windows(19_045)), "Windows 10");
        assert_eq!(os_name(windows(22_000)), "Windows 11");
        assert_eq!(os_name(windows(26_100)), "Windows 11");
        assert_eq!(
            architecture(PROCESSOR_ARCHITECTURE_AMD64),
            Architecture::X86_64
        );
        assert_eq!(
            architecture(PROCESSOR_ARCHITECTURE_ARM64),
            Architecture::Aarch64
        );
        assert_eq!(
            architecture(PROCESSOR_ARCHITECTURE_IA64),
            Architecture::Other
        );
        // This machine's own answer: a Windows 10 kernel or later.
        let info = WindowsOs.info();
        assert_eq!(info.version.major, 10, "{info:?}");
        assert!(info.name.starts_with("Windows"));
    }
}
