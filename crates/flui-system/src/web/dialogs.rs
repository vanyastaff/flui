//! The open and save dialogs of a page.
//!
//! - **Open**: the File System Access picker (`showOpenFilePicker`) where
//!   the browser has it (Chromium, in a secure context) and it can express
//!   the request's filters; `<input type=file>` otherwise. Either way the
//!   picks' declared sizes are checked against the request's
//!   [`max_bytes`](crate::dialog::OpenRequest::max_bytes) before a byte is
//!   read, and the bytes read are counted against it again, since a file may
//!   change between the two.
//! - **Save**: a request carries the contents it saves. The save picker
//!   (`showSaveFilePicker`) writes them where the user chose; where the
//!   browser has none, or the page is not handling a gesture (which the
//!   picker requires and a download does not), they are downloaded under the
//!   suggested name, and the answer is the file the page asked for, since a
//!   download cannot report where it was saved.
//!
//! A browser gives a page no way to close its picker. Dropping a dialog's
//! hold stops its answer and frees the window; a picker still up keeps the
//! page busy until the browser answers it, since a page shows one picker at
//! a time. `<input type=file>` answers a dismissal with its `cancel` event
//! (Chromium 113, Firefox 91, Safari 16.4); an engine without the event
//! answers nothing for a dismissed input, and its dialog ends only when its
//! caller drops it or its window closes. No focus-return heuristic stands
//! in: a pick whose `change` comes after the window regains focus (a large
//! file a phone copies first) would be answered as a cancel.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{BrowserException, SystemFailure, Unsupported, UnsupportedReason};
use js_sys::{Array, Object, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, Event, File, HtmlAnchorElement, HtmlInputElement, Url};

use super::page::{Page, call_promise, exception, method, property, set};
use super::rules;
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileFilter, OpenRequest,
    OpenResult, PickedFile, Picks, SaveLocation, SaveRequest, SaveResult,
};

/// How long a download's object URL outlives its click before it is
/// revoked: the browser starts the download asynchronously, and revoking the
/// URL first cancels it in some engines (`FileSaver.js` waits 40 s).
const DOWNLOAD_URL_LIFETIME_MS: i32 = 60_000;

/// Whether a browser picker is up on the page: the browser shows one at a
/// time and refuses a second, and a page cannot close one. Kept per UI
/// runtime.
#[derive(Debug, Default)]
pub(crate) struct PagePicker {
    showing: Cell<bool>,
}

/// A page's file dialogs.
pub(crate) struct WebDialogs {
    page: Rc<Page>,
    picker: Rc<PagePicker>,
}

impl WebDialogs {
    pub(crate) fn new(page: Rc<Page>, picker: Rc<PagePicker>) -> Self {
        Self { page, picker }
    }
}

/// The refusal of a dialog for `reason`.
fn unsupported(reason: UnsupportedReason) -> DialogError {
    DialogError::Unsupported(Unsupported::of::<FileDialogCapability>(reason))
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

/// Set while the dialog's caller waits for its answer; cleared when the
/// handle drops the hold.
struct Waiting(Rc<Cell<bool>>);

impl Waiting {
    fn new() -> (Self, Rc<Cell<bool>>) {
        let waiting = Rc::new(Cell::new(true));
        (Self(Rc::clone(&waiting)), waiting)
    }
}

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
        if request.picks_folders() {
            // A browser can hand a page a folder (`showDirectoryPicker`, a
            // `webkitdirectory` input), but no pick this handle has carries
            // a folder's tree.
            return answered(answer, unsupported(UnsupportedReason::NotYetOnThisHost));
        }
        if !self.page.is_activated() {
            return answered(answer, unsupported(UnsupportedReason::Unavailable));
        }
        if self.picker.showing.get() {
            return answered(answer, DialogError::Busy);
        }
        let picker = self.page.window_method("showOpenFilePicker");
        match (picker, rules::picker_types(&request.filters)) {
            (Some(picker), Some(types)) => {
                let options = Object::new();
                set(&options, "multiple", &request.is_multiple().into());
                set(&options, "types", &picker_types(&types));
                set(
                    &options,
                    "excludeAcceptAllOption",
                    &(!request.filters.is_empty()).into(),
                );
                match call_promise(&picker, self.page.view(), &options) {
                    Ok(promise) => self.open_with_picker(promise, request.byte_limit(), answer),
                    Err(thrown) => answered(answer, DialogError::Failed(thrown.into())),
                }
            }
            _ => InputPicker::show(&self.page, &self.picker, &request, answer),
        }
    }

    fn save(
        &self,
        _: &WindowRef,
        request: SaveRequest,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let Some(contents) = request.contents_to_write().cloned() else {
            // A save without contents answers a path to write later, which
            // a page is never given.
            return answered(answer, unsupported(UnsupportedReason::NotOnThisPlatform));
        };
        let name = rules::save_name(request.suggested_name.as_deref(), &request.filters);
        let picker = self
            .page
            .window_method("showSaveFilePicker")
            .filter(|_| self.page.is_activated());
        match (picker, rules::picker_types(&request.filters)) {
            (Some(picker), Some(types)) => {
                if self.picker.showing.get() {
                    return answered(answer, DialogError::Busy);
                }
                let options = Object::new();
                set(&options, "suggestedName", &JsValue::from_str(&name));
                set(&options, "types", &picker_types(&types));
                set(
                    &options,
                    "excludeAcceptAllOption",
                    &(!request.filters.is_empty()).into(),
                );
                match call_promise(&picker, self.page.view(), &options) {
                    Ok(promise) => self.save_with_picker(promise, contents, answer),
                    Err(thrown) => answered(answer, DialogError::Failed(thrown.into())),
                }
            }
            _ => {
                let saved = download(&self.page, &name, &contents, &request.filters);
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(saved.and_then(|()| {
                    let file = PickedFile::try_from_contents(name, contents)
                        .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
                    Ok(Some(SaveLocation::new(file, None)))
                }));
                DialogHold::new(())
            }
        }
    }
}

