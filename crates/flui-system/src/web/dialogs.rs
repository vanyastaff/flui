//! The open and save dialogs of a page.
//!
//! - **Open**: the File System Access picker (`showOpenFilePicker`) where
//!   the browser has it (Chromium, in a secure context) and it can express
//!   the request's filters; `<input type=file>` otherwise, and in its place
//!   when the document may not show the API's picker (a cross-origin frame)
//!   while the page still handles the gesture. Every pick is checked against
//!   the filters, which an input only suggests, and the picks' declared sizes
//!   against the request's
//!   [`max_bytes`](crate::dialog::OpenRequest::max_bytes), before a byte is
//!   read; the bytes read are counted again, since a file may change between
//!   the two.
//! - **Folder**: the directory picker (`showDirectoryPicker`) where the
//!   browser has it, whose handle is walked for the files under it;
//!   `<input type=file webkitdirectory>` otherwise, and in the picker's
//!   place as for files, whose files say where they are under the folder
//!   (`webkitRelativePath`). Either way the answer is one folder, its name
//!   and its files at their paths under it
//!   ([`PickedForm::Folder`](crate::dialog::PickedForm::Folder)): files are
//!   all it carries, since the input reports nothing else. The folder is
//!   listed and checked once, before a byte is read: the walk goes no
//!   further than the request's
//!   [`max_entries`](crate::dialog::OpenRequest::max_entries), an input's
//!   files are counted before one is looked at, and the files' declared
//!   sizes are checked against `max_bytes`. A browser with neither API has
//!   no folder chooser
//!   ([`NotOnThisDevice`](UnsupportedReason::NotOnThisDevice)), and so has
//!   one whose input takes the attribute and still picks files, which shows
//!   only in its answer.
//! - **Save**: a request carries the contents it saves. The save picker
//!   (`showSaveFilePicker`) writes them where the user chose, even when the
//!   dialog was dropped after the user chose (the browser has made or
//!   emptied the file by then). Where the browser has no picker, refuses it,
//!   or the page is not handling a gesture (which the picker requires and a
//!   download does not), they are handed to the browser as a download
//!   ([`PickedForm::Download`](crate::dialog::PickedForm::Download)): where
//!   and whether that was written, the browser does not say.
//!
//! A browser gives a page no way to close its picker, and shows one at a
//! time: the page, every window and UI runtime of it, stays busy while one
//! is up, after its dialog was dropped too (`ShownPicker`). A picker of the
//! API ends when its promise settles. An input ends on its `change`, its
//! `cancel` (Chromium 113, Firefox 91, Safari 16.4) or the page being hidden
//! for good (`pagehide`). Only on an engine without `cancel` does it also
//! end when the window regains focus and no `change` follows within
//! [`FOCUS_GRACE_MS`], which answers a cancel: there a pick whose `change`
//! comes later than that (a large file a phone copies first, a folder the
//! browser lists first) is lost, the price of an end at all.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{BrowserException, SystemFailure, UnsupportedReason};
use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, Event, File, FileList, HtmlAnchorElement, HtmlInputElement, Url,
};

use super::page::{Page, call_promise, exception, method, property, set};
use super::rules::{self, PickerEnd};
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileFilter, FolderListing,
    FolderPath, OpenRequest, OpenResult, PickedFile, Picks, SaveLocation, SaveRequest, SaveResult,
};

/// How long a download's object URL outlives its click before it is
/// revoked: the browser starts the download asynchronously, and revoking the
/// URL first cancels it in some engines (`FileSaver.js` waits 40 s).
const DOWNLOAD_URL_LIFETIME_MS: i32 = 60_000;

/// How long an input waits for its `change` after the window regains focus
/// before it answers a cancel: the end of an input on an engine without its
/// `cancel` event ([`Page::input_fires_cancel`]), where the browser may
/// report the pick a moment after the focus.
pub(crate) const FOCUS_GRACE_MS: i32 = 3_000;

/// The document's expando that is set while a browser picker is up on the
/// page: kept on the document, so every window and every UI runtime of the
/// page sees the one picker a browser shows.
const PICKER_SHOWN: &str = "__fluiPickerShown";

/// A browser picker up on a page, from the call that shows it until the
/// browser ends it: the browser shows one at a time and refuses a second,
/// and a page cannot close one. While this lives, the page shows no other.
struct ShownPicker {
    document: web_sys::Document,
}

impl ShownPicker {
    /// Mark a picker as up on `page`; `None` while one already is,
    /// whichever window or UI runtime of the page showed it.
    fn try_on(page: &Page) -> Option<Self> {
        let document = page.document();
        if property(document.as_ref(), PICKER_SHOWN).is_some() {
            return None;
        }
        set(document.as_ref(), PICKER_SHOWN, &JsValue::TRUE);
        Some(Self {
            document: document.clone(),
        })
    }
}

