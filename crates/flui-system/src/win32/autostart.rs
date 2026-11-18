//! Autostart over the current user's `Run` key, as the system's own Startup
//! apps settings read it.
//!
//! The entry is a `REG_SZ` value of
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` holding the running
//! executable's quoted path. Its name is the executable's stem and a hash of
//! its full path ([`entry_name`]), so two applications that happen to share
//! a file name never overwrite each other's entry, and an application moved
//! elsewhere reads as not enabled until it enables again. The user turning
//! the entry off in Settings or Task Manager is recorded beside it, under
//! `…\Explorer\StartupApproved\Run`, as a value whose first byte is odd;
//! that record is the user's and never overwritten, so a request to enable
//! an entry the user turned off answers [`AutostartState::Disabled`]: what
//! the system grants. A request's reason is not shown: the `Run` key asks
//! nobody. A packaged application's start-up task is its manifest's, so a
//! packaged process is refused.
//!
//! Each request runs on its own thread ([`ShellThreads`]).

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::{
    APPMODEL_ERROR_NO_PACKAGE, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA,
    WIN32_ERROR,
};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::{PCWSTR, w};

use super::{ShellThreads, wide};
use crate::autostart::{
    AutostartBackend, AutostartCapability, AutostartError, AutostartRequest, AutostartResult,
    AutostartState,
};

/// The current user's start-up entries as the provider reaches them, on a
/// request's thread. Each failure is the Win32 error the call reported.
pub(super) trait LoginRegistry: Send + Sync + 'static {
    /// Whether the process runs packaged.
    fn packaged(&self) -> bool;

    /// The running executable's path.
    fn executable(&self) -> Result<PathBuf, i32>;

    /// The `Run` value `name`, if there is one, as the registry holds it.
    fn run_value(&self, name: &str) -> Result<Option<OsString>, i32>;

    /// Write the `Run` value `name`.
    fn set_run_value(&self, name: &str, command: &OsStr) -> Result<(), i32>;

    /// Remove the `Run` value `name`; one that is not there is removed.
    fn delete_run_value(&self, name: &str) -> Result<(), i32>;

    /// Whether the user left the entry `name` on: `Some(false)` when they
    /// turned it off, `None` when the system recorded nothing.
    fn approved(&self, name: &str) -> Result<Option<bool>, i32>;
}

/// The entry name of `executable`: its stem, then 16 hex digits of the
/// FNV-1a hash of its path, both in lower case, since Windows names one file
/// whatever the case of its path.
pub(super) fn entry_name(executable: &Path) -> String {
    let stem = executable
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().to_lowercase());
    let lower = executable.to_string_lossy().to_lowercase();
    let hash = lower.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{stem}-{hash:016x}")
}

/// The command the entry runs: the executable's path, quoted, with no
/// arguments, unit for unit (no lossy conversion, so an entry is compared
/// with exactly what was written).
pub(super) fn command(executable: &Path) -> OsString {
    let mut command = OsString::from("\"");
    command.push(executable.as_os_str());
    command.push("\"");
    command
}

/// The error a registry call's `code` means.
fn autostart_error(code: i32) -> AutostartError {
    if i32::try_from(ERROR_ACCESS_DENIED.0).is_ok_and(|denied| denied == code) {
        AutostartError::Denied
    } else {
        AutostartError::Failed(SystemFailure::Os(code))
    }
}

/// Do what `request` asks of `registry` and answer the state after it.
pub(super) fn answer(registry: &dyn LoginRegistry, request: &AutostartRequest) -> AutostartResult {
    if registry.packaged() {
        return Err(AutostartError::Unsupported(Unsupported::of::<
            AutostartCapability,
        >(
            UnsupportedReason::NotYetOnThisHost,
        )));
    }
    let executable = registry.executable().map_err(autostart_error)?;
    let name = entry_name(&executable);
    let command = command(&executable);
    if request.is_enabled() {
        registry
            .set_run_value(&name, &command)
            .map_err(autostart_error)?;
    } else {
        registry.delete_run_value(&name).map_err(autostart_error)?;
    }
    match registry.run_value(&name).map_err(autostart_error)? {
        Some(value) if value == command => {
            match registry.approved(&name).map_err(autostart_error)? {
                // The user turned it off: the system does not start it.
                Some(false) => Ok(AutostartState::Disabled),
                Some(true) | None => Ok(AutostartState::Enabled),
            }
        }
        Some(_) | None => Ok(AutostartState::Disabled),
    }
}

/// [`AutostartBackend`] over the `Run` key.
pub(crate) struct WindowsAutostart {
    registry: Arc<dyn LoginRegistry>,
    threads: Rc<ShellThreads>,
}

impl WindowsAutostart {
    pub(crate) fn new(threads: Rc<ShellThreads>) -> Self {
        Self::over(Arc::new(Registry), threads)
    }

    pub(super) fn over(registry: Arc<dyn LoginRegistry>, threads: Rc<ShellThreads>) -> Self {
        Self { registry, threads }
    }
}

impl AutostartBackend for WindowsAutostart {
    fn request(&self, request: AutostartRequest, answer_to: Resolver<AutostartResult>) {
        let registry = Arc::clone(&self.registry);
        self.threads.answer(
            "flui-autostart",
            answer_to,
            || {
                Err(AutostartError::Unsupported(Unsupported::of::<
                    AutostartCapability,
                >(
                    UnsupportedReason::Unavailable,
                )))
            },
            move || answer(&*registry, &request),
        );
    }
}

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const APPROVED: PCWSTR =
    w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");

/// The real registry.
struct Registry;

fn code(error: WIN32_ERROR) -> i32 {
    i32::try_from(error.0).unwrap_or(i32::MAX)
}

impl LoginRegistry for Registry {
    fn packaged(&self) -> bool {
        let mut length = 0_u32;
        // SAFETY: asks only for the length, into a live out-parameter; a
        // process with no package answers `APPMODEL_ERROR_NO_PACKAGE`.
        let answered = unsafe { GetCurrentPackageFullName(&raw mut length, None) };
        answered != APPMODEL_ERROR_NO_PACKAGE
    }

    fn executable(&self) -> Result<PathBuf, i32> {
        std::env::current_exe().map_err(|error| error.raw_os_error().unwrap_or(i32::MAX))
    }

    fn run_value(&self, name: &str) -> Result<Option<OsString>, i32> {
        // The longest value read: a command line is at most 32 767 units.
        const MAX_UNITS: usize = 32 * 1024;
        let name = wide(name);
        let mut buffer = vec![0_u16; 1024];
        loop {
            let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
            // SAFETY: `buffer` is a live buffer of `bytes` bytes; the call
            // writes a NUL-terminated string into it and its size back.
            let read = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    RUN,
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&raw mut bytes),
                )
            };
            if read == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if read == ERROR_MORE_DATA {
                // The size the registry reports is checked before anything
                // is allocated for it: a value longer than a command line
                // is not this entry's.
                let needed = usize::try_from(bytes).unwrap_or(usize::MAX) / 2 + 1;
                if needed > MAX_UNITS || needed <= buffer.len() {
                    return Err(code(ERROR_MORE_DATA));
                }
                buffer.resize(needed, 0);
                continue;
            }
            if read.is_err() {
                return Err(code(read));
            }
            let units = usize::try_from(bytes).unwrap_or(0) / 2;
            let text = buffer.get(..units).unwrap_or_default();
            let text = text.split(|unit| *unit == 0).next().unwrap_or_default();
            return Ok(Some(OsString::from_wide(text)));
        }
    }

    fn set_run_value(&self, name: &str, command: &OsStr) -> Result<(), i32> {
        let name = wide(name);
        let data = wide(command);
        let bytes = u32::try_from(data.len() * 2).map_err(|_| code(ERROR_MORE_DATA))?;
        // SAFETY: `name` and `data` are NUL-terminated buffers that outlive
        // the call; `bytes` is `data`'s size, its NUL included, as `REG_SZ`
        // asks.
        let written = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN,
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                bytes,
            )
        };
        if written.is_err() {
            return Err(code(written));
        }
        Ok(())
    }

    fn delete_run_value(&self, name: &str) -> Result<(), i32> {
        let name = wide(name);
        // SAFETY: `name` is a NUL-terminated buffer that outlives the call.
        let deleted = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, PCWSTR(name.as_ptr())) };
        if deleted.is_err() && deleted != ERROR_FILE_NOT_FOUND {
            return Err(code(deleted));
        }
        Ok(())
    }

    fn approved(&self, name: &str) -> Result<Option<bool>, i32> {
        let name = wide(name);
        let mut value = [0_u8; 16];
        let mut bytes = u32::try_from(value.len()).unwrap_or(0);
        // SAFETY: `value` is a live buffer of `bytes` bytes.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                APPROVED,
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_BINARY,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&raw mut bytes),
            )
        };
        if read == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if read.is_err() {
            return Err(code(read));
        }
        // An even first byte (2, 6) is on, an odd one (3, 7) off.
        Ok((bytes > 0).then(|| value[0] % 2 == 0))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use flui_platform::{SystemFailure, UnsupportedReason};

    use super::{LoginRegistry, answer, command, entry_name};
    use crate::autostart::{AutostartError, AutostartRequest, AutostartState};

    /// A registry held in memory.
    struct MemoryRegistry {
        packaged: bool,
        executable: PathBuf,
        run: Mutex<BTreeMap<String, OsString>>,
        approved: Mutex<BTreeMap<String, bool>>,
        write_error: Option<i32>,
    }

    impl MemoryRegistry {
        fn new() -> Self {
            Self {
                packaged: false,
                executable: PathBuf::from(r"C:\Apps\Notes\notes.exe"),
                run: Mutex::new(BTreeMap::new()),
                approved: Mutex::new(BTreeMap::new()),
                write_error: None,
            }
        }
    }

    impl LoginRegistry for MemoryRegistry {
        fn packaged(&self) -> bool {
            self.packaged
        }

        fn executable(&self) -> Result<PathBuf, i32> {
            Ok(self.executable.clone())
        }

        fn run_value(&self, name: &str) -> Result<Option<OsString>, i32> {
            Ok(self
                .run
                .lock()
                .expect("BUG: the test's lock")
                .get(name)
                .cloned())
        }

        fn set_run_value(&self, name: &str, command: &OsStr) -> Result<(), i32> {
            if let Some(code) = self.write_error {
                return Err(code);
            }
            self.run
                .lock()
                .expect("BUG: the test's lock")
                .insert(name.to_owned(), command.to_os_string());
            Ok(())
        }

        fn delete_run_value(&self, name: &str) -> Result<(), i32> {
            self.run.lock().expect("BUG: the test's lock").remove(name);
            Ok(())
        }

        fn approved(&self, name: &str) -> Result<Option<bool>, i32> {
            Ok(self
                .approved
                .lock()
                .expect("BUG: the test's lock")
                .get(name)
                .copied())
        }
    }

    #[test]
    fn enabling_writes_the_quoted_executable_and_the_users_choice_stands() {
        let registry = MemoryRegistry::new();
        assert_eq!(
            answer(
                &registry,
                &AutostartRequest::enable().reason("Sync at login")
            ),
            Ok(AutostartState::Enabled)
        );
        let name = entry_name(&registry.executable);
        assert_eq!(
            registry.run_value(&name),
            Ok(Some(OsString::from(r#""C:\Apps\Notes\notes.exe""#)))
        );

        registry
            .approved
            .lock()
            .expect("BUG: the test's lock")
            .insert(name.clone(), false);
        assert_eq!(
            answer(&registry, &AutostartRequest::enable()),
            Ok(AutostartState::Disabled),
            "the user turned it off: enabling again does not override them"
        );
        assert_eq!(
            registry.approved(&name),
            Ok(Some(false)),
            "the user's record is never written"
        );

        assert_eq!(
            answer(&registry, &AutostartRequest::disable()),
            Ok(AutostartState::Disabled)
        );
        assert_eq!(registry.run_value(&name), Ok(None));
    }

    #[test]
    fn an_entry_whose_write_did_not_take_answers_disabled() {
        // A registry whose read hands back another executable's command (a
        // policy rewriting the value): the entry is not this one's.
        struct Rewriting(MemoryRegistry);
        impl LoginRegistry for Rewriting {
            fn packaged(&self) -> bool {
                false
            }
            fn executable(&self) -> Result<PathBuf, i32> {
                self.0.executable()
            }
            fn run_value(&self, _name: &str) -> Result<Option<OsString>, i32> {
                Ok(Some(command(Path::new(r"C:\Old\notes.exe"))))
            }
            fn set_run_value(&self, name: &str, command: &OsStr) -> Result<(), i32> {
                self.0.set_run_value(name, command)
            }
            fn delete_run_value(&self, name: &str) -> Result<(), i32> {
                self.0.delete_run_value(name)
            }
            fn approved(&self, name: &str) -> Result<Option<bool>, i32> {
                self.0.approved(name)
            }
        }
        assert_eq!(
            answer(
                &Rewriting(MemoryRegistry::new()),
                &AutostartRequest::enable()
            ),
            Ok(AutostartState::Disabled)
        );
    }

    #[test]
    fn a_packaged_process_and_a_refused_write_answer_their_errors() {
        let packaged = MemoryRegistry {
            packaged: true,
            ..MemoryRegistry::new()
        };
        let refusal = answer(&packaged, &AutostartRequest::enable()).expect_err("packaged");
        assert!(matches!(
            refusal,
            AutostartError::Unsupported(unsupported)
                if unsupported.reason == UnsupportedReason::NotYetOnThisHost
        ));

        let denied = MemoryRegistry {
            write_error: Some(5),
            ..MemoryRegistry::new()
        };
        assert_eq!(
            answer(&denied, &AutostartRequest::enable()),
            Err(AutostartError::Denied)
        );
        let failed = MemoryRegistry {
            write_error: Some(1009),
            ..MemoryRegistry::new()
        };
        assert_eq!(
            answer(&failed, &AutostartRequest::enable()),
            Err(AutostartError::Failed(SystemFailure::Os(1009)))
        );
    }

    #[test]
    fn two_executables_of_one_name_have_two_entries() {
        let one = entry_name(Path::new(r"C:\A\app.exe"));
        let other = entry_name(Path::new(r"C:\B\app.exe"));
        assert!(one.starts_with("app-") && other.starts_with("app-"));
        assert_ne!(one, other);
        assert_eq!(
            one,
            entry_name(Path::new(r"c:\a\APP.exe")),
            "a path differing only in case is the same file on Windows"
        );
    }

    #[test]
    fn an_autostart_request_is_answered_from_a_shell_thread_through_the_handle() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::Arc;

        use flui_platform::capability::Abandoned;

        use crate::autostart::{Autostart, AutostartResult};
        use crate::testing::win32::TestOwner;
        use crate::win32::ShellThreads;

        let owner = TestOwner::new();
        let ctx = owner.ctx(None);
        let registry = Arc::new(MemoryRegistry::new());
        let autostart = Autostart::new(
            super::WindowsAutostart::over(
                Arc::clone(&registry) as Arc<dyn LoginRegistry>,
                Rc::new(ShellThreads::default()),
            ),
            &ctx,
        );
        let landed: Rc<RefCell<Option<Result<AutostartResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&landed);
        autostart
            .request(AutostartRequest::enable())
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        assert!(landed.borrow().is_none(), "never inside the call");
        owner.turn_until(|| landed.borrow().is_some());
        assert_eq!(*landed.borrow(), Some(Ok(Ok(AutostartState::Enabled))));
        assert!(
            registry
                .run_value(&entry_name(&registry.executable))
                .is_ok_and(|value| value.is_some())
        );
    }
}
