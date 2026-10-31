//! What the browser tells a page about its own life, against the same fake
//! page as the display tests: Page Lifecycle `freeze` and `resume`, `pagehide`,
//! `fullscreenchange`, a canvas losing and regaining its WebGL context, and
//! the soft keyboard over it.
//! Runs on Node in the wasm lane; nothing here touches a real browser.

use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::RecordingWindowHandler;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use flui_native::{HostWindow, WindowOpen};
use flui_platform::{SessionEndPhase, WindowCapabilities, WindowExecutionState, WindowMode};
use flui_platform::{SurfaceChange, WindowOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::display::{Probe, Running, install_probe, settle};
use crate::app::AppCall;

#[wasm_bindgen(inline_js = r#"
let held = [];
let canvases = [];

// Record the window listeners the platform adds, and the canvases it creates,
// on top of the display probe's fake page. Installed after that probe and
// before the platform runs.
export function watchPage() {
    const add = globalThis.addEventListener;
    const remove = globalThis.removeEventListener;
    held = [];
    canvases = [];
    globalThis.addEventListener = (name, callback, options) => {
        held.push({ name, callback });
        add(name, callback, options);
    };
    globalThis.removeEventListener = (name, callback, options) => {
        held = held.filter(entry => entry.name !== name || entry.callback !== callback);
        remove(name, callback, options);
    };
    const document = globalThis.document;
    const create = document.createElement;
    document.createElement = (...args) => {
        const element = create.apply(document, args);
        if (args[0] === 'canvas') canvases.push(element);
        return element;
    };
}
export function fireDocument(name) { globalThis.document.dispatchEvent(new Event(name)); }
export function fireWindow(name, persisted) {
    const event = Object.assign(new Event(name), { persisted });
    for (const entry of held.slice()) if (entry.name === name) entry.callback(event);
}
export function windowListeners(name) { return held.filter(entry => entry.name === name).length; }
export function documentListeners() { return globalThis.document.listenerCount(); }
// Dispatch a cancelable event on the page's canvas; whether a listener
// cancelled it, as the browser reads that to decide on restoring a context.
export function fireOnCanvas(name) {
    const event = new Event(name, { cancelable: true });
    canvases[canvases.length - 1].dispatchEvent(event);
    return event.defaultPrevented;
}
// Some other element of the page (a video, say) is fullscreen.
export function setForeignFullscreen() {
    globalThis.document.fullscreenElement = Object.assign(new EventTarget(), { contains: () => false });
}
// The page's own container of the canvas is fullscreen, as in an app that wraps the canvas.
export function setContainerFullscreen() {
    const canvas = canvases[canvases.length - 1];
    globalThis.document.fullscreenElement = Object.assign(new EventTarget(), { contains: node => node === canvas });
}
export function setFullscreen(on) {
    globalThis.document.fullscreenElement = on ? canvases[canvases.length - 1] : null;
}
// A soft keyboard, hidden: a visual viewport over a layout viewport the size
// of the page's canvas (300 by 150), and `navigator.virtualKeyboard` when
// `virtual` is set.
let keyboard = null;
let visual = null;
export function installKeyboard(virtual) {
    keyboard = Object.assign(new EventTarget(), {
        overlaysContent: false,
        boundingRect: { x: 0, y: 0, width: 0, height: 0 },
    });
    if (virtual) globalThis.navigator.virtualKeyboard = keyboard;
    visual = Object.assign(new EventTarget(), { offsetTop: 0, height: 150, scale: 1 });
    globalThis.visualViewport = visual;
    globalThis.document.documentElement = { clientWidth: 300, clientHeight: 150 };
}
export function removeKeyboard() {
    delete globalThis.navigator.virtualKeyboard;
    delete globalThis.visualViewport;
    delete globalThis.document.documentElement;
}
export function keyboardOverlaysContent() { return keyboard.overlaysContent; }
// The visual viewport shrunk to `height` at `scale`, as a keyboard or a pinch
// leaves it.
export function setVisualViewport(height, scale) {
    Object.assign(visual, { height, scale });
    visual.dispatchEvent(new Event('resize'));
}
// The virtual keyboard's rectangle from `top` to the layout bottom.
export function moveVirtualKeyboard(top) {
    keyboard.boundingRect = { x: 0, y: top, width: 300, height: 150 - top };
    keyboard.dispatchEvent(new Event('geometrychange'));
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = watchPage)]
    fn watch_page();
    #[wasm_bindgen(js_name = fireDocument)]
    fn fire_document(name: &str);
    #[wasm_bindgen(js_name = fireWindow)]
    fn fire_window(name: &str, persisted: bool);
    #[wasm_bindgen(js_name = windowListeners)]
    fn window_listeners(name: &str) -> usize;
    #[wasm_bindgen(js_name = documentListeners)]
    fn document_listeners() -> usize;
    #[wasm_bindgen(js_name = fireOnCanvas)]
    fn fire_on_canvas(name: &str) -> bool;
    #[wasm_bindgen(js_name = setForeignFullscreen)]
    fn set_foreign_fullscreen();
    #[wasm_bindgen(js_name = setContainerFullscreen)]
    fn set_container_fullscreen();
    #[wasm_bindgen(js_name = setFullscreen)]
    fn set_fullscreen(on: bool);
    #[wasm_bindgen(js_name = installKeyboard)]
    fn install_keyboard(virtual_keyboard: bool);
    #[wasm_bindgen(js_name = removeKeyboard)]
    fn remove_keyboard();
    #[wasm_bindgen(js_name = setVisualViewport)]
    fn set_visual_viewport(height: f64, scale: f64);
    #[wasm_bindgen(js_name = keyboardOverlaysContent)]
    fn keyboard_overlays_content() -> bool;
    #[wasm_bindgen(js_name = moveVirtualKeyboard)]
    fn move_virtual_keyboard(top: f64);
}