impl Drop for ShownPicker {
    fn drop(&mut self) {
        let _ = Reflect::delete_property(self.document.as_ref(), &JsValue::from_str(PICKER_SHOWN));
    }
}

/// A page's file dialogs.
pub(crate) struct WebDialogs {
    page: Rc<Page>,
    focus_grace_ms: i32,
}

impl WebDialogs {
    pub(crate) fn new(page: Rc<Page>) -> Self {
        Self {
            page,
            focus_grace_ms: FOCUS_GRACE_MS,
        }
    }

    /// These dialogs, an input waiting `focus_grace_ms` after the window
    /// regains focus: a test's bound, shorter than the page's.
    #[cfg(test)]
    pub(crate) fn with_focus_grace(mut self, focus_grace_ms: i32) -> Self {
        self.focus_grace_ms = focus_grace_ms;
        self
    }
}

/// The refusal of a dialog for `reason`.
fn unsupported(reason: UnsupportedReason) -> DialogError {
    DialogError::Unsupported(flui_platform::Unsupported::of::<FileDialogCapability>(
        reason,
    ))
}

/// Answer `answer` with `error` and hold nothing: the dialog ended before
/// anything was shown.
fn answered<T: 'static>(
    answer: Resolver<Result<T, DialogError>>,
    error: DialogError,
) -> DialogHold {
    // A retired owner has nobody left to answer.
    let _ = answer.resolve(Err(error));
    DialogHold::new(())
}

/// What the handle holds for an open dialog: whether its caller waits, and
/// the input shown for it, at once or in place of a refused picker.
struct OpenHold {
    waiting: Rc<Cell<bool>>,
    input: Rc<RefCell<Option<Rc<InputPicker>>>>,
}

impl Drop for OpenHold {
    fn drop(&mut self) {
        self.waiting.set(false);
        let input = self.input.borrow_mut().take();
        if let Some(input) = input {
            input.let_go();
        }
    }
}

