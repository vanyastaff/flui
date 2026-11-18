//! The file dialogs over the common item dialogs (`IFileOpenDialog`,
//! `IFileSaveDialog`), each on its own apartment thread and owned by the
//! acquiring widget's window ([`super::modal`]).
//!
//! A filter maps to a `COMDLG_FILTERSPEC` of patterns, which the dialog
//! matches ignoring case and to the end of the name, as the contract does
//! (ADR-0360 §3, §4, ADR-0372): `*.ext` for an extension (`*.tar.gz`), the
//! name itself for a file name (`Makefile`), and `*.*` for a filter of every
//! file, which Windows matches against a name with no dot as well. The
//! dialog filters by name only, so a filter with a MIME type is refused with
//! `NotOnThisPlatform` rather than narrowed. The first filter starts
//! selected; a save reports the one the user left selected, and names a
//! typed file that has no extension after the first filter's first.
//!
//! The dialog is owned by a window, which is what makes it modal to it: a
//! window whose handle names no Win32 window answers `NoWindow`, and nothing
//! is shown. The folder a request opens in is named to the dialog as a shell
//! item, in the form the shell reads ([`super::verbatim`]); one that is not
//! there (gone, on a drive unplugged or a share that does not answer) is
//! left out, and the dialog opens where the system would, as the portal's
//! and the Apple panels do; one the shell cannot name for another reason
//! fails the request with the shell's code before anything is shown.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Foundation::{
    ERROR_BAD_NET_NAME, ERROR_BAD_NETPATH, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
    ERROR_INVALID_DRIVE, ERROR_NOT_READY, ERROR_PATH_NOT_FOUND, WIN32_ERROR,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
    FOS_FORCESHOWHIDDEN, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
    FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, Interface as _, PCWSTR};

use super::modal::{Dismisser, ModalWindows, show_answering};
use super::verbatim::without_verbatim;
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

/// The pattern of a filter of every file: the one Windows' own "All files"
/// entries carry, which it matches against a name with no dot too.
const EVERY_FILE: &str = "*.*";

/// What both dialogs are told besides their own options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CommonSpec {
    pub(super) title: Option<String>,
    pub(super) accept_label: Option<String>,
    /// The folder the dialog opens in, in the form the shell reads.
    pub(super) directory: Option<PathBuf>,
    /// The filters in order; the first starts selected.
    pub(super) filters: Vec<FilterSpec>,
    /// Whether hidden and system items are shown whatever the user's
    /// setting (`FOS_FORCESHOWHIDDEN`).
    pub(super) show_hidden: bool,
}

impl CommonSpec {
    /// The options a dialog adds to its own for this spec.
    fn options(&self) -> FILEOPENDIALOGOPTIONS {
        if self.show_hidden {
            FOS_FORCESHOWHIDDEN
        } else {
            FILEOPENDIALOGOPTIONS(0)
        }
    }
}

/// An open dialog as the system is asked for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OpenSpec {
    pub(super) common: CommonSpec,
    pub(super) folders: bool,
    pub(super) multiple: bool,
}

impl OpenSpec {
    /// The options the dialog is given beside the ones it starts with: file
    /// system items that exist, the process's folder left alone.
    fn options(&self) -> FILEOPENDIALOGOPTIONS {
        let mut options = FOS_FORCEFILESYSTEM
            | FOS_PATHMUSTEXIST
            | FOS_FILEMUSTEXIST
            | FOS_NOCHANGEDIR
            | self.common.options();
        if self.multiple {
            options |= FOS_ALLOWMULTISELECT;
        }
        if self.folders {
            options |= FOS_PICKFOLDERS;
        }
        options
    }
}

/// A save dialog as the system is asked for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SaveSpec {
    pub(super) common: CommonSpec,
    pub(super) file_name: Option<String>,
    /// The extension a typed name without one gets: the first filter's
    /// first, when it has one.
    pub(super) default_extension: Option<String>,
}

