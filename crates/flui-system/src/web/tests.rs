//! The web providers in a browser (headless Chrome in CI): each provider
//! over a canvas of the test page, reached through a window's raw handle as
//! a real window's is, with the page's word on user activation stated by
//! the test (no test can make a gesture), the browser's events dispatched at
//! the elements the provider shows, and `window.open`, the save and
//! directory pickers and the Badging API replaced for the test that calls
//! them. No test opens a picker or a window: a picker is refused by the
//! browser without a real gesture, which the tests of that path read as its
//! answer, and a folder's files are stand-ins that say where they are.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::{
    Abandoned, Capability, CapabilityRegistrar, Completion, CompletionSource, HostBridge, OwnerJob,
    OwnerPoster, OwnerScope, PendingCompletions, Plugin as _, ProviderContext, RuntimeLocals,
    WindowCloser, WindowRef,
};
use flui_platform::launch::{
    ExternalUri, LaunchError, Launcher, LauncherBackend as _, LauncherCapability, Scheme,
};
use flui_platform::{
    PlatformWindow, SurfaceTarget, SwapChainStatistics, Unsupported, UnsupportedReason, WindowId,
    WindowIdMint,
};
use js_sys::{Array, Promise, Reflect};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    WebCanvasWindowHandle, WindowHandle,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{Element, Event, EventInit, HtmlCanvasElement, HtmlDialogElement, HtmlInputElement};

use super::badge::WebBadge;
use super::dialogs::WebDialogs;
use super::launcher::WebLauncher;
use super::message_box::WebMessageBoxes;
use super::page::{Activation, Page};
use super::share::WebShare;
use crate::autostart::AutostartCapability;
use crate::dialog::MessageBoxCapability;
use crate::dialog::{
    Dialog, DialogError, FileDialogCapability, FileDialogs, FileFilter, MessageAnswer,
    MessageBoxRequest, MessageBoxes, MessageButtons, MessageLevel, OpenRequest, OpenResult,
    PickedForm, Picks, SaveRequest,
};
use crate::jump_list::JumpListCapability;
use crate::os::OsCapability;
use crate::share::{Share, ShareBackend as _, ShareCapability, ShareError, ShareRequest};
use crate::shortcuts::GlobalShortcutCapability;
use crate::taskbar::{EntryState, Honoured, Progress, TaskbarEntry};
use crate::user_dirs::UserDirCapability;

wasm_bindgen_test_configure!(run_in_browser);

type Jobs = Arc<Mutex<Vec<OwnerJob>>>;

/// An owner whose turns the test runs.
struct TestOwner {
    jobs: Jobs,
    poster: OwnerPoster,
    pending: PendingCompletions,
    completions: CompletionSource,
    locals: RuntimeLocals,
    bridge: HostBridge,
    /// The ids of the runtime's windows, one each.
    windows: WindowIdMint,
}

impl TestOwner {
    fn new() -> Self {
        let jobs: Jobs = Arc::default();
        let queue = Arc::clone(&jobs);
        let poster = OwnerPoster::new(move |job| {
            queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            Ok(())
        });
        let pending = PendingCompletions::new();
        let completions = CompletionSource::new(poster.clone(), pending.clone());
        Self {
            jobs,
            poster,
            pending,
            completions,
            locals: RuntimeLocals::new(),
            bridge: HostBridge::new(),
            windows: WindowIdMint::new(),
        }
    }

    fn ctx(&self, window: &WindowRef) -> ProviderContext<'_> {
        ProviderContext::new(
            &self.poster,
            &self.completions,
            &self.locals,
            &self.bridge,
            Some(window.clone()),
        )
    }

    /// Run the jobs queued so far.
    fn turn(&self) {
        let jobs = std::mem::take(&mut *self.jobs.lock().unwrap_or_else(PoisonError::into_inner));
        let scope = OwnerScope::new(&self.pending);
        for job in jobs {
            job.run(&scope);
        }
    }

    /// Run turns, letting the browser run its tasks between them, until
    /// `slot` holds an outcome; panic after two hundred rounds.
    async fn until_landed<T>(&self, slot: &Slot<T>) {
        for _ in 0..200 {
            self.turn();
            if slot.borrow().is_some() {
                return;
            }
            browser_task().await;
        }
        panic!("the answer did not arrive");
    }

    /// Run a few rounds of turns and browser tasks: what a test waits on
    /// that should not come.
    async fn settle(&self) {
        for _ in 0..10 {
            self.turn();
            browser_task().await;
        }
    }
}

/// Wait for the browser to run its queued tasks (a `setTimeout` of 0).
async fn browser_task() {
    let promise = Promise::new(&mut |resolve, _| {
        let view = web_sys::window().expect("the test runs in a page");
        let _ = view.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0);
    });
    let _ = JsFuture::from(promise).await;
}

type Slot<T> = Rc<RefCell<Option<Result<T, Abandoned>>>>;

/// A dialog's outcome, landed in a slot; `on_complete` keeps the dialog up
/// until it answers.
fn land<T: 'static>(dialog: Dialog<T>) -> Slot<T> {
    let slot: Slot<T> = Rc::default();
    let into = Rc::clone(&slot);
    dialog.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

/// A completion's outcome, landed in a slot.
fn landed<T: 'static>(completion: Completion<T>) -> Slot<T> {
    let slot: Slot<T> = Rc::default();
    let into = Rc::clone(&slot);
    completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    slot
}

/// A window's target whose handle names a canvas of the test page: the
/// address of the canvas's `JsValue`, leaked for the test's life, which a
/// `usize` carries across the target's `Send` bound.
struct CanvasTarget(usize);

impl HasWindowHandle for CanvasTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = std::ptr::NonNull::new(std::ptr::with_exposed_provenance_mut::<
            std::ffi::c_void,
        >(self.0))
        .ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::WebCanvas(WebCanvasWindowHandle::new(pointer));
        // SAFETY: the address is of a leaked `JsValue` holding the canvas,
        // which lives for the rest of the test run.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for CanvasTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for CanvasTarget {
    fn pre_present_notify(
        &self,
        _: Option<flui_sdk::foundation::FrameStamp>,
        _: SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}

/// A host's window over a [`CanvasTarget`]: what a provider reaches through
/// the `WindowRef` it is handed, and nothing else.
fn canvas_host_window(id: WindowId, target: Arc<CanvasTarget>) -> Rc<dyn PlatformWindow> {
    Rc::new(
        flui_platform::testing::MockWindow::new()
            .with_id(id)
            .with_surface_target(target),
    )
}

/// A window of its own on the test page: a canvas in a `<div>` of the
/// body, the window's handle naming it, and the window's closer.
struct CanvasWindow {
    root: Element,
    canvas: HtmlCanvasElement,
    window: WindowRef,
    closer: WindowCloser,
    /// Kept for the window's life: the reference does not keep it.
    _host: Rc<dyn PlatformWindow>,
}

impl CanvasWindow {
    /// A window of `owner`'s runtime.
    fn new(owner: &TestOwner) -> Self {
        let document = web_sys::window()
            .and_then(|view| view.document())
            .expect("the test runs in a page");
        let root = document.create_element("div").expect("a div");
        let canvas = document
            .create_element("canvas")
            .expect("a canvas")
            .unchecked_into::<HtmlCanvasElement>();
        root.append_child(&canvas).expect("the canvas goes in");
        document
            .body()
            .expect("the page has a body")
            .append_child(&root)
            .expect("the window goes in the page");
        let value: &'static JsValue = Box::leak(Box::new(JsValue::from(canvas.clone())));
        let target = Arc::new(CanvasTarget(std::ptr::from_ref(value).expose_provenance()));
        let (closer, lifetime) = WindowCloser::new();
        let id = owner
            .windows
            .next()
            .expect("a test opens far fewer windows than ids");
        let host: Rc<dyn PlatformWindow> = canvas_host_window(id, target);
        Self {
            root,
            canvas,
            window: WindowRef::new(&host, lifetime),
            closer,
            _host: host,
        }
    }

    /// The window's page, with user activation as `active` says.
    fn page(&self, active: bool) -> Rc<Page> {
        self.page_with(&Rc::new(Cell::new(active)))
    }

    /// The window's page, with user activation as `active` says when asked:
    /// a test changes it while a call waits on the browser.
    fn page_with(&self, active: &Rc<Cell<bool>>) -> Rc<Page> {
        Rc::new(
            Page::for_canvas(self.canvas.clone(), Activation::Stated(Rc::clone(active)))
                .expect("the canvas is in the page"),
        )
    }

    /// The first element of the window's part of the page `selector` finds.
    fn find(&self, selector: &str) -> Option<Element> {
        self.root.query_selector(selector).ok().flatten()
    }
}

impl Drop for CanvasWindow {
    fn drop(&mut self) {
        self.closer.close();
        self.root.remove();
    }
}

/// How long a test's input waits for its `change` after the window regains
/// focus.
const TEST_FOCUS_GRACE_MS: i32 = 20;

/// A test page's file dialogs for `window` over `page`.
fn dialogs_on(owner: &TestOwner, window: &CanvasWindow, page: Rc<Page>) -> FileDialogs {
    quiet_input_pickers();
    FileDialogs::try_new(
        WebDialogs::new(page).with_focus_grace(TEST_FOCUS_GRACE_MS),
        &owner.ctx(&window.window),
    )
    .expect("a window has dialogs")
}

/// Make an input's `showPicker` a no-op for the rest of the run: the
/// browser shows no chooser a test could answer, and without a real gesture
/// it throws. The tests answer an input through its events instead.
fn quiet_input_pickers() {
    thread_local! {
        static QUIET: Cell<bool> = const { Cell::new(false) };
    }
    if QUIET.with(|quiet| quiet.replace(true)) {
        return;
    }
    let view: JsValue = the_view().into();
    let prototype = Reflect::get(&view, &"HTMLInputElement".into())
        .and_then(|input| Reflect::get(&input, &"prototype".into()))
        .expect("the page has inputs");
    let quiet = js_sys::Function::new_no_args("");
    Reflect::set(&prototype, &"showPicker".into(), &quiet).expect("the method is replaced");
}

/// A test page's file dialogs for `window`, user activation as `active`
/// says.
fn dialogs(owner: &TestOwner, window: &CanvasWindow, active: bool) -> FileDialogs {
    dialogs_on(owner, window, window.page(active))
}

/// A filter whose extension the File System Access picker refuses, which
/// sends an open dialog to `<input type=file>` on every browser.
fn input_only() -> FileFilter {
    FileFilter::new("Notes").with_extension("my-notes")
}