/// Set while a save's caller waits for its answer; cleared when the handle
/// drops the hold.
struct Waiting(Rc<Cell<bool>>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl FileDialogBackend for WebDialogs {
    fn open(
        &self,
        _: &WindowRef,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let picker = if request.picks_folders() {
            self.page.window_method("showDirectoryPicker")
        } else {
            self.page.window_method("showOpenFilePicker")
        };
        if request.picks_folders() && picker.is_none() && !input_takes_folders(&self.page) {
            // No gesture brings a folder chooser, so this refusal comes
            // before the one a later call could pass.
            return answered(answer, rules::no_folder_chooser());
        }
        if !self.page.is_activated() {
            return answered(answer, rules::unavailable());
        }
        let Some(shown) = ShownPicker::try_on(&self.page) else {
            return answered(answer, DialogError::Busy);
        };
        let hold = OpenHold {
            waiting: Rc::new(Cell::new(true)),
            input: Rc::default(),
        };
        let options = picker.and_then(|picker| Some((picker, picker_options(&request)?)));
        let request = Rc::new(request);
        match options {
            Some((picker, options)) => match call_promise(&picker, self.page.view(), &options) {
                Ok(promise) => self.open_with_picker(promise, shown, request, answer, &hold),
                Err(thrown) => self.end_picker(thrown, shown, &request, answer, &hold),
            },
            None => self.show_input(shown, &request, answer, &hold),
        }
        DialogHold::new(hold)
    }

    fn save(
        &self,
        _: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let Some(contents) = request.contents.clone() else {
            // A save without contents answers a path to write later, which
            // a page is never given.
            return answered(answer, unsupported(UnsupportedReason::NotOnThisPlatform));
        };
        let name = rules::save_name(request.suggested_name.as_deref(), &request.filters);
        let picker = self
            .page
            .window_method("showSaveFilePicker")
            .filter(|_| self.page.is_activated());
        let (Some(picker), Some(types)) = (picker, rules::picker_types(&request.filters)) else {
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(downloaded(&self.page, name, &contents, &request.filters));
            return DialogHold::new(());
        };
        let Some(shown) = ShownPicker::try_on(&self.page) else {
            return answered(answer, DialogError::Busy);
        };
        let options = Object::new();
        set(&options, "suggestedName", &JsValue::from_str(&name));
        set(&options, "types", &picker_types(&types.types));
        set(
            &options,
            "excludeAcceptAllOption",
            &(!types.accepts_all).into(),
        );
        match call_promise(&picker, self.page.view(), &options) {
            Ok(promise) => {
                self.save_with_picker(promise, shown, name, contents, request.filters, answer)
            }
            Err(thrown) if rules::is_activation_refusal(thrown) => {
                let _ = answer.resolve(downloaded(&self.page, name, &contents, &request.filters));
                DialogHold::new(())
            }
            Err(thrown) => answered(answer, failed(thrown)),
        }
    }
}

impl WebDialogs {
    /// Show an input for `request` in the place `hold` keeps for it, as the
    /// page's picker `shown` marks.
    fn show_input(
        &self,
        shown: ShownPicker,
        request: &Rc<OpenRequest>,
        answer: Resolver<OpenResult>,
        hold: &OpenHold,
    ) {
        let input = InputPicker::show(&self.page, shown, request, answer, self.focus_grace_ms);
        *hold.input.borrow_mut() = input;
    }

    /// End an open picker the browser refused with `thrown` at once: as a
    /// cancel, with the input in its place, or with the refusal.
    fn end_picker(
        &self,
        thrown: BrowserException,
        shown: ShownPicker,
        request: &Rc<OpenRequest>,
        answer: Resolver<OpenResult>,
        hold: &OpenHold,
    ) {
        match rules::picker_rejection(thrown, self.page.is_activated()) {
            PickerEnd::Cancelled => {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(Ok(None));
            }
            PickerEnd::FallBack => self.show_input(shown, request, answer, hold),
            PickerEnd::Refused(error) => {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(Err(error));
            }
        }
    }

    /// Answer `answer` when the open or directory picker's `promise`
    /// settles, with what was picked checked and read, or with the input in
    /// its place. The page's picker is up, as `shown` marks, until then, and
    /// while the input stands in.
    fn open_with_picker(
        &self,
        promise: js_sys::Promise,
        shown: ShownPicker,
        request: Rc<OpenRequest>,
        answer: Resolver<OpenResult>,
        hold: &OpenHold,
    ) {
        let waiting = Rc::clone(&hold.waiting);
        let input = Rc::clone(&hold.input);
        let page = Rc::clone(&self.page);
        let focus_grace_ms = self.focus_grace_ms;
        wasm_bindgen_futures::spawn_local(async move {
            let settled = JsFuture::from(promise).await;
            if !waiting.get() {
                // The caller let the dialog go: nobody reads the files.
                return;
            }
            let rejection = match settled {
                Ok(picked) => {
                    // The browser's picker is gone; reading holds no page.
                    drop(shown);
                    let result = answer_of(read_from_picker(&picked, &request, &waiting).await);
                    // A retired owner has nobody left to answer.
                    let _ = answer.resolve(result);
                    return;
                }
                Err(rejection) => exception(&rejection),
            };
            match rules::picker_rejection(rejection, page.is_activated()) {
                PickerEnd::Cancelled => {
                    // A retired owner has nobody left to answer.
                    let _ = answer.resolve(Ok(None));
                }
                PickerEnd::FallBack => {
                    let stood_in =
                        InputPicker::show(&page, shown, &request, answer, focus_grace_ms);
                    *input.borrow_mut() = stood_in;
                }
                PickerEnd::Refused(error) => {
                    // A retired owner has nobody left to answer.
                    let _ = answer.resolve(Err(error));
                }
            }
        });
    }

    /// Answer `answer` when the save picker's `promise` settles: after
    /// `contents` are written to the file the user chose, which the browser
    /// has made or emptied by then, so the write completes even when the
    /// caller let the dialog go; as a download when the browser refused the
    /// picker for want of a gesture or of an allowed document.
    fn save_with_picker(
        &self,
        promise: js_sys::Promise,
        shown: ShownPicker,
        name: String,
        contents: Arc<[u8]>,
        filters: Vec<FileFilter>,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let waiting = Rc::new(Cell::new(true));
        let still_waiting = Rc::clone(&waiting);
        let page = Rc::clone(&self.page);
        wasm_bindgen_futures::spawn_local(async move {
            let settled = JsFuture::from(promise).await;
            // The browser's picker is gone; writing holds no page.
            drop(shown);
            let result = match settled {
                Ok(handle) => write_through(&handle, &contents).await.and_then(|()| {
                    let written = property(&handle, "name")
                        .and_then(|name| name.as_string())
                        .unwrap_or_default();
                    let file = PickedFile::try_from_contents(written, contents)
                        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
                    Ok(Some(SaveLocation::new(file, None)))
                }),
                Err(rejection) => match exception(&rejection) {
                    BrowserException::Abort => Ok(None),
                    refusal if rules::is_activation_refusal(refusal) => {
                        if !still_waiting.get() {
                            // The caller let the dialog go before anything
                            // was chosen: nothing is downloaded.
                            return;
                        }
                        downloaded(&page, name, &contents, &filters)
                    }
                    other => Err(failed(other)),
                },
            };
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(result);
        });
        DialogHold::new(Waiting(waiting))
    }
}

/// Whether the page's file input says it takes a folder: it reflects
/// `webkitdirectory`. A browser may reflect the attribute and show a file
/// chooser all the same, which only its answer tells
/// ([`rules::input_folder`]).
fn input_takes_folders(page: &Page) -> bool {
    page.document()
        .create_element("input")
        .is_ok_and(|input| property(input.as_ref(), "webkitdirectory").is_some())
}

/// What `request`'s picker of the File System Access API is shown with: a
/// folder to read and never write, or the files' filters as the API takes
/// them. `None` for a filter the API cannot express, which sends the request
/// to the input.
fn picker_options(request: &OpenRequest) -> Option<Object> {
    let options = Object::new();
    if request.picks_folders() {
        set(&options, "mode", &JsValue::from_str("read"));
    } else {
        let types = rules::picker_types(&request.filters)?;
        set(&options, "multiple", &request.is_multiple().into());
        set(&options, "types", &picker_types(&types.types));
        set(
            &options,
            "excludeAcceptAllOption",
            &(!types.accepts_all).into(),
        );
    }
    Some(options)
}

/// The `types` option of a File System Access picker.
fn picker_types(types: &[rules::PickerType<'_>]) -> JsValue {
    let list = Array::new();
    for picker_type in types {
        let entry = Object::new();
        set(
            &entry,
            "description",
            &JsValue::from_str(picker_type.description),
        );
        let accept = Object::new();
        for (mime_type, extensions) in &picker_type.accept {
            let extensions = extensions
                .iter()
                .map(|extension| JsValue::from_str(extension))
                .collect::<Array>();
            set(&accept, mime_type, &extensions);
        }
        set(&entry, "accept", &accept);
        list.push(&entry);
    }
    list.into()
}

/// The browser's failure `exception` as a dialog's.
fn failed(exception: BrowserException) -> DialogError {
    DialogError::Failed(exception.into())
}

/// Await `promise`, or the browser's failure.
async fn settle(
    promise: Result<js_sys::Promise, BrowserException>,
) -> Result<JsValue, DialogError> {
    JsFuture::from(promise.map_err(failed)?)
        .await
        .map_err(|rejection| failed(exception(&rejection)))
}

/// The files the picker's `handles` (a list of `FileSystemFileHandle`s)
/// stand for.
async fn files_of_handles(handles: &JsValue) -> Result<Vec<File>, DialogError> {
    let handles = handles
        .dyn_ref::<Array>()
        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
    let mut files = Vec::new();
    for handle in handles.iter() {
        files.push(file_of_handle(&handle).await?);
    }
    Ok(files)
}

/// The file `handle` (a `FileSystemFileHandle`) stands for, as it is now.
async fn file_of_handle(handle: &JsValue) -> Result<File, DialogError> {
    let get_file =
        method(handle, "getFile").ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
    settle(call_promise(&get_file, handle, &JsValue::UNDEFINED))
        .await?
        .dyn_into::<File>()
        .map_err(|_| DialogError::Failed(SystemFailure::MalformedAnswer))
}

/// Why what the user picked was not read to an answer.
enum Unread {
    /// The dialog's caller let it go: nobody reads further, and nobody
    /// hears.
    LetGo,
    /// This refusal is the answer.
    Refused(DialogError),
}

impl From<DialogError> for Unread {
    fn from(refusal: DialogError) -> Self {
        Self::Refused(refusal)
    }
}

/// The answer of a dialog whose picks were `read`; a dialog its caller let
/// go answers a cancel nobody hears.
fn answer_of(read: Result<Option<Picks>, Unread>) -> OpenResult {
    match read {
        Ok(picks) => Ok(picks),
        Err(Unread::LetGo) => Ok(None),
        Err(Unread::Refused(refusal)) => Err(refusal),
    }
}

const MALFORMED: DialogError = DialogError::Failed(SystemFailure::MalformedAnswer);

/// What `request` answers to what its picker of the File System Access API
/// resolved with: the picked files (a list of `FileSystemFileHandle`s) or
/// the picked folder (a `FileSystemDirectoryHandle`), checked and read.
async fn read_from_picker(
    picked: &JsValue,
    request: &OpenRequest,
    waiting: &Cell<bool>,
) -> Result<Option<Picks>, Unread> {
    if !request.picks_folders() {
        let files = files_of_handles(picked).await?;
        return read_picks(&files, request, waiting).await;
    }
    let listed = walk_folder(picked, request.max_entries, waiting).await?;
    let listed = listed
        .try_map(async |handle: JsValue| -> Result<File, Unread> {
            if !waiting.get() {
                return Err(Unread::LetGo);
            }
            Ok(file_of_handle(&handle).await?)
        })
        .await?;
    read_folder(listed, request.max_bytes, waiting).await
}

/// What `request` answers to the files its input holds: the picked files,
/// or the folder their `webkitRelativePath`s name, checked and read. A
/// folder's files are counted against the request's bound before one of
/// them is looked at.
async fn read_from_input(
    list: Option<FileList>,
    request: &OpenRequest,
    waiting: &Cell<bool>,
) -> Result<Option<Picks>, Unread> {
    let Some(list) = list else {
        return Ok(None);
    };
    let files = (0..list.length()).filter_map(|at| list.get(at));
    if !request.picks_folders() {
        return read_picks(&files.collect::<Vec<File>>(), request, waiting).await;
    }
    let listed = usize::try_from(list.length()).unwrap_or(usize::MAX);
    rules::admit_entries(listed, request.max_entries)?;
    let placed = files.map(|file| {
        let relative_path = property(file.as_ref(), "webkitRelativePath")
            .and_then(|path| path.as_string())
            .unwrap_or_default();
        (relative_path, file)
    });
    match rules::input_folder(placed)? {
        Some(listed) => read_folder(listed, request.max_bytes, waiting).await,
        None => Ok(None),
    }
}

/// Walk `directory` (a `FileSystemDirectoryHandle`) for the files under it,
/// at any depth, each a `FileSystemFileHandle` at its path. The walk meets
/// at most `max_entries` files and folders: one more is
/// [`DialogError::TooManyEntries`], and it ends there, as it does once
/// `waiting` clears. A folder is walked through its `values()`, an async
/// iterator of the handles it holds.
async fn walk_folder(
    directory: &JsValue,
    max_entries: usize,
    waiting: &Cell<bool>,
) -> Result<FolderListing<JsValue>, Unread> {
    let text = |handle: &JsValue, name: &str| property(handle, name)?.as_string();
    let mut files: Vec<(FolderPath, JsValue)> = Vec::new();
    // The folders still to walk, each at its path under the picked one,
    // which has none.
    let mut folders: Vec<(Option<FolderPath>, JsValue)> = vec![(None, directory.clone())];
    let mut met: usize = 0;
    while let Some((folder_path, folder)) = folders.pop() {
        let entries = method(&folder, "values")
            .ok_or(MALFORMED)?
            .call0(&folder)
            .map_err(|thrown| failed(exception(&thrown)))?;
        let next = method(&entries, "next").ok_or(MALFORMED)?;
        loop {
            if !waiting.get() {
                return Err(Unread::LetGo);
            }
            let step = settle(call_promise(&next, &entries, &JsValue::UNDEFINED)).await?;
            if property(&step, "done").is_some_and(|done| done.is_truthy()) {
                break;
            }
            let entry = property(&step, "value").ok_or(MALFORMED)?;
            met = met.saturating_add(1);
            rules::admit_entries(met, max_entries)?;
            let name = text(&entry, "name").ok_or(MALFORMED)?;
            let path = match &folder_path {
                Some(folder_path) => folder_path.try_child(&name),
                None => FolderPath::try_from_name(&name),
            }
            .ok_or(MALFORMED)?;
            match text(&entry, "kind").as_deref() {
                Some("file") => files.push((path, entry)),
                Some("directory") => folders.push((Some(path), entry)),
                _ => return Err(MALFORMED.into()),
            }
        }
    }
    let name = text(directory, "name").ok_or(MALFORMED)?;
    Ok(FolderListing::try_new(name, files).ok_or(MALFORMED)?)
}

/// The pick of the folder `listed`: its files' declared sizes checked
/// against `limit` together, then each read ([`Reading`]).
async fn read_folder(
    listed: FolderListing<File>,
    limit: u64,
    waiting: &Cell<bool>,
) -> Result<Option<Picks>, Unread> {
    let mut reading = Reading::try_new(listed.held(), limit, waiting)?;
    let read = listed
        .try_map(async |file: File| reading.read(&file).await)
        .await?;
    Ok(Picks::try_new(vec![read.into_pick()]))
}

/// The picks `files` stand for: each is checked against the request's
/// filters before anything is read, their declared sizes against its
/// `max_bytes` together, then each is read ([`Reading`]). No file is an
/// empty answer, which a browser gives for a dismissed picker.
async fn read_picks(
    files: &[File],
    request: &OpenRequest,
    waiting: &Cell<bool>,
) -> Result<Option<Picks>, Unread> {
    let described: Vec<(String, String)> = files
        .iter()
        .map(|file| (file.name(), file.type_()))
        .collect();
    rules::admit_picks(
        described
            .iter()
            .map(|(name, mime_type)| (name.as_str(), Some(mime_type.as_str()))),
        &request.filters,
    )?;
    let mut reading = Reading::try_new(files, request.max_bytes, waiting)?;
    let mut picks = Vec::with_capacity(files.len());
    for (file, (name, _)) in files.iter().zip(described) {
        let contents = reading.read(file).await?;
        picks.push(PickedFile::try_from_contents(name, contents).ok_or(MALFORMED)?);
    }
    Ok(Picks::try_new(picks))
}

/// The files of one answer being read, within its request's `max_bytes`
/// together and while its caller waits.
struct Reading<'a> {
    limit: u64,
    /// The bytes read so far.
    read: u64,
    waiting: &'a Cell<bool>,
}

impl<'a> Reading<'a> {
    /// Start reading `files` within `limit` bytes together: the sizes the
    /// browser declares for them are checked first, so
    /// [`DialogError::TooLarge`] comes before a byte is read.
    fn try_new<'f>(
        files: impl IntoIterator<Item = &'f File>,
        limit: u64,
        waiting: &'a Cell<bool>,
    ) -> Result<Self, DialogError> {
        rules::admit_sizes(files.into_iter().map(|file| file.size()), limit)?;
        Ok(Self {
            limit,
            read: 0,
            waiting,
        })
    }

    /// What `file` holds, copied once into a buffer of its own. The bytes
    /// read are counted again as they arrive, since a file may change after
    /// its size was declared; reading stops once nobody waits.
    async fn read(&mut self, file: &File) -> Result<Arc<[u8]>, Unread> {
        if !self.waiting.get() {
            return Err(Unread::LetGo);
        }
        let buffer = settle(Ok(file.array_buffer())).await?;
        let bytes = Uint8Array::new(&buffer);
        self.read = self.read.saturating_add(u64::from(bytes.length()));
        crate::dialog::admit_read(self.read, self.limit)?;
        let length = usize::try_from(bytes.length()).map_err(|_| DialogError::TooLarge {
            actual: self.read,
            limit: self.limit,
        })?;
        let mut held: Arc<[u8]> = std::iter::repeat_n(0, length).collect();
        bytes.copy_to(Arc::get_mut(&mut held).expect("BUG: a buffer just made has no other owner"));
        Ok(held)
    }
}

