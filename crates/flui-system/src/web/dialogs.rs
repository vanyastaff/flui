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
//! time: the page stays busy while one is up, after its dialog was dropped
//! too. A picker of the API ends when its promise settles. An input ends on
//! its `change`, its `cancel` (Chromium 113, Firefox 91, Safari 16.4), the
//! page being hidden for good (`pagehide`), or, for an engine without
//! `cancel`, the window regaining focus with no `change` within
//! [`FOCUS_GRACE_MS`], which answers a cancel: a pick whose `change` comes
//! later than that (a large file a phone copies first) is lost, the price of
//! an end on every engine.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{Resolver, WindowRef};
use flui_platform::{BrowserException, SystemFailure, UnsupportedReason};
use js_sys::{Array, Object, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, Event, File, HtmlAnchorElement, HtmlInputElement, Url};

use super::page::{Page, call_promise, exception, method, property, set};
use super::rules::{self, PickerEnd};
use crate::dialog::{
    DialogError, DialogHold, FileDialogBackend, FileDialogCapability, FileFilter, OpenRequest,
    OpenResult, PickedFile, Picks, SaveLocation, SaveRequest, SaveResult,
};

/// How long a download's object URL outlives its click before it is
/// revoked: the browser starts the download asynchronously, and revoking the
/// URL first cancels it in some engines (`FileSaver.js` waits 40 s).
const DOWNLOAD_URL_LIFETIME_MS: i32 = 60_000;

/// How long an input waits for its `change` after the window regains focus
/// before it answers a cancel: the end of an input on an engine without its
/// `cancel` event, where the browser may report the pick a moment after the
/// focus.
pub(crate) const FOCUS_GRACE_MS: i32 = 3_000;

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
    focus_grace_ms: i32,
}

impl WebDialogs {
    pub(crate) fn new(page: Rc<Page>, picker: Rc<PagePicker>) -> Self {
        Self {
            page,
            picker,
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
        if request.picks_folders() {
            // A browser can hand a page a folder (`showDirectoryPicker`, a
            // `webkitdirectory` input), but no pick this handle has carries
            // a folder's tree.
            return answered(answer, unsupported(UnsupportedReason::NotYetOnThisHost));
        }
        if !self.page.is_activated() {
            return answered(answer, rules::unavailable());
        }
        if self.picker.showing.get() {
            return answered(answer, DialogError::Busy);
        }
        let hold = OpenHold {
            waiting: Rc::new(Cell::new(true)),
            input: Rc::default(),
        };
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
                    Ok(promise) => self.open_with_picker(promise, request, answer, &hold),
                    Err(thrown) => self.end_picker(thrown, &request, answer, &hold),
                }
            }
            _ => self.show_input(&request, answer, &hold),
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
            Ok(promise) => self.save_with_picker(promise, name, contents, request.filters, answer),
            Err(thrown) if rules::is_activation_refusal(thrown) => {
                let _ = answer.resolve(downloaded(&self.page, name, &contents, &request.filters));
                DialogHold::new(())
            }
            Err(thrown) => answered(answer, failed(thrown)),
        }
    }
}

impl WebDialogs {
    /// Show an input for `request` in the place `hold` keeps for it.
    fn show_input(&self, request: &OpenRequest, answer: Resolver<OpenResult>, hold: &OpenHold) {
        let shown = InputPicker::show(
            &self.page,
            &self.picker,
            request,
            answer,
            self.focus_grace_ms,
        );
        *hold.input.borrow_mut() = shown;
    }

    /// End an open picker the browser refused with `thrown` at once: as a
    /// cancel, with the input in its place, or with the refusal.
    fn end_picker(
        &self,
        thrown: BrowserException,
        request: &OpenRequest,
        answer: Resolver<OpenResult>,
        hold: &OpenHold,
    ) {
        match rules::picker_rejection(thrown, self.page.is_activated()) {
            PickerEnd::Cancelled => drop(answer.resolve(Ok(None))),
            PickerEnd::FallBack => self.show_input(request, answer, hold),
            PickerEnd::Refused(error) => drop(answer.resolve(Err(error))),
        }
    }

