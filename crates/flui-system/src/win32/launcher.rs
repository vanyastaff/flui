//! The launcher over the shell: `ShellExecuteExW` with the `open` verb for a
//! URI (through its scheme's registered class) and a document,
//! `SHOpenFolderAndSelectItems` to reveal, and `IFileOperation` with
//! `FOFX_RECYCLEONDELETE` to trash. Never a command line: no string reaches
//! `cmd` or `explorer`.
//!
//! Each request runs on its own apartment thread ([`ShellThreads`]), since
//! every one may touch the file system or start a handler that takes its
//! time, and answers its own resolver from there. An open first opens the
//! file and judges what the handle holds (ADR-0360 §2): its final name
//! (`GetFinalPathNameByHandleW`, which follows every link the open
//! followed) against the floor, then the system's own `AssocIsDangerous`;
//! only a file both admit is handed to the shell, by that final name, while
//! the handle keeps it from being renamed or deleted.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::launch::{
    ExternalUri, LaunchError, LaunchResult, LauncherBackend, LauncherCapability, runs_when_opened,
};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_BAD_NETPATH, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
    ERROR_INVALID_DRIVE, ERROR_INVALID_NAME, ERROR_NO_ASSOCIATION, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FileBasicInfo,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
    OPEN_EXISTING, VOLUME_NAME_DOS,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::GetProcessId;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    AssocIsDangerous, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
    ILFree, SEE_MASK_CLASSNAME, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW, SHOpenFolderAndSelectItems, SHParseDisplayName, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, SW_SHOWNORMAL};
use windows::core::{PCWSTR, w};

use super::dialogs::shell_item;
use super::{Owner, ShellThreads, os_code, wide};

/// A file opened to be judged, and kept open while it is handed on.
pub(super) struct HeldFile {
    /// The name the handle resolves to: every link the open followed,
    /// followed.
    pub(super) final_path: PathBuf,
    /// Whether it is a folder, which is shown, not run.
    pub(super) directory: bool,
    /// What keeps the file open; dropped once it was handed on.
    pub(super) _handle: Box<dyn Send>,
}

/// What a trash operation did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trashed {
    /// The item is in the recycle bin.
    Done,
    /// The user or the system called it off (the user declined to delete
    /// for good an item the recycle bin cannot take).
    Aborted,
}

/// The shell as the launcher reaches it, on a request's apartment thread.
/// Each failure is the Win32 error or `HRESULT` the call reported.
pub(super) trait ShellSystem: Send + Sync + 'static {
    /// Open `path` to judge it.
    fn hold(&self, path: &Path) -> Result<HeldFile, i32>;

    /// Whether the system calls a file of this name dangerous to open
    /// (`AssocIsDangerous` on its extension).
    fn is_dangerous(&self, final_path: &Path) -> bool;

    /// Open `target` with the `open` verb; for a URI, `class` is its
    /// scheme, whose registered handler alone may take it.
    fn execute(&self, target: &OsStr, class: Option<&str>, owner: Option<Owner>)
    -> Result<(), i32>;

    /// Show `path` selected in its folder.
    fn reveal(&self, path: &Path) -> Result<(), i32>;

    /// Move `path` to the recycle bin.
    fn trash(&self, path: &Path, owner: Option<Owner>) -> Result<Trashed, i32>;
}

/// The launch error a shell call's `code` means.
fn launch_error(code: i32) -> LaunchError {
    let named = [
        (ERROR_FILE_NOT_FOUND, LaunchError::NotFound),
        (ERROR_PATH_NOT_FOUND, LaunchError::NotFound),
        (ERROR_INVALID_NAME, LaunchError::NotFound),
        (ERROR_INVALID_DRIVE, LaunchError::NotFound),
        (ERROR_BAD_NETPATH, LaunchError::NotFound),
        (ERROR_ACCESS_DENIED, LaunchError::Denied),
        (ERROR_SHARING_VIOLATION, LaunchError::Denied),
        (ERROR_CANCELLED, LaunchError::Denied),
        (ERROR_NO_ASSOCIATION, LaunchError::NoHandler),
    ];
    named
        .into_iter()
        .find(|(error, _)| i32::try_from(error.0).is_ok_and(|named| named == code))
        .map_or(
            LaunchError::Failed(SystemFailure::Os(code)),
            |(_, meaning)| meaning,
        )
}