/// Write `contents` through the save picker's `handle` (a
/// `FileSystemFileHandle`): a writable stream, written and closed; a stream
/// that fails is aborted, so the file keeps what it held.
async fn write_through(handle: &JsValue, contents: &[u8]) -> Result<(), DialogError> {
    let create = method(handle, "createWritable")
        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
    let stream = settle(call_promise(&create, handle, &JsValue::UNDEFINED)).await?;
    let written = async {
        let write =
            method(&stream, "write").ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
        let bytes = Uint8Array::from(contents);
        settle(call_promise(&write, &stream, &bytes)).await?;
        let close =
            method(&stream, "close").ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
        settle(call_promise(&close, &stream, &JsValue::UNDEFINED)).await
    }
    .await;
    if written.is_err()
        && let Some(abort) = method(&stream, "abort")
    {
        // The write's failure is the answer; the abort's own is not.
        let _ = abort.call0(&stream);
    }
    written.map(|_| ())
}

/// `contents` handed to the browser as a download offered as `name`, and
/// the answer that says so.
fn downloaded(page: &Page, name: String, contents: &[u8], filters: &[FileFilter]) -> SaveResult {
    download(page, &name, contents, filters)?;
    let file = PickedFile::try_download(name)
        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
    Ok(Some(SaveLocation::new(file, None)))
}

