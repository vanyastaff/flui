//! The file dialogs over the common item dialogs (`IFileOpenDialog`,
//! `IFileSaveDialog`), each on its own apartment thread and owned by the
//! acquiring widget's window ([`super::modal`]).
//!
//! A filter maps to a `COMDLG_FILTERSPEC` of `*.ext` patterns, which the
//! dialog matches ignoring case and to the end of the name (`*.tar.gz`), as
//! the contract does (ADR-0360 §3, §4). The dialog filters by name only, so
//! a filter with a MIME type is refused with `NotOnThisPlatform` rather than
//! narrowed. The first filter starts selected; a save reports the one the
//! user left selected, and names a typed file that has no extension after
//! that filter's first.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::ERROR_CANCELLED;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
    FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog,
    IFileDialog, IFileOpenDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName,
    SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, Interface as _, PCWSTR};

use super::modal::{Dismisser, show_answering};
use super::{Owner, os_code, wide};
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileFilter, OpenRequest,
    OpenResult, PickedFile, Picks, SaveLocation, SaveRequest, SaveResult,
};

/// One filter as the dialog takes it: the name it shows and its patterns,
/// `*.png;*.jpg`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FilterSpec {
    pub(super) name: String,
    pub(super) pattern: String,
}

/// What both dialogs are told besides their own options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CommonSpec {
    pub(super) title: Option<String>,
    pub(super) accept_label: Option<String>,
    pub(super) directory: Option<PathBuf>,
    /// The filters in order; the first starts selected.
    pub(super) filters: Vec<FilterSpec>,
}

/// An open dialog as the system is asked for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OpenSpec {
    pub(super) common: CommonSpec,
    pub(super) folders: bool,
    pub(super) multiple: bool,
}

/// A save dialog as the system is asked for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SaveSpec {
    pub(super) common: CommonSpec,
    pub(super) file_name: Option<String>,
    /// The extension a typed name without one gets: the first filter's
    /// first. The dialog follows the filter the user selects from there.
    pub(super) default_extension: Option<String>,
}

/// One item the user picked, as the dialog names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum OsPick {
    /// A file system path.
    Path(PathBuf),
    /// An item with no file system path (a library, a device's folder).
    NotAFile,
}

/// What a dialog answered: its picks, `None` for a cancel, or the code it
/// failed with.
pub(super) type OsAnswer<T> = Result<Option<T>, i32>;

/// The common item dialogs as the provider reaches them, on the dialog's
/// thread inside its apartment. Each call shows its dialog owned by `owner`,
/// installs the dialog's close on `dismisser` before it shows it, and
/// returns when the dialog ends.
pub(super) trait DialogSystem: Send + Sync + 'static {
    /// Show an open dialog.
    fn open(
        &self,
        owner: Option<Owner>,
        spec: &OpenSpec,
        dismisser: &Dismisser,
    ) -> OsAnswer<Vec<OsPick>>;

    /// Show a save dialog; the answer carries the selected filter's 1-based
    /// index, as `GetFileTypeIndex` reports it.
    fn save(
        &self,
        owner: Option<Owner>,
        spec: &SaveSpec,
        dismisser: &Dismisser,
    ) -> OsAnswer<(OsPick, u32)>;
}

/// [`FileDialogBackend`] over the common item dialogs.
pub(crate) struct WindowsDialogs {
    system: Arc<dyn DialogSystem>,
}

impl WindowsDialogs {
    pub(crate) fn new() -> Self {
        Self::over(Arc::new(ComDialogs))
    }

    pub(super) fn over(system: Arc<dyn DialogSystem>) -> Self {
        Self { system }
    }
}

/// The patterns of `filters`, or the refusal of a filter the dialog cannot
/// show: one with a MIME type, since the dialog filters by name only.
fn filter_specs(filters: &[FileFilter]) -> Result<Vec<FilterSpec>, DialogError> {
    filters
        .iter()
        .map(|filter| {
            if !filter.mime_types().is_empty() {
                return Err(DialogError::Unsupported(Unsupported::of::<
                    FileDialogCapability,
                >(
                    UnsupportedReason::NotOnThisPlatform,
                )));
            }
            let pattern = filter
                .extensions()
                .iter()
                .map(|extension| format!("*.{extension}"))
                .collect::<Vec<String>>()
                .join(";");
            Ok(FilterSpec {
                name: filter.name().to_owned(),
                pattern,
            })
        })
        .collect::<Result<Vec<FilterSpec>, DialogError>>()
}