/// Set `input`'s files to `files` (name and text) and dispatch `change`,
/// as the browser does when the user picks them.
fn pick(input: &HtmlInputElement, files: &[(&str, &str)]) {
    let transfer = web_sys::DataTransfer::new().expect("a DataTransfer");
    for (name, text) in files {
        let parts = Array::of1(&JsValue::from_str(text));
        let file = web_sys::File::new_with_str_sequence(&parts, name).expect("a file");
        transfer
            .items()
            .add_with_file(&file)
            .expect("the file goes in");
    }
    input.set_files(transfer.files().as_ref());
    dispatch(input, "change", false);
}

/// Dispatch the event `kind` at `target`; returns whether it was not
/// cancelled.
fn dispatch(target: &web_sys::EventTarget, kind: &str, cancelable: bool) -> bool {
    let init = EventInit::new();
    init.set_cancelable(cancelable);
    let event = Event::new_with_event_init_dict(kind, &init).expect("an event");
    target
        .dispatch_event(&event)
        .expect("the event is dispatched")
}

fn the_input(window: &CanvasWindow) -> HtmlInputElement {
    window
        .find("input[type=file]")
        .expect("the dialog shows an input")
        .unchecked_into::<HtmlInputElement>()
}

fn the_view() -> web_sys::Window {
    web_sys::window().expect("the test runs in a page")
}

/// The links the window's part of the page follows, caught before the
/// browser follows them: each one's `download` and `href`.
struct CaughtLinks {
    links: Rc<RefCell<Vec<(String, String)>>>,
    _catch: Closure<dyn FnMut(Event)>,
}

impl CaughtLinks {
    fn on(window: &CanvasWindow) -> Self {
        let links: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
        let seen = Rc::clone(&links);
        let catch = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            if let Some(link) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::HtmlAnchorElement>().ok())
            {
                event.prevent_default();
                seen.borrow_mut().push((link.download(), link.href()));
            }
        });
        window
            .root
            .add_event_listener_with_callback("click", catch.as_ref().unchecked_ref())
            .expect("the test listens for the click");
        Self {
            links,
            _catch: catch,
        }
    }
}

#[wasm_bindgen_test]
fn a_window_reaches_its_page_through_the_canvas_its_handle_names() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let page = Page::of(&window.window).expect("the window names a canvas");
    let host = page.host().expect("the canvas has a parent");
    assert!(
        host.is_same_node(Some(window.root.as_ref())),
        "the page's elements go beside the window's canvas"
    );
}

#[wasm_bindgen_test]
async fn an_input_answers_the_files_the_user_picked_with_their_contents() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let answered = land(
        dialogs.open(
            OpenRequest::files()
                .allow_multiple()
                .filter(input_only().with_mime_type("text/plain")),
        ),
    );
    let input = the_input(&window);
    assert_eq!(input.accept(), ".my-notes,text/plain");
    assert!(input.multiple());

    pick(&input, &[("a.my-notes", "first"), ("B.MY-NOTES", "second")]);
    owner.until_landed(&answered).await;

    let picks = match answered.borrow_mut().take() {
        Some(Ok(Ok(Some(picks)))) => picks,
        other => panic!("two picks: {other:?}"),
    };
    let read: Vec<(Option<String>, Option<Vec<u8>>, Option<&Path>)> = picks
        .iter()
        .map(|pick| {
            (
                pick.file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                pick.contents().map(<[u8]>::to_vec),
                pick.path(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (Some("a.my-notes".to_owned()), Some(b"first".to_vec()), None),
            (
                Some("B.MY-NOTES".to_owned()),
                Some(b"second".to_vec()),
                None
            ),
        ]
    );
    assert!(
        window.find("input").is_none(),
        "the input leaves the page once it answered"
    );
}

#[wasm_bindgen_test]
async fn a_pick_the_filters_do_not_mean_refuses_the_answer() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered = land(
        dialogs(&owner, &window, true)
            .open(OpenRequest::files().allow_multiple().filter(input_only())),
    );

    // The browser's "All files" lets the user pick past the filter.
    pick(
        &the_input(&window),
        &[("a.my-notes", "first"), ("run.exe", "second")],
    );
    owner.until_landed(&answered).await;

    assert_eq!(*answered.borrow(), Some(Ok(Err(DialogError::NotInFilter))));
}

/// What an open dialog an `<input>` answers picks: the rows of the tests of
/// what every input does, whatever it was shown for.
#[derive(Clone, Copy, Debug)]
enum InputFor {
    /// Files under a filter the File System Access picker refuses.
    Files,
    /// A folder, on a browser with no directory picker.
    Folder,
}

impl InputFor {
    const EACH: [Self; 2] = [Self::Files, Self::Folder];

    fn request(self) -> OpenRequest {
        match self {
            Self::Files => OpenRequest::files().filter(input_only()),
            Self::Folder => OpenRequest::folders(),
        }
    }

    /// Answer `input` with one pick of the kind, as the browser does when
    /// the user picks it.
    fn pick(self, input: &HtmlInputElement) {
        match self {
            Self::Files => pick(input, &[("a.my-notes", "late")]),
            Self::Folder => pick_folder(input, &[("Project/a.txt", "late")]),
        }
    }
}

/// Hide the page's directory picker until the guard drops, as a browser
/// without the File System Access API has none: a folder dialog goes to the
/// input.
fn without_directory_picker() -> Replaced {
    let view: JsValue = the_view().into();
    Replaced::new(&view, "showDirectoryPicker", &JsValue::UNDEFINED)
}

/// Give `target` an own property `name` holding `value`, over the accessor
/// its prototype has.
fn define(target: &JsValue, name: &str, value: &JsValue) {
    let target: &js_sys::Object = target.unchecked_ref();
    let descriptor = js_sys::Object::new();
    let _ = Reflect::set(&descriptor, &"value".into(), value);
    let _ = Reflect::set(&descriptor, &"configurable".into(), &true.into());
    js_sys::Object::define_property(target, &JsValue::from_str(name), &descriptor);
}

/// The files a test hands an input in place of the browser's list, and what
/// the provider did with them: a page cannot make a file that says where it
/// is under a folder, or one whose declared size is not its contents', so
/// each stand-in states its own.
struct ListedFiles {
    files: Array,
    /// The name of each file whose contents were asked for.
    read: Array,
    /// Each place of the list that was looked at.
    touched: Array,
}

impl ListedFiles {
    fn new() -> Self {
        Self {
            files: Array::new(),
            read: Array::new(),
            touched: Array::new(),
        }
    }

    /// Add the file at `path` holding `text`: a path with a folder before
    /// its name is the file's `webkitRelativePath`, as a folder's file has
    /// one; a name alone is a file picked by itself, which has none.
    fn file(self, path: &str, text: &str) -> Self {
        self.listing(path, text, None)
    }

    /// Add a file as [`file`](Self::file) does, which declares `size` bytes
    /// whatever its text holds.
    fn declaring(self, path: &str, text: &str, size: f64) -> Self {
        self.listing(path, text, Some(size))
    }

    fn listing(self, path: &str, text: &str, size: Option<f64>) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let parts = Array::of1(&JsValue::from_str(text));
        let file = web_sys::File::new_with_str_sequence(&parts, name).expect("a file");
        if path.contains('/') {
            define(&file, "webkitRelativePath", &JsValue::from_str(path));
        }
        if let Some(size) = size {
            define(&file, "size", &JsValue::from_f64(size));
        }
        js_sys::Function::new_with_args(
            "file, read",
            "const contents = file.arrayBuffer.bind(file);
            Object.defineProperty(file, 'arrayBuffer', { configurable: true, value: () => {
                read.push(file.name);
                return contents();
            } });",
        )
        .call2(&JsValue::UNDEFINED, &file, &self.read)
        .expect("the file records its reads");
        self.files.push(&file);
        self
    }

    /// Give `input` the files and dispatch `change`, as the browser does
    /// when the user picks them.
    fn pick_into(&self, input: &HtmlInputElement) {
        let list = js_sys::Function::new_with_args(
            "files, touched",
            "return new Proxy(files, { get(files, place) {
                if (place !== 'length') touched.push(String(place));
                return files[place];
            } });",
        )
        .call2(&JsValue::UNDEFINED, &self.files, &self.touched)
        .expect("the list records what is looked at");
        define(input, "files", &list);
        dispatch(input, "change", false);
    }

    /// The names of the files whose contents the provider asked for.
    fn read(&self) -> Vec<String> {
        self.read
            .iter()
            .filter_map(|name| name.as_string())
            .collect::<Vec<String>>()
    }

    /// The places of the list the provider looked at.
    fn touched(&self) -> Vec<String> {
        self.touched
            .iter()
            .filter_map(|place| place.as_string())
            .collect::<Vec<String>>()
    }
}

/// Give `input` the files of a picked folder (each its `webkitRelativePath`
/// and its text) and dispatch `change`, as the browser does when the user
/// picks the folder.
fn pick_folder(input: &HtmlInputElement, files: &[(&str, &str)]) {
    files
        .iter()
        .fold(ListedFiles::new(), |listed, (relative_path, text)| {
            listed.file(relative_path, text)
        })
        .pick_into(input);
}

/// Make the page's browser one whose file input fires no `cancel`, until
/// the guard drops: the provider tells by `Headers.prototype.getSetCookie`,
/// which no engine has without the event.
fn without_input_cancel() -> Replaced {
    let view: JsValue = the_view().into();
    let prototype = Reflect::get(&view, &"Headers".into())
        .and_then(|headers| Reflect::get(&headers, &"prototype".into()))
        .expect("the page has headers");
    Replaced::new(&prototype, "getSetCookie", &JsValue::UNDEFINED)
}

/// Wait `ms` milliseconds, the browser running its tasks meanwhile.
async fn browser_wait(ms: i32) {
    let promise = Promise::new(&mut |resolve, _| {
        let _ = the_view().set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

/// The picks an open dialog landed; panics on any other outcome.
fn the_picks(answered: &Slot<OpenResult>) -> Picks {
    match answered.borrow_mut().take() {
        Some(Ok(Ok(Some(picks)))) => picks,
        other => panic!("picks: {other:?}"),
    }
}

/// The one folder `picks` hold: its name, and each of its files as its path
/// under the folder and its text.
fn the_folder(picks: &Picks) -> (String, Vec<(String, String)>) {
    assert_eq!(picks.len(), 1, "a browser picks one folder: {picks:?}");
    let PickedForm::Folder { name, files } = picks.first().form() else {
        panic!("a folder handed over: {picks:?}");
    };
    let files = files
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_owned(),
                String::from_utf8_lossy(file.contents()).into_owned(),
            )
        })
        .collect::<Vec<(String, String)>>();
    (name.to_owned(), files)
}

