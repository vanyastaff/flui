//! The web providers in a browser (headless Chrome in CI): each provider
//! over a canvas of the test page, reached through a window's raw handle as
//! a real window's is, with the page's word on user activation stated by
//! the test (no test can make a gesture), the browser's events dispatched at
//! the elements the provider shows, and `window.open` and the Badging API
//! replaced for the test that calls them. No test opens a picker or a
//! window: a picker is refused by the browser without a real gesture, which
//! the tests of that path read as its answer.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::{
    Abandoned, Completion, CompletionSource, HostBridge, OwnerJob, OwnerPoster, OwnerScope,
    PendingCompletions, ProviderContext, RuntimeLocals, WindowCloser, WindowRef,
};
use flui_platform::drag::PlatformDragSource;
use flui_platform::haptics::PlatformHaptics;
use flui_platform::launch::{ExternalUri, LaunchError, Launcher, LauncherCapability, Scheme};
use flui_platform::{
    AdmittedCommand, ApplyError, BrowserException, Cursor, CursorError, GestureGeometry,
    PlatformSystemChrome, PlatformTextInput, PlatformWindow, PreferenceQueryError, RedrawRequester,
    SurfaceTarget, SwapChainStatistics, SystemFailure, Unsupported, UnsupportedReason, WindowAt,
    WindowExtent, WindowGeometry, WindowId, WindowIdMint, WindowState,
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
    Dialog, DialogError, FileDialogCapability, FileDialogs, FileFilter, OpenRequest, SaveRequest,
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
        Rc::new(
            Page::for_canvas(self.canvas.clone(), Activation::Stated(active))
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

/// A test page's file dialogs for `window`, the page's picker shared
/// through `owner`.
fn dialogs(owner: &TestOwner, window: &CanvasWindow, active: bool) -> FileDialogs {
    let ctx = owner.ctx(&window.window);
    FileDialogs::try_new(
        WebDialogs::new(window.page(active), ctx.runtime_local::<PagePicker>()),
        &ctx,
    )
    .expect("a window has dialogs")
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

    pick(&input, &[("a.txt", "first"), ("b.txt", "second")]);
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
            (Some("a.txt".to_owned()), Some(b"first".to_vec()), None),
            (Some("b.txt".to_owned()), Some(b"second".to_vec()), None),
        ]
    );
    assert!(
        window.find("input").is_none(),
        "the input leaves the page once it answered"
    );
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
        &[("a.txt", "12345"), ("b.txt", "6789")],
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
async fn a_dropped_dialog_takes_its_input_down_and_frees_the_page() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let shown = dialogs.open(OpenRequest::files().filter(input_only()));
    let input = the_input(&window);
    drop(shown);
    assert!(window.find("input").is_none(), "the input left the page");

    // A pick that arrives after the drop answers nobody, and the page shows
    // the next dialog.
    pick(&input, &[("late.txt", "late")]);
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
    let shown = dialogs(&owner, &first, true).open(OpenRequest::files().filter(input_only()));
    let refused =
        land(dialogs(&owner, &second, true).open(OpenRequest::files().filter(input_only())));
    owner.until_landed(&refused).await;
    assert_eq!(*refused.borrow(), Some(Ok(Err(DialogError::Busy))));
    assert!(second.find("input").is_none());
    drop(shown);
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

/// Whether the test's browser has the File System Access pickers.
fn has_pickers() -> bool {
    let view: JsValue = web_sys::window().expect("the test runs in a page").into();
    Reflect::get(&view, &JsValue::from_str("showOpenFilePicker"))
        .is_ok_and(|picker| picker.is_function())
}

#[wasm_bindgen_test]
async fn the_browsers_refusal_of_its_picker_is_the_dialogs_answer() {
    // Headless Chrome has the pickers and refuses them with no real gesture:
    // the path the page takes where the API exists.
    assert!(
        has_pickers(),
        "the browser the suite runs in has the pickers"
    );
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let dialogs = dialogs(&owner, &window, true);
    let opened = land(
        dialogs.open(OpenRequest::files().filter(FileFilter::new("Text").with_extension("txt"))),
    );
    owner.until_landed(&opened).await;
    assert_eq!(
        *opened.borrow(),
        Some(Ok(Err(DialogError::Failed(SystemFailure::Browser(
            BrowserException::Security
        )))))
    );
    assert!(window.find("input").is_none(), "no input stood in");

    let saved = land(dialogs.save(SaveRequest::new().contents(b"x".to_vec())));
    owner.until_landed(&saved).await;
    assert_eq!(
        *saved.borrow(),
        Some(Ok(Err(DialogError::Failed(SystemFailure::Browser(
            BrowserException::Security
        )))))
    );
}

#[wasm_bindgen_test]
async fn a_save_outside_a_gesture_downloads_its_contents_under_the_suggested_name() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    // The download's click is caught before the browser follows it.
    let clicks: Rc<RefCell<Vec<(String, String)>>> = Rc::default();
    let seen = Rc::clone(&clicks);
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
        other => panic!("a saved file: {other:?}"),
    };
    assert_eq!(location.filter(), None, "a download reports no filter");
    assert_eq!(location.file().path(), None);
    assert_eq!(location.file().contents(), Some(&b"written"[..]));
    assert_eq!(
        location
            .file()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Some("notes.txt".to_owned())
    );
    let clicked = clicks.borrow();
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
/// `replacement` until the guard drops, so a test reaches no real window or
/// badge; an own property it hid is put back.
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

#[wasm_bindgen_test]
async fn a_page_opens_apart_without_its_opener_and_a_mail_link_is_handed_off() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let seen = Rc::clone(&calls);
    let open = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> JsValue>::new(
        move |url: JsValue, target: JsValue, features: JsValue| {
            seen.borrow_mut().push(
                [url, target, features]
                    .iter()
                    .filter_map(JsValue::as_string)
                    .collect(),
            );
            JsValue::NULL
        },
    );
    let view: JsValue = web_sys::window().expect("the test runs in a page").into();
    let _replaced = Replaced::new(&view, "open", open.as_ref());
    let launcher = launcher(&owner, &window, true);

    let page =
        ExternalUri::try_new("https://example.org/a?b=c", &[Scheme::HTTPS]).expect("an https link");
    let mail =
        ExternalUri::try_new("mailto:someone@example.org", &[Scheme::MAILTO]).expect("a mail link");
    let opened = landed(launcher.open_uri(&page));
    let mailed = landed(launcher.open_uri(&mail));
    owner.until_landed(&mailed).await;

    assert_eq!(*opened.borrow(), Some(Ok(Ok(()))));
    assert_eq!(*mailed.borrow(), Some(Ok(Ok(()))));
    assert_eq!(
        *calls.borrow(),
        [
            vec![
                "https://example.org/a?b=c".to_owned(),
                "_blank".to_owned(),
                "noopener,noreferrer".to_owned()
            ],
            vec!["mailto:someone@example.org".to_owned(), "_self".to_owned()],
        ]
    );
}

#[wasm_bindgen_test]
async fn a_launch_outside_a_gesture_and_a_path_are_refused_on_the_web() {
    let owner = TestOwner::new();
    let window = CanvasWindow::new(&owner);
    let page = ExternalUri::try_new("https://example.org/", &[Scheme::HTTPS]).expect("a link");
    let outside = landed(launcher(&owner, &window, false).open_uri(&page));
    let path = landed(launcher(&owner, &window, true).reveal_path(Path::new("/home/me/a.txt")));
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