/// The open dialog `request` asks for.
fn open_spec(request: &OpenRequest) -> Result<OpenSpec, DialogError> {
    Ok(OpenSpec {
        common: CommonSpec {
            title: request.title.clone(),
            accept_label: request.accept_label.clone(),
            directory: request.directory.clone(),
            filters: filter_specs(&request.filters)?,
        },
        folders: request.picks_folders(),
        multiple: request.is_multiple(),
    })
}

/// The save dialog `request` asks for.
fn save_spec(request: &SaveRequest) -> Result<SaveSpec, DialogError> {
    Ok(SaveSpec {
        common: CommonSpec {
            title: request.title.clone(),
            accept_label: request.accept_label.clone(),
            directory: request.directory.clone(),
            filters: filter_specs(&request.filters)?,
        },
        file_name: request.suggested_name.clone(),
        default_extension: request
            .filters
            .first()
            .and_then(|filter| filter.extensions().first().cloned()),
    })
}

/// The failure of a dialog that answered `code`.
fn failed(code: i32) -> DialogError {
    DialogError::Failed(SystemFailure::Os(code))
}

/// A pick as the handle hands it out: a local file, or the refusal of one
/// that is not.
fn picked(pick: OsPick) -> Result<PickedFile, DialogError> {
    match pick {
        OsPick::Path(path) => {
            PickedFile::try_new(path).ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
        }
        OsPick::NotAFile => Err(DialogError::NotAFile),
    }
}

/// What an open dialog's answer means.
fn open_answer(answer: OsAnswer<Vec<OsPick>>) -> OpenResult {
    match answer {
        Err(code) => Err(failed(code)),
        Ok(None) => Ok(None),
        Ok(Some(picks)) => {
            let files = picks
                .into_iter()
                .map(picked)
                .collect::<Result<Vec<PickedFile>, DialogError>>()?;
            // An accepted dialog with nothing picked is outside its contract.
            Picks::try_new(files)
                .map(Some)
                .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))
        }
    }
}

/// What a save dialog's answer means, for a request that offered `filters`
/// filters: the 1-based index becomes 0-based, and none is reported when
/// the request offered none. An index past the filters is reported as is,
/// for the handle to refuse.
fn save_answer(answer: OsAnswer<(OsPick, u32)>, filters: usize) -> SaveResult {
    match answer {
        Err(code) => Err(failed(code)),
        Ok(None) => Ok(None),
        Ok(Some((pick, index))) => {
            let file = picked(pick)?;
            let filter = (filters > 0)
                .then(|| usize::try_from(index).ok()?.checked_sub(1))
                .flatten();
            Ok(Some(SaveLocation::new(file, filter)))
        }
    }
}