#[wasm_bindgen_test]
async fn a_dismissed_input_answers_a_cancel() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    for shown_for in InputFor::EACH {
        let answered = land(dialogs.open(shown_for.request()));

        dispatch(&the_input(&window), "cancel", false);
        owner.until_landed(&answered).await;

        assert_eq!(*answered.borrow(), Some(Ok(Ok(None))), "{shown_for:?}");
        assert!(window.find("input").is_none(), "{shown_for:?}");
    }
}

#[wasm_bindgen_test]
async fn an_input_on_an_engine_that_fires_cancel_is_not_timed_after_the_focus() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    for shown_for in InputFor::EACH {
        let answered = land(dialogs.open(shown_for.request()));
        let input = the_input(&window);

        // The chooser closed and the window has the focus again, while the
        // browser still lists the folder or asks the user to confirm it.
        dispatch(&the_view(), "focus", false);
        browser_wait(TEST_FOCUS_GRACE_MS * 4).await;
        owner.settle().await;
        assert_eq!(*answered.borrow(), None, "no cancel for {shown_for:?}");
        assert!(window.find("input").is_some(), "the input is still up");

        // The pick the browser reports then is the answer.
        shown_for.pick(&input);
        owner.until_landed(&answered).await;
        assert!(
            matches!(&*answered.borrow(), Some(Ok(Ok(Some(picks)))) if picks.len() == 1),
            "the late pick of {shown_for:?}: {:?}",
            answered.borrow()
        );
    }
}

#[wasm_bindgen_test]
async fn an_input_with_no_change_after_the_window_regains_focus_answers_a_cancel() {
    let _no_directory_picker = without_directory_picker();
    let _no_cancel = without_input_cancel();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    for shown_for in InputFor::EACH {
        let answered = land(dialogs.open(shown_for.request()));

        // An engine without `cancel`: the chooser closes and the window has
        // the focus again, and nothing more comes.
        dispatch(&the_view(), "focus", false);
        owner.until_landed(&answered).await;
        assert_eq!(*answered.borrow(), Some(Ok(Ok(None))), "{shown_for:?}");
        assert!(window.find("input").is_none(), "{shown_for:?}");

        // A `change` that comes within the bound after the focus still
        // answers.
        let picked = land(dialogs.open(shown_for.request()));
        let input = the_input(&window);
        dispatch(&the_view(), "focus", false);
        shown_for.pick(&input);
        owner.until_landed(&picked).await;
        assert!(
            matches!(&*picked.borrow(), Some(Ok(Ok(Some(picks)))) if picks.len() == 1),
            "the pick of {shown_for:?}: {:?}",
            picked.borrow()
        );
    }
}

#[wasm_bindgen_test]
async fn an_input_whose_page_is_hidden_for_good_answers_a_cancel() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    for shown_for in InputFor::EACH {
        let answered = land(dialogs(&owner, &window, true).open(shown_for.request()));

        dispatch(&the_view(), "pagehide", false);
        owner.until_landed(&answered).await;

        assert_eq!(*answered.borrow(), Some(Ok(Ok(None))), "{shown_for:?}");
    }
}

#[wasm_bindgen_test]
async fn picks_past_the_requests_bound_are_refused_before_they_are_read() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let request = || {
        OpenRequest::files()
            .allow_multiple()
            .max_bytes(8)
            .filter(input_only())
    };
    let too_large = Some(Ok(Err(DialogError::TooLarge {
        actual: 9,
        limit: 8,
    })));

    // The sizes the browser declares are past the bound, whatever the files
    // hold: nothing is read.
    let declared = ListedFiles::new()
        .declaring("a.my-notes", "12", 6.0)
        .declaring("b.my-notes", "34", 3.0);
    let answered = land(dialogs.open(request()));
    declared.pick_into(&the_input(&window));
    owner.until_landed(&answered).await;
    assert_eq!(*answered.borrow(), too_large);
    assert_eq!(declared.read(), Vec::<String>::new(), "no file was read");

    // A file that holds more than it declared is caught as it is read, and
    // the file after it is not read.
    let grown = ListedFiles::new()
        .declaring("a.my-notes", "123456789", 1.0)
        .file("b.my-notes", "0");
    let answered = land(dialogs.open(request()));
    grown.pick_into(&the_input(&window));
    owner.until_landed(&answered).await;
    assert_eq!(*answered.borrow(), too_large);
    assert_eq!(grown.read(), ["a.my-notes"]);
}

#[wasm_bindgen_test]
async fn a_dropped_dialog_keeps_the_page_busy_until_its_input_answers() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    for shown_for in InputFor::EACH {
        let shown = dialogs.open(shown_for.request());
        let input = the_input(&window);
        drop(shown);

        // The browser's chooser is still up: the page shows no second one.
        let refused = land(dialogs.open(shown_for.request()));
        owner.until_landed(&refused).await;
        assert_eq!(
            *refused.borrow(),
            Some(Ok(Err(DialogError::Busy))),
            "{shown_for:?}"
        );

        // The pick that ends it answers nobody, and frees the page.
        shown_for.pick(&input);
        assert!(window.find("input").is_none(), "the input left the page");
        let next = land(dialogs.open(shown_for.request()));
        dispatch(&the_input(&window), "cancel", false);
        owner.until_landed(&next).await;
        assert_eq!(*next.borrow(), Some(Ok(Ok(None))), "{shown_for:?}");
    }
}

#[wasm_bindgen_test]
async fn a_page_shows_one_picker_whichever_window_asks() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let first = CanvasWindow::new(&owner);
    let second = CanvasWindow::new(&owner);
    for (shown_for, asked_for) in [
        (InputFor::Files, InputFor::Files),
        (InputFor::Folder, InputFor::Files),
        (InputFor::Files, InputFor::Folder),
    ] {
        let shown = land(dialogs(&owner, &first, true).open(shown_for.request()));
        let refused = land(dialogs(&owner, &second, true).open(asked_for.request()));
        owner.until_landed(&refused).await;
        assert_eq!(
            *refused.borrow(),
            Some(Ok(Err(DialogError::Busy))),
            "{asked_for:?} while {shown_for:?} is up"
        );
        assert!(second.find("input").is_none());
        dispatch(&the_input(&first), "cancel", false);
        owner.until_landed(&shown).await;
    }
}

#[wasm_bindgen_test]
async fn a_page_shows_one_picker_whichever_runtime_asks() {
    let _no_directory_picker = without_directory_picker();
    // Two UI runtimes of one page, each with a window of its own.
    let (first_owner, second_owner) = (TestOwner::new(), TestOwner::new());
    let first = CanvasWindow::new(&first_owner);
    let second = CanvasWindow::new(&second_owner);
    let shown = land(dialogs(&first_owner, &first, true).open(InputFor::Files.request()));

    let refused = land(dialogs(&second_owner, &second, true).open(InputFor::Folder.request()));
    second_owner.until_landed(&refused).await;
    assert_eq!(
        *refused.borrow(),
        Some(Ok(Err(DialogError::Busy))),
        "the browser's chooser is the page's, whichever runtime showed it"
    );
    assert!(second.find("input").is_none(), "no second input was shown");

    // The other runtime's chooser ends: the page is free for this one.
    dispatch(&the_input(&first), "cancel", false);
    first_owner.until_landed(&shown).await;
    let next = land(dialogs(&second_owner, &second, true).open(InputFor::Folder.request()));
    dispatch(&the_input(&second), "cancel", false);
    second_owner.until_landed(&next).await;
    assert_eq!(*next.borrow(), Some(Ok(Ok(None))));
}

/// The refusal of a dialog for `reason`, as it lands.
fn refused_as(reason: UnsupportedReason) -> Option<Result<OpenResult, Abandoned>> {
    Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
        FileDialogCapability,
    >(reason)))))
}

#[wasm_bindgen_test]
async fn a_picker_outside_a_gesture_is_refused_before_the_browser_is_asked() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    // A directory picker that would be asked, were the gesture not checked.
    let picker = DirectoryPickerStandIn::install();
    for shown_for in InputFor::EACH {
        let answered = land(dialogs(&owner, &window, false).open(shown_for.request()));
        owner.until_landed(&answered).await;
        assert_eq!(
            *answered.borrow(),
            refused_as(UnsupportedReason::Unavailable),
            "{shown_for:?}"
        );
        assert!(window.find("input").is_none(), "no input was shown");
    }
    assert!(picker.modes.borrow().is_empty(), "no picker was asked for");
}

/// A stand-in for the directory picker: `showDirectoryPicker` answers when
/// the test settles it, with a handle the provider walks.
struct DirectoryPickerStandIn {
    /// The `resolve` and `reject` of the call that waits for its answer.
    pending: Rc<RefCell<Option<(js_sys::Function, js_sys::Function)>>>,
    /// The `mode` each call asked for.
    modes: Rc<RefCell<Vec<Option<String>>>>,
    /// What the handles it answered with were asked for: `entry` and a name
    /// for each entry a walk met, `file` and a name for each file opened,
    /// `read` and a name for each file whose contents were asked for.
    asked: Array,
    _picker: Closure<dyn FnMut(JsValue) -> JsValue>,
    _replaced: Replaced,
}

impl DirectoryPickerStandIn {
    fn install() -> Self {
        let pending: Rc<RefCell<Option<(js_sys::Function, js_sys::Function)>>> = Rc::default();
        let modes: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let (calls, seen) = (Rc::clone(&pending), Rc::clone(&modes));
        let picker = Closure::<dyn FnMut(JsValue) -> JsValue>::new(move |options: JsValue| {
            seen.borrow_mut().push(
                Reflect::get(&options, &"mode".into())
                    .ok()
                    .and_then(|mode| mode.as_string()),
            );
            let calls = Rc::clone(&calls);
            Promise::new(&mut |resolve, reject| *calls.borrow_mut() = Some((resolve, reject)))
                .into()
        });
        let view: JsValue = the_view().into();
        let replaced = Replaced::new(&view, "showDirectoryPicker", picker.as_ref());
        Self {
            pending,
            modes,
            asked: Array::new(),
            _picker: picker,
            _replaced: replaced,
        }
    }

