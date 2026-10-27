//! What the browser tells a page about its own life, against the same fake
//! page as the display tests: Page Lifecycle `freeze` and `resume`, `pagehide`,
//! `fullscreenchange`, and a canvas losing and regaining its WebGL context.
//! Runs on Node in the wasm lane; nothing here touches a real browser.

use flui_testing::window_commands::AdmitAndApply as _;
use std::sync::{Arc, Mutex};

use flui_native::{HostWindow, WindowOpen};
use flui_platform::WindowOptions;
use flui_platform::{
    SessionEndAnswer, SessionEndPhase, WindowCapabilities, WindowExecutionState, WindowMode,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

use super::display::{Probe, Running, install_probe, settle};

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
}

fn open(running: &Running) -> Arc<dyn HostWindow> {
    let owner = running.owner.borrow();
    match owner
        .as_ref()
        .expect("owner")
        .open_window(WindowOptions::default())
        .expect("window")
    {
        WindowOpen::Ready(window) => window,
        WindowOpen::Pending(_) => panic!("a Web window opens at once"),
    }
}

/// Start the fake page and the platform on it, with the window opened.
async fn page() -> (Probe, Running, Arc<dyn HostWindow>) {
    install_probe("none");
    let probe = Probe;
    watch_page();
    let running = Running::start();
    settle().await;
    let window = open(&running);
    (probe, running, window)
}

fn recorder<T: Send + 'static>() -> (Arc<Mutex<Vec<T>>>, Arc<Mutex<Vec<T>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    (Arc::clone(&seen), seen)
}

/// Each execution state the window's snapshots carry after this call, once
/// per change.
fn record_executions(window: &Arc<dyn HostWindow>) -> Arc<Mutex<Vec<WindowExecutionState>>> {
    let (seen, log) = recorder();
    let observed = Arc::downgrade(window);
    let mut last = window.state().execution;
    window.on_state_changed(Box::new(move || {
        let Some(window) = observed.upgrade() else {
            return;
        };
        let execution = window.state().execution;
        if execution != last {
            last = execution;
            log.lock().unwrap().push(execution);
        }
    }));
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
        *seen.lock().unwrap(),
        [
            WindowExecutionState::Suspended,
            WindowExecutionState::Running
        ],
        "each change is reported once"
    );
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_pagehide_reports_the_session_ending() {
    let (_probe, running, _window) = page().await;
    let (seen, log) = recorder();
    running
        .owner
        .borrow()
        .as_ref()
        .expect("owner")
        .on_session_end(Box::new(move |phase| {
            log.lock().unwrap().push(phase);
            SessionEndAnswer::Proceed
        }));

    fire_window("pagehide", false);
    assert_eq!(*seen.lock().unwrap(), [SessionEndPhase::Ending]);
    // A page the browser cached comes back: the ending is undone. A first
    // load raises `pageshow` too, and says nothing.
    fire_window("pageshow", false);
    assert_eq!(seen.lock().unwrap().len(), 1, "a first load ends nothing");
    fire_window("pageshow", true);
    assert_eq!(
        *seen.lock().unwrap(),
        [SessionEndPhase::Ending, SessionEndPhase::Cancelled]
    );
    // And it can be hidden again.
    fire_window("pagehide", false);
    assert_eq!(seen.lock().unwrap().len(), 3);

    running.quit();
    settle().await;
    assert_eq!(
        window_listeners("pagehide"),
        0,
        "quit removes the listeners"
    );
    assert_eq!(window_listeners("pageshow"), 0);
}

#[wasm_bindgen_test]
async fn web_pagehide_is_not_replayed_to_a_later_hook() {
    let (_probe, running, _window) = page().await;
    assert_eq!(window_listeners("pagehide"), 1);
    fire_window("pagehide", false);
    let (seen, log) = recorder();
    running
        .owner
        .borrow()
        .as_ref()
        .expect("owner")
        .on_session_end(Box::new(move |phase| {
            log.lock().unwrap().push(phase);
            SessionEndAnswer::Proceed
        }));
    assert!(seen.lock().unwrap().is_empty(), "a notice is one-off");
    running.quit();
    settle().await;
}

/// Runs its action when dropped: a capture whose destructor calls the
/// platform.
struct OnDrop(Option<Box<dyn FnOnce() + Send>>);

impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(action) = self.0.take() {
            action();
        }
    }
}