impl WebDialogs {
    /// Answer `answer` when the open picker's `promise` settles, with the
    /// picked files read within `limit`.
    fn open_with_picker(
        &self,
        promise: js_sys::Promise,
        limit: u64,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let (hold, waiting) = Waiting::new();
        let picker = Rc::clone(&self.picker);
        picker.showing.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let settled = JsFuture::from(promise).await;
            picker.showing.set(false);
            if !waiting.get() {
                // The caller let the dialog go: nobody reads the files.
                return;
            }
            let result = match settled {
                Ok(handles) => match files_of_handles(&handles).await {
                    Ok(files) => read_picks(files, limit, &waiting).await,
                    Err(failure) => Err(failure),
                },
                Err(rejection) => {
                    rules::picker_rejection(exception(&rejection)).map_or(Ok(None), Err)
                }
            };
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(result);
        });
        DialogHold::new(hold)
    }

    /// Answer `answer` when the save picker's `promise` settles, after
    /// `contents` are written to the file the user chose.
    fn save_with_picker(
        &self,
        promise: js_sys::Promise,
        contents: Arc<[u8]>,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let (hold, waiting) = Waiting::new();
        let picker = Rc::clone(&self.picker);
        picker.showing.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let settled = JsFuture::from(promise).await;
            picker.showing.set(false);
            let handle = match settled {
                Ok(handle) => handle,
                Err(rejection) => {
                    let result =
                        rules::picker_rejection(exception(&rejection)).map_or(Ok(None), Err);
                    let _ = answer.resolve(result);
                    return;
                }
            };
            if !waiting.get() {
                // The caller let the dialog go before the user chose: the
                // contents are not written.
                return;
            }
            let result = write_through(&handle, &contents).await.and_then(|()| {
                let name = property(&handle, "name")
                    .and_then(|name| name.as_string())
                    .unwrap_or_default();
                let file = PickedFile::try_from_contents(name, contents)
                    .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
                Ok(Some(SaveLocation::new(file, None)))
            });
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(result);
        });
        DialogHold::new(hold)
    }
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
        let get_file = method(&handle, "getFile")
            .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?;
        let file = settle(call_promise(&get_file, &handle, &JsValue::UNDEFINED)).await?;
        files.push(
            file.dyn_into::<File>()
                .map_err(|_| DialogError::Failed(SystemFailure::MalformedAnswer))?,
        );
    }
    Ok(files)
}