    /// Answer the picker with the folder `name` holding `tree`: a JSON
    /// object whose strings are files, by their text, whose pairs are files
    /// by their text and the size they declare whatever the text holds, and
    /// whose objects are the folders under it. A folder lists what it holds
    /// through `values()`, as a `FileSystemDirectoryHandle` does.
    fn choose(&self, name: &str, tree: &str) {
        let handle = js_sys::Function::new_with_args(
            "name, tree, asked",
            "const fileOf = (name, text, size) => {
                const file = new File([text], name);
                const contents = file.arrayBuffer.bind(file);
                Object.defineProperty(file, 'arrayBuffer', { value: () => {
                    asked.push('read ' + name);
                    return contents();
                } });
                if (size !== undefined) Object.defineProperty(file, 'size', { value: size });
                return file;
            };
            const handle = (name, held) => typeof held === 'string' || Array.isArray(held)
                ? { kind: 'file', name, getFile: async () => {
                      asked.push('file ' + name);
                      return fileOf(name, ...[held].flat());
                  } }
                : { kind: 'directory', name, values: async function* () {
                      for (const [child, inside] of Object.entries(held)) {
                          asked.push('entry ' + child);
                          yield handle(child, inside);
                      }
                  } };
            return handle(name, JSON.parse(tree));",
        )
        .call3(
            &JsValue::UNDEFINED,
            &JsValue::from_str(name),
            &JsValue::from_str(tree),
            &self.asked,
        )
        .expect("the handle is made");
        let (resolve, _) = self.take_pending();
        resolve
            .call1(&JsValue::UNDEFINED, &handle)
            .expect("the picker answers");
    }

    /// End the picker with the exception `name`, as the browser does when
    /// the user dismisses it (`AbortError`).
    fn refuse(&self, name: &str) {
        let refusal = js_sys::Error::new("refused");
        refusal.set_name(name);
        let (_, reject) = self.take_pending();
        reject
            .call1(&JsValue::UNDEFINED, &refusal)
            .expect("the picker refuses");
    }

    fn take_pending(&self) -> (js_sys::Function, js_sys::Function) {
        self.pending
            .borrow_mut()
            .take()
            .expect("the picker was asked for")
    }

    fn asked(&self) -> Vec<String> {
        self.asked
            .iter()
            .filter_map(|asked| asked.as_string())
            .collect::<Vec<String>>()
    }
}

#[wasm_bindgen_test]
async fn a_directory_picker_answers_the_folder_with_its_files_where_they_are() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let picker = DirectoryPickerStandIn::install();
    let answered =
        land(dialogs(&owner, &window, true).open(OpenRequest::folders().allow_multiple()));
    assert_eq!(
        *picker.modes.borrow(),
        [Some("read".to_owned())],
        "the folder is read, never written"
    );

    picker.choose(
        "Project",
        r#"{"src":{"main.rs":"fn main() {}","web":{"mod.rs":"mod web;"},"empty":{}},
            "README.md":"read me","docs":{}}"#,
    );
    owner.until_landed(&answered).await;

    assert_eq!(
        the_folder(&the_picks(&answered)),
        (
            "Project".to_owned(),
            vec![
                ("README.md".to_owned(), "read me".to_owned()),
                ("src/main.rs".to_owned(), "fn main() {}".to_owned()),
                ("src/web/mod.rs".to_owned(), "mod web;".to_owned()),
            ]
        ),
        "the files at their paths, in the paths' order; a folder with no file leaves no trace"
    );
    assert!(window.find("input").is_none(), "no input stood in");
}

#[wasm_bindgen_test]
async fn a_folder_input_answers_the_folder_its_files_name() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered =
        land(dialogs(&owner, &window, true).open(OpenRequest::folders().allow_multiple()));
    let input = the_input(&window);
    assert!(input.webkitdirectory(), "the input asks for a folder");
    assert!(!input.multiple(), "one folder");
    assert_eq!(input.accept(), "");

    pick_folder(
        &input,
        &[
            ("Project/src/main.rs", "fn main() {}"),
            ("Project/README.md", "read me"),
        ],
    );
    owner.until_landed(&answered).await;

    assert_eq!(
        the_folder(&the_picks(&answered)),
        (
            "Project".to_owned(),
            vec![
                ("README.md".to_owned(), "read me".to_owned()),
                ("src/main.rs".to_owned(), "fn main() {}".to_owned()),
            ]
        )
    );
    assert!(
        window.find("input").is_none(),
        "the input leaves the page once it answered"
    );
}

#[wasm_bindgen_test]
async fn a_folder_past_the_requests_bounds_is_refused_before_it_is_read() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let too_large = Some(Ok(Err(DialogError::TooLarge {
        actual: 9,
        limit: 8,
    })));

    {
        let picker = DirectoryPickerStandIn::install();
        let crowded = land(dialogs.open(OpenRequest::folders().max_entries(2)));
        picker.choose(
            "Project",
            r#"{"a.txt":"1","src":{"b.txt":"2"},"c.txt":"3","d.txt":"4"}"#,
        );
        owner.until_landed(&crowded).await;
        assert_eq!(
            *crowded.borrow(),
            Some(Ok(Err(DialogError::TooManyEntries { limit: 2 })))
        );
        assert_eq!(
            picker.asked(),
            ["entry a.txt", "entry src", "entry c.txt"],
            "the walk ends at the entry past the bound, and no file is opened"
        );
    }
    {
        // The sizes the browser declares are past the bound, whatever the
        // files hold: each is opened for its size, and none is read.
        let picker = DirectoryPickerStandIn::install();
        let heavy = land(dialogs.open(OpenRequest::folders().max_bytes(8)));
        picker.choose("Project", r#"{"a.txt":["12",6],"src":{"b.txt":["34",3]}}"#);
        owner.until_landed(&heavy).await;
        assert_eq!(*heavy.borrow(), too_large);
        assert_eq!(
            picker.asked(),
            [
                "entry a.txt",
                "entry src",
                "entry b.txt",
                "file a.txt",
                "file b.txt"
            ]
        );
    }
    {
        // A file that holds more than it declared is caught as it is read,
        // and the file after it is not read.
        let picker = DirectoryPickerStandIn::install();
        let grown = land(dialogs.open(OpenRequest::folders().max_bytes(8)));
        picker.choose("Project", r#"{"a.txt":["123456789",1],"b.txt":"0"}"#);
        owner.until_landed(&grown).await;
        assert_eq!(*grown.borrow(), too_large);
        assert_eq!(
            picker.asked().last().map(String::as_str),
            Some("read a.txt")
        );
    }

    let _no_directory_picker = without_directory_picker();
    // An input's files are counted before one of them is looked at.
    let crowded_files = ListedFiles::new()
        .file("Project/a.txt", "1")
        .file("Project/b.txt", "2");
    let crowded = land(dialogs.open(OpenRequest::folders().max_entries(1)));
    crowded_files.pick_into(&the_input(&window));
    owner.until_landed(&crowded).await;
    assert_eq!(
        *crowded.borrow(),
        Some(Ok(Err(DialogError::TooManyEntries { limit: 1 })))
    );
    assert_eq!(crowded_files.touched(), Vec::<String>::new());

    let heavy_files = ListedFiles::new()
        .declaring("Project/a.txt", "12", 6.0)
        .declaring("Project/src/b.txt", "34", 3.0);
    let heavy = land(dialogs.open(OpenRequest::folders().max_bytes(8)));
    heavy_files.pick_into(&the_input(&window));
    owner.until_landed(&heavy).await;
    assert_eq!(*heavy.borrow(), too_large);
    assert_eq!(heavy_files.read(), Vec::<String>::new(), "no file was read");
}

#[wasm_bindgen_test]
async fn a_dismissed_directory_picker_answers_a_cancel() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let picker = DirectoryPickerStandIn::install();
    let answered = land(dialogs(&owner, &window, true).open(OpenRequest::folders()));

    picker.refuse("AbortError");
    owner.until_landed(&answered).await;

    assert_eq!(*answered.borrow(), Some(Ok(Ok(None))));
    assert!(window.find("input").is_none(), "no input stood in");
}

#[wasm_bindgen_test]
async fn a_dropped_folder_dialog_keeps_the_page_busy_until_its_picker_settles() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let picker = DirectoryPickerStandIn::install();
    let dialogs = dialogs(&owner, &window, true);
    drop(dialogs.open(OpenRequest::folders()));

    // The browser's picker is still up: the page shows no second one.
    let refused = land(dialogs.open(OpenRequest::folders()));
    owner.until_landed(&refused).await;
    assert_eq!(*refused.borrow(), Some(Ok(Err(DialogError::Busy))));
    assert_eq!(picker.modes.borrow().len(), 1, "one picker was asked for");

    // The pick that ends it is read by nobody, and frees the page.
    picker.choose("Project", r#"{"a.txt":"late"}"#);
    owner.settle().await;
    assert_eq!(
        picker.asked(),
        Vec::<String>::new(),
        "nobody walks a folder nobody waits for"
    );
    let next = land(dialogs.open(OpenRequest::folders()));
    picker.refuse("AbortError");
    owner.until_landed(&next).await;
    assert_eq!(*next.borrow(), Some(Ok(Ok(None))));
}

#[wasm_bindgen_test]
async fn a_folder_dialog_ends_with_its_window_and_its_late_pick_is_read_by_nobody() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let other = CanvasWindow::new(&owner);
    let picker = DirectoryPickerStandIn::install();
    let answers = Rc::new(Cell::new(0_u32));
    let (counted, answered) = (Rc::clone(&answers), Slot::<OpenResult>::default());
    let landed = Rc::clone(&answered);
    dialogs(&owner, &window, true)
        .open(OpenRequest::folders())
        .on_complete(move |outcome| {
            counted.set(counted.get() + 1);
            *landed.borrow_mut() = Some(outcome);
        });

    window.closer.close();
    owner.until_landed(&answered).await;
    assert_eq!(*answered.borrow(), Some(Ok(Err(DialogError::WindowClosed))));

    // The browser's picker outlives the window: the page stays busy until
    // the user ends it, and what they pick then goes to nobody.
    let refused = land(dialogs(&owner, &other, true).open(OpenRequest::folders()));
    owner.until_landed(&refused).await;
    assert_eq!(*refused.borrow(), Some(Ok(Err(DialogError::Busy))));
    picker.choose("Project", r#"{"a.txt":"late"}"#);
    owner.settle().await;
    assert_eq!(answers.get(), 1, "the dialog answered once");
    assert_eq!(
        picker.asked(),
        Vec::<String>::new(),
        "nobody walks the late pick"
    );
    let next = land(dialogs(&owner, &other, true).open(OpenRequest::folders()));
    picker.refuse("AbortError");
    owner.until_landed(&next).await;
    assert_eq!(*next.borrow(), Some(Ok(Ok(None))), "the page is free again");
}

/// Make the page's inputs reflect no `webkitdirectory` until the guard
/// drops, as those of a browser whose file input takes no folder.
fn without_folder_inputs() -> Replaced {
    let view: JsValue = the_view().into();
    let prototype = Reflect::get(&view, &"HTMLInputElement".into())
        .and_then(|input| Reflect::get(&input, &"prototype".into()))
        .expect("the page has inputs");
    Replaced::new(&prototype, "webkitdirectory", &JsValue::UNDEFINED)
}

