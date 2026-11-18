//! The launcher over the shell: `ShellExecuteExW` with the `open` verb for a
//! URI and a document, `SHOpenFolderAndSelectItems` to reveal, and
//! `IFileOperation` with `FOFX_RECYCLEONDELETE` to trash. Never a command
//! line: no string reaches `cmd` or `explorer`.
//!
//! A URI goes to the shell as it is, so Windows resolves the handler the
//! user chose for its scheme (`UrlAssociations\…\UserChoice`, the default
//! browser); the seam admitted only the schemes its caller named. A
//! one-letter scheme is refused, since the shell reads `c:…` as a path on
//! drive C.
//!
//! Each request runs on its own apartment thread ([`ShellThreads`]), since
//! every one may touch the file system or start a handler that takes its
//! time, and answers its own resolver from there. An open first opens the
//! file and judges what the handle holds (ADR-0360 §2): its final name
//! (`GetFinalPathNameByHandleW`, which follows every link the open
//! followed) must be on a local drive, then pass the floor and the
//! system's own `AssocIsDangerous`; only a file all three admit is handed
//! to the shell, by that final name. The handle reads the file's data and
//! does not share deletion, so while it is held, until `ShellExecuteExW`
//! returns, nobody can rename, move or delete the file; writing to it stays
//! shared, since refusing that would refuse a document another application
//! keeps open.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::launch::{
    ExternalUri, LaunchError, LaunchResult, LauncherBackend, LauncherCapability, is_local_path,
    runs_when_opened,
};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_BAD_NETPATH, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
    ERROR_INVALID_DRIVE, ERROR_INVALID_NAME, ERROR_NO_ASSOCIATION, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FileBasicInfo, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandleEx,
    GetFinalPathNameByHandleW, OPEN_EXISTING, VOLUME_NAME_DOS,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::GetProcessId;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    AssocIsDangerous, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation,
    ILFree, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    SHOpenFolderAndSelectItems, SHParseDisplayName, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, SW_SHOWNORMAL};
use windows::core::{PCWSTR, w};

use super::dialogs::shell_item;
use super::verbatim::without_verbatim;
use super::{Owner, ShellThreads, os_code, wide};

/// A file opened to be judged, and kept open while it is handed on.
pub(crate) struct HeldFile {
    /// The name the handle resolves to: every link the open followed,
    /// followed.
    pub(crate) final_path: PathBuf,
    /// Whether it is a folder, which is shown, not run.
    pub(crate) directory: bool,
    /// What keeps the file open; dropped once it was handed on.
    pub(crate) _handle: Box<dyn Send>,
}

/// What a trash operation did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trashed {
    /// The item is in the recycle bin.
    Done,
    /// The user or the system called it off (the user declined to delete
    /// for good an item the recycle bin cannot take).
    Aborted,
}

/// The shell as the launcher reaches it, on a request's apartment thread.
/// Each failure is the Win32 error or `HRESULT` the call reported.
pub(crate) trait ShellSystem: Send + Sync + 'static {
    /// Open `path` to judge it.
    fn hold(&self, path: &Path) -> Result<HeldFile, i32>;

    /// Whether the system calls a file of this name dangerous to open
    /// (`AssocIsDangerous` on its extension).
    fn is_dangerous(&self, final_path: &Path) -> bool;

    /// Open `target`, a URI or a file's name, with the `open` verb.
    fn execute(&self, target: &OsStr, owner: Option<Owner>) -> Result<(), i32>;

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
/// hand the final name to the shell only when it is local and both the
/// floor and the system admit it.
pub(super) fn open_document(
    system: &dyn ShellSystem,
    path: &Path,
    owner: Option<Owner>,
) -> LaunchResult {
    let held = system.hold(path).map_err(launch_error)?;
    // A link on a local drive that leads to a share is judged where it led.
    if !is_local_path(&held.final_path) {
        return Err(LaunchError::NotLocal);
    }
    if !held.directory
        && (runs_when_opened(&held.final_path) || system.is_dangerous(&held.final_path))
    {
        return Err(LaunchError::NotADocument);
    }
    let opened = system
        .execute(held.final_path.as_os_str(), owner)
        .map_err(launch_error);
    // Released only after the shell took the file.
    drop(held);
    opened
}