/// The picks `files` stand for, read within `limit` together: their
/// declared sizes are checked before anything is read, and the bytes read
/// again as they arrive. Reading stops once `waiting` clears. No file is an
/// empty answer, which a browser gives for a dismissed picker.
async fn read_picks(files: Vec<File>, limit: u64, waiting: &Cell<bool>) -> OpenResult {
    rules::admit_sizes(files.iter().map(|file| file.size()), limit)?;
    let mut read: u64 = 0;
    let mut picks = Vec::with_capacity(files.len());
    for file in files {
        if !waiting.get() {
            return Ok(None);
        }
        let buffer = settle(Ok(file.array_buffer())).await?;
        let bytes = Uint8Array::new(&buffer);
        read = read.saturating_add(u64::from(bytes.length()));
        rules::admit_read(read, limit)?;
        let contents: Arc<[u8]> = Arc::from(bytes.to_vec());
        picks.push(
            PickedFile::try_from_contents(file.name(), contents)
                .ok_or(DialogError::Failed(SystemFailure::MalformedAnswer))?,
        );
    }
    Ok(Picks::try_new(picks))
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

/// Download `contents` as `name`: a blob's object URL, clicked as a link's
/// download, and revoked [`DOWNLOAD_URL_LIFETIME_MS`] later.
fn download(
    page: &Page,
    name: &str,
    contents: &[u8],
    filters: &[FileFilter],
) -> Result<(), DialogError> {
    let host = page
        .host()
        .ok_or(unsupported(UnsupportedReason::Unavailable))?;
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

/// An `<input type=file>` shown for one open dialog, until the browser
/// answers or the dialog's caller lets it go.
struct InputPicker {
    input: HtmlInputElement,
    picker: Rc<PagePicker>,
    answer: Cell<Option<Resolver<OpenResult>>>,
    limit: u64,
    waiting: Rc<Cell<bool>>,
    /// The `change` and `cancel` listeners, kept while the input is up.
    listeners: RefCell<Option<[Closure<dyn FnMut(Event)>; 2]>>,
}

impl InputPicker {
    /// Show an input for `request` in `page`, answering `answer` with what
    /// the user picks.
    fn show(
        page: &Page,
        picker: &Rc<PagePicker>,
        request: &OpenRequest,
        answer: Resolver<OpenResult>,
    ) -> DialogHold {
        let input = match page.document().create_element("input") {
            Ok(element) => element.unchecked_into::<HtmlInputElement>(),
            Err(thrown) => return answered(answer, failed(exception(&thrown))),
        };
        input.set_type("file");
        input.set_multiple(request.is_multiple());
        input.set_accept(&rules::accept_attribute(&request.filters));
        input.set_hidden(true);
        // In the document, or Safari on iOS may not fire `change`.
        let Some(host) = page.host() else {
            return answered(answer, unsupported(UnsupportedReason::Unavailable));
        };
        if let Err(thrown) = host.append_child(&input) {
            return answered(answer, failed(exception(&thrown)));
        }
        let shown = Rc::new(Self {
            input,
            picker: Rc::clone(picker),
            answer: Cell::new(Some(answer)),
            limit: request.byte_limit(),
            waiting: Rc::new(Cell::new(true)),
            listeners: RefCell::new(None),
        });
        let changed = Rc::downgrade(&shown);
        let change = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            if let Some(shown) = changed.upgrade() {
                shown.picked();
            }
        });
        let cancelled = Rc::downgrade(&shown);
        let cancel = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            if let Some(shown) = cancelled.upgrade() {
                shown.finish(Ok(None));
            }
        });
        let _ = shown
            .input
            .add_event_listener_with_callback("change", change.as_ref().unchecked_ref());
        let _ = shown
            .input
            .add_event_listener_with_callback("cancel", cancel.as_ref().unchecked_ref());
        *shown.listeners.borrow_mut() = Some([change, cancel]);
        shown.picker.showing.set(true);

        // `showPicker` where the browser has it, which throws a refusal a
        // `click` would swallow.
        let opened = match method(shown.input.as_ref(), "showPicker") {
            Some(show_picker) => show_picker
                .call0(shown.input.as_ref())
                .map(|_| ())
                .map_err(|thrown| exception(&thrown)),
            None => {
                shown.input.click();
                Ok(())
            }
        };
        if let Err(refused) = opened {
            shown.finish(Err(failed(refused)));
        }
        DialogHold::new(InputHold(shown))
    }

    /// The input's `change`: read what it holds and answer.
    fn picked(self: Rc<Self>) {
        let files = self.input.files();
        let Some(answer) = self.answer.take() else {
            return;
        };
        self.detach();
        let files: Vec<File> = files
            .map(|list| (0..list.length()).filter_map(|at| list.get(at)).collect())
            .unwrap_or_default();
        let limit = self.limit;
        let waiting = Rc::clone(&self.waiting);
        wasm_bindgen_futures::spawn_local(async move {
            let result = read_picks(files, limit, &waiting).await;
            if waiting.get() {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(result);
            }
        });
    }

    /// Answer `result`, once, and take the input down.
    fn finish(&self, result: OpenResult) {
        if let Some(answer) = self.answer.take() {
            self.detach();
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(result);
        }
    }

    /// Take the input out of the page and stop listening to it. The
    /// listeners stay alive until the picker drops, since one may be
    /// running.
    fn detach(&self) {
        self.picker.showing.set(false);
        if let Some([change, cancel]) = self.listeners.borrow().as_ref() {
            let _ = self
                .input
                .remove_event_listener_with_callback("change", change.as_ref().unchecked_ref());
            let _ = self
                .input
                .remove_event_listener_with_callback("cancel", cancel.as_ref().unchecked_ref());
        }
        self.input.remove();
    }
}

/// What the handle holds for an input: dropping it ends the dialog, takes
/// the input down and drops the answer.
struct InputHold(Rc<InputPicker>);

impl Drop for InputHold {
    fn drop(&mut self) {
        self.0.waiting.set(false);
        if self.0.answer.take().is_some() {
            self.0.detach();
        }
    }
}