/// Download `contents` as `name`: a blob's object URL, clicked as a link's
/// download, and revoked [`DOWNLOAD_URL_LIFETIME_MS`] later.
fn download(
    page: &Page,
    name: &str,
    contents: &[u8],
    filters: &[FileFilter],
) -> Result<(), DialogError> {
    let host = page.host().ok_or(rules::unavailable())?;
    let options = BlobPropertyBag::new();
    options.set_type(rules::download_type(filters));
    let parts = Array::of1(&Uint8Array::from(contents));
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|thrown| failed(exception(&thrown)))?;
    let url =
        Url::create_object_url_with_blob(&blob).map_err(|thrown| failed(exception(&thrown)))?;
    let link = page
        .document()
        .create_element("a")
        .map_err(|thrown| failed(exception(&thrown)))?
        .unchecked_into::<HtmlAnchorElement>();
    link.set_href(&url);
    link.set_download(name);
    link.set_hidden(true);
    // Firefox follows a link's download only from a link in the document.
    let attached = host.append_child(&link);
    link.click();
    if attached.is_ok() {
        let _ = host.remove_child(&link);
    }
    let revoke_url = url.clone();
    let revoke = Closure::once_into_js(move || {
        let _ = Url::revoke_object_url(&revoke_url);
    });
    if page
        .view()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            revoke.unchecked_ref(),
            DOWNLOAD_URL_LIFETIME_MS,
        )
        .is_err()
    {
        // No timer: the URL goes now, at the risk of this download, rather
        // than for the page's life.
        let _ = Url::revoke_object_url(&url);
    }
    Ok(())
}