/// Open `uri` in the handler the user chose for its scheme. A one-letter
/// scheme is refused: the shell reads `c:…` as a path on drive C.
pub(super) fn open_uri(
    system: &dyn ShellSystem,
    uri: &str,
    scheme: &str,
    owner: Option<Owner>,
) -> LaunchResult {
    if scheme.len() == 1 {
        return Err(LaunchError::Unsupported(Unsupported::of::<
            LauncherCapability,
        >(
            UnsupportedReason::NotOnThisPlatform,
        )));
    }
    system.execute(OsStr::new(uri), owner).map_err(launch_error)
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

/// The name a held file is judged and handed on by, from the final name the
/// system answered for its handle: without the verbatim prefix, which the
/// shell does not read. A share stays a share without it
/// (`\\?\UNC\server\share\…` is `\\server\share\…`), so the locality judge
/// still refuses it.
fn held_name(answered: &Path) -> PathBuf {
    without_verbatim(answered)
}

/// The process the foreground right goes to after `ShellExecuteExW`: the
/// handler the shell started, by the id it reported, and only when the call
/// succeeded. A failed call, or a handler the shell did not start (one
/// already running reports no process, so its id is 0), is handed nothing.
fn foreground_recipient(executed: bool, process_id: u32) -> Option<u32> {
    (executed && process_id != 0).then_some(process_id)
}

/// The real shell.
struct Shell;

impl ShellSystem for Shell {
    fn hold(&self, path: &Path) -> Result<HeldFile, i32> {
        let name = wide(path);
        // SAFETY: `name` is a NUL-terminated buffer that outlives the call.
        // The handle reads data, so the sharing rules apply to it (an open
        // for attributes only would hold nothing back): it shares reading
        // and writing and not deletion, so while it is open no other open may
        // rename, move or delete the file; it opens a folder too
        // (`BACKUP_SEMANTICS`, where reading data is listing it).
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .map_err(|error| os_code(error.code()))?;
        // SAFETY: `handle` was just opened and nothing else owns it: the
        // owned handle closes it once, on whichever thread drops it.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let raw = HANDLE(handle.as_raw_handle());
        let mut buffer = vec![0_u16; 512];
        let final_path = loop {
            // SAFETY: `buffer` is a live, writable buffer of the length the
            // call is given; the handle is open.
            let written = unsafe {
                GetFinalPathNameByHandleW(
                    raw,
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
                break held_name(Path::new(&OsString::from_wide(&buffer)));
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
                raw,
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

    fn execute(&self, target: &OsStr, owner: Option<Owner>) -> Result<(), i32> {
        let target = wide(target);
        let mut info = SHELLEXECUTEINFOW {
            cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())
                .expect("BUG: SHELLEXECUTEINFOW is a few hundred bytes"),
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOCLOSEPROCESS,
            hwnd: owner.map_or_default(Owner::hwnd),
            lpVerb: w!("open"),
            lpFile: PCWSTR(target.as_ptr()),
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
                if let Some(id) = foreground_recipient(executed.is_ok(), GetProcessId(process)) {
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
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;

    use flui_platform::capability::Abandoned;
    use flui_platform::launch::{ExternalUri, LaunchError, LaunchResult, Launcher, Scheme};
    use flui_platform::{SystemFailure, UnsupportedReason};

    use super::{
        Shell, ShellSystem, Trashed, WindowsLauncher, foreground_recipient, held_name,
        launch_error, open_document, open_uri, trash,
    };
    use crate::testing::win32::{FakeShell, TestOwner};
    use crate::win32::ShellThreads;

    /// The foreground right goes to the handler the shell started, by its
    /// id; to no process when the shell started none or failed.
    #[test]
    fn the_foreground_goes_only_to_the_handler_the_shell_started() {
        let cases: [(&str, bool, u32, Option<u32>); 4] = [
            ("a handler the shell started", true, 4242, Some(4242)),
            ("a handler already running", true, 0, None),
            ("the shell failed", false, 4242, None),
            ("the shell failed and started none", false, 0, None),
        ];
        for (case, executed, process_id, expected) in cases {
            assert_eq!(
                foreground_recipient(executed, process_id),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn an_open_judges_the_file_the_handle_holds_not_the_path_it_was_given() {
        let asked = Path::new(r"C:\docs\report.pdf");

        let document = FakeShell::holding(r"C:\docs\report.pdf");
        assert_eq!(open_document(&document, asked, None), Ok(()));
        assert_eq!(
            document.executed(),
            [r"C:\docs\report.pdf".to_owned()],
            "the shell is handed the judged name"
        );

        // A link swapped to a program: the final name runs, nothing is
        // handed on.
        let swapped = FakeShell::holding(r"C:\Windows\System32\cmd.exe");
        assert_eq!(
            open_document(&swapped, asked, None),
            Err(LaunchError::NotADocument)
        );
        assert_eq!(swapped.executed(), Vec::<String>::new());

        // A type the floor does not list and the system calls dangerous.
        let dangerous = FakeShell {
            dangerous: true,
            ..FakeShell::holding(r"C:\docs\report.custom")
        };
        assert_eq!(
            open_document(&dangerous, asked, None),
            Err(LaunchError::NotADocument)
        );
        assert_eq!(dangerous.executed(), Vec::<String>::new());

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

    /// Each final name `GetFinalPathNameByHandleW` answers is judged as the
    /// held file's name ([`held_name`]): a share, with or without the
    /// verbatim prefix it is answered with, is still a share.
    #[test]
    fn a_link_on_a_local_drive_that_leads_off_it_is_refused_where_it_led() {
        let asked = Path::new(r"C:\docs\report.pdf");
        for (answered, held) in [
            (
                r"\\?\UNC\server\share\report.pdf",
                r"\\server\share\report.pdf",
            ),
            (
                r"\\?\GLOBALROOT\Device\Mup\server\share\report.pdf",
                r"\\?\GLOBALROOT\Device\Mup\server\share\report.pdf",
            ),
        ] {
            assert_eq!(
                held_name(Path::new(answered)),
                Path::new(held),
                "a share keeps its server and share"
            );
            let shell = FakeShell::holding(held);
            assert_eq!(
                open_document(&shell, asked, None),
                Err(LaunchError::NotLocal),
                "{answered}"
            );
            assert_eq!(shell.executed(), Vec::<String>::new(), "{answered}");
        }
        assert_eq!(
            held_name(Path::new(r"\\?\D:\docs\report.pdf")),
            Path::new(r"D:\docs\report.pdf")
        );
        let drive = FakeShell::holding(r"D:\docs\report.pdf");
        assert_eq!(open_document(&drive, asked, None), Ok(()));
        assert_eq!(
            drive.executed(),
            [r"D:\docs\report.pdf".to_owned()],
            "a local drive is handed on in the form the shell reads"
        );
    }

    #[test]
    fn a_uri_goes_as_it_is_to_the_users_handler_and_a_one_letter_scheme_is_refused() {
        let shell = FakeShell::holding(r"C:\unused");
        assert_eq!(
            open_uri(&shell, "https://example.com/a?b", "https", None),
            Ok(())
        );
        assert_eq!(shell.executed(), ["https://example.com/a?b".to_owned()]);

        let refused = open_uri(&shell, "c:report.pdf", "c", None)
            .expect_err("the shell reads a one-letter scheme as a drive");
        assert_eq!(
            refused.unsupported().map(|unsupported| unsupported.reason),
            Some(UnsupportedReason::NotOnThisPlatform)
        );
        assert_eq!(shell.executed().len(), 1, "nothing more reached the shell");
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

    /// The real `hold` on a file of the test's own in the temporary folder:
    /// nothing is launched.
    #[test]
    fn a_held_file_cannot_be_renamed_until_it_is_let_go() {
        let folder = std::env::temp_dir().join(format!("flui-system-held-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a temporary folder");
        let document = folder.join("a.pdf");
        let program = folder.join("b.exe");
        std::fs::write(&document, b"%PDF").expect("a document");

        let held = Shell.hold(&document).expect("the document opens");
        assert!(!held.directory);
        assert_eq!(
            held.final_path.file_name(),
            document.file_name(),
            "the final name is the document's"
        );
        assert!(
            std::fs::rename(&document, &program).is_err(),
            "a held file cannot become a program under the judged name"
        );
        drop(held);
        std::fs::rename(&document, &program).expect("a file let go can be renamed");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_launcher_request_runs_off_the_owner_and_answers_at_an_owner_turn() {
        let owner = TestOwner::new();
        let ctx = owner.ctx(None);
        let launcher = Launcher::new(
            WindowsLauncher::over(
                Arc::new(FakeShell::holding(r"C:\Windows\System32\cmd.exe")),
                Rc::new(ShellThreads::default()),
                None,
            ),
            &ctx,
        );
        let answered: Rc<RefCell<Option<Result<LaunchResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&answered);
        launcher
            .open_path(&std::env::temp_dir().join("report.pdf"))
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        assert!(answered.borrow().is_none(), "never inside the call");
        owner.turn_until(|| answered.borrow().is_some());
        assert_eq!(
            *answered.borrow(),
            Some(Ok(Err(LaunchError::NotADocument))),
            "judged by the file the handle holds"
        );

        let drive = Scheme::try_new("c").expect("a scheme the caller allows");
        let uri = ExternalUri::try_new("c:report.pdf", &[drive]).expect("admitted at the seam");
        let refused: Rc<RefCell<Option<Result<LaunchResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&refused);
        launcher
            .open_uri(&uri)
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| refused.borrow().is_some());
        assert!(matches!(
            *refused.borrow(),
            Some(Ok(Err(LaunchError::Unsupported(_))))
        ));
    }
}