#[wasm_bindgen_test]
async fn a_browser_with_no_folder_chooser_refuses_a_folder_dialog_for_good() {
    let _no_directory_picker = without_directory_picker();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);

    {
        // Neither API: refused before the gesture is asked about, since no
        // later gesture brings a chooser.
        let _no_folder_inputs = without_folder_inputs();
        for active in [false, true] {
            let answered = land(dialogs(&owner, &window, active).open(OpenRequest::folders()));
            owner.until_landed(&answered).await;
            assert_eq!(
                *answered.borrow(),
                refused_as(UnsupportedReason::NotOnThisDevice),
                "in a gesture: {active}"
            );
            assert!(window.find("input").is_none(), "no input was shown");
        }
    }

    // An input that takes the attribute and shows a file chooser all the
    // same (Chrome for Android before 132, Safari on iOS before 18.4): its
    // files say nothing of a folder.
    let answered = land(dialogs(&owner, &window, true).open(OpenRequest::folders()));
    pick(&the_input(&window), &[("a.txt", "one"), ("b.txt", "two")]);
    owner.until_landed(&answered).await;
    assert_eq!(
        *answered.borrow(),
        refused_as(UnsupportedReason::NotOnThisDevice)
    );
    assert!(window.find("input").is_none(), "the input left the page");
}

/// A File System Access picker `name` that refuses as one in a
/// cross-origin frame does, until the guard drops: the API is not on every
/// test browser, and a real one would ask for a real gesture.
fn refusing_picker(name: &str) -> (Closure<dyn FnMut(JsValue) -> JsValue>, Replaced) {
    let picker = Closure::<dyn FnMut(JsValue) -> JsValue>::new(|_: JsValue| {
        let refusal =
            js_sys::Error::new("Cross origin sub frames aren't allowed to show a file picker.");
        refusal.set_name("SecurityError");
        Promise::reject(&refusal).into()
    });
    let view: JsValue = the_view().into();
    let replaced = Replaced::new(&view, name, picker.as_ref());
    (picker, replaced)
}

/// Run turns and browser tasks until `window` shows an input; panic after
/// two hundred rounds.
async fn until_an_input(owner: &TestOwner, window: &CanvasWindow) -> HtmlInputElement {
    for _ in 0..200 {
        owner.turn();
        if window.find("input[type=file]").is_some() {
            return the_input(window);
        }
        browser_task().await;
    }
    panic!("no input was shown");
}

#[wasm_bindgen_test]
async fn a_refused_picker_hands_over_to_the_input_while_the_gesture_lasts() {
    // The picker a cross-origin frame gets: refused with `SecurityError`.
    let _picker = refusing_picker("showOpenFilePicker");
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let text = || OpenRequest::files().filter(FileFilter::new("Text").with_extension("txt"));
    let opened = land(dialogs(&owner, &window, true).open(text()));
    let input = until_an_input(&owner, &window).await;
    assert_eq!(input.accept(), ".txt");
    dispatch(&input, "cancel", false);
    owner.until_landed(&opened).await;
    assert_eq!(*opened.borrow(), Some(Ok(Ok(None))));

    // The gesture spent before the browser refused: one answer, the same
    // as the provider's own check.
    let active = Rc::new(Cell::new(true));
    let spent = land(dialogs_on(&owner, &window, window.page_with(&active)).open(text()));
    active.set(false);
    owner.until_landed(&spent).await;
    assert_eq!(
        *spent.borrow(),
        Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
            FileDialogCapability,
        >(
            UnsupportedReason::Unavailable
        )))))
    );
    assert!(window.find("input").is_none(), "no input stood in");
}

#[wasm_bindgen_test]
async fn a_refused_directory_picker_hands_over_to_the_folder_input_while_the_gesture_lasts() {
    // The picker a cross-origin frame gets: refused with `SecurityError`.
    let _picker = refusing_picker("showDirectoryPicker");
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let opened = land(dialogs(&owner, &window, true).open(OpenRequest::folders()));
    let input = until_an_input(&owner, &window).await;
    assert!(input.webkitdirectory(), "the input asks for a folder");
    pick_folder(&input, &[("Project/a.txt", "first")]);
    owner.until_landed(&opened).await;
    assert_eq!(
        the_folder(&the_picks(&opened)),
        (
            "Project".to_owned(),
            vec![("a.txt".to_owned(), "first".to_owned())]
        )
    );

    // The gesture spent before the browser refused: one answer, the same
    // as the provider's own check.
    let active = Rc::new(Cell::new(true));
    let spent =
        land(dialogs_on(&owner, &window, window.page_with(&active)).open(OpenRequest::folders()));
    active.set(false);
    owner.until_landed(&spent).await;
    assert_eq!(*spent.borrow(), refused_as(UnsupportedReason::Unavailable));
    assert!(window.find("input").is_none(), "no input stood in");
}

#[wasm_bindgen_test]
async fn a_refused_save_picker_hands_the_contents_over_as_a_download() {
    let _picker = refusing_picker("showSaveFilePicker");
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let caught = CaughtLinks::on(&window);
    let saved = land(
        dialogs(&owner, &window, true).save(
            SaveRequest::new()
                .suggested_name("notes.txt")
                .contents(b"x".to_vec()),
        ),
    );
    owner.until_landed(&saved).await;
    let location = match saved.borrow_mut().take() {
        Some(Ok(Ok(Some(location)))) => location,
        other => panic!("a download: {other:?}"),
    };
    assert_eq!(
        location.file().form(),
        PickedForm::Download { name: "notes.txt" }
    );
    assert_eq!(caught.links.borrow().len(), 1);
}

#[wasm_bindgen_test]
async fn a_save_outside_a_gesture_is_a_download_whose_fate_is_not_known() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let caught = CaughtLinks::on(&window);

    let saved = land(
        dialogs(&owner, &window, false).save(
            SaveRequest::new()
                .suggested_name("notes")
                .filter(
                    FileFilter::new("Text")
                        .with_extension("txt")
                        .with_mime_type("text/plain"),
                )
                .contents(b"written".to_vec()),
        ),
    );
    owner.until_landed(&saved).await;

    let location = match saved.borrow_mut().take() {
        Some(Ok(Ok(Some(location)))) => location,
        other => panic!("a download: {other:?}"),
    };
    assert_eq!(location.filter(), None, "a download reports no filter");
    assert_eq!(
        location.file().form(),
        PickedForm::Download { name: "notes.txt" },
        "where and whether it was written is not known"
    );
    assert_eq!(location.file().contents(), None);
    let clicked = caught.links.borrow();
    let [(name, href)] = clicked.as_slice() else {
        panic!("one download was clicked: {clicked:?}");
    };
    assert_eq!(name, "notes.txt");
    assert!(
        href.starts_with("blob:"),
        "the download is the blob: {href}"
    );
    assert!(window.find("a").is_none(), "the link left the page");
}

/// A stand-in for the save picker: `showSaveFilePicker` answers when the
/// test resolves it, with a handle whose stream records what it is given.
struct SavePickerStandIn {
    resolve: Rc<RefCell<Option<js_sys::Function>>>,
    written: Rc<RefCell<Vec<String>>>,
    _picker: Closure<dyn FnMut(JsValue) -> JsValue>,
    _replaced: Replaced,
}

impl SavePickerStandIn {
    fn install() -> Self {
        let resolve: Rc<RefCell<Option<js_sys::Function>>> = Rc::default();
        let written: Rc<RefCell<Vec<String>>> = Rc::default();
        let pending = Rc::clone(&resolve);
        let picker = Closure::<dyn FnMut(JsValue) -> JsValue>::new(move |_: JsValue| {
            let pending = Rc::clone(&pending);
            Promise::new(&mut |answer, _| *pending.borrow_mut() = Some(answer)).into()
        });
        let view: JsValue = the_view().into();
        let replaced = Replaced::new(&view, "showSaveFilePicker", picker.as_ref());
        Self {
            resolve,
            written,
            _picker: picker,
            _replaced: replaced,
        }
    }

    /// Answer the picker with a handle named `name`.
    fn choose(&self, name: &str) {
        let record = |step: &'static str, written: &Rc<RefCell<Vec<String>>>| {
            let written = Rc::clone(written);
            Closure::<dyn FnMut(JsValue) -> JsValue>::new(move |_: JsValue| {
                written.borrow_mut().push(step.to_owned());
                Promise::resolve(&JsValue::UNDEFINED).into()
            })
            .into_js_value()
        };
        let stream = js_sys::Object::new();
        let _ = Reflect::set(&stream, &"write".into(), &record("write", &self.written));
        let _ = Reflect::set(&stream, &"close".into(), &record("close", &self.written));
        let _ = Reflect::set(&stream, &"abort".into(), &record("abort", &self.written));
        let handle = js_sys::Object::new();
        let _ = Reflect::set(&handle, &"name".into(), &JsValue::from_str(name));
        let create =
            Closure::<dyn FnMut() -> JsValue>::new(move || Promise::resolve(&stream).into())
                .into_js_value();
        let _ = Reflect::set(&handle, &"createWritable".into(), &create);
        let resolve = self
            .resolve
            .borrow_mut()
            .take()
            .expect("the picker was asked for");
        resolve
            .call1(&JsValue::UNDEFINED, &handle)
            .expect("the picker answers");
    }
}

#[wasm_bindgen_test]
async fn a_save_the_user_chose_is_written_after_its_dialog_was_dropped() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let picker = SavePickerStandIn::install();
    let shown = dialogs(&owner, &window, true).save(
        SaveRequest::new()
            .filter(FileFilter::new("Text").with_extension("txt"))
            .contents(b"kept".to_vec()),
    );
    owner.settle().await;
    drop(shown);

    // The browser has made or emptied the chosen file by now.
    picker.choose("notes.txt");
    owner.settle().await;
    assert_eq!(*picker.written.borrow(), ["write", "close"]);
}

#[wasm_bindgen_test]
async fn a_save_without_contents_has_no_path_to_answer_on_the_web() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let saved = land(dialogs(&owner, &window, true).save(SaveRequest::new()));
    owner.until_landed(&saved).await;
    assert_eq!(
        *saved.borrow(),
        Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
            FileDialogCapability,
        >(
            UnsupportedReason::NotOnThisPlatform
        )))))
    );
}

/// The page's `target[name]` shadowed by an own property holding
/// `replacement` until the guard drops, so a test reaches no real window,
/// picker or badge; an own property it hid is put back.
struct Replaced {
    target: js_sys::Object,
    name: JsValue,
    hidden: JsValue,
}