/// The page's window and the handler it was opened with.
struct PageWindow {
    window: Rc<dyn HostWindow>,
    handler: Rc<RecordingWindowHandler>,
}

impl std::ops::Deref for PageWindow {
    type Target = Rc<dyn HostWindow>;

    fn deref(&self) -> &Self::Target {
        &self.window
    }
}

fn open(running: &Running) -> PageWindow {
    let owner = running.owner.borrow();
    let handler = RecordingWindowHandler::new();
    match owner
        .as_ref()
        .expect("owner")
        .open_window(WindowOptions::default(), handler.clone())
        .expect("window")
    {
        WindowOpen::Ready(window) => PageWindow { window, handler },
        WindowOpen::Pending(_) => panic!("a Web window opens at once"),
    }
}

/// Start the fake page and the platform on it, with the window opened.
async fn page() -> (Probe, Running, PageWindow) {
    install_probe("none");
    let probe = Probe;
    watch_page();
    let running = Running::start();
    settle().await;
    let window = open(&running);
    (probe, running, window)
}

/// A log a test reads and a callback writes.
type Log<T> = Arc<Mutex<Vec<T>>>;

fn recorder<T: Send + 'static>() -> (Log<T>, Log<T>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    (Arc::clone(&seen), seen)
}

/// Each execution state the window's snapshots carry after this call, once
/// per change.
fn record_executions(window: &PageWindow) -> Arc<Mutex<Vec<WindowExecutionState>>> {
    let (seen, log) = recorder();
    let observed = std::rc::Rc::downgrade(&window.window);
    let last = Cell::new(window.state().execution);
    window.handler.react_to_state_change(move || {
        let Some(window) = observed.upgrade() else {
            return;
        };
        let execution = window.state().execution;
        if execution != last.replace(execution) {
            log.lock().expect("recorded values").push(execution);
        }
    });
    seen
}

#[wasm_bindgen_test]
async fn web_freeze_and_resume_set_the_execution_state() {
    let (_probe, running, window) = page().await;
    let seen = record_executions(&window);
    assert_eq!(window.state().execution, WindowExecutionState::Running);

    fire_document("freeze");
    assert_eq!(window.state().execution, WindowExecutionState::Suspended);
    fire_document("freeze");
    fire_document("resume");
    assert_eq!(window.state().execution, WindowExecutionState::Running);
    fire_document("resume");

    assert_eq!(
        *seen.lock().expect("recorded values"),
        [
            WindowExecutionState::Suspended,
            WindowExecutionState::Running
        ],
        "each change is reported once"
    );
    running.quit();
    settle().await;
}

/// The session-end phases the page's handler heard, in order.
fn phases(running: &Running) -> Vec<SessionEndPhase> {
    running
        .log
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            AppCall::SessionEnd(phase) => Some(phase),
            _ => None,
        })
        .collect()
}

