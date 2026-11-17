//! The user's folders over `SHGetKnownFolderPath`, each found on its own
//! thread ([`ShellThreads`]), since a redirected folder may reach the
//! network; what the system is over `RtlGetVersion`, which reports the real
//! build whatever the executable's manifest declares, and
//! `GetNativeSystemInfo`, which reports the machine's architecture, not an
//! emulated process's.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::{
    GetNativeSystemInfo, OSVERSIONINFOW, PROCESSOR_ARCHITECTURE, PROCESSOR_ARCHITECTURE_AMD64,
    PROCESSOR_ARCHITECTURE_ARM, PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_INTEL,
    SYSTEM_INFO,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
    FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_RoamingAppData, FOLDERID_Videos,
    KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::GUID;

use super::{ShellThreads, os_code};
use crate::os::{
    Architecture, FolderError, FolderResult, KnownFolder, OsBackend, OsCapability, OsInfo,
    OsVersion,
};

/// The known folder id of `folder`.
pub(super) fn folder_id(folder: KnownFolder) -> GUID {
    match folder {
        KnownFolder::Home => FOLDERID_Profile,
        KnownFolder::Desktop => FOLDERID_Desktop,
        KnownFolder::Documents => FOLDERID_Documents,
        KnownFolder::Downloads => FOLDERID_Downloads,
        KnownFolder::Pictures => FOLDERID_Pictures,
        KnownFolder::Music => FOLDERID_Music,
        KnownFolder::Videos => FOLDERID_Videos,
        KnownFolder::Config => FOLDERID_RoamingAppData,
        KnownFolder::Data | KnownFolder::Cache => FOLDERID_LocalAppData,
    }
}

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

/// What finding a folder answered, as its code says: a folder the user does
/// not have is [`FolderError::NotFound`].
pub(super) fn folder_answer(found: Result<PathBuf, i32>) -> FolderResult {
    found.map_err(|code| {
        if i32::try_from(ERROR_FILE_NOT_FOUND.0).is_ok_and(|missing| missing == code) {
            FolderError::NotFound
        } else {
            FolderError::Failed(SystemFailure::Os(code))
        }
    })
}

/// [`OsBackend`] over the shell's known folders.
pub(crate) struct WindowsOs {
    threads: Rc<ShellThreads>,
}

impl WindowsOs {
    pub(crate) fn new(threads: Rc<ShellThreads>) -> Self {
        Self { threads }
    }
}

/// The path of the known folder `id`.
fn known_folder(id: GUID) -> Result<PathBuf, i32> {
    // SAFETY: `id` outlives the call; the string it returns is the caller's
    // to free with `CoTaskMemFree`, which happens once, after its code units
    // were copied out.
    unsafe {
        let path = SHGetKnownFolderPath(&raw const id, KF_FLAG_DEFAULT, None)
            .map_err(|error| os_code(error.code()))?;
        let owned = OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.as_ptr().cast_const().cast()));
        Ok(PathBuf::from(owned))
    }
}

impl OsBackend for WindowsOs {
    fn folder(&self, folder: KnownFolder, answer: Resolver<FolderResult>) {
        let id = folder_id(folder);
        self.threads.answer(
            "flui-known-folder",
            answer,
            || {
                Err(FolderError::Unsupported(Unsupported::of::<OsCapability>(
                    UnsupportedReason::Unavailable,
                )))
            },
            move || folder_answer(known_folder(id)),
        );
    }

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
    use std::path::PathBuf;

    use flui_platform::SystemFailure;
    use windows::Win32::System::SystemInformation::{
        PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_IA64,
    };

    use super::{architecture, folder_answer, os_name};
    use crate::os::{Architecture, FolderError, OsVersion};

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
        assert_eq!(folder_answer(Err(2)), Err(FolderError::NotFound));
        assert_eq!(
            folder_answer(Err(5)),
            Err(FolderError::Failed(SystemFailure::Os(5)))
        );
        assert_eq!(
            folder_answer(Ok(PathBuf::from(r"C:\Users\a"))),
            Ok(PathBuf::from(r"C:\Users\a"))
        );
    }
}