impl Replaced {
    fn new(target: &JsValue, name: &str, replacement: &JsValue) -> Self {
        let target: js_sys::Object = target.clone().unchecked_into();
        let name = JsValue::from_str(name);
        let hidden = js_sys::Object::get_own_property_descriptor(&target, &name);
        let descriptor = js_sys::Object::new();
        let _ = Reflect::set(&descriptor, &"value".into(), replacement);
        let _ = Reflect::set(&descriptor, &"configurable".into(), &true.into());
        let _ = Reflect::set(&descriptor, &"writable".into(), &true.into());
        js_sys::Object::define_property(&target, &name, &descriptor);
        Self {
            target,
            name,
            hidden,
        }
    }
}

impl Drop for Replaced {
    fn drop(&mut self) {
        let _ = Reflect::delete_property(&self.target, &self.name);
        if !self.hidden.is_undefined() {
            js_sys::Object::define_property(&self.target, &self.name, self.hidden.unchecked_ref());
        }
    }
}

fn launcher(owner: &TestOwner, window: &CanvasWindow, active: bool) -> Launcher {
    Launcher::new(
        WebLauncher::new(window.page(active)),
        &owner.ctx(&window.window),
    )
}

/// A stand-in for `window.open`: records its arguments and answers
/// `context` (a blank context's stand-in, or `null` for a blocked one).
fn open_stand_in(
    calls: &Rc<RefCell<Vec<Vec<String>>>>,
    context: JsValue,
) -> Closure<dyn FnMut(JsValue, JsValue) -> JsValue> {
    let seen = Rc::clone(calls);
    Closure::new(move |url: JsValue, target: JsValue| {
        seen.borrow_mut().push(
            [url, target]
                .iter()
                .filter_map(JsValue::as_string)
                .collect(),
        );
        context.clone()
    })
}

#[wasm_bindgen_test]
async fn a_page_opens_apart_without_its_opener_or_its_referrer() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let blank = the_view()
        .document()
        .expect("the page's document")
        .implementation()
        .expect("its implementation")
        .create_html_document()
        .expect("a blank document");
    let context = js_sys::Object::new();
    let _ = Reflect::set(&context, &"opener".into(), &JsValue::from_str("the app"));
    let _ = Reflect::set(&context, &"document".into(), &blank);
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let open = open_stand_in(&calls, context.clone().into());
    let view: JsValue = the_view().into();
    let _replaced = Replaced::new(&view, "open", open.as_ref());

    let page =
        ExternalUri::try_new("https://example.org/a?b=c", &[Scheme::HTTPS]).expect("an https link");
    let opened = landed(launcher(&owner, &window, true).open_uri(&page));
    owner.until_landed(&opened).await;

    assert_eq!(*opened.borrow(), Some(Ok(Ok(()))));
    assert_eq!(
        *calls.borrow(),
        [vec!["about:blank".to_owned(), "_blank".to_owned()]]
    );
    assert!(
        Reflect::get(&context, &"opener".into()).is_ok_and(|opener| opener.is_null()),
        "the new context cannot reach the application"
    );
    let link = blank
        .query_selector("a")
        .ok()
        .flatten()
        .expect("the blank document follows a link")
        .unchecked_into::<web_sys::HtmlAnchorElement>();
    assert_eq!(link.href(), "https://example.org/a?b=c");
    assert_eq!(link.rel(), "noreferrer");
}

#[wasm_bindgen_test]
async fn a_blocked_page_is_refused_and_a_mail_link_is_handed_off_by_the_page() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let caught = CaughtLinks::on(&window);
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let open = open_stand_in(&calls, JsValue::NULL);
    let view: JsValue = the_view().into();
    let _replaced = Replaced::new(&view, "open", open.as_ref());
    let launcher = launcher(&owner, &window, true);

    let page = ExternalUri::try_new("https://example.org/", &[Scheme::HTTPS]).expect("a link");
    let mail =
        ExternalUri::try_new("mailto:someone@example.org", &[Scheme::MAILTO]).expect("a mail link");
    let socket = ExternalUri::try_new(
        "wss://example.org/feed",
        &[Scheme::try_new("wss").expect("a scheme")],
    )
    .expect("a socket");
    let blocked = landed(launcher.open_uri(&page));
    let mailed = landed(launcher.open_uri(&mail));
    let connected = landed(launcher.open_uri(&socket));
    owner.until_landed(&connected).await;

    assert_eq!(
        *blocked.borrow(),
        Some(Ok(Err(LaunchError::Unsupported(Unsupported::of::<
            LauncherCapability,
        >(
            UnsupportedReason::Unavailable
        )))))
    );
    assert_eq!(*mailed.borrow(), Some(Ok(Ok(()))));
    assert_eq!(
        *caught.links.borrow(),
        [(String::new(), "mailto:someone@example.org".to_owned())],
        "the mail link is followed from the page, not by a navigation of it"
    );
    assert_eq!(*connected.borrow(), Some(Ok(Err(LaunchError::NoHandler))));
    assert_eq!(calls.borrow().len(), 1, "only the page asked for a context");
}

#[wasm_bindgen_test]
async fn a_launch_outside_a_gesture_and_a_path_are_refused_on_the_web() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let page = ExternalUri::try_new("https://example.org/", &[Scheme::HTTPS]).expect("a link");
    let outside = landed(launcher(&owner, &window, false).open_uri(&page));
    // The contract refuses a path wasm32's `Path` calls relative before a
    // backend hears it; the backend's own answer is asked for directly.
    let (asked, answer) = owner.completions.pending();
    WebLauncher::new(window.page(true)).reveal_path(PathBuf::from("/home/me/a.txt"), answer);
    let path = landed(asked);
    owner.until_landed(&path).await;
    let refused = |reason| {
        Some(Ok(Err(LaunchError::Unsupported(Unsupported::of::<
            LauncherCapability,
        >(reason)))))
    };
    assert_eq!(*outside.borrow(), refused(UnsupportedReason::Unavailable));
    assert_eq!(
        *path.borrow(),
        refused(UnsupportedReason::NotOnThisPlatform)
    );
}

/// A stand-in for one Badging API call, recording each count it is called
/// with (`None` for no argument).
fn badge_call(calls: &Rc<RefCell<Vec<Option<f64>>>>) -> Closure<dyn FnMut(JsValue) -> JsValue> {
    let seen = Rc::clone(calls);
    Closure::new(move |count: JsValue| {
        seen.borrow_mut().push(count.as_f64());
        Promise::resolve(&JsValue::UNDEFINED).into()
    })
}

#[wasm_bindgen_test]
async fn the_entry_shows_its_count_as_the_icons_badge_and_honours_no_progress() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let navigator: JsValue = web_sys::window()
        .expect("the test runs in a page")
        .navigator()
        .into();
    let set: Rc<RefCell<Vec<Option<f64>>>> = Rc::default();
    let clear: Rc<RefCell<Vec<Option<f64>>>> = Rc::default();
    let set_badge = badge_call(&set);
    let clear_badge = badge_call(&clear);
    let _set = Replaced::new(&navigator, "setAppBadge", set_badge.as_ref());
    let _clear = Replaced::new(&navigator, "clearAppBadge", clear_badge.as_ref());

    let entry = TaskbarEntry::new(
        WebBadge::try_new(&window.page(true)).expect("the browser has the Badging API"),
        &owner.ctx(&window.window),
    );
    assert_eq!(entry.honours(), Honoured::new(false, false, true, false));
    entry.set_count(Some(3));
    entry.set_count(Some(u64::MAX));
    entry.set_count(None);
    entry.set_progress(Progress::Indeterminate);
    owner.settle().await;

    assert_eq!(
        *set.borrow(),
        [Some(3.0), Some(9_007_199_254_740_991.0)],
        "a count past what a double holds shows as the largest it holds"
    );
    // The progress, which the badge does not show, still goes with the state.
    assert_eq!(*clear.borrow(), [None, None]);
    assert_eq!(
        entry.state(),
        EntryState::new(Progress::Indeterminate, None)
    );
}

#[wasm_bindgen_test]
fn a_browser_without_the_badging_api_has_no_entry() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let navigator: JsValue = web_sys::window()
        .expect("the test runs in a page")
        .navigator()
        .into();
    let _set = Replaced::new(&navigator, "setAppBadge", &JsValue::UNDEFINED);
    assert_eq!(
        WebBadge::try_new(&window.page(true)).err(),
        Some(Unsupported::of::<crate::taskbar::TaskbarCapability>(
            UnsupportedReason::NotOnThisDevice
        ))
    );
}

fn message_boxes(owner: &TestOwner, window: &CanvasWindow) -> MessageBoxes {
    MessageBoxes::try_new(
        WebMessageBoxes::new(window.page(true)),
        &owner.ctx(&window.window),
    )
    .expect("a window has message boxes")
}

fn the_box(window: &CanvasWindow) -> HtmlDialogElement {
    window
        .find("dialog")
        .expect("the box is on the page")
        .unchecked_into::<HtmlDialogElement>()
}

/// The text of the element `dialog`'s `relation` names (`labelledBy`,
/// `describedBy`), read through ARIA element reflection where the browser
/// has it, through the id its attribute holds otherwise; `None` when it
/// names nothing in the box.
fn aria_text(dialog: &HtmlDialogElement, relation: &str) -> Option<String> {
    let reflected = Reflect::get(
        dialog,
        &JsValue::from_str(&format!("aria{relation}Elements")),
    )
    .ok()
    .and_then(|elements| elements.dyn_into::<Array>().ok())
    .and_then(|elements| elements.get(0).dyn_into::<Element>().ok());
    let element = reflected.or_else(|| {
        let attribute = format!("aria-{}", relation.to_ascii_lowercase());
        let id = dialog.get_attribute(&attribute)?;
        dialog.query_selector(&format!("#{id}")).ok().flatten()
    })?;
    dialog
        .contains(Some(&element))
        .then(|| element.text_content())
        .flatten()
}

/// Make the test page refuse inline styles (a content security policy
/// without `'unsafe-inline'`) for the rest of the run, and check that it
/// does: a `style` attribute set afterwards changes nothing.
fn refuse_inline_styles() {
    thread_local! {
        static REFUSED: Cell<bool> = const { Cell::new(false) };
    }
    let document = the_view().document().expect("the page's document");
    if !REFUSED.with(|refused| refused.replace(true)) {
        let policy = document.create_element("meta").expect("a meta element");
        let _ = policy.set_attribute("http-equiv", "Content-Security-Policy");
        let _ = policy.set_attribute("content", "style-src 'self'");
        document
            .head()
            .expect("the page has a head")
            .append_child(&policy)
            .expect("the policy goes in the head");
    }
    let probe = document
        .create_element("p")
        .expect("a probe")
        .unchecked_into::<web_sys::HtmlElement>();
    let _ = probe.set_attribute("style", "white-space: pre-wrap");
    assert_eq!(
        probe.style().get_property_value("white-space").as_deref(),
        Ok(""),
        "the page refuses inline style attributes"
    );
}