/// The listeners an input keeps while it is up: its `change` and `cancel`,
/// the window's `pagehide`, and, on an engine that fires no `cancel`, the
/// bound that ends it.
struct InputListeners {
    change: Closure<dyn FnMut(Event)>,
    cancel: Closure<dyn FnMut(Event)>,
    hidden: Closure<dyn FnMut(Event)>,
    focus_bound: Option<FocusBound>,
}

/// How an input ends on an engine whose file input fires no `cancel`: the
/// window's `focus`, and the timer it starts, which answers a cancel when no
/// `change` came first.
struct FocusBound {
    focus: Closure<dyn FnMut(Event)>,
    grace: Closure<dyn FnMut()>,
}

/// An `<input type=file>` shown for one open dialog, until the browser
/// answers it. It outlives its dialog's hold: while the browser's chooser
/// is up the page is busy, whether anyone waits for the answer or not.
struct InputPicker {
    input: HtmlInputElement,
    view: web_sys::Window,
    /// The page's picker, up until the input ends.
    shown: Cell<Option<ShownPicker>>,
    answer: Cell<Option<Resolver<OpenResult>>>,
    /// What its files are checked and read by.
    request: Rc<OpenRequest>,
    /// Cleared when the dialog's caller lets it go.
    waiting: Rc<Cell<bool>>,
    ended: Cell<bool>,
    focus_grace_ms: i32,
    grace_timer: Cell<Option<i32>>,
    /// Alive until the input itself drops, since one may be running.
    listeners: RefCell<Option<InputListeners>>,
    /// The input's own hold on itself after its dialog's hold dropped,
    /// released once it ends.
    alive: RefCell<Option<Rc<InputPicker>>>,
}