/// Open the document at `path`: hold it, judge what the handle holds, and
/// hand the final name to the shell only when both the floor and the
/// system admit it.
pub(super) fn open_document(
    system: &dyn ShellSystem,
    path: &Path,
    owner: Option<Owner>,
) -> LaunchResult {
    let held = system.hold(path).map_err(launch_error)?;
    if !held.directory
        && (runs_when_opened(&held.final_path) || system.is_dangerous(&held.final_path))
    {
        return Err(LaunchError::NotADocument);
    }
    let opened = system
        .execute(held.final_path.as_os_str(), None, owner)
        .map_err(launch_error);
    // Released only after the shell took the file.
    drop(held);
    opened
}

/// Open `uri` through its scheme's registered handler.
pub(super) fn open_uri(
    system: &dyn ShellSystem,
    uri: &str,
    scheme: &str,
    owner: Option<Owner>,
) -> LaunchResult {
    system
        .execute(OsStr::new(uri), Some(scheme), owner)
        .map_err(launch_error)
}

/// Move `path` to the recycle bin; an operation called off is refused.
pub(super) fn trash(system: &dyn ShellSystem, path: &Path, owner: Option<Owner>) -> LaunchResult {
    match system.trash(path, owner).map_err(launch_error)? {
        Trashed::Done => Ok(()),
        Trashed::Aborted => Err(LaunchError::Denied),
    }
}

/// [`LauncherBackend`] over the Windows shell.
pub(crate) struct WindowsLauncher {
    system: Arc<dyn ShellSystem>,
    threads: Rc<ShellThreads>,
    /// The acquiring widget's window: what the shell's own prompts (a
    /// permanent delete) are owned by, while it is open.
    window: Option<WindowRef>,
}

impl WindowsLauncher {
    pub(crate) fn new(threads: Rc<ShellThreads>, window: Option<WindowRef>) -> Self {
        Self::over(Arc::new(Shell), threads, window)
    }

    pub(super) fn over(
        system: Arc<dyn ShellSystem>,
        threads: Rc<ShellThreads>,
        window: Option<WindowRef>,
    ) -> Self {
        Self {
            system,
            threads,
            window,
        }
    }

    /// The window the shell's prompts are owned by: the acquiring widget's,
    /// while it is open.
    fn owner(&self) -> Option<Owner> {
        self.window
            .as_ref()
            .filter(|window| !window.lifetime().is_closed())
            .and_then(Owner::of)
    }

    /// Run `work` on a thread of its own and answer `answer` with its
    /// result; refused for now when the runtime has its most threads
    /// running.
    fn run(
        &self,
        answer: Resolver<LaunchResult>,
        work: impl FnOnce(&dyn ShellSystem) -> LaunchResult + Send + 'static,
    ) {
        let system = Arc::clone(&self.system);
        self.threads.answer(
            "flui-shell",
            answer,
            || {
                Err(LaunchError::Unsupported(Unsupported::of::<
                    LauncherCapability,
                >(
                    UnsupportedReason::Unavailable
                )))
            },
            move || work(&*system),
        );
    }
}

impl LauncherBackend for WindowsLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        let owner = self.owner();
        let scheme = uri.scheme().to_ascii_lowercase();
        let uri = uri.as_str().to_owned();
        self.run(answer, move |system| open_uri(system, &uri, &scheme, owner));
    }

    fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        let owner = self.owner();
        self.run(answer, move |system| open_document(system, &path, owner));
    }

    fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.run(answer, move |system| {
            system.reveal(&path).map_err(launch_error)
        });
    }

    fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        let owner = self.owner();
        self.run(answer, move |system| trash(system, &path, owner));
    }
}

/// The real shell.
struct Shell;