impl SaveSpec {
    /// The options the dialog is given beside the ones it starts with: a
    /// file system path in a folder that exists, a prompt before a file is
    /// replaced, the process's folder left alone.
    fn options(&self) -> FILEOPENDIALOGOPTIONS {
        FOS_FORCEFILESYSTEM
            | FOS_PATHMUSTEXIST
            | FOS_OVERWRITEPROMPT
            | FOS_NOCHANGEDIR
            | self.common.options()
    }
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
    fn open(&self, owner: Owner, spec: &OpenSpec, dismisser: &Dismisser) -> OsAnswer<Vec<OsPick>>;

    /// Show a save dialog; the answer carries the selected filter's 1-based
    /// index, as `GetFileTypeIndex` reports it.
    fn save(&self, owner: Owner, spec: &SaveSpec, dismisser: &Dismisser)
    -> OsAnswer<(OsPick, u32)>;
}

/// [`FileDialogBackend`] over the common item dialogs.
pub(crate) struct WindowsDialogs {
    system: Arc<dyn DialogSystem>,
    windows: Rc<ModalWindows>,
}

impl WindowsDialogs {
    pub(crate) fn new(windows: Rc<ModalWindows>) -> Self {
        Self::over(Arc::new(ComDialogs), windows)
    }

    pub(super) fn over(system: Arc<dyn DialogSystem>, windows: Rc<ModalWindows>) -> Self {
        Self { system, windows }
    }
}

/// The refusal of a request the dialogs cannot show, for `reason`.
fn unsupported(reason: UnsupportedReason) -> DialogError {
    DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(reason))
}

/// The patterns of `filters`, or the refusal of a filter the dialog cannot
/// show: one with a MIME type, since the dialog filters by name only.
fn filter_specs(filters: &[FileFilter]) -> Result<Vec<FilterSpec>, DialogError> {
    filters
        .iter()
        .map(|filter| {
            if !filter.mime_types().is_empty() {
                return Err(unsupported(UnsupportedReason::NotOnThisPlatform));
            }
            let pattern = if filter.is_all_files() {
                EVERY_FILE.to_owned()
            } else {
                // A checked extension or file name holds no `*`, `?` or `;`,
                // so each pattern stands for itself.
                let by_extension = filter
                    .extensions()
                    .iter()
                    .map(|extension| format!("*.{extension}"));
                by_extension
                    .chain(filter.file_names().iter().cloned())
                    .collect::<Vec<String>>()
                    .join(";")
            };
            Ok(FilterSpec {
                name: filter.name().to_owned(),
                pattern,
            })
        })
        .collect::<Result<Vec<FilterSpec>, DialogError>>()
}

/// What `request`'s title, label, folder, filters and hidden files ask of
/// either dialog.
fn common_spec(
    title: Option<&str>,
    accept_label: Option<&str>,
    directory: Option<&Path>,
    filters: &[FileFilter],
    show_hidden: bool,
) -> Result<CommonSpec, DialogError> {
    Ok(CommonSpec {
        title: title.map(str::to_owned),
        accept_label: accept_label.map(str::to_owned),
        directory: directory.map(without_verbatim),
        filters: filter_specs(filters)?,
        show_hidden,
    })
}

/// The open dialog `request` asks for.
fn open_spec(request: &OpenRequest) -> Result<OpenSpec, DialogError> {
    Ok(OpenSpec {
        common: common_spec(
            request.title.as_deref(),
            request.accept_label.as_deref(),
            request.directory.as_deref(),
            &request.filters,
            request.show_hidden,
        )?,
        folders: request.picks_folders(),
        multiple: request.is_multiple(),
    })
}