impl InputPicker {
    /// Show an input for `request` in `page`, whose picker `shown` marks as
    /// up until the input ends, answering `answer` with what the user
    /// picks; `None` when it ended before it was shown, `answer` answered.
    ///
    /// The input ends on its `change`, its `cancel` or the page's
    /// `pagehide`. Where the engine fires no `cancel`
    /// ([`Page::input_fires_cancel`]) it also ends `focus_grace_ms` after
    /// the window regains focus with no `change`; an engine that fires it is
    /// never timed, so a pick it reports late (a folder it lists first, or
    /// asks the user to confirm) is still the answer.
    fn show(
        page: &Page,
        shown: ShownPicker,
        request: &Rc<OpenRequest>,
        answer: Resolver<OpenResult>,
        focus_grace_ms: i32,
    ) -> Option<Rc<Self>> {
        let input = match page.document().create_element("input") {
            Ok(element) => element.unchecked_into::<HtmlInputElement>(),
            Err(thrown) => {
                let _ = answer.resolve(Err(failed(exception(&thrown))));
                return None;
            }
        };
        input.set_type("file");
        if request.picks_folders() {
            // A browser's folder chooser takes one folder, whatever the
            // input's `multiple` says, and a folder dialog has no filters.
            input.set_webkitdirectory(true);
        } else {
            input.set_multiple(request.is_multiple());
            input.set_accept(&rules::accept_attribute(&request.filters));
        }
        input.set_hidden(true);
        // In the document, or Safari on iOS may not fire `change`.
        let Some(host) = page.host() else {
            let _ = answer.resolve(Err(rules::unavailable()));
            return None;
        };
        if let Err(thrown) = host.append_child(&input) {
            let _ = answer.resolve(Err(failed(exception(&thrown))));
            return None;
        }
        let picker = Rc::new(Self {
            input,
            view: page.view().clone(),
            shown: Cell::new(Some(shown)),
            answer: Cell::new(Some(answer)),
            request: Rc::clone(request),
            waiting: Rc::new(Cell::new(true)),
            ended: Cell::new(false),
            focus_grace_ms,
            grace_timer: Cell::new(None),
            listeners: RefCell::new(None),
            alive: RefCell::new(None),
        });
        let listeners = InputListeners {
            change: picker.listener(Self::picked),
            cancel: picker.listener(|picker| picker.finish(Ok(None))),
            hidden: picker.listener(|picker| picker.finish(Ok(None))),
            focus_bound: (!page.input_fires_cancel()).then(|| FocusBound {
                focus: picker.listener(Self::focused),
                grace: {
                    let ended = Rc::downgrade(&picker);
                    Closure::new(move || {
                        if let Some(picker) = ended.upgrade() {
                            picker.finish(Ok(None));
                        }
                    })
                },
            }),
        };
        let _ = picker
            .input
            .add_event_listener_with_callback("change", listeners.change.as_ref().unchecked_ref());
        let _ = picker
            .input
            .add_event_listener_with_callback("cancel", listeners.cancel.as_ref().unchecked_ref());
        let _ = picker.view.add_event_listener_with_callback(
            "pagehide",
            listeners.hidden.as_ref().unchecked_ref(),
        );
        if let Some(bound) = &listeners.focus_bound {
            let _ = picker
                .view
                .add_event_listener_with_callback("focus", bound.focus.as_ref().unchecked_ref());
        }
        *picker.listeners.borrow_mut() = Some(listeners);

        // `showPicker` where the browser has it, which throws a refusal a
        // `click` would swallow.
        let opened = if let Some(show_picker) = method(picker.input.as_ref(), "showPicker") {
            show_picker
                .call0(picker.input.as_ref())
                .map(|_| ())
                .map_err(|thrown| exception(&thrown))
        } else {
            picker.input.click();
            Ok(())
        };
        if let Err(refused) = opened {
            let error = if rules::is_activation_refusal(refused) {
                rules::unavailable()
            } else {
                failed(refused)
            };
            picker.finish(Err(error));
        }
        Some(picker)
    }