#[wasm_bindgen_test]
async fn a_message_box_is_a_modal_dialog_answering_the_button_pressed() {
    refuse_inline_styles();
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered = land(
        message_boxes(&owner, &window).show(
            MessageBoxRequest::new("Save the changes?\nThey are lost otherwise.")
                .title("Unsaved changes")
                .level(MessageLevel::Warning)
                .buttons(MessageButtons::YesNoCancel),
        ),
    );
    let dialog = the_box(&window);
    assert!(dialog.open(), "the box is shown");
    assert_eq!(dialog.get_attribute("role").as_deref(), Some("alertdialog"));
    assert_eq!(
        aria_text(&dialog, "LabelledBy").as_deref(),
        Some("Unsaved changes")
    );
    assert_eq!(
        aria_text(&dialog, "DescribedBy").as_deref(),
        Some("Save the changes?\nThey are lost otherwise.")
    );
    let message = dialog
        .query_selector("p")
        .ok()
        .flatten()
        .expect("the message")
        .unchecked_into::<web_sys::HtmlElement>();
    assert_eq!(
        message.style().get_property_value("white-space").as_deref(),
        Ok("pre-wrap"),
        "the message keeps its line breaks where inline styles are refused"
    );
    let labels: Vec<String> = (0..3)
        .filter_map(|at| {
            dialog
                .query_selector_all("button")
                .ok()?
                .get(at)?
                .text_content()
        })
        .collect();
    assert_eq!(labels, ["Yes", "No", "Cancel"]);

    dialog
        .query_selector("button[value=no]")
        .ok()
        .flatten()
        .expect("a No button")
        .unchecked_into::<web_sys::HtmlElement>()
        .click();
    owner.until_landed(&answered).await;

    assert_eq!(*answered.borrow(), Some(Ok(Ok(MessageAnswer::No))));
    assert!(window.find("dialog").is_none(), "the box left the page");
}

#[wasm_bindgen_test]
fn a_box_shows_its_detail_its_labels_and_focuses_its_default_button() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let shown = message_boxes(&owner, &window).show(
        MessageBoxRequest::new("Delete the file?")
            .detail("It cannot be restored.")
            .buttons(MessageButtons::OkCancel)
            .button_label(MessageAnswer::Ok, "Delete")
            .default_button(MessageAnswer::Cancel),
    );
    let dialog = the_box(&window);
    let paragraphs = dialog.query_selector_all("p").expect("the paragraphs");
    let texts: Vec<String> = (0..paragraphs.length())
        .filter_map(|at| paragraphs.get(at)?.text_content())
        .collect();
    assert_eq!(texts, ["Delete the file?", "It cannot be restored."]);
    let buttons = dialog.query_selector_all("button").expect("the buttons");
    let button = |at: u32| {
        buttons
            .get(at)
            .expect("a button")
            .unchecked_into::<web_sys::HtmlElement>()
    };
    assert_eq!(button(0).text_content().as_deref(), Some("Delete"));
    assert_eq!(button(1).text_content().as_deref(), Some("Cancel"));
    assert!(
        !button(0).autofocus(),
        "the default is not the first button"
    );
    assert!(button(1).autofocus(), "the default button takes focus");
    drop(shown);
}

#[wasm_bindgen_test]
fn a_box_with_no_title_is_named_by_its_message() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let shown = message_boxes(&owner, &window).show(MessageBoxRequest::new("Saved."));
    let dialog = the_box(&window);
    assert_eq!(aria_text(&dialog, "LabelledBy").as_deref(), Some("Saved."));
    assert_eq!(aria_text(&dialog, "DescribedBy").as_deref(), Some("Saved."));
    drop(shown);
}

#[wasm_bindgen_test]
fn two_runtimes_boxes_each_name_their_own() {
    // Two UI runtimes of one page each mint their first window id.
    let (first_owner, second_owner) = (TestOwner::new(), TestOwner::new());
    let (first, second) = (
        CanvasWindow::new(&first_owner),
        CanvasWindow::new(&second_owner),
    );
    assert_eq!(first.window.id(), second.window.id());
    let shown = [
        message_boxes(&first_owner, &first).show(MessageBoxRequest::new("one").title("First")),
        message_boxes(&second_owner, &second).show(MessageBoxRequest::new("two").title("Second")),
    ];
    assert_eq!(
        aria_text(&the_box(&first), "LabelledBy").as_deref(),
        Some("First")
    );
    assert_eq!(
        aria_text(&the_box(&second), "LabelledBy").as_deref(),
        Some("Second")
    );
    drop(shown);
}

/// `HTMLDialogElement.prototype.showModal` replaced by one that throws an
/// `InvalidStateError`, until the guard drops.
fn throwing_show_modal() -> Replaced {
    let view: JsValue = the_view().into();
    let prototype = Reflect::get(&view, &"HTMLDialogElement".into())
        .and_then(|dialog| Reflect::get(&dialog, &"prototype".into()))
        .expect("the page has dialogs");
    let throwing =
        js_sys::Function::new_no_args("throw new DOMException('refused', 'InvalidStateError');");
    Replaced::new(&prototype, "showModal", &throwing)
}

#[wasm_bindgen_test]
async fn a_box_the_browser_will_not_show_answers_its_failure_once() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let refused = {
        let _throwing = throwing_show_modal();
        land(message_boxes(&owner, &window).show(MessageBoxRequest::new("Saved.")))
    };
    owner.until_landed(&refused).await;
    assert_eq!(
        *refused.borrow(),
        Some(Ok(Err(DialogError::Failed(
            flui_platform::SystemFailure::Browser(flui_platform::BrowserException::InvalidState)
        ))))
    );
    assert!(window.find("dialog").is_none(), "no box was left behind");

    // A Yes or No box closed by the browser is shown again; if the browser
    // will not, the box answers that failure, once.
    let answered = land(
        message_boxes(&owner, &window)
            .show(MessageBoxRequest::new("Replace it?").buttons(MessageButtons::YesNo)),
    );
    let dialog = the_box(&window);
    let throwing = throwing_show_modal();
    dialog.close();
    owner.until_landed(&answered).await;
    drop(throwing);
    assert_eq!(
        *answered.borrow(),
        Some(Ok(Err(DialogError::Failed(
            flui_platform::SystemFailure::Browser(flui_platform::BrowserException::InvalidState)
        ))))
    );
    assert!(window.find("dialog").is_none(), "the box left the page");
}

#[wasm_bindgen_test]
async fn a_box_closed_without_a_button_answers_what_closing_means() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let boxes = message_boxes(&owner, &window);
    let cancelled =
        land(boxes.show(MessageBoxRequest::new("Go on?").buttons(MessageButtons::OkCancel)));
    let dialog = the_box(&window);
    assert!(
        dispatch(&dialog, "cancel", true),
        "Escape may close a box that can be cancelled"
    );
    dialog.close();
    owner.until_landed(&cancelled).await;
    assert_eq!(*cancelled.borrow(), Some(Ok(Ok(MessageAnswer::Cancel))));
}

#[wasm_bindgen_test]
async fn a_yes_or_no_box_stays_until_one_is_chosen() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let boxes = message_boxes(&owner, &window);
    let answered =
        land(boxes.show(MessageBoxRequest::new("Replace it?").buttons(MessageButtons::YesNo)));
    let dialog = the_box(&window);
    assert!(
        !dispatch(&dialog, "cancel", true),
        "Escape does not close a box with no way out but an answer"
    );
    dialog.close();
    owner.settle().await;
    assert_eq!(*answered.borrow(), None, "closing is no answer");
    assert!(dialog.open(), "the box is shown again");

    dialog
        .query_selector("button[value=yes]")
        .ok()
        .flatten()
        .expect("a Yes button")
        .unchecked_into::<web_sys::HtmlElement>()
        .click();
    owner.until_landed(&answered).await;
    assert_eq!(*answered.borrow(), Some(Ok(Ok(MessageAnswer::Yes))));
}

#[wasm_bindgen_test]
fn a_dropped_box_is_closed_and_taken_down() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let shown = message_boxes(&owner, &window).show(MessageBoxRequest::new("Working…"));
    assert!(the_box(&window).open());
    drop(shown);
    assert!(window.find("dialog").is_none());
}

#[wasm_bindgen_test]
async fn a_box_whose_window_closes_is_taken_down_and_answers_so() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered = land(message_boxes(&owner, &window).show(MessageBoxRequest::new("Working…")));
    assert!(the_box(&window).open());
    window.closer.close();
    owner.until_landed(&answered).await;
    assert_eq!(*answered.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    assert!(window.find("dialog").is_none(), "the box left the page");
}

/// A stand-in for `navigator.share`, recording what each call shares and
/// answering from `answers` (an exception's name, `None` for a share taken),
/// last first, until the guard drops.
struct ShareStandIn {
    shared: Rc<RefCell<Vec<(Option<String>, Option<String>)>>>,
    _share: Closure<dyn FnMut(JsValue) -> JsValue>,
    _replaced: Replaced,
}

impl ShareStandIn {
    fn install(answers: Vec<Option<&'static str>>) -> Self {
        Self::consuming(answers, &Rc::new(Cell::new(true)))
    }

    /// A stand-in that, as a browser does, spends the page's gesture
    /// (`activation`) when it is called.
    fn consuming(answers: Vec<Option<&'static str>>, activation: &Rc<Cell<bool>>) -> Self {
        let shared: Rc<RefCell<Vec<(Option<String>, Option<String>)>>> = Rc::default();
        let seen = Rc::clone(&shared);
        let answers = RefCell::new(answers);
        let activation = Rc::clone(activation);
        let share = Closure::<dyn FnMut(JsValue) -> JsValue>::new(move |data: JsValue| {
            activation.set(false);
            let read = |name: &str| {
                Reflect::get(&data, &JsValue::from_str(name))
                    .ok()
                    .and_then(|value| value.as_string())
            };
            seen.borrow_mut().push((read("text"), read("url")));
            match answers.borrow_mut().pop().flatten() {
                None => Promise::resolve(&JsValue::UNDEFINED).into(),
                Some(name) => {
                    let error = js_sys::Error::new("refused");
                    error.set_name(name);
                    Promise::reject(&error).into()
                }
            }
        });
        let navigator: JsValue = the_view().navigator().into();
        let replaced = Replaced::new(&navigator, "share", share.as_ref());
        Self {
            shared,
            _share: share,
            _replaced: replaced,
        }
    }
}