#[wasm_bindgen_test]
async fn web_pagehide_reports_the_session_ending() {
    let (_probe, running, _window) = page().await;

    fire_window("pagehide", false);
    assert_eq!(phases(&running), [SessionEndPhase::Ending]);
    // A page the browser cached comes back: the ending is undone. A first
    // load raises `pageshow` too, and says nothing.
    fire_window("pageshow", false);
    assert_eq!(phases(&running).len(), 1, "a first load ends nothing");
    fire_window("pageshow", true);
    assert_eq!(
        phases(&running),
        [SessionEndPhase::Ending, SessionEndPhase::Cancelled]
    );
    // And it can be hidden again.
    fire_window("pagehide", false);
    assert_eq!(phases(&running).len(), 3);

    running.quit();
    settle().await;
    assert_eq!(
        window_listeners("pagehide"),
        0,
        "quit removes the listeners"
    );
    assert_eq!(window_listeners("pageshow"), 0);
}

/// The handler asks the page to quit from inside a session end: the quit
/// waits for the owner's turn, so the phase that follows is still heard, and
/// the handler hears the quit once.
#[wasm_bindgen_test]
async fn web_session_end_handler_reenters_the_platform() {
    let (_probe, running, _window) = page().await;
    let proxy = running.owner.borrow().as_ref().expect("owner").proxy();
    let asked = Cell::new(false);
    running.log.react(move |call, _| {
        if matches!(call, AppCall::SessionEnd(_)) && !asked.replace(true) {
            proxy.request_quit().expect("quit");
        }
    });

    fire_window("pagehide", false);
    fire_window("pageshow", true);
    assert_eq!(
        phases(&running),
        [SessionEndPhase::Ending, SessionEndPhase::Cancelled]
    );
    settle().await;
    assert_eq!(running.log.count(AppCall::Quit), 1, "the quit, once");
}

