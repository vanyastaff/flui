//! The web providers in a browser (headless Chrome in CI): each provider
//! over a canvas of the test page, reached through a window's raw handle as
//! a real window's is, with the page's word on user activation stated by
//! the test (no test can make a gesture), the browser's events dispatched at
//! the elements the provider shows, and `window.open` and the Badging API
//! replaced for the test that calls them. No test opens a picker or a
//! window: a picker is refused by the browser without a real gesture, which
//! the tests of that path read as its answer.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::{
    Abandoned, Completion, CompletionSource, HostBridge, OwnerJob, OwnerPoster, OwnerScope,
    PendingCompletions, ProviderContext, RuntimeLocals, WindowCloser, WindowRef,
};
use flui_platform::drag::PlatformDragSource;
use flui_platform::haptics::PlatformHaptics;
use flui_platform::launch::{
    ExternalUri, LaunchError, Launcher, LauncherBackend as _, LauncherCapability, Scheme,
};
use flui_platform::{
    AdmittedCommand, ApplyError, Cursor, CursorError, GestureGeometry, PlatformSystemChrome,
    PlatformTextInput, PlatformWindow, PreferenceQueryError, RedrawRequester, SurfaceTarget,
    SwapChainStatistics, Unsupported, UnsupportedReason, WindowAt, WindowExtent, WindowGeometry,
    WindowId, WindowIdMint, WindowState,
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
use web_sys::{Element, Event, EventInit, HtmlCanvasElement, HtmlInputElement};

use super::badge::WebBadge;
use super::dialogs::{PagePicker, WebDialogs};
use super::launcher::WebLauncher;
use super::page::{Activation, Page};
use crate::dialog::{
    Dialog, DialogError, FileDialogCapability, FileDialogs, FileFilter, OpenRequest, PickedForm,
    SaveRequest,
};
use crate::taskbar::{EntryState, Honoured, Progress, TaskbarEntry};

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
struct CanvasHostWindow {
    id: WindowId,
    target: Arc<CanvasTarget>,
}

impl PlatformWindow for CanvasHostWindow {
    flui_platform::window_takes_no_frame_verbs!();

    fn id(&self) -> WindowId {
        self.id
    }
    fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
        Ok(None)
    }
    fn request_redraw(&self) {}
    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        Arc::new(|| {})
    }
    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        Arc::clone(&self.target) as Arc<dyn SurfaceTarget>
    }
    fn state(&self) -> WindowState {
        WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO)
    }
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        command.against(&self.state().capabilities)?;
        Ok(())
    }
    fn set_cursor(&self, _: &Cursor) -> Result<(), CursorError> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into())
    }
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        None
    }
    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        None
    }
    fn window_at(&self, _: flui_sdk::foundation::geometry::Point<f64>) -> WindowAt {
        WindowAt::Unknown
    }
    fn reveal_after_first_frame(&self) {}
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
        let host: Rc<dyn PlatformWindow> = Rc::new(CanvasHostWindow { id, target });
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

/// A test page's file dialogs for `window` over `page`, the page's picker
/// shared through `owner`.
fn dialogs_on(owner: &TestOwner, window: &CanvasWindow, page: Rc<Page>) -> FileDialogs {
    quiet_input_pickers();
    let ctx = owner.ctx(&window.window);
    FileDialogs::try_new(
        WebDialogs::new(page, ctx.runtime_local::<PagePicker>())
            .with_focus_grace(TEST_FOCUS_GRACE_MS),
        &ctx,
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

#[wasm_bindgen_test]
async fn a_dismissed_input_answers_a_cancel() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let answered = land(dialogs.open(OpenRequest::files().filter(input_only())));

    dispatch(&the_input(&window), "cancel", false);
    owner.until_landed(&answered).await;

    assert_eq!(*answered.borrow(), Some(Ok(Ok(None))));
    assert!(window.find("input").is_none());
}

#[wasm_bindgen_test]
async fn an_input_with_no_change_after_the_window_regains_focus_answers_a_cancel() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let answered = land(dialogs.open(OpenRequest::files().filter(input_only())));

    // An engine without `cancel`: the chooser closes and the window has the
    // focus again, and nothing more comes.
    dispatch(&the_view(), "focus", false);
    owner.until_landed(&answered).await;
    assert_eq!(*answered.borrow(), Some(Ok(Ok(None))));
    assert!(window.find("input").is_none());

    // A `change` that comes within the bound after the focus still answers.
    let picked = land(dialogs.open(OpenRequest::files().filter(input_only())));
    let input = the_input(&window);
    dispatch(&the_view(), "focus", false);
    pick(&input, &[("a.my-notes", "late")]);
    owner.until_landed(&picked).await;
    assert!(
        matches!(&*picked.borrow(), Some(Ok(Ok(Some(picks)))) if picks.len() == 1),
        "the pick: {:?}",
        picked.borrow()
    );
}