impl FileDialogBackend for WindowsDialogs {
    fn open(
        &self,
        window: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let spec = match open_spec(&request) {
            Ok(spec) => spec,
            Err(refusal) => {
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
        };
        let owner = Owner::of(window);
        let system = Arc::clone(&self.system);
        show_answering::<FileDialogCapability, _, _>(
            answer,
            move |dismisser| system.open(owner, &spec, dismisser),
            open_answer,
        )
    }

    fn save(
        &self,
        window: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let spec = match save_spec(&request) {
            Ok(spec) => spec,
            Err(refusal) => {
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
        };
        let filters = spec.common.filters.len();
        let owner = Owner::of(window);
        let system = Arc::clone(&self.system);
        show_answering::<FileDialogCapability, _, _>(
            answer,
            move |dismisser| system.save(owner, &spec, dismisser),
            move |answer| save_answer(answer, filters),
        )
    }
}

/// The real common item dialogs.
struct ComDialogs;

/// The code of a failed COM call.
fn code(error: &windows::core::Error) -> i32 {
    os_code(error.code())
}

/// Tell `dialog` what `spec` asks of both dialogs. A folder that cannot be
/// parsed (gone, unreachable) leaves the dialog where the system opens it.
fn apply_common(dialog: &IFileDialog, spec: &CommonSpec) -> Result<(), i32> {
    // SAFETY: every call below is a checked method of a live dialog on its
    // own apartment thread; each string argument is an `HSTRING` or a
    // NUL-terminated buffer that outlives its call.
    unsafe {
        if let Some(title) = &spec.title {
            dialog
                .SetTitle(&HSTRING::from(title.as_str()))
                .map_err(|error| code(&error))?;
        }
        if let Some(label) = &spec.accept_label {
            dialog
                .SetOkButtonLabel(&HSTRING::from(label.as_str()))
                .map_err(|error| code(&error))?;
        }
        if let Some(directory) = &spec.directory
            && let Ok(folder) = shell_item(directory)
        {
            dialog.SetFolder(&folder).map_err(|error| code(&error))?;
        }
        if !spec.filters.is_empty() {
            let names = spec
                .filters
                .iter()
                .map(|filter| wide(&filter.name))
                .collect::<Vec<Vec<u16>>>();
            let patterns = spec
                .filters
                .iter()
                .map(|filter| wide(&filter.pattern))
                .collect::<Vec<Vec<u16>>>();
            let specs = names
                .iter()
                .zip(&patterns)
                .map(|(name, pattern)| COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(pattern.as_ptr()),
                })
                .collect::<Vec<COMDLG_FILTERSPEC>>();
            dialog.SetFileTypes(&specs).map_err(|error| code(&error))?;
            dialog.SetFileTypeIndex(1).map_err(|error| code(&error))?;
        }
    }
    Ok(())
}

/// The shell item at `path`.
pub(super) fn shell_item(path: &Path) -> windows::core::Result<IShellItem> {
    let path = wide(path);
    // SAFETY: `path` is a NUL-terminated buffer that outlives the call.
    unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }
}

/// The file system path of `item`, or [`OsPick::NotAFile`].
fn os_pick(item: &IShellItem) -> OsPick {
    // SAFETY: a checked method of a live item; the string it returns is the
    // caller's to free with `CoTaskMemFree`, which happens once, after its
    // code units were copied out.
    unsafe {
        match item.GetDisplayName(SIGDN_FILESYSPATH) {
            Ok(name) => {
                let path = OsString::from_wide(name.as_wide());
                CoTaskMemFree(Some(name.as_ptr().cast_const().cast()));
                OsPick::Path(PathBuf::from(path))
            }
            Err(_) => OsPick::NotAFile,
        }
    }
}

/// Show `dialog` owned by `owner`, closable through `dismisser`: `Ok(true)`
/// when the user accepted, `Ok(false)` when the dialog was cancelled.
fn show(dialog: &IFileDialog, owner: Option<Owner>, dismisser: &Dismisser) -> Result<bool, i32> {
    let closing = dialog.clone();
    dismisser.on_dismiss(move || {
        // SAFETY: a checked method of the live dialog, called from the
        // dismissal window's procedure on the dialog's own thread while
        // `Show` runs its modal loop; `Show` then returns the code given.
        let _ = unsafe { closing.Close(ERROR_CANCELLED.to_hresult()) };
    });
    // SAFETY: a checked method of a live dialog on its apartment thread;
    // `owner` is a window of another thread, which a dialog may be owned by.
    match unsafe { dialog.Show(owner.map(Owner::hwnd)) } {
        Ok(()) => Ok(true),
        Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => Ok(false),
        Err(error) => Err(code(&error)),
    }
}

