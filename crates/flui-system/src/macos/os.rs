//! What the system is: macOS, at the version `NSProcessInfo` reports, on the
//! processor the machine has. A process Rosetta translates runs as x86-64
//! on an Arm Mac, which `sysctl.proc_translated` says.

use crate::os::{Architecture, OsInfo, OsVersion};

use super::version::MacOSVersion;

/// What the system is, for a process built for `built_for` that Rosetta
/// translates when `translated`.
pub(super) fn info(version: MacOSVersion, built_for: Architecture, translated: bool) -> OsInfo {
    let (major, minor, patch) = version.components();
    let architecture = if translated {
        Architecture::Aarch64
    } else {
        built_for
    };
    OsInfo::new(
        "macOS",
        OsVersion {
            major,
            minor,
            patch,
        },
        architecture,
    )
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::MacOs;

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "sysctlbyname, for the translation flag")]
mod appkit {
    use super::super::version::MacOSVersion;
    use super::info;
    use crate::os::{Architecture, OsBackend, OsInfo};

    /// [`OsBackend`] over the version a UI runtime read.
    pub(crate) struct MacOs {
        version: MacOSVersion,
    }

    impl MacOs {
        pub(crate) fn new(version: MacOSVersion) -> Self {
            Self { version }
        }
    }

    /// Whether Rosetta translates this process; `false` where the system
    /// has no such flag (an Intel Mac).
    fn translated() -> bool {
        let mut value: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        // SAFETY: the name is NUL-terminated, `value` and `size` outlive the
        // call and `size` holds `value`'s size; nothing is written.
        let status = unsafe {
            libc::sysctlbyname(
                c"sysctl.proc_translated".as_ptr(),
                (&raw mut value).cast::<libc::c_void>(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        status == 0 && value == 1
    }

    impl OsBackend for MacOs {
        fn info(&self) -> OsInfo {
            let built_for = if cfg!(target_arch = "aarch64") {
                Architecture::Aarch64
            } else {
                Architecture::X86_64
            };
            info(self.version, built_for, translated())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::version::MacOSVersion;
    use super::info;
    use crate::os::{Architecture, OsVersion};

    #[test]
    fn the_system_is_macos_at_its_version_on_the_machines_processor() {
        let native = info(MacOSVersion::new(15, 4, 1), Architecture::Aarch64, false);
        assert_eq!(native.name, "macOS");
        assert_eq!(
            native.version,
            OsVersion {
                major: 15,
                minor: 4,
                patch: 1
            }
        );
        assert_eq!(native.architecture, Architecture::Aarch64);
        let intel = info(MacOSVersion::new(13, 0, 0), Architecture::X86_64, false);
        assert_eq!(intel.architecture, Architecture::X86_64);
        let rosetta = info(MacOSVersion::new(14, 0, 0), Architecture::X86_64, true);
        assert_eq!(
            rosetta.architecture,
            Architecture::Aarch64,
            "a translated process runs on an Arm Mac"
        );
    }
}