/// Closes a file handle when dropped.
struct FileHandle(HANDLE);

// SAFETY: a file handle is a kernel object reference any thread may close;
// this one is only closed, once, by its drop.
unsafe impl Send for FileHandle {}

impl Drop for FileHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `Shell::hold` and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// `\\?\C:\…` as `C:\…`: the final name a handle reports carries the
/// verbatim prefix, which the shell does not need for a drive path.
fn without_verbatim(path: Vec<u16>) -> PathBuf {
    let path = OsString::from_wide(&path);
    let path = PathBuf::from(path);
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

impl ShellSystem for Shell {
    fn hold(&self, path: &Path) -> Result<HeldFile, i32> {
        let name = wide(path);
        // SAFETY: `name` is a NUL-terminated buffer that outlives the call;
        // the handle asks for attributes only, shares reading and writing
        // but not deleting (so the file cannot be renamed or removed while
        // it is held), and opens a folder too (`BACKUP_SEMANTICS`).
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .map_err(|error| os_code(error.code()))?;
        let handle = FileHandle(handle);
        let mut buffer = vec![0_u16; 512];
        let final_path = loop {
            // SAFETY: `buffer` is a live, writable buffer of the length the
            // call is given; the handle is open.
            let written = unsafe {
                GetFinalPathNameByHandleW(
                    handle.0,
                    &mut buffer,
                    GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
                )
            };
            let written = usize::try_from(written).unwrap_or(usize::MAX);
            if written == 0 {
                return Err(os_code(windows::core::Error::from_thread().code()));
            }
            if written < buffer.len() {
                buffer.truncate(written);
                break without_verbatim(buffer);
            }
            // Too small: `written` is the length needed, NUL included. A
            // path longer than the system's own limit is not one.
            if written > 32 * 1024 {
                return Err(i32::try_from(ERROR_INVALID_NAME.0).unwrap_or(i32::MAX));
            }
            buffer.resize(written, 0);
        };
        let mut info = FILE_BASIC_INFO::default();
        // SAFETY: `info` is a live out-parameter of the size given; the
        // handle is open with attribute access.
        unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileBasicInfo,
                std::ptr::from_mut(&mut info).cast(),
                u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>())
                    .expect("BUG: FILE_BASIC_INFO is a few dozen bytes"),
            )
        }
        .map_err(|error| os_code(error.code()))?;
        Ok(HeldFile {
            final_path,
            directory: info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
            _handle: Box::new(handle),
        })
    }

    fn is_dangerous(&self, final_path: &Path) -> bool {
        let Some(extension) = final_path.extension() else {
            return false;
        };
        let mut dotted = OsString::from(".");
        dotted.push(extension);
        let dotted = wide(dotted);
        // SAFETY: `dotted` is a NUL-terminated buffer that outlives the call.
        unsafe { AssocIsDangerous(PCWSTR(dotted.as_ptr())) }.as_bool()
    }

    fn execute(
        &self,
        target: &OsStr,
        class: Option<&str>,
        owner: Option<Owner>,
    ) -> Result<(), i32> {
        let target = wide(target);
        let class = class.map(wide);
        let mut mask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOCLOSEPROCESS;
        if class.is_some() {
            mask |= SEE_MASK_CLASSNAME;
        }
        let mut info = SHELLEXECUTEINFOW {
            cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())
                .expect("BUG: SHELLEXECUTEINFOW is a few hundred bytes"),
            fMask: mask,
            hwnd: owner.map_or_default(Owner::hwnd),
            lpVerb: w!("open"),
            lpFile: PCWSTR(target.as_ptr()),
            lpClass: class
                .as_ref()
                .map_or(PCWSTR::null(), |class| PCWSTR(class.as_ptr())),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // SAFETY: `info` is a live, initialised `SHELLEXECUTEINFOW` whose
        // `cbSize` matches its type; its strings are NUL-terminated buffers
        // that outlive the call (`NOASYNC` makes it finish before it
        // returns), and the shell writes only `hInstApp` and `hProcess`.
        let executed = unsafe { ShellExecuteExW(&raw mut info) };
        let process = info.hProcess;
        if !process.is_invalid() {
            // SAFETY: `process` is the handle the shell returned for the
            // handler it started, owned here and closed once.
            unsafe {
                let id = GetProcessId(process);
                // The handler the shell started may come to the foreground;
                // nothing is handed to a process the shell did not start.
                if executed.is_ok() && id != 0 {
                    let _ = AllowSetForegroundWindow(id);
                }
                let _ = CloseHandle(process);
            }
        }
        executed.map_err(|error| os_code(error.code()))
    }

    fn reveal(&self, path: &Path) -> Result<(), i32> {
        let name = wide(path);
        let mut item: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `name` outlives the call, `item` is a live out-parameter;
        // the list it returns is freed once below with `ILFree`.
        unsafe { SHParseDisplayName(PCWSTR(name.as_ptr()), None, &raw mut item, 0, None) }
            .map_err(|error| os_code(error.code()))?;
        // SAFETY: `item` is the absolute list parsed above: with no children
        // given, the shell opens its parent folder and selects it.
        let revealed = unsafe { SHOpenFolderAndSelectItems(item, None, 0) };
        // SAFETY: frees the list `SHParseDisplayName` allocated, once.
        unsafe { ILFree(Some(item.cast_const())) };
        revealed.map_err(|error| os_code(error.code()))
    }

    fn trash(&self, path: &Path, owner: Option<Owner>) -> Result<Trashed, i32> {
        let code = |error: windows::core::Error| os_code(error.code());
        // SAFETY: the operation is created and used on this apartment thread
        // only; every call is a checked method of it, each argument live for
        // its call.
        unsafe {
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(code)?;
            // Recycled, never deleted silently: an item the recycle bin
            // cannot take asks the user first (`WANTNUKEWARNING`).
            operation
                .SetOperationFlags(
                    FOF_ALLOWUNDO
                        | FOFX_RECYCLEONDELETE
                        | FOF_NOCONFIRMATION
                        | FOF_SILENT
                        | FOF_NOERRORUI
                        | FOF_WANTNUKEWARNING
                        | FOFX_EARLYFAILURE,
                )
                .map_err(code)?;
            if let Some(owner) = owner {
                operation.SetOwnerWindow(owner.hwnd()).map_err(code)?;
            }
            let item = shell_item(path).map_err(code)?;
            operation.DeleteItem(&item, None).map_err(code)?;
            operation.PerformOperations().map_err(code)?;
            if operation.GetAnyOperationsAborted().map_err(code)?.as_bool() {
                Ok(Trashed::Aborted)
            } else {
                Ok(Trashed::Done)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use flui_platform::SystemFailure;
    use flui_platform::launch::LaunchError;

    use super::{
        HeldFile, ShellSystem, Trashed, launch_error, open_document, open_uri, trash,
        without_verbatim,
    };
    use crate::win32::Owner;

    /// A shell whose file system holds one file under the name it resolves
    /// to, and which records what it was asked to execute.
    struct FakeShell {
        final_path: PathBuf,
        directory: bool,
        dangerous: bool,
        hold_error: Option<i32>,
        executed: Mutex<Vec<(String, Option<String>)>>,
        trashed: Result<Trashed, i32>,
    }

    impl FakeShell {
        fn holding(final_path: &str) -> Self {
            Self {
                final_path: PathBuf::from(final_path),
                directory: false,
                dangerous: false,
                hold_error: None,
                executed: Mutex::new(Vec::new()),
                trashed: Ok(Trashed::Done),
            }
        }

        fn executed(&self) -> Vec<(String, Option<String>)> {
            self.executed.lock().expect("BUG: the test's lock").clone()
        }
    }

    impl ShellSystem for FakeShell {
        fn hold(&self, _: &Path) -> Result<HeldFile, i32> {
            if let Some(code) = self.hold_error {
                return Err(code);
            }
            Ok(HeldFile {
                final_path: self.final_path.clone(),
                directory: self.directory,
                _handle: Box::new(()),
            })
        }

        fn is_dangerous(&self, _: &Path) -> bool {
            self.dangerous
        }

        fn execute(
            &self,
            target: &OsStr,
            class: Option<&str>,
            _: Option<Owner>,
        ) -> Result<(), i32> {
            self.executed.lock().expect("BUG: the test's lock").push((
                target.to_string_lossy().into_owned(),
                class.map(str::to_owned),
            ));
            Ok(())
        }

        fn reveal(&self, _: &Path) -> Result<(), i32> {
            Ok(())
        }

        fn trash(&self, _: &Path, _: Option<Owner>) -> Result<Trashed, i32> {
            self.trashed
        }
    }

    #[test]
    fn an_open_judges_the_file_the_handle_holds_not_the_path_it_was_given() {
        let asked = Path::new(r"C:\docs\report.pdf");

        let document = FakeShell::holding(r"C:\docs\report.pdf");
        assert_eq!(open_document(&document, asked, None), Ok(()));
        assert_eq!(
            document.executed(),
            [(r"C:\docs\report.pdf".to_owned(), None)],
            "the shell is handed the judged name"
        );

        // A link swapped to a program: the final name runs, nothing is
        // handed on.
        let swapped = FakeShell::holding(r"C:\Windows\System32\cmd.exe");
        assert_eq!(
            open_document(&swapped, asked, None),
            Err(LaunchError::NotADocument)
        );
        assert_eq!(swapped.executed(), []);

        // A type the floor does not list and the system calls dangerous.
        let dangerous = FakeShell {
            dangerous: true,
            ..FakeShell::holding(r"C:\docs\report.custom")
        };
        assert_eq!(
            open_document(&dangerous, asked, None),
            Err(LaunchError::NotADocument)
        );
        assert_eq!(dangerous.executed(), []);

        // A folder is shown, whatever its name.
        let folder = FakeShell {
            directory: true,
            ..FakeShell::holding(r"C:\tools\setup.exe")
        };
        assert_eq!(open_document(&folder, asked, None), Ok(()));

        let missing = FakeShell {
            hold_error: Some(2),
            ..FakeShell::holding(r"C:\gone")
        };
        assert_eq!(
            open_document(&missing, asked, None),
            Err(LaunchError::NotFound)
        );
    }

    #[test]
    fn a_uri_goes_to_its_schemes_registered_class() {
        let shell = FakeShell::holding(r"C:\unused");
        assert_eq!(
            open_uri(&shell, "https://example.com/a?b", "https", None),
            Ok(())
        );
        assert_eq!(
            shell.executed(),
            [(
                "https://example.com/a?b".to_owned(),
                Some("https".to_owned())
            )]
        );
    }

    #[test]
    fn a_trash_called_off_is_refused_and_shell_codes_name_their_errors() {
        let aborted = FakeShell {
            trashed: Ok(Trashed::Aborted),
            ..FakeShell::holding(r"C:\unused")
        };
        assert_eq!(
            trash(&aborted, Path::new(r"C:\a"), None),
            Err(LaunchError::Denied)
        );

        let cases = [
            (2, LaunchError::NotFound),
            (3, LaunchError::NotFound),
            (5, LaunchError::Denied),
            (1223, LaunchError::Denied),
            (1155, LaunchError::NoHandler),
            (31, LaunchError::Failed(SystemFailure::Os(31))),
        ];
        for (code, expected) in cases {
            assert_eq!(launch_error(code), expected, "code {code}");
        }
    }

    #[test]
    fn a_final_name_loses_its_verbatim_prefix_only_on_a_drive() {
        let wide = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(
            without_verbatim(wide(r"\\?\C:\docs\a.pdf")),
            PathBuf::from(r"C:\docs\a.pdf")
        );
        assert_eq!(
            without_verbatim(wide(r"\\?\UNC\server\share\a.pdf")),
            PathBuf::from(r"\\?\UNC\server\share\a.pdf"),
            "a share keeps its prefix, so the floor's locality still reads it"
        );
    }
}