impl DialogSystem for ComDialogs {
    fn open(
        &self,
        owner: Option<Owner>,
        spec: &OpenSpec,
        dismisser: &Dismisser,
    ) -> OsAnswer<Vec<OsPick>> {
        // SAFETY: the dialog is created and used on this apartment thread
        // only; every call is a checked method of it.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| code(&error))?;
        // SAFETY: as above.
        unsafe {
            let mut options = dialog.GetOptions().map_err(|error| code(&error))?
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_FILEMUSTEXIST
                | FOS_NOCHANGEDIR;
            if spec.multiple {
                options |= FOS_ALLOWMULTISELECT;
            }
            if spec.folders {
                options |= FOS_PICKFOLDERS;
            }
            dialog.SetOptions(options).map_err(|error| code(&error))?;
        }
        let as_dialog = dialog.cast::<IFileDialog>().map_err(|error| code(&error))?;
        apply_common(&as_dialog, &spec.common)?;
        if !show(&as_dialog, owner, dismisser)? {
            return Ok(None);
        }
        // SAFETY: as above; the array and its items are released with their
        // last reference.
        unsafe {
            let results = dialog.GetResults().map_err(|error| code(&error))?;
            let count = results.GetCount().map_err(|error| code(&error))?;
            let mut picks = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
            for index in 0..count {
                let item = results.GetItemAt(index).map_err(|error| code(&error))?;
                picks.push(os_pick(&item));
            }
            Ok(Some(picks))
        }
    }

    fn save(
        &self,
        owner: Option<Owner>,
        spec: &SaveSpec,
        dismisser: &Dismisser,
    ) -> OsAnswer<(OsPick, u32)> {
        // SAFETY: as in `open`.
        let dialog: IFileSaveDialog =
            unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| code(&error))?;
        // SAFETY: as in `open`; each string is an `HSTRING` that outlives
        // its call.
        unsafe {
            let options = dialog.GetOptions().map_err(|error| code(&error))?
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_OVERWRITEPROMPT
                | FOS_NOCHANGEDIR;
            dialog.SetOptions(options).map_err(|error| code(&error))?;
            if let Some(name) = &spec.file_name {
                dialog
                    .SetFileName(&HSTRING::from(name.as_str()))
                    .map_err(|error| code(&error))?;
            }
            if let Some(extension) = &spec.default_extension {
                dialog
                    .SetDefaultExtension(&HSTRING::from(extension.as_str()))
                    .map_err(|error| code(&error))?;
            }
        }
        let as_dialog = dialog.cast::<IFileDialog>().map_err(|error| code(&error))?;
        apply_common(&as_dialog, &spec.common)?;
        if !show(&as_dialog, owner, dismisser)? {
            return Ok(None);
        }
        // SAFETY: as in `open`.
        unsafe {
            let item = dialog.GetResult().map_err(|error| code(&error))?;
            let index = if spec.common.filters.is_empty() {
                0
            } else {
                dialog.GetFileTypeIndex().map_err(|error| code(&error))?
            };
            Ok(Some((os_pick(&item), index)))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use flui_platform::capability::Abandoned;
    use flui_platform::{SystemFailure, UnsupportedReason};

    use super::{
        CommonSpec, DialogSystem, FilterSpec, OpenSpec, OsAnswer, OsPick, SaveSpec, WindowsDialogs,
        open_answer, open_spec, save_answer, save_spec,
    };
    use crate::dialog::{
        Dialog, DialogError, FileDialogs, FileFilter, OpenRequest, OpenResult, PickedFile, Picks,
        SaveLocation, SaveRequest,
    };
    use crate::win32::Owner;
    use crate::win32::modal::Dismisser;
    use crate::win32::test_owner::{self, TestOwner};

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn a_filter_becomes_the_dialogs_name_patterns_and_the_first_starts_selected() {
        let request = OpenRequest::files()
            .allow_multiple()
            .title("Pictures")
            .accept_label("Use")
            .directory(temp("in"))
            .filter(
                FileFilter::new("Images")
                    .with_extension("PNG")
                    .with_extension("tar.gz"),
            )
            .filter(FileFilter::new("Text").with_extension("txt"));
        assert_eq!(
            open_spec(&request),
            Ok(OpenSpec {
                common: CommonSpec {
                    title: Some("Pictures".to_owned()),
                    accept_label: Some("Use".to_owned()),
                    directory: Some(temp("in")),
                    filters: vec![
                        FilterSpec {
                            name: "Images".to_owned(),
                            pattern: "*.png;*.tar.gz".to_owned(),
                        },
                        FilterSpec {
                            name: "Text".to_owned(),
                            pattern: "*.txt".to_owned(),
                        },
                    ],
                },
                folders: false,
                multiple: true,
            })
        );

        let save = SaveRequest::new().suggested_name("report").filter(
            FileFilter::new("Archive")
                .with_extension("tar.gz")
                .with_extension("zip"),
        );
        let spec = save_spec(&save).expect("a save without MIME types maps");
        assert_eq!(spec.file_name.as_deref(), Some("report"));
        assert_eq!(
            spec.default_extension.as_deref(),
            Some("tar.gz"),
            "a typed name without an extension gets the selected filter's first"
        );
    }

    #[test]
    fn a_mime_type_the_dialog_cannot_filter_by_is_refused_not_dropped() {
        let request = OpenRequest::files().filter(
            FileFilter::new("Images")
                .with_extension("png")
                .with_mime_type("image/*"),
        );
        let refusal = open_spec(&request).expect_err("the dialog filters by name only");
        assert_eq!(
            refusal.unsupported().map(|unsupported| unsupported.reason),
            Some(UnsupportedReason::NotOnThisPlatform)
        );
    }

    #[test]
    fn the_dialogs_answers_become_picks_cancels_and_failures() {
        let path = |name: &str| OsPick::Path(temp(name));
        let file = |name: &str| PickedFile::try_new(temp(name)).expect("absolute");
        let cases: Vec<(OsAnswer<Vec<OsPick>>, OpenResult)> = vec![
            (
                Ok(Some(vec![path("a"), path("b")])),
                Ok(Picks::try_new(vec![file("a"), file("b")])),
            ),
            (Ok(None), Ok(None)),
            (
                Ok(Some(vec![path("a"), OsPick::NotAFile])),
                Err(DialogError::NotAFile),
            ),
            (
                Ok(Some(Vec::new())),
                Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            ),
            (
                Ok(Some(vec![OsPick::Path(PathBuf::from("relative"))])),
                Err(DialogError::Failed(SystemFailure::MalformedAnswer)),
            ),
            (Err(5), Err(DialogError::Failed(SystemFailure::Os(5)))),
        ];
        for (answer, expected) in cases {
            assert_eq!(open_answer(answer.clone()), expected, "{answer:?}");
        }

        let saved = |index| save_answer(Ok(Some((path("s"), index))), 2);
        assert_eq!(saved(1), Ok(Some(SaveLocation::new(file("s"), Some(0)))));
        assert_eq!(saved(2), Ok(Some(SaveLocation::new(file("s"), Some(1)))));
        assert_eq!(
            saved(7),
            Ok(Some(SaveLocation::new(file("s"), Some(6)))),
            "an index past the filters is the handle's to refuse"
        );
        assert_eq!(saved(0), Ok(Some(SaveLocation::new(file("s"), None))));
        assert_eq!(
            save_answer(Ok(Some((path("s"), 1))), 0),
            Ok(Some(SaveLocation::new(file("s"), None))),
            "no filter offered, none reported"
        );
    }

    /// How a [`ModalSystem`] dialog ended.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Ended {
        Shown,
        Answered,
        ClosedByItsDismissal,
    }