#[wasm_bindgen_test]
async fn an_input_whose_page_is_hidden_for_good_answers_a_cancel() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered =
        land(dialogs(&owner, &window, true).open(OpenRequest::files().filter(input_only())));

    dispatch(&the_view(), "pagehide", false);
    owner.until_landed(&answered).await;

    assert_eq!(*answered.borrow(), Some(Ok(Ok(None))));
}

#[wasm_bindgen_test]
async fn picks_past_the_requests_bound_are_refused_before_they_are_read() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let answered = land(
        dialogs.open(
            OpenRequest::files()
                .allow_multiple()
                .max_bytes(8)
                .filter(input_only()),
        ),
    );

    pick(
        &the_input(&window),
        &[("a.my-notes", "12345"), ("b.my-notes", "6789")],
    );
    owner.until_landed(&answered).await;

    assert_eq!(
        *answered.borrow(),
        Some(Ok(Err(DialogError::TooLarge {
            actual: 9,
            limit: 8
        })))
    );
}

#[wasm_bindgen_test]
async fn a_dropped_dialog_keeps_the_page_busy_until_its_input_answers() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let shown = dialogs.open(OpenRequest::files().filter(input_only()));
    let input = the_input(&window);
    drop(shown);

    // The browser's chooser is still up: the page shows no second one.
    let refused = land(dialogs.open(OpenRequest::files().filter(input_only())));
    owner.until_landed(&refused).await;
    assert_eq!(*refused.borrow(), Some(Ok(Err(DialogError::Busy))));

    // The pick that ends it answers nobody, and frees the page.
    pick(&input, &[("late.my-notes", "late")]);
    assert!(window.find("input").is_none(), "the input left the page");
    let next = land(dialogs.open(OpenRequest::files().filter(input_only())));
    dispatch(&the_input(&window), "cancel", false);
    owner.until_landed(&next).await;
    assert_eq!(*next.borrow(), Some(Ok(Ok(None))));
}

#[wasm_bindgen_test]
async fn a_page_shows_one_picker_whichever_window_asks() {
    let owner = TestOwner::new();
    let first = CanvasWindow::new(&owner);
    let second = CanvasWindow::new(&owner);
    let shown = land(dialogs(&owner, &first, true).open(OpenRequest::files().filter(input_only())));
    let refused =
        land(dialogs(&owner, &second, true).open(OpenRequest::files().filter(input_only())));
    owner.until_landed(&refused).await;
    assert_eq!(*refused.borrow(), Some(Ok(Err(DialogError::Busy))));
    assert!(second.find("input").is_none());
    dispatch(&the_input(&first), "cancel", false);
    owner.until_landed(&shown).await;
}

#[wasm_bindgen_test]
async fn a_picker_outside_a_gesture_is_refused_before_the_browser_is_asked() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered =
        land(dialogs(&owner, &window, false).open(OpenRequest::files().filter(input_only())));
    owner.until_landed(&answered).await;
    assert_eq!(
        *answered.borrow(),
        Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
            FileDialogCapability,
        >(
            UnsupportedReason::Unavailable
        )))))
    );
    assert!(window.find("input").is_none(), "no input was shown");
}

#[wasm_bindgen_test]
async fn a_folder_dialog_is_not_yet_on_the_web() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let answered = land(dialogs(&owner, &window, true).open(OpenRequest::folders()));
    owner.until_landed(&answered).await;
    assert_eq!(
        *answered.borrow(),
        Some(Ok(Err(DialogError::Unsupported(Unsupported::of::<
            FileDialogCapability,
        >(
            UnsupportedReason::NotYetOnThisHost
        )))))
    );
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