    /// A listener that runs `on` with the input while it lives.
    fn listener(self: &Rc<Self>, on: fn(&Rc<Self>)) -> Closure<dyn FnMut(Event)> {
        let input = Rc::downgrade(self);
        Closure::new(move |_: Event| {
            if let Some(input) = input.upgrade() {
                on(&input);
            }
        })
    }

    /// The input's `change`: check and read what it holds, and answer.
    fn picked(self: &Rc<Self>) {
        let files = self.input.files();
        if self.ended.replace(true) {
            return;
        }
        let answer = self.answer.take();
        self.detach();
        let Some(answer) = answer else {
            return;
        };
        let request = Rc::clone(&self.request);
        let waiting = Rc::clone(&self.waiting);
        wasm_bindgen_futures::spawn_local(async move {
            let result = answer_of(read_from_input(files, &request, &waiting).await);
            if waiting.get() {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(result);
            }
        });
    }

    /// The window regained focus on an engine without `cancel`: the chooser
    /// has closed, and nothing more comes if nothing was picked.
    fn focused(self: &Rc<Self>) {
        if self.ended.get() {
            return;
        }
        if let Some(timer) = self.grace_timer.take() {
            self.view.clear_timeout_with_handle(timer);
        }
        let listeners = self.listeners.borrow();
        let Some(bound) = listeners
            .as_ref()
            .and_then(|listeners| listeners.focus_bound.as_ref())
        else {
            return;
        };
        if let Ok(timer) = self
            .view
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                bound.grace.as_ref().unchecked_ref(),
                self.focus_grace_ms,
            )
        {
            self.grace_timer.set(Some(timer));
        }
    }

    /// End with `result`, once, answering it if the caller still waits.
    fn finish(&self, result: OpenResult) {
        if self.ended.replace(true) {
            return;
        }
        let answer = self.answer.take();
        self.detach();
        if let Some(answer) = answer
            && self.waiting.get()
        {
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(result);
        }
    }

    /// The caller let the dialog go: nobody waits for the answer, and the
    /// input keeps itself, and the page busy, until the browser ends it.
    fn let_go(self: &Rc<Self>) {
        self.waiting.set(false);
        drop(self.answer.take());
        if !self.ended.get() {
            *self.alive.borrow_mut() = Some(Rc::clone(self));
        }
    }

    /// Take the input out of the page, stop listening and free the page.
    /// The listeners stay alive until the input drops, since one may be
    /// running; the input's hold on itself is released after the current
    /// task, for the same reason.
    fn detach(&self) {
        drop(self.shown.take());
        if let Some(timer) = self.grace_timer.take() {
            self.view.clear_timeout_with_handle(timer);
        }
        if let Some(listeners) = self.listeners.borrow().as_ref() {
            let _ = self.input.remove_event_listener_with_callback(
                "change",
                listeners.change.as_ref().unchecked_ref(),
            );
            let _ = self.input.remove_event_listener_with_callback(
                "cancel",
                listeners.cancel.as_ref().unchecked_ref(),
            );
            let _ = self.view.remove_event_listener_with_callback(
                "pagehide",
                listeners.hidden.as_ref().unchecked_ref(),
            );
            if let Some(bound) = &listeners.focus_bound {
                let _ = self.view.remove_event_listener_with_callback(
                    "focus",
                    bound.focus.as_ref().unchecked_ref(),
                );
            }
        }
        self.input.remove();
        let alive = self.alive.borrow_mut().take();
        if let Some(alive) = alive {
            wasm_bindgen_futures::spawn_local(async move { drop(alive) });
        }
    }
}