    /// A dialog system whose dialog stays up, pumping its thread's messages
    /// as a modal loop does, until the test answers it or its dismissal
    /// closes it.
    struct ModalSystem {
        answers: Mutex<mpsc::Receiver<OsAnswer<Vec<OsPick>>>>,
        ended: Mutex<mpsc::Sender<Ended>>,
    }

    impl DialogSystem for ModalSystem {
        fn open(
            &self,
            _: Option<Owner>,
            _: &OpenSpec,
            dismisser: &Dismisser,
        ) -> OsAnswer<Vec<OsPick>> {
            let closed = Rc::new(Cell::new(false));
            let closing = Rc::clone(&closed);
            dismisser.on_dismiss(move || closing.set(true));
            let _ = self
                .ended
                .lock()
                .expect("BUG: the test's lock")
                .send(Ended::Shown);
            loop {
                pump();
                if closed.get() {
                    let _ = self
                        .ended
                        .lock()
                        .expect("BUG: the test's lock")
                        .send(Ended::ClosedByItsDismissal);
                    return Ok(None);
                }
                let answer = self
                    .answers
                    .lock()
                    .expect("BUG: the test's lock")
                    .try_recv();
                if let Ok(answer) = answer {
                    let _ = self
                        .ended
                        .lock()
                        .expect("BUG: the test's lock")
                        .send(Ended::Answered);
                    return answer;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        fn save(&self, _: Option<Owner>, _: &SaveSpec, _: &Dismisser) -> OsAnswer<(OsPick, u32)> {
            Ok(None)
        }
    }

    /// Dispatch this thread's waiting messages.
    #[expect(unsafe_code, reason = "a test's own message pump")]
    fn pump() {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
        };
        let mut message = MSG::default();
        // SAFETY: `message` is a live out-parameter; only this thread's own
        // messages are dispatched.
        while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            // SAFETY: as above.
            unsafe { DispatchMessageW(&raw const message) };
        }
    }

