//! What Android is: its version and the device's processor, from the
//! system properties (`ro.build.version.release`, `ro.product.cpu.abi`),
//! which the process reads from memory.

use crate::os::{Architecture, OsVersion};

/// The version a release string names: its leading dot-separated numbers
/// (`15`, `8.1.0`). A release with no number (a preview's code name) has no
/// version to state, and answers `0.0.0`.
pub(crate) fn version_of(release: &str) -> OsVersion {
    let mut parts = release
        .split('.')
        .map_while(|part| part.trim().parse::<u32>().ok());
    OsVersion {
        major: parts.next().unwrap_or(0),
        minor: parts.next().unwrap_or(0),
        patch: parts.next().unwrap_or(0),
    }
}

/// The architecture an Android ABI name is of.
pub(crate) fn architecture_of(abi: &str) -> Architecture {
    match abi {
        "arm64-v8a" => Architecture::Aarch64,
        "armeabi-v7a" | "armeabi" => Architecture::Arm,
        "x86_64" => Architecture::X86_64,
        "x86" => Architecture::X86,
        _ => Architecture::Other,
    }
}

#[cfg(target_os = "android")]
pub(crate) use provider::AndroidOs;

#[cfg(target_os = "android")]
#[expect(unsafe_code, reason = "bionic's system property read")]
mod provider {
    use std::ffi::CStr;

    use super::{architecture_of, version_of};
    use crate::os::{OsBackend, OsInfo};

    /// The size of the buffer `__system_property_get` fills
    /// (`PROP_VALUE_MAX`).
    const PROPERTY_VALUE_MAX: usize = 92;

    /// The value of the system property `name`, empty when it is not set.
    fn property(name: &CStr) -> String {
        let mut value = [0_u8; PROPERTY_VALUE_MAX];
        // SAFETY: `name` is NUL-terminated and `value` holds
        // `PROP_VALUE_MAX` bytes, the most the call writes, its NUL
        // included.
        let length =
            unsafe { libc::__system_property_get(name.as_ptr(), value.as_mut_ptr().cast()) };
        let length = usize::try_from(length).unwrap_or(0).min(value.len());
        String::from_utf8_lossy(&value[..length]).into_owned()
    }

    /// [`OsBackend`] over the system properties, read once per acquisition.
    pub(crate) struct AndroidOs {
        info: OsInfo,
    }

    impl AndroidOs {
        /// What the system's properties say it is.
        pub(crate) fn read() -> Self {
            Self {
                info: OsInfo::new(
                    "Android",
                    version_of(&property(c"ro.build.version.release")),
                    architecture_of(&property(c"ro.product.cpu.abi")),
                ),
            }
        }
    }

    impl OsBackend for AndroidOs {
        fn info(&self) -> OsInfo {
            self.info.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{architecture_of, version_of};
    use crate::os::{Architecture, OsVersion};

    #[test]
    fn a_release_is_its_leading_numbers_and_an_abi_its_architecture() {
        let version = |major, minor, patch| OsVersion {
            major,
            minor,
            patch,
        };
        for (release, expected) in [
            ("15", version(15, 0, 0)),
            ("8.1.0", version(8, 1, 0)),
            ("6.0.1", version(6, 0, 1)),
            ("12L", version(0, 0, 0)),
            ("14.beta", version(14, 0, 0)),
            ("VanillaIceCream", version(0, 0, 0)),
            ("", version(0, 0, 0)),
        ] {
            assert_eq!(version_of(release), expected, "{release:?}");
        }
        for (abi, expected) in [
            ("arm64-v8a", Architecture::Aarch64),
            ("armeabi-v7a", Architecture::Arm),
            ("x86_64", Architecture::X86_64),
            ("x86", Architecture::X86),
            ("riscv64", Architecture::Other),
            ("", Architecture::Other),
        ] {
            assert_eq!(architecture_of(abi), expected, "{abi:?}");
        }
    }
}