    /// Answer `answer` when the open picker's `promise` settles, with the
    /// picked files checked and read, or with the input in its place.
    fn open_with_picker(
        &self,
        promise: js_sys::Promise,
        request: OpenRequest,
        answer: Resolver<OpenResult>,
        hold: &OpenHold,
    ) {
        let waiting = Rc::clone(&hold.waiting);
        let input = Rc::clone(&hold.input);
        let page = Rc::clone(&self.page);
        let picker = Rc::clone(&self.picker);
        let focus_grace_ms = self.focus_grace_ms;
        picker.showing.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let settled = JsFuture::from(promise).await;
            picker.showing.set(false);
            if !waiting.get() {
                // The caller let the dialog go: nobody reads the files.
                return;
            }
            let rejection = match settled {
                Ok(handles) => {
                    let result = match files_of_handles(&handles).await {
                        Ok(files) => {
                            read_picks(files, &request.filters, request.max_bytes, &waiting).await
                        }
                        Err(failure) => Err(failure),
                    };
                    // A retired owner has nobody left to answer.
                    let _ = answer.resolve(result);
                    return;
                }
                Err(rejection) => exception(&rejection),
            };
            match rules::picker_rejection(rejection, page.is_activated()) {
                PickerEnd::Cancelled => drop(answer.resolve(Ok(None))),
                PickerEnd::FallBack => {
                    let shown = InputPicker::show(&page, &picker, &request, answer, focus_grace_ms);
                    *input.borrow_mut() = shown;
                }
                PickerEnd::Refused(error) => drop(answer.resolve(Err(error))),
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
        name: String,
        contents: Arc<[u8]>,
        filters: Vec<FileFilter>,
        answer: Resolver<SaveResult>,
    ) -> DialogHold {
        let waiting = Rc::new(Cell::new(true));
        let still_waiting = Rc::clone(&waiting);
        let page = Rc::clone(&self.page);
        let picker = Rc::clone(&self.picker);
        picker.showing.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            let settled = JsFuture::from(promise).await;
            picker.showing.set(false);
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

/// The picks `files` stand for, read within `limit` together: each is
/// checked against `filters` and their declared sizes against `limit`
/// before anything is read, and the bytes read are counted again as they
/// arrive, each file copied once, into the pick's own buffer. Reading stops
/// once `waiting` clears. No file is an empty answer, which a browser gives
/// for a dismissed picker.
async fn read_picks(
    files: Vec<File>,
    filters: &[FileFilter],
    limit: u64,
    waiting: &Cell<bool>,
) -> OpenResult {
    let described: Vec<(String, String)> = files
        .iter()
        .map(|file| (file.name(), file.type_()))
        .collect();
    rules::admit_picks(
        described
            .iter()
            .map(|(name, mime_type)| (name.as_str(), Some(mime_type.as_str()))),
        filters,
    )?;
    rules::admit_sizes(files.iter().map(|file| file.size()), limit)?;
    let mut read: u64 = 0;
    let mut picks = Vec::with_capacity(files.len());
    for (file, (name, _)) in files.into_iter().zip(described) {
        if !waiting.get() {
            return Ok(None);
        }
        let buffer = settle(Ok(file.array_buffer())).await?;
        let bytes = Uint8Array::new(&buffer);
        read = read.saturating_add(u64::from(bytes.length()));
        rules::admit_read(read, limit)?;
        let length = usize::try_from(bytes.length()).map_err(|_| DialogError::TooLarge {
            actual: read,
            limit,
        })?;
        let mut contents: Arc<[u8]> = std::iter::repeat_n(0, length).collect();
        bytes.copy_to(
            Arc::get_mut(&mut contents).expect("BUG: a buffer just made has no other owner"),
        );
        picks.push(
            PickedFile::try_from_contents(name, contents)
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
/// the window's `focus` and `pagehide`, and the timer the focus starts.
struct InputListeners {
    change: Closure<dyn FnMut(Event)>,
    cancel: Closure<dyn FnMut(Event)>,
    focus: Closure<dyn FnMut(Event)>,
    hidden: Closure<dyn FnMut(Event)>,
    grace: Closure<dyn FnMut()>,
}

/// An `<input type=file>` shown for one open dialog, until the browser
/// answers it. It outlives its dialog's hold: while the browser's chooser
/// is up the page is busy, whether anyone waits for the answer or not.
struct InputPicker {
    input: HtmlInputElement,
    view: web_sys::Window,
    picker: Rc<PagePicker>,
    answer: Cell<Option<Resolver<OpenResult>>>,
    filters: Vec<FileFilter>,
    limit: u64,
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
    /// Show an input for `request` in `page`, answering `answer` with what
    /// the user picks; `None` when it ended before it was shown, `answer`
    /// answered.
    fn show(
        page: &Page,
        picker: &Rc<PagePicker>,
        request: &OpenRequest,
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
        input.set_multiple(request.is_multiple());
        input.set_accept(&rules::accept_attribute(&request.filters));
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
        let shown = Rc::new(Self {
            input,
            view: page.view().clone(),
            picker: Rc::clone(picker),
            answer: Cell::new(Some(answer)),
            filters: request.filters.clone(),
            limit: request.max_bytes,
            waiting: Rc::new(Cell::new(true)),
            ended: Cell::new(false),
            focus_grace_ms,
            grace_timer: Cell::new(None),
            listeners: RefCell::new(None),
            alive: RefCell::new(None),
        });
        let listeners = InputListeners {
            change: shown.listener(Self::picked),
            cancel: shown.listener(|shown| shown.finish(Ok(None))),
            focus: shown.listener(Self::focused),
            hidden: shown.listener(|shown| shown.finish(Ok(None))),
            grace: {
                let ended = Rc::downgrade(&shown);
                Closure::new(move || {
                    if let Some(shown) = ended.upgrade() {
                        shown.finish(Ok(None));
                    }
                })
            },
        };
        let _ = shown
            .input
            .add_event_listener_with_callback("change", listeners.change.as_ref().unchecked_ref());
        let _ = shown
            .input
            .add_event_listener_with_callback("cancel", listeners.cancel.as_ref().unchecked_ref());
        let _ = shown
            .view
            .add_event_listener_with_callback("focus", listeners.focus.as_ref().unchecked_ref());
        let _ = shown.view.add_event_listener_with_callback(
            "pagehide",
            listeners.hidden.as_ref().unchecked_ref(),
        );
        *shown.listeners.borrow_mut() = Some(listeners);
        shown.picker.showing.set(true);

        // `showPicker` where the browser has it, which throws a refusal a
        // `click` would swallow.
        let opened = if let Some(show_picker) = method(shown.input.as_ref(), "showPicker") {
            show_picker
                .call0(shown.input.as_ref())
                .map(|_| ())
                .map_err(|thrown| exception(&thrown))
        } else {
            shown.input.click();
            Ok(())
        };
        if let Err(refused) = opened {
            let error = if rules::is_activation_refusal(refused) {
                rules::unavailable()
            } else {
                failed(refused)
            };
            shown.finish(Err(error));
        }
        Some(shown)
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
        let files: Vec<File> =
            files.map_or_default(|list| (0..list.length()).filter_map(|at| list.get(at)).collect());
        let filters = self.filters.clone();
        let limit = self.limit;
        let waiting = Rc::clone(&self.waiting);
        wasm_bindgen_futures::spawn_local(async move {
            let result = read_picks(files, &filters, limit, &waiting).await;
            if waiting.get() {
                // A retired owner has nobody left to answer.
                let _ = answer.resolve(result);
            }
        });
    }

    /// The window regained focus: the chooser has closed, and an engine
    /// without `cancel` says nothing more if nothing was picked.
    fn focused(self: &Rc<Self>) {
        if self.ended.get() {
            return;
        }
        if let Some(timer) = self.grace_timer.take() {
            self.view.clear_timeout_with_handle(timer);
        }
        let listeners = self.listeners.borrow();
        let Some(listeners) = listeners.as_ref() else {
            return;
        };
        if let Ok(timer) = self
            .view
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                listeners.grace.as_ref().unchecked_ref(),
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
        self.picker.showing.set(false);
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
                "focus",
                listeners.focus.as_ref().unchecked_ref(),
            );
            let _ = self.view.remove_event_listener_with_callback(
                "pagehide",
                listeners.hidden.as_ref().unchecked_ref(),
            );
        }
        self.input.remove();
        let alive = self.alive.borrow_mut().take();
        if let Some(alive) = alive {
            wasm_bindgen_futures::spawn_local(async move { drop(alive) });
        }
    }
}