/// The save dialog `request` asks for.
fn save_spec(request: &SaveRequest) -> Result<SaveSpec, DialogError> {
    Ok(SaveSpec {
        common: common_spec(
            request.title.as_deref(),
            request.accept_label.as_deref(),
            request.directory.as_deref(),
            &request.filters,
            request.show_hidden,
        )?,
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

/// The Win32 window a dialog in `window` is owned by, or the refusal of a
/// window that is none: a dialog with no owner would be modal to nothing.
fn owned(window: &WindowRef) -> Result<Owner, DialogError> {
    Owner::of(window).ok_or_else(|| unsupported(UnsupportedReason::NoWindow))
}

impl FileDialogBackend for WindowsDialogs {
    fn open(
        &self,
        window: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let (owner, spec) = match owned(window).and_then(|owner| Ok((owner, open_spec(&request)?)))
        {
            Ok(shown) => shown,
            Err(refusal) => {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
        };
        let system = Arc::clone(&self.system);
        show_answering::<FileDialogCapability, _, _>(
            &self.windows,
            window.id(),
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
        let (owner, spec) = match owned(window).and_then(|owner| Ok((owner, save_spec(&request)?)))
        {
            Ok(shown) => shown,
            Err(refusal) => {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(Err(refusal));
                return DialogHold::new(());
            }
        };
        let filters = spec.common.filters.len();
        let system = Arc::clone(&self.system);
        show_answering::<FileDialogCapability, _, _>(
            &self.windows,
            window.id(),
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

/// The Win32 codes the shell names a folder that is not there by: gone
/// (`ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`), on a drive letter
/// nothing is mapped to (`ERROR_INVALID_DRIVE`) or a drive with no medium
/// (`ERROR_NOT_READY`), on a share that does not answer
/// (`ERROR_BAD_NETPATH`, `ERROR_BAD_NET_NAME`).
const FOLDER_NOT_THERE: [WIN32_ERROR; 6] = [
    ERROR_FILE_NOT_FOUND,
    ERROR_PATH_NOT_FOUND,
    ERROR_INVALID_DRIVE,
    ERROR_NOT_READY,
    ERROR_BAD_NETPATH,
    ERROR_BAD_NET_NAME,
];

/// Whether the shell's `code` for a folder says it is not there.
fn is_folder_not_there(code: i32) -> bool {
    FOLDER_NOT_THERE
        .iter()
        .any(|not_there| i32::try_from(not_there.0) == Ok(code))
}

/// Tell `dialog` what `spec` asks of both dialogs. A folder that is not
/// there (a remembered folder deleted since, a drive unplugged) is left out,
/// and the dialog opens where the system would, as on every host; a folder
/// the shell cannot name for another reason fails with its code.
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
        if let Some(directory) = &spec.directory {
            match shell_item(directory).map_err(|error| code(&error)) {
                Ok(folder) => dialog.SetFolder(&folder).map_err(|error| code(&error))?,
                Err(code) if is_folder_not_there(code) => {}
                Err(code) => return Err(code),
            }
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
fn show(dialog: &IFileDialog, owner: Owner, dismisser: &Dismisser) -> Result<bool, i32> {
    let closing = dialog.clone();
    dismisser.on_dismiss(move || {
        // SAFETY: a checked method of the live dialog, called from the
        // dismissal window's procedure on the dialog's own thread while
        // `Show` runs its modal loop; `Show` then returns the code given.
        let _ = unsafe { closing.Close(ERROR_CANCELLED.to_hresult()) };
    });
    // SAFETY: a checked method of a live dialog on its apartment thread;
    // `owner` is a window of another thread, which a dialog may be owned by.
    match unsafe { dialog.Show(Some(owner.hwnd())) } {
        Ok(()) => Ok(true),
        Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => Ok(false),
        Err(error) => Err(code(&error)),
    }
}

impl DialogSystem for ComDialogs {
    fn open(&self, owner: Owner, spec: &OpenSpec, dismisser: &Dismisser) -> OsAnswer<Vec<OsPick>> {
        // SAFETY: the dialog is created and used on this apartment thread
        // only; every call is a checked method of it.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| code(&error))?;
        // SAFETY: as above.
        unsafe {
            let options = dialog.GetOptions().map_err(|error| code(&error))? | spec.options();
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
        owner: Owner,
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
            let options = dialog.GetOptions().map_err(|error| code(&error))? | spec.options();
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

    use flui_platform::capability::{Abandoned, WindowRef};
    use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{
        FILEOPENDIALOGOPTIONS, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
        FOS_FORCESHOWHIDDEN, FOS_OVERWRITEPROMPT, FOS_PICKFOLDERS, FileOpenDialog, IFileDialog,
        IFileOpenDialog,
    };
    use windows::core::Interface as _;

    use super::{
        CommonSpec, DialogSystem, FilterSpec, OpenSpec, OsAnswer, OsPick, SaveSpec, WindowsDialogs,
        apply_common, is_folder_not_there, open_answer, open_spec, save_answer, save_spec,
    };
    use crate::dialog::{
        Dialog, DialogError, FileDialogCapability, FileDialogs, FileFilter, OpenRequest,
        OpenResult, PickedFile, Picks, SaveLocation, SaveRequest, SaveResult,
    };
    use crate::testing::win32::{self, TestOwner, pump};
    use crate::win32::Owner;
    use crate::win32::modal::Dismisser;

    /// The value of the Win32 window the tests' dialogs are owned by, which
    /// no stand-in hands to the system.
    const OWNER: isize = 0x0F1E;

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
            .filter(FileFilter::new("Text").with_extension("txt"))
            .filter(
                FileFilter::new("Build files")
                    .with_file_name("Makefile")
                    .with_extension("mk")
                    .with_file_name(".env"),
            )
            .filter(FileFilter::all_files("All files"));
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
                        // A file name is its own pattern: no wildcard, so
                        // the dialog matches the whole name.
                        FilterSpec {
                            name: "Build files".to_owned(),
                            pattern: "*.mk;Makefile;.env".to_owned(),
                        },
                        FilterSpec {
                            name: "All files".to_owned(),
                            pattern: "*.*".to_owned(),
                        },
                    ],
                    show_hidden: false,
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
        let any = SaveRequest::new()
            .filter(FileFilter::all_files("All files"))
            .filter(FileFilter::new("Text").with_extension("txt"));
        assert_eq!(
            save_spec(&any).expect("maps").default_extension,
            None,
            "a first filter of every file names no extension to add"
        );
    }

    #[test]
    fn a_verbatim_folder_is_named_to_the_dialog_in_the_form_the_shell_reads() {
        let cases = [
            (r"\\?\C:\Users\me\Documents", r"C:\Users\me\Documents"),
            (r"\\?\UNC\server\share\docs", r"\\server\share\docs"),
            (r"C:\Users\me", r"C:\Users\me"),
            (r"\\server\share\docs", r"\\server\share\docs"),
        ];
        for (asked, named) in cases {
            let open = open_spec(&OpenRequest::folders().directory(asked)).expect("maps");
            assert_eq!(open.common.directory, Some(PathBuf::from(named)), "{asked}");
            let save = save_spec(&SaveRequest::new().directory(asked)).expect("maps");
            assert_eq!(save.common.directory, Some(PathBuf::from(named)), "{asked}");
        }
    }

    #[test]
    fn hidden_files_are_shown_whatever_the_users_setting_only_when_the_request_asks() {
        let shows = |options: FILEOPENDIALOGOPTIONS| options.contains(FOS_FORCESHOWHIDDEN);
        let open = |request: &OpenRequest| open_spec(request).expect("maps").options();
        let save = |request: &SaveRequest| save_spec(request).expect("maps").options();
        assert!(shows(open(&OpenRequest::files().show_hidden())));
        assert!(shows(open(&OpenRequest::folders().show_hidden())));
        assert!(shows(save(&SaveRequest::new().show_hidden())));
        assert!(!shows(open(&OpenRequest::files())));
        assert!(!shows(save(&SaveRequest::new())));

        // What each dialog always asks for stands beside it.
        let picked = open(&OpenRequest::folders().allow_multiple().show_hidden());
        assert!(picked.contains(
            FOS_PICKFOLDERS | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM
        ));
        assert!(save(&SaveRequest::new().show_hidden()).contains(FOS_OVERWRITEPROMPT));
    }

    /// The shell's own answers for a folder, through the dialog object it
    /// would be shown by: one that is not there is left out, one it cannot
    /// name otherwise fails the request. The dialog is made and never
    /// shown.
    #[test]
    fn a_folder_that_is_gone_is_left_out_and_one_the_shell_cannot_name_fails() {
        let _apartment = crate::win32::threads::Apartment::enter();
        // SAFETY: the dialog is created, used and released on this thread,
        // inside the apartment entered above; it is never shown.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
                .expect("the shell makes its open dialog");
        let dialog = dialog
            .cast::<IFileDialog>()
            .expect("an open dialog is a file dialog");
        let in_folder = |directory: PathBuf| CommonSpec {
            directory: Some(directory),
            ..CommonSpec::default()
        };

        let gone = temp(&format!(
            "flui-system-no-such-folder-{}",
            std::process::id()
        ));
        assert_eq!(
            apply_common(&dialog, &in_folder(gone.clone())),
            Ok(()),
            "a remembered folder deleted since opens the dialog where the system would"
        );
        assert_eq!(
            apply_common(&dialog, &in_folder(gone.join("deeper"))),
            Ok(()),
            "and so does one whose parent is gone"
        );
        let unnamed = apply_common(&dialog, &in_folder(PathBuf::from(r"C:\no<such>folder")));
        assert!(
            unnamed.is_err_and(|code| !is_folder_not_there(code)),
            "a folder the shell cannot name is not one that is merely gone: {unnamed:?}"
        );
        assert_eq!(
            open_answer(unnamed.map(|()| None)),
            Err(DialogError::Failed(SystemFailure::Os(
                unnamed.expect_err("checked above")
            ))),
            "which the caller hears as the system's failure"
        );
        assert_eq!(
            apply_common(&dialog, &in_folder(std::env::temp_dir())),
            Ok(()),
            "a folder that is there is taken"
        );
    }

    #[test]
    fn a_window_that_is_no_win32_window_is_refused_and_shows_no_dialog() {
        let owner = TestOwner::new();
        let (window, _closer) = win32::window();
        let (system, _answer, ended) = modal();
        let dialogs = dialogs_over(system, &owner, window);
        let no_window = Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
            FileDialogCapability,
        >(
            UnsupportedReason::NoWindow,
        )))));

        let opened = land(dialogs.open(OpenRequest::files()));
        owner.turn_until(|| opened.borrow().is_some());
        assert_eq!(*opened.borrow(), no_window, "a dialog is modal to a window");
        let saved: Rc<RefCell<Option<Result<SaveResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&saved);
        dialogs
            .save(SaveRequest::new())
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| saved.borrow().is_some());
        assert_eq!(
            saved.borrow_mut().take(),
            Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
                FileDialogCapability,
            >(
                UnsupportedReason::NoWindow,
            )))))
        );
        assert_eq!(
            ended.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "no dialog was shown without an owner"
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
        /// When set, a closed dialog stays in its thread until this hears:
        /// a system dialog that takes its time to go down.
        linger: Mutex<Option<mpsc::Receiver<()>>>,
    }

    impl DialogSystem for ModalSystem {
        fn open(&self, _: Owner, _: &OpenSpec, dismisser: &Dismisser) -> OsAnswer<Vec<OsPick>> {
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
                    let linger = self.linger.lock().expect("BUG: the test's lock").take();
                    if let Some(linger) = linger {
                        let _ = linger.recv();
                    }
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

        fn save(&self, _: Owner, _: &SaveSpec, _: &Dismisser) -> OsAnswer<(OsPick, u32)> {
            Ok(None)
        }
    }

    type Answers = mpsc::Sender<OsAnswer<Vec<OsPick>>>;

    fn modal() -> (Arc<ModalSystem>, Answers, mpsc::Receiver<Ended>) {
        let (system, answer, ended, _) = modal_lingering(false);
        (system, answer, ended)
    }

    fn modal_lingering(
        lingers: bool,
    ) -> (
        Arc<ModalSystem>,
        Answers,
        mpsc::Receiver<Ended>,
        Option<mpsc::Sender<()>>,
    ) {
        let (answer, answers) = mpsc::channel();
        let (end, ended) = mpsc::channel();
        let (let_go, linger) = mpsc::channel();
        let system = ModalSystem {
            answers: Mutex::new(answers),
            ended: Mutex::new(end),
            linger: Mutex::new(lingers.then_some(linger)),
        };
        (Arc::new(system), answer, ended, lingers.then_some(let_go))
    }

    fn dialogs_over(system: Arc<ModalSystem>, owner: &TestOwner, window: WindowRef) -> FileDialogs {
        FileDialogs::try_new(
            WindowsDialogs::over(system, Rc::default()),
            &owner.ctx(Some(window)),
        )
        .expect("a window")
    }

    /// Open a dialog on `dialogs` once the window's previous dialog thread
    /// has left its dialog, which it does just after its close ran.
    fn open_when_free(owner: &TestOwner, dialogs: &FileDialogs) -> Landed {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let next = land(dialogs.open(OpenRequest::files()));
            // A refusal reaches the caller two turns on: the backend's
            // answer settles at the first, the handle's at the second.
            owner.turn();
            owner.turn();
            if *next.borrow() != Some(Ok(Err(DialogError::Busy))) {
                return next;
            }
            assert!(std::time::Instant::now() < deadline, "the thread left");
            std::thread::sleep(Duration::from_millis(1));
        }
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
        let (window, _closer) = win32::win32_window(OWNER);
        let (system, answer, ended) = modal();
        let dialogs = dialogs_over(system, &owner, window);
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
        let (window, _closer) = win32::win32_window(OWNER);
        let (system, _answer, ended) = modal();
        let dialogs = dialogs_over(system, &owner, window);
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
        let next = open_when_free(&owner, &dialogs);
        assert_eq!(
            *next.borrow(),
            None,
            "the window is free: the next dialog is up"
        );
    }

    #[test]
    fn a_window_takes_no_new_dialog_until_the_dismissed_one_has_left_its_thread() {
        let owner = TestOwner::new();
        let (window, _closer) = win32::win32_window(OWNER);
        let (system, _answer, ended, let_go) = modal_lingering(true);
        let let_go = let_go.expect("a lingering dialog");
        let dialogs = dialogs_over(system, &owner, window);
        let dialog = dialogs.open(OpenRequest::files());
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Shown)
        );
        drop(dialog);
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::ClosedByItsDismissal)
        );

        // Closed, but still in its thread: the old dialog would re-enable
        // the window under a new one.
        let early = land(dialogs.open(OpenRequest::files()));
        owner.turn_until(|| early.borrow().is_some());
        assert_eq!(
            early.borrow_mut().take(),
            Some(Ok(Err(DialogError::Busy))),
            "a typed refusal while the old dialog is leaving"
        );

        let_go.send(()).expect("the old dialog lingers");
        let next = open_when_free(&owner, &dialogs);
        assert_eq!(*next.borrow(), None, "the next dialog is up");
        assert_eq!(
            ended.recv_timeout(Duration::from_secs(10)),
            Ok(Ended::Shown)
        );
    }

    #[test]
    fn a_dialog_ends_with_its_window_and_is_closed_on_its_thread() {
        let owner = TestOwner::new();
        let (window, closer) = win32::win32_window(OWNER);
        let (system, _answer, ended) = modal();
        let dialogs = dialogs_over(system, &owner, window);
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