    type Answers = mpsc::Sender<OsAnswer<Vec<OsPick>>>;

    fn modal() -> (Arc<ModalSystem>, Answers, mpsc::Receiver<Ended>) {
        let (answer, answers) = mpsc::channel();
        let (end, ended) = mpsc::channel();
        let system = ModalSystem {
            answers: Mutex::new(answers),
            ended: Mutex::new(end),
        };
        (Arc::new(system), answer, ended)
    }

    type Landed = Rc<RefCell<Option<Result<OpenResult, Abandoned>>>>;

    fn land(dialog: Dialog<OpenResult>) -> Landed {
        let landed: Landed = Rc::default();
        let into = Rc::clone(&landed);
        dialog.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        landed
    }

    #[test]
    fn a_dialog_answers_from_its_thread_at_an_owner_turn() {
        let owner = TestOwner::new();
        let (window, _closer) = test_owner::window();
        let (system, answer, ended) = modal();
        let dialogs = FileDialogs::try_new(WindowsDialogs::over(system), &owner.ctx(Some(window)))
            .expect("a window");
        let landed = land(dialogs.open(OpenRequest::files()));
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Shown)
        );
        answer
            .send(Ok(Some(vec![OsPick::Path(temp("picked.txt"))])))
            .expect("the dialog listens");
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Answered)
        );
        owner.turn_until(|| landed.borrow().is_some());
        let picked = PickedFile::try_new(temp("picked.txt")).expect("absolute");
        assert_eq!(
            landed.borrow_mut().take(),
            Some(Ok(Ok(Picks::try_new(vec![picked]))))
        );
    }

    #[test]
    fn a_dropped_dialog_is_closed_on_its_thread_and_frees_the_window() {
        let owner = TestOwner::new();
        let (window, _closer) = test_owner::window();
        let (system, _answer, ended) = modal();
        let dialogs = FileDialogs::try_new(WindowsDialogs::over(system), &owner.ctx(Some(window)))
            .expect("a window");
        let dialog = dialogs.open(OpenRequest::files());
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Shown)
        );
        let busy = land(dialogs.open(OpenRequest::files()));
        owner.turn_until(|| busy.borrow().is_some());
        assert_eq!(busy.borrow_mut().take(), Some(Ok(Err(DialogError::Busy))));

        drop(dialog);
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::ClosedByItsDismissal),
            "the dialog's own close ran on its thread"
        );
        let next = land(dialogs.open(OpenRequest::files()));
        owner.turn();
        assert_eq!(
            *next.borrow(),
            None,
            "the window is free: the next dialog is up"
        );
    }

    #[test]
    fn a_dialog_ends_with_its_window_and_is_closed_on_its_thread() {
        let owner = TestOwner::new();
        let (window, closer) = test_owner::window();
        let (system, _answer, ended) = modal();
        let dialogs = FileDialogs::try_new(WindowsDialogs::over(system), &owner.ctx(Some(window)))
            .expect("a window");
        let landed = land(dialogs.open(OpenRequest::files()));
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Shown)
        );
        closer.close();
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::ClosedByItsDismissal)
        );
        owner.turn_until(|| landed.borrow().is_some());
        assert_eq!(
            landed.borrow_mut().take(),
            Some(Ok(Err(DialogError::WindowClosed)))
        );
    }
}