/// A session-end hook installs its successor and asks the page to quit from
/// inside its call; its captures, which call the platform when dropped, are
/// dropped after the call, and the successor hears the next phase.
#[wasm_bindgen_test]
async fn web_session_end_hook_reenters_the_platform() {
    let (_probe, running, _window) = page().await;
    let (shared, proxy) = {
        let owner = running.owner.borrow();
        let owner = owner.as_ref().expect("owner");
        (owner.shared(), owner.proxy())
    };
    let reached = Arc::new(Mutex::new(false));
    let (dropping, observed) = (shared.clone(), Arc::clone(&reached));
    let calls_back = OnDrop(Some(Box::new(move || {
        dropping.on_quit(Box::new(|| {}));
        *observed.lock().unwrap() = true;
    })));
    let (seen, log) = recorder();
    let installing = shared.clone();
    shared.on_session_end(Box::new(move |_| {
        let _ = &calls_back;
        let log = Arc::clone(&log);
        installing.on_session_end(Box::new(move |phase| {
            log.lock().unwrap().push(phase);
            SessionEndAnswer::Proceed
        }));
        proxy.request_quit().expect("quit");
        SessionEndAnswer::Proceed
    }));

    fire_window("pagehide", false);
    assert!(
        *reached.lock().unwrap(),
        "the replaced hook's destructor reached the platform"
    );
    fire_window("pageshow", true);
    assert_eq!(*seen.lock().unwrap(), [SessionEndPhase::Cancelled]);
    settle().await;
}

#[wasm_bindgen_test]
async fn web_fullscreenchange_republishes_the_window_state() {
    let (_probe, running, window) = page().await;
    let (seen, log) = recorder();
    window.on_state_changed(Box::new(move || log.lock().unwrap().push(())));
    let before = window.state();
    assert_eq!(before.mode, WindowMode::Normal);
    // Another element going fullscreen is not this window going fullscreen.
    set_foreign_fullscreen();
    fire_document("fullscreenchange");
    assert_eq!(window.state().mode, WindowMode::Normal);
    assert!(seen.lock().unwrap().is_empty());

    set_fullscreen(true);
    fire_document("fullscreenchange");
    let entered = window.state();
    assert_eq!(entered.mode, WindowMode::Fullscreen);
    assert!(entered.revision > before.revision);
    assert_eq!(seen.lock().unwrap().len(), 1);

    fire_document("fullscreenchange");
    assert_eq!(
        window.state().revision,
        entered.revision,
        "no change, no revision"
    );
    assert_eq!(seen.lock().unwrap().len(), 1);

    set_fullscreen(false);
    fire_document("fullscreenchange");
    assert_eq!(window.state().mode, WindowMode::Normal);
    assert_eq!(seen.lock().unwrap().len(), 2);

    // A container of the canvas going fullscreen takes the canvas with it.
    set_container_fullscreen();
    fire_document("fullscreenchange");
    assert_eq!(window.state().mode, WindowMode::Fullscreen);
    assert_eq!(seen.lock().unwrap().len(), 3);
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_context_loss_is_prevented_and_reported_as_surface_status() {
    let (_probe, running, window) = page().await;
    let (seen, log) = recorder();
    window.on_surface_status_change(Box::new(move |available| {
        log.lock().unwrap().push(available)
    }));

    assert!(
        fire_on_canvas("webglcontextlost"),
        "without preventDefault the browser never restores the context"
    );
    assert_eq!(*seen.lock().unwrap(), [false]);
    fire_on_canvas("webglcontextrestored");
    assert_eq!(*seen.lock().unwrap(), [false, true]);
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
    window.on_surface_status_change(Box::new(move |available| {
        surface_log.lock().unwrap().push(available);
    }));

    // The window hears both while it is open.
    fire_document("freeze");
    assert!(fire_on_canvas("webglcontextlost"));
    assert_eq!(executions.lock().unwrap().len(), 1);
    assert_eq!(surfaces.lock().unwrap().len(), 1);

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
    assert_eq!(executions.lock().unwrap().len(), 1, "nothing after close");
    assert_eq!(surfaces.lock().unwrap().len(), 1, "nothing after close");
    running.quit();
    settle().await;
}

#[wasm_bindgen_test]
async fn web_window_reports_what_a_page_can_do() {
    let (_probe, running, window) = page().await;
    let mut expected = WindowCapabilities::NONE;
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