fn share(owner: &TestOwner, window: &CanvasWindow, active: bool) -> Share {
    Share::try_new(
        WebShare::try_new(window.page(active)).expect("the page has a share sheet"),
        &owner.ctx(&window.window),
    )
    .expect("a window has a share sheet")
}

#[wasm_bindgen_test]
async fn a_share_reaches_the_web_share_api_and_a_dismissed_sheet_is_not_reported() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    // The browser spends the gesture when it is asked, so the provider must
    // read whether it was there at the call, not when the answer comes.
    let active = Rc::new(Cell::new(true));
    let browser = ShareStandIn::consuming(
        vec![Some("NotAllowedError"), Some("AbortError"), None],
        &active,
    );
    let share = Share::try_new(
        WebShare::try_new(window.page_with(&active)).expect("the page has a share sheet"),
        &owner.ctx(&window.window),
    )
    .expect("a window has a share sheet");
    let request = || {
        ShareRequest::new()
            .text("Look")
            .uri(ExternalUri::try_new("https://example.org/", &[Scheme::HTTPS]).expect("a link"))
    };

    let mut answers = Vec::new();
    for _ in 0..3 {
        // Each share from a gesture of its own.
        active.set(true);
        let answer = landed(share.share(request()));
        owner.until_landed(&answer).await;
        answers.push(answer);
    }
    let [done, dismissed, refused] = <[Slot<crate::share::ShareResult>; 3]>::try_from(answers)
        .unwrap_or_else(|_| unreachable!("three shares were made"));

    assert_eq!(*done.borrow(), Some(Ok(Ok(()))));
    assert_eq!(*dismissed.borrow(), Some(Ok(Ok(()))));
    assert_eq!(
        *refused.borrow(),
        Some(Ok(Err(ShareError::Unsupported(Unsupported::of::<
            ShareCapability,
        >(
            UnsupportedReason::NotDeclared {
                what: super::rules::SHARE_POLICY
            }
        ))))),
        "refused while the browser said the gesture was there: the frame lacks the policy"
    );
    assert_eq!(
        browser.shared.borrow()[0],
        (
            Some("Look".to_owned()),
            Some("https://example.org/".to_owned())
        )
    );
}

#[wasm_bindgen_test]
async fn a_share_of_files_a_link_off_the_web_or_outside_a_gesture_never_asks_the_browser() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let browser = ShareStandIn::install(Vec::new());
    let mail =
        ExternalUri::try_new("mailto:someone@example.org", &[Scheme::MAILTO]).expect("a mail link");
    // The contract refuses a path wasm32's `Path` calls relative before a
    // backend hears it; the backend's own answer is asked for directly.
    let (asked, answer) = owner.completions.pending();
    WebShare::try_new(window.page(true))
        .expect("the stand-in is the page's share sheet")
        .share(
            &window.window,
            ShareRequest::new().file("/home/me/a.txt"),
            answer,
        );
    let files = landed(asked);
    let off_the_web = landed(share(&owner, &window, true).share(ShareRequest::new().uri(mail)));
    let outside = landed(share(&owner, &window, false).share(ShareRequest::new().text("Look")));
    owner.until_landed(&outside).await;
    let refused = |reason| {
        Some(Ok(Err(ShareError::Unsupported(Unsupported::of::<
            ShareCapability,
        >(reason)))))
    };
    assert_eq!(
        *files.borrow(),
        refused(UnsupportedReason::NotOnThisPlatform)
    );
    assert_eq!(
        *off_the_web.borrow(),
        refused(UnsupportedReason::NotOnThisPlatform)
    );
    assert_eq!(*outside.borrow(), refused(UnsupportedReason::Unavailable));
    assert!(
        browser.shared.borrow().is_empty(),
        "the browser was never asked"
    );
}

#[wasm_bindgen_test]
fn a_browser_without_web_share_has_no_share_sheet() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let navigator: JsValue = the_view().navigator().into();
    // Hide the API where the browser has it, as a browser without it is.
    let _replaced = Replaced::new(&navigator, "share", &JsValue::UNDEFINED);
    assert_eq!(
        WebShare::try_new(window.page(true)).err(),
        Some(Unsupported::of::<ShareCapability>(
            UnsupportedReason::NotOnThisDevice
        ))
    );
}

/// The handle `SystemPlugin` registers for `C` on a page, acquired in
/// `window` of `owner`'s runtime: what an application gets through the seam.
fn system_handle<C: Capability>(
    owner: &TestOwner,
    window: &WindowRef,
) -> Result<C::Handle, Unsupported> {
    let mut registrar = CapabilityRegistrar::new();
    crate::SystemPlugin::new().register(&mut registrar);
    let entry = registrar
        .into_entries()
        .into_iter()
        .find(|entry| entry.capability() == std::any::TypeId::of::<C>())
        .expect("the plugin registers every service");
    entry.provide(&owner.ctx(window)).map(|handle| {
        *handle
            .downcast::<C::Handle>()
            .expect("a provider hands out its capability's handle")
    })
}

/// A window of `owner`'s runtime whose handle names no canvas, as a
/// headless window's does not, open while the returned host and closer live.
fn canvasless_window(owner: &TestOwner) -> (WindowRef, Rc<dyn PlatformWindow>, WindowCloser) {
    let (closer, lifetime) = WindowCloser::new();
    let host = canvas_host_window(
        owner
            .windows
            .next()
            .expect("a test opens far fewer windows than ids"),
        Arc::new(CanvasTarget(0)),
    );
    (WindowRef::new(&host, lifetime), host, closer)
}

/// What a page's acquisition of `C` answers: provided, or refused for
/// this reason.
fn acquisition<C: Capability>(
    owner: &TestOwner,
    window: &WindowRef,
) -> Result<(), UnsupportedReason> {
    system_handle::<C>(owner, window)
        .map(|_| ())
        .map_err(|refusal| refusal.reason)
}

/// The rows of the crate's conformance table (`tests/conformance.rs`) for
/// the page's own providers, through `SystemPlugin`'s registration: that
/// table runs on the headless host, whose windows are no canvases and
/// whose harness waits on threads, so the web subject runs here, in a
/// browser, with canvas windows.
#[wasm_bindgen_test]
async fn the_web_providers_keep_the_system_services_contract() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let page = &window.window;
    // The Web Share API where the test's browser may lack it; nothing here
    // reaches it.
    let browser = ShareStandIn::install(Vec::new());

    // Each service is provided, or refused for a lasting reason.
    assert_eq!(acquisition::<LauncherCapability>(&owner, page), Ok(()));
    assert_eq!(acquisition::<FileDialogCapability>(&owner, page), Ok(()));
    assert_eq!(acquisition::<MessageBoxCapability>(&owner, page), Ok(()));
    assert_eq!(acquisition::<ShareCapability>(&owner, page), Ok(()));
    assert!(
        matches!(
            acquisition::<crate::taskbar::TaskbarCapability>(&owner, page),
            Ok(()) | Err(UnsupportedReason::NotOnThisDevice)
        ),
        "the badge is there where the browser has the Badging API"
    );
    let not_a_page = Err(UnsupportedReason::NotOnThisPlatform);
    assert_eq!(acquisition::<AutostartCapability>(&owner, page), not_a_page);
    assert_eq!(
        acquisition::<GlobalShortcutCapability>(&owner, page),
        not_a_page
    );
    assert_eq!(acquisition::<UserDirCapability>(&owner, page), not_a_page);
    assert_eq!(acquisition::<JumpListCapability>(&owner, page), not_a_page);
    assert_eq!(acquisition::<OsCapability>(&owner, page), not_a_page);

    // A window that is not a canvas has no page.
    let (canvasless, _host, _closer) = canvasless_window(&owner);
    for refused in [
        acquisition::<LauncherCapability>(&owner, &canvasless),
        acquisition::<FileDialogCapability>(&owner, &canvasless),
        acquisition::<MessageBoxCapability>(&owner, &canvasless),
        acquisition::<ShareCapability>(&owner, &canvasless),
    ] {
        assert_eq!(refused, Err(UnsupportedReason::NoWindow));
    }

    // A refusal the seam owes settles at an owner turn, never inside the
    // call, and a malformed request is refused without asking the browser.
    let launcher = system_handle::<LauncherCapability>(&owner, page).expect("a launcher");
    let relative = landed(launcher.open_path(Path::new("relative.pdf")));
    assert!(relative.borrow().is_none(), "never inside the call");
    let dialogs = system_handle::<FileDialogCapability>(&owner, page).expect("dialogs");
    let boxes = system_handle::<MessageBoxCapability>(&owner, page).expect("message boxes");
    let share = system_handle::<ShareCapability>(&owner, page).expect("a share sheet");
    let misnamed = land(dialogs.save(SaveRequest::new().suggested_name("dir/name")));
    let empty_box = land(boxes.show(MessageBoxRequest::new("")));
    let empty_share = landed(share.share(ShareRequest::new()));
    owner.until_landed(&empty_share).await;
    assert_eq!(*relative.borrow(), Some(Ok(Err(LaunchError::RelativePath))));
    assert_eq!(
        *misnamed.borrow(),
        Some(Ok(Err(DialogError::InvalidRequest(
            crate::dialog::MalformedRequest::SuggestedName
        ))))
    );
    assert_eq!(
        *empty_box.borrow(),
        Some(Ok(Err(DialogError::InvalidRequest(
            crate::dialog::MalformedRequest::EmptyMessage
        ))))
    );
    assert_eq!(
        *empty_share.borrow(),
        Some(Ok(Err(ShareError::InvalidRequest(
            crate::share::MalformedShare::Empty
        ))))
    );
    assert!(
        window.find("dialog, input, a").is_none(),
        "nothing was shown"
    );

    // The handles of a closed window answer that it closed, and never ask
    // the browser.
    window.closer.close();
    let opened = land(dialogs.open(OpenRequest::files()));
    let shown = land(boxes.show(MessageBoxRequest::new("Saved")));
    let shared = landed(share.share(ShareRequest::new().text("x")));
    owner.until_landed(&shared).await;
    assert_eq!(*opened.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    assert_eq!(*shown.borrow(), Some(Ok(Err(DialogError::WindowClosed))));
    assert_eq!(*shared.borrow(), Some(Ok(Err(ShareError::WindowClosed))));
    assert!(window.find("dialog, input").is_none(), "nothing was shown");
    assert!(
        browser.shared.borrow().is_empty(),
        "no row asked the browser to share"
    );
}