#[wasm_bindgen_test]
async fn web_fullscreenchange_republishes_the_window_state() {
    let (_probe, running, window) = page().await;
    let (seen, log) = recorder();
    window.handler.react_to_state_change(move || {
        log.lock().expect("recorded values").push(());
    });
    let before = window.state();
    assert_eq!(before.mode, WindowMode::Normal);
    // Another element going fullscreen is not this window going fullscreen.
    set_foreign_fullscreen();
    fire_document("fullscreenchange");
    assert_eq!(window.state().mode, WindowMode::Normal);
    assert!(seen.lock().expect("recorded values").is_empty());

    set_fullscreen(true);
    fire_document("fullscreenchange");
    let entered = window.state();
    assert_eq!(entered.mode, WindowMode::Fullscreen);
    assert!(entered.revision > before.revision);
    assert_eq!(seen.lock().expect("recorded values").len(), 1);

    fire_document("fullscreenchange");
    assert_eq!(
        window.state().revision,
        entered.revision,
        "no change, no revision"
    );
    assert_eq!(seen.lock().expect("recorded values").len(), 1);

    set_fullscreen(false);
    fire_document("fullscreenchange");
    assert_eq!(window.state().mode, WindowMode::Normal);
    assert_eq!(seen.lock().expect("recorded values").len(), 2);

    // A container of the canvas going fullscreen takes the canvas with it.
    set_container_fullscreen();
    fire_document("fullscreenchange");
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    assert_eq!(seen.lock().expect("recorded values").len(), 3);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_context_loss_is_prevented_and_reported_as_surface_status() {
    let (_probe, running, window) = page().await;
    let (seen, log) = recorder();
    window.handler.react_to_surface_change(move |change| {
        log.lock().expect("recorded values").push(change);
    });

    assert!(
        fire_on_canvas("webglcontextlost"),
        "without preventDefault the browser never restores the context"
    );
    assert_eq!(
        *seen.lock().expect("recorded values"),
        [SurfaceChange::Release]
    );
    fire_on_canvas("webglcontextrestored");
    assert_eq!(
        *seen.lock().expect("recorded values"),
        [SurfaceChange::Release, SurfaceChange::Rebuild]
    );
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_page_events_leave_with_the_window() {
    install_probe("none");
    let _probe = Probe;
    watch_page();
    let running = Running::start();
    settle().await;
    let baseline = document_listeners();
    let window = open(&running);
    assert!(
        document_listeners() > baseline,
        "the window listens on the page"
    );

    let executions = record_executions(&window);
    let (surfaces, surface_log) = recorder();
    window.handler.react_to_surface_change(move |change| {
        surface_log.lock().expect("recorded values").push(change);
    });

    // The window hears both while it is open.
    fire_document("freeze");
    assert!(fire_on_canvas("webglcontextlost"));
    assert_eq!(executions.lock().expect("recorded values").len(), 1);
    assert_eq!(surfaces.lock().expect("recorded values").len(), 1);

    window
        .admit_and_apply(flui_platform::WindowCommand::Close)
        .expect("a page retires its canvas");
    assert_eq!(
        document_listeners(),
        baseline,
        "the window's freeze, resume and fullscreenchange listeners are gone"
    );
    fire_document("freeze");
    assert!(
        !fire_on_canvas("webglcontextlost"),
        "a closed window no longer takes the context loss"
    );
    fire_document("resume");
    assert_eq!(
        executions.lock().expect("recorded values").len(),
        1,
        "nothing after close"
    );
    assert_eq!(
        surfaces.lock().expect("recorded values").len(),
        1,
        "nothing after close"
    );
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_window_reports_what_a_page_can_do() {
    let (_probe, running, window) = page().await;
    let mut expected = WindowCapabilities::NONE;
    // An estimate from the visual viewport: the page has no
    // `navigator.virtualKeyboard`.
    expected.reports_keyboard_inset = flui_platform::Availability::Emulated;
    expected.sets_title = flui_platform::Availability::Native;
    expected.enters_fullscreen = flui_platform::Availability::Native;
    expected.closes = flui_platform::Availability::Emulated;
    assert_eq!(window.state().capabilities, expected);
    assert_eq!(
        running
            .owner
            .borrow()
            .as_ref()
            .expect("owner")
            .window_capabilities(),
        expected,
        "known before a window opens"
    );
    running.quit();
    settle().await;
}

/// The fake page with a soft keyboard, `navigator.virtualKeyboard` among it
/// when `virtual_keyboard` is set, and the window opened on it.
async fn keyboard_page(virtual_keyboard: bool) -> (Probe, Running, PageWindow) {
    install_probe("none");
    let probe = Probe;
    watch_page();
    install_keyboard(virtual_keyboard);
    let running = Running::start();
    settle().await;
    let window = open(&running);
    (probe, running, window)
}

#[wasm_bindgen_test]
async fn web_window_reads_the_keyboard_from_the_visual_viewport() {
    let (_probe, running, window) = keyboard_page(false).await;
    let keyboard = || window.state().insets.keyboard();
    assert_eq!(keyboard(), flui_platform::KeyboardInset::HIDDEN);
    // A page zoomed in by a pinch has a smaller visual viewport and no
    // keyboard.
    set_visual_viewport(70.0, 2.0);
    assert_eq!(keyboard(), flui_platform::KeyboardInset::HIDDEN);
    // A keyboard that shrinks the visual viewport takes the layout viewport
    // below it.
    set_visual_viewport(70.0, 1.0);
    assert_eq!(keyboard().target().bottom, 80.0);
    assert_eq!(keyboard().motion(), None, "a browser names no motion");
    set_visual_viewport(150.0, 1.0);
    assert_eq!(keyboard(), flui_platform::KeyboardInset::HIDDEN);
    remove_keyboard();
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_window_has_the_virtual_keyboard_overlay_its_page() {
    let (_probe, running, window) = keyboard_page(true).await;
    assert!(
        keyboard_overlays_content(),
        "the window asks for the keyboard's rectangle"
    );
    assert_eq!(
        window.state().capabilities.keyboard_inset(),
        Ok(flui_platform::Availability::Native)
    );
    let keyboard = || window.state().insets.keyboard();
    move_virtual_keyboard(100.0);
    assert_eq!(keyboard().target().bottom, 50.0);
    // The rectangle wins over the visual viewport, which an overlaying
    // keyboard leaves whole.
    set_visual_viewport(70.0, 1.0);
    assert_eq!(keyboard().target().bottom, 50.0);
    move_virtual_keyboard(150.0);
    assert_eq!(keyboard(), flui_platform::KeyboardInset::HIDDEN);
    remove_keyboard();
    running.quit();
    settle().await;
}
