//! What the system is: its name as Apple writes it (macOS; iOS or iPadOS,
//! as the device says), at the version `NSProcessInfo` reports, on the
//! processor the machine has. A process Rosetta translates (an x86-64 build
//! on an Arm Mac, macOS's own or the iOS Simulator's) runs on Arm64, which
//! `sysctl.proc_translated` says.

use crate::os::{Architecture, OsInfo, OsVersion};

/// What the system is: `name` at `version`, for a process built for
/// `built_for` that Rosetta translates when `translated`.
pub(super) fn info(
    name: &str,
    (major, minor, patch): (u32, u32, u32),
    built_for: Architecture,
    translated: bool,
) -> OsInfo {
    let architecture = if translated {
        Architecture::Aarch64
    } else {
        built_for
    };
    OsInfo::new(
        name,
        OsVersion {
            major,
            minor,
            patch,
        },
        architecture,
    )
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) use darwin::AppleOs;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[expect(unsafe_code, reason = "sysctlbyname, for the translation flag")]
mod darwin {
    use super::info;
    use crate::os::{Architecture, OsBackend, OsInfo};

    /// [`OsBackend`] over the name and the version a UI runtime read.
    pub(crate) struct AppleOs {
        name: String,
        version: (u32, u32, u32),
    }

    impl AppleOs {
        /// The system called `name` at `version`, as `(major, minor,
        /// patch)`.
        pub(crate) fn new(name: impl Into<String>, version: (u32, u32, u32)) -> Self {
            Self {
                name: name.into(),
                version,
            }
        }
    }

    /// Whether Rosetta translates this process; `false` where the system
    /// has no such flag (an Intel Mac, an iPhone).
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

    impl OsBackend for AppleOs {
        fn info(&self) -> OsInfo {
            let built_for = if cfg!(target_arch = "aarch64") {
                Architecture::Aarch64
            } else {
                Architecture::X86_64
            };
            info(&self.name, self.version, built_for, translated())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::info;
    use crate::os::{Architecture, OsVersion};

    #[test]
    fn the_system_is_its_name_at_its_version_on_the_machines_processor() {
        let native = info("macOS", (15, 4, 1), Architecture::Aarch64, false);
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
        let intel = info("macOS", (13, 0, 0), Architecture::X86_64, false);
        assert_eq!(intel.architecture, Architecture::X86_64);
        let rosetta = info("macOS", (14, 0, 0), Architecture::X86_64, true);
        assert_eq!(
            rosetta.architecture,
            Architecture::Aarch64,
            "a translated process runs on an Arm Mac"
        );
        let tablet = info("iPadOS", (18, 2, 0), Architecture::Aarch64, false);
        assert_eq!(tablet.name, "iPadOS", "the name is the one the device says");
        let simulator = info("iOS", (26, 0, 0), Architecture::X86_64, true);
        assert_eq!(
            simulator.architecture,
            Architecture::Aarch64,
            "an x86-64 Simulator build Rosetta translates runs on an Arm Mac"
        );
    }
}
