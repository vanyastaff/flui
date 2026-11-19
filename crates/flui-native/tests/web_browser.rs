//! The web backend's producer in a real browser (headless Chrome):
//! DOM events dispatched at the real
//! window's canvas, observed through the public input callback and the
//! event's own `defaultPrevented` and propagation.
//!
//! The events are constructed (untrusted), so they exercise the backend's
//! listeners and translation, not the browser's own input routing or its
//! default actions.
#![cfg(target_arch = "wasm32")]

use flui_native::testing::app;

use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::RecordingWindowHandler;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::{DragSessionId, UiRuntimeId};
use flui_native::{HostWindow, Platform, WebPlatform};
use flui_platform::Availability;
use flui_platform::input::keyboard::{ImeComposition, KeyEvent};
use flui_platform::input::pointer::{PanZoomPhase, PointerEvent, PointerKind};
use flui_platform::text::text_store::{
    CommitGate, CompositionEnd, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
    TextStoreHost, TextStoreObserver, TextStoreStatus, utf16::Utf16Offset, utf16::Utf16Range,
};
use flui_platform::transfer::data_transfer::{
    DataTransferOffer, DropFeedback, Mime, RepresentationDescriptor, RepresentationIndex,
    TransferActions, TransferError, TransferFormat, TransferLimits, TransferPayload,
};
use flui_platform::transfer::drag::{
    DragError, DragSessionKey, OsDragEnd, OsDragRequest, OsDragStart, OsDragTerms, ProcessNonce,
};
use flui_platform::{DispatchEventResult, WindowOptions};
use flui_platform::{DragDropEvent, PlatformInput};
use flui_testing::text_store_kit::InMemoryTextStore;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[path = "web_browser/http.rs"]
mod http;

#[path = "web_browser/accessibility.rs"]
mod accessibility;

#[wasm_bindgen(inline_js = r#"
function canvas() { return document.getElementById('flui-canvas'); }
// A canvas the window makes is in a root of the window's own, which goes with
// it; a test that failed left both.
export function freshCanvas() { (canvas()?.closest('flui-view') ?? canvas())?.remove(); }
// The tags of the canvas's parent's children, and whether that parent is a
// root of the window's own; null with no canvas.
export function canvasSiblings() {
    const parent = canvas()?.parentElement;
    if (!parent) return null;
    const second = canvas().nextElementSibling;
    const third = second?.nextElementSibling;
    return JSON.stringify({
        root: parent.tagName === 'FLUI-VIEW',
        order: [canvas(), second, third].map(element => element?.tagName.toLowerCase() ?? null),
        // A root holds the three alone, the semantics host last.
        last: parent.tagName === 'FLUI-VIEW' ? parent.lastElementChild === third : null,
    });
}
// Whether the page holds a root of a window's own, or any of its hosts.
export function hostElementsInPage() {
    return document.querySelectorAll('flui-view, flui-text-input, flui-semantics').length;
}
// The page canvas's `style`, `tabindex`, `width` and `height` attributes.
export function canvasAttributes() {
    return JSON.stringify(['style', 'tabindex', 'width', 'height'].map(name => canvas().getAttribute(name)));
}
// A canvas the page put in the document, styled as a page would.
export function pageCanvas() {
    canvas()?.remove();
    const element = document.createElement('canvas');
    element.id = 'flui-canvas';
    element.style.width = '300px';
    element.style.height = '200px';
    document.body.append(element);
}
export function touchAction() { return getComputedStyle(canvas()).touchAction; }
export function canvasOrigin() {
    const rect = canvas().getBoundingClientRect();
    return [rect.left, rect.top];
}
// Dispatch `event` at the canvas; report [defaultPrevented, reachedAncestor, timeStamp].
function dispatch(event) {
    let reached = false;
    const ancestor = () => { reached = true; };
    document.body.addEventListener(event.type, ancestor);
    canvas().dispatchEvent(event);
    document.body.removeEventListener(event.type, ancestor);
    return [event.defaultPrevented, reached, event.timeStamp];
}
export function wheel(ctrl, mode, dx, dy, x, y) {
    return dispatch(new WheelEvent('wheel', {
        bubbles: true, cancelable: true, ctrlKey: ctrl, deltaMode: mode,
        deltaX: dx, deltaY: dy, clientX: x, clientY: y,
    }));
}
export function gesture(type, scale, rotation, x, y) {
    const event = new Event(type, { bubbles: true, cancelable: true });
    for (const [name, value] of Object.entries({ scale, rotation, clientX: x, clientY: y,
        shiftKey: false, ctrlKey: false, altKey: false, metaKey: false })) {
        Object.defineProperty(event, name, { value });
    }
    return dispatch(event);
}
export function tiltedPen(x, y, tiltX, tiltY) {
    const event = new PointerEvent('pointerdown', {
        bubbles: true, cancelable: true, pointerId: 7, pointerType: 'pen', isPrimary: true,
        button: 0, buttons: 1, pressure: 0.5, clientX: x, clientY: y, tiltX, tiltY,
    });
    // A browser of Pointer Events 2 reports the tilt planes alone.
    Object.defineProperty(event, 'altitudeAngle', { value: undefined });
    Object.defineProperty(event, 'azimuthAngle', { value: undefined });
    dispatch(event);
    canvas().dispatchEvent(new PointerEvent('pointerup', {
        bubbles: true, cancelable: true, pointerId: 7, pointerType: 'pen', isPrimary: true,
        button: 0, buttons: 0, clientX: x, clientY: y,
    }));
}
// A pointer event of `pointerType` at the canvas: [defaultPrevented, reachedAncestor, timeStamp].
export function pointerAt(type, pointerId, pointerType, button, buttons, x, y) {
    return dispatch(new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerId, pointerType, isPrimary: true,
        button, buttons, pressure: buttons ? 0.5 : 0, clientX: x, clientY: y,
    }));
}
// A Control-held primary mouse press or release at the canvas (macOS's
// secondary click).
export function controlClick(type, x, y) {
    canvas().dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerId: 1, pointerType: 'mouse', isPrimary: true,
        button: 0, buttons: type === 'pointerdown' ? 1 : 0, ctrlKey: true, clientX: x, clientY: y,
    }));
}
export function contextMenu(button, ctrlKey) {
    return dispatch(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button, ctrlKey }));
}
// The page losing the OS focus (Alt+Tab).
export function pageBlur() { window.dispatchEvent(new FocusEvent('blur')); }
// The page hidden (another tab), or shown again.
export function hidePage(hidden) {
    if (hidden) Object.defineProperty(document, 'hidden', { value: true, configurable: true });
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
}
export function sleep(millis) { return new Promise(resolve => setTimeout(resolve, millis)); }
let drag;
// A drag over the canvas: string representations [[type, data]], files
// [[name, type, content]].
export function beginDrag(strings, files, effectAllowed) {
    drag = new DataTransfer();
    for (const [type, data] of JSON.parse(strings)) drag.setData(type, data);
    for (const [name, type, content] of JSON.parse(files)) drag.items.add(new File([content], name, { type }));
    // A constructed DataTransfer ignores both effects outside a real drag;
    // they are supplied as plain properties (a trusted drag
    // checks the browser's own).
    Object.defineProperty(drag, 'effectAllowed', { value: effectAllowed, writable: true });
    Object.defineProperty(drag, 'dropEffect', { value: 'none', writable: true });
}
export function dragAt(type, x, y) {
    const event = new DragEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: drag });
    const [prevented, reached] = dispatch(event);
    return JSON.stringify({ prevented, reached, dropEffect: drag.dropEffect });
}
export function pressMouse(type, x, y) {
    canvas().dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerId: 1, pointerType: 'mouse', isPrimary: true,
        button: 0, buttons: type === 'pointerdown' ? 1 : 0, clientX: x, clientY: y,
    }));
}
// The browser's dragstart at the canvas: what the source stored.
export function dragStart() {
    drag = new DataTransfer();
    Object.defineProperty(drag, 'effectAllowed', { value: 'uninitialized', writable: true });
    Object.defineProperty(drag, 'dropEffect', { value: 'none', writable: true });
    const event = new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer: drag });
    canvas().dispatchEvent(event);
    const stored = {};
    for (const type of drag.types) stored[type] = drag.getData(type);
    return JSON.stringify({ prevented: event.defaultPrevented, stored, effectAllowed: drag.effectAllowed });
}
export function dragEnd(dropEffect) {
    drag.dropEffect = dropEffect;
    canvas().dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: drag }));
}
let hiddenEditContext;
// Open the next window as a browser without EditContext would.
export function hideEditContext() { hiddenEditContext = globalThis.EditContext; delete globalThis.EditContext; }
export function restoreEditContext() { if (hiddenEditContext) globalThis.EditContext = hiddenEditContext; hiddenEditContext = undefined; }
export function focusCanvas() { canvas().focus(); return document.activeElement === canvas(); }
export function editContextText() { return canvas().editContext ? canvas().editContext.text : null; }
export function editContextSelection() {
    const context = canvas().editContext;
    return context ? [context.selectionStart, context.selectionEnd] : [];
}
// The input method's update: the browser updates the context's own copy,
// then reports it (a constructed event alone does not change the copy).
export function textUpdate(start, end, text, selectionStart, selectionEnd) {
    const context = canvas().editContext;
    context.updateText(start, end, text);
    context.updateSelection(selectionStart, selectionEnd);
    context.dispatchEvent(new TextUpdateEvent('textupdate', {
        updateRangeStart: start, updateRangeEnd: end, text, selectionStart, selectionEnd,
    }));
}
export function editContextComposition(type) { canvas().editContext.dispatchEvent(new Event(type)); }
// A key at the canvas, or at whatever holds focus: [defaultPrevented].
export function keyAtFocus(key, keyCode) {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, key, code: key });
    if (keyCode !== undefined) Object.defineProperty(event, 'keyCode', { value: keyCode });
    document.activeElement.dispatchEvent(event);
    return event.defaultPrevented;
}
// The hidden textarea, in the text-input host that follows the canvas.
function editor() {
    const host = canvas().nextElementSibling;
    return host?.tagName === 'FLUI-TEXT-INPUT' ? host.querySelector('textarea') : null;
}
export function editorState() {
    const area = editor();
    return JSON.stringify(area ? {
        value: area.value, start: area.selectionStart, end: area.selectionEnd,
        focused: document.activeElement === area,
        fontSize: parseFloat(getComputedStyle(area).fontSize),
    } : null);
}
// A touch contact going down at the canvas at (x0, y0) and lifting at
// (x1, y1): a tap where the two are the same.
export function touchStroke(x0, y0, x1, y1) {
    const at = (type, x, y) => canvas().dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerId: 3, pointerType: 'touch', isPrimary: true,
        button: 0, buttons: type === 'pointerdown' ? 1 : 0, clientX: x, clientY: y,
    }));
    at('pointerdown', x0, y0);
    at('pointerup', x1, y1);
}
// A page canvas with an `inputmode` of the page's own and no `style`
// attribute at all.
export function bareCanvas() {
    canvas()?.remove();
    const element = document.createElement('canvas');
    element.id = 'flui-canvas';
    element.setAttribute('inputmode', 'decimal');
    document.body.append(element);
}
export function canvasAttribute(name) { return canvas().getAttribute(name); }
// Mark the textarea, to tell later whether it is the same element.
export function markEditor() { editor().dataset.mark = 'kept'; }
export function editorIsMarked() { return editor()?.dataset.mark === 'kept'; }
export function textareaCount() { return canvas().nextElementSibling.querySelectorAll('textarea').length; }
export function editorAriaHidden() { return editor().getAttribute('aria-hidden'); }
// The soft keyboard as the page reads it: 'up' is a `navigator.virtualKeyboard`
// that overlays the page and covers its lower part, as Chromium on Android
// reports a shown keyboard (its `show()` counted, which shows nothing for an
// element without `virtualkeyboardpolicy="manual"`); 'hidden' is the same API
// reporting an empty rectangle, as Chromium on Android does after Back;
// 'inferred' is the page as headless Chrome has it, no `virtualKeyboard` and a
// visual viewport over the whole layout viewport (an inset of zero, which
// Safari also reads with its keyboard up); 'unknown' is a page with neither.
let shows = 0;
// Stop the canvas's own `focus` listeners (the window's forwarding to the
// editor) from hearing a focus, by a capturing listener above it.
const blockForwarding = event => { if (event.target === canvas()) event.stopPropagation(); };
export function blockCanvasFocusForwarding(blocked) {
    if (blocked) {
        window.addEventListener('focus', blockForwarding, true);
    } else {
        window.removeEventListener('focus', blockForwarding, true);
    }
}
// The page's own `visualViewport`, an own property of the global object
// that a plain `delete` would remove for good: the fakes put it back.
const ownViewport = Object.getOwnPropertyDescriptor(window, 'visualViewport');
function restoreViewport() {
    if (ownViewport) {
        Object.defineProperty(window, 'visualViewport', ownViewport);
    } else {
        delete window.visualViewport;
    }
}
export function fakeKeyboard(state) {
    shows = 0;
    const rect = state === 'up' ? new DOMRect(0, innerHeight / 2, innerWidth, innerHeight / 2)
        : state === 'hidden' ? new DOMRect(0, 0, 0, 0) : undefined;
    const keyboard = rect ? Object.assign(new EventTarget(), {
        overlaysContent: true,
        boundingRect: rect,
        show() { shows += 1; },
    }) : undefined;
    Object.defineProperty(navigator, 'virtualKeyboard', { value: keyboard, configurable: true });
    if (state === 'unknown') {
        Object.defineProperty(window, 'visualViewport', { value: null, configurable: true });
    } else {
        restoreViewport();
    }
}
export function restoreKeyboard() {
    delete navigator.virtualKeyboard;
    restoreViewport();
}
// Type `text` into the focused textarea as a keyboard does, through the
// browser's editing, so it enters the textarea's undo history.
export function insertIntoEditor(text) {
    editor().focus();
    document.execCommand('insertText', false, text);
}
// A tap as Chrome on a phone delivers it: the pointer events, then the
// compatibility mouse events, whose `mousedown` focuses the canvas
// (`tabindex=0`), then `click`.
export function tapWithCompatibilityEvents(x, y) {
    const pointer = (type, buttons) => canvas().dispatchEvent(new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerId: 4, pointerType: 'touch', isPrimary: true,
        button: 0, buttons, clientX: x, clientY: y,
    }));
    const mouse = type => canvas().dispatchEvent(new MouseEvent(type, {
        bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y,
    }));
    pointer('pointerdown', 1);
    pointer('pointerup', 0);
    mouse('mousedown');
    canvas().focus({ preventScroll: true });
    mouse('mouseup');
    mouse('click');
}
// The browser's undo in the focused textarea; its value afterwards.
export function undoInEditor() {
    editor().focus();
    document.execCommand('undo');
    return editor().value;
}
export function keyboardShows() { return shows; }
// Count the focus and blur events the textarea gets from now on.
let editorFocuses = 0;
let editorBlurs = 0;
export function countEditorFocus() {
    editorFocuses = 0;
    editorBlurs = 0;
    const area = editor();
    if (!area.dataset.counted) {
        area.dataset.counted = 'yes';
        area.addEventListener('focus', () => { editorFocuses += 1; });
        area.addEventListener('blur', () => { editorBlurs += 1; });
    }
}
export function editorFocusCount() { return editorFocuses; }
export function editorBlurCount() { return editorBlurs; }
// Resolve after `count` animation frames.
export function frames(count) {
    return new Promise(resolve => {
        const step = left => (left === 0 ? resolve() : requestAnimationFrame(() => step(left - 1)));
        step(count);
    });
}
// What holds the page's focus: the canvas, the textarea, or the tag.
export function pageFocus() {
    const active = document.activeElement;
    if (active === canvas()) return 'canvas';
    if (active && active === editor()) return 'editor';
    return active ? active.tagName.toLowerCase() : 'nothing';
}
// Move the page's focus to the body, as a tap on a canvas a mobile browser
// does not focus leaves it.
export function blurAll() { document.activeElement?.blur(); }
// The textarea's hints as [inputmode, autocomplete, enterkeyhint], each null
// when absent.
export function editorHints() {
    const area = editor();
    return JSON.stringify(['inputmode', 'autocomplete', 'enterkeyhint'].map(name => area.getAttribute(name)));
}
// The textarea's [autocapitalize, autocorrect, spellcheck], each null when
// absent.
export function editorCorrection() {
    const area = editor();
    return JSON.stringify(['autocapitalize', 'autocorrect', 'spellcheck'].map(name => area.getAttribute(name)));
}
// The input method types into the textarea: its value becomes `value`.
export function typeIntoEditor(value, caret, composing) {
    const area = editor();
    area.value = value;
    area.setSelectionRange(caret, caret);
    area.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing: composing,
        inputType: composing ? 'insertCompositionText' : 'insertText' }));
}
export function editorComposition(type) { editor().dispatchEvent(new CompositionEvent(type, { bubbles: true })); }
// Hold the page's thread for `ms` at the start of the next animation frame, before any
// callback registered after this one.
export function holdNextFrame(ms) {
    requestAnimationFrame(() => { const end = performance.now() + ms; while (performance.now() < end) {} });
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = freshCanvas)]
    fn fresh_canvas();
    #[wasm_bindgen(js_name = holdNextFrame)]
    fn hold_next_frame(millis: f64);
    #[wasm_bindgen(js_name = pageCanvas)]
    fn page_canvas();
    #[wasm_bindgen(js_name = touchAction)]
    fn touch_action() -> String;
    #[wasm_bindgen(js_name = canvasOrigin)]
    fn canvas_origin() -> Vec<f64>;
    fn wheel(ctrl: bool, mode: u32, dx: f64, dy: f64, x: f64, y: f64) -> Vec<f64>;
    fn gesture(kind: &str, scale: f64, rotation: f64, x: f64, y: f64) -> Vec<f64>;
    #[wasm_bindgen(js_name = tiltedPen)]
    fn tilted_pen(x: f64, y: f64, tilt_x: f64, tilt_y: f64);
    fn sleep(millis: u32) -> js_sys::Promise;
    #[wasm_bindgen(js_name = pointerAt)]
    fn pointer_at(
        kind: &str,
        id: i32,
        pointer_type: &str,
        button: i16,
        buttons: u16,
        x: f64,
        y: f64,
    ) -> Vec<f64>;
    #[wasm_bindgen(js_name = contextMenu)]
    fn context_menu(button: i16, ctrl: bool) -> Vec<f64>;
    #[wasm_bindgen(js_name = controlClick)]
    fn control_click(kind: &str, x: f64, y: f64);
    #[wasm_bindgen(js_name = pageBlur)]
    fn page_blur();
    #[wasm_bindgen(js_name = hidePage)]
    fn hide_page(hidden: bool);
    #[wasm_bindgen(js_name = beginDrag)]
    fn begin_drag(strings: &str, files: &str, effect_allowed: &str);
    #[wasm_bindgen(js_name = dragAt)]
    fn drag_at(kind: &str, x: f64, y: f64) -> String;
    #[wasm_bindgen(js_name = pressMouse)]
    fn press_mouse(kind: &str, x: f64, y: f64);
    #[wasm_bindgen(js_name = dragStart)]
    fn drag_start() -> String;
    #[wasm_bindgen(js_name = dragEnd)]
    fn drag_end(drop_effect: &str);
    #[wasm_bindgen(js_name = hideEditContext)]
    fn hide_edit_context();
    #[wasm_bindgen(js_name = restoreEditContext)]
    fn restore_edit_context();
    #[wasm_bindgen(js_name = focusCanvas)]
    fn focus_canvas() -> bool;
    #[wasm_bindgen(js_name = editContextText)]
    fn edit_context_text() -> Option<String>;
    #[wasm_bindgen(js_name = editContextSelection)]
    fn edit_context_selection() -> Vec<u32>;
    #[wasm_bindgen(js_name = textUpdate)]
    fn text_update(start: u32, end: u32, text: &str, selection_start: u32, selection_end: u32);
    #[wasm_bindgen(js_name = editContextComposition)]
    fn edit_context_composition(kind: &str);
    #[wasm_bindgen(js_name = keyAtFocus)]
    fn key_at_focus(key: &str, key_code: Option<u32>) -> bool;
    #[wasm_bindgen(js_name = editorState)]
    fn editor_state() -> String;
    #[wasm_bindgen(js_name = typeIntoEditor)]
    fn type_into_editor(value: &str, caret: u32, composing: bool);
    #[wasm_bindgen(js_name = editorComposition)]
    fn editor_composition(kind: &str);
    #[wasm_bindgen(js_name = canvasSiblings)]
    fn canvas_siblings() -> Option<String>;
    #[wasm_bindgen(js_name = hostElementsInPage)]
    fn host_elements_in_page() -> u32;
    #[wasm_bindgen(js_name = canvasAttributes)]
    fn canvas_attributes() -> String;
    #[wasm_bindgen(js_name = touchStroke)]
    fn touch_stroke(x0: f64, y0: f64, x1: f64, y1: f64);
    #[wasm_bindgen(js_name = pageFocus)]
    fn page_focus() -> String;
    #[wasm_bindgen(js_name = bareCanvas)]
    fn bare_canvas();
    #[wasm_bindgen(js_name = canvasAttribute)]
    fn canvas_attribute(name: &str) -> Option<String>;
    #[wasm_bindgen(js_name = markEditor)]
    fn mark_editor();
    #[wasm_bindgen(js_name = editorIsMarked)]
    fn editor_is_marked() -> bool;
    #[wasm_bindgen(js_name = textareaCount)]
    fn textarea_count() -> u32;
    #[wasm_bindgen(js_name = editorAriaHidden)]
    fn editor_aria_hidden() -> Option<String>;
    #[wasm_bindgen(js_name = fakeKeyboard)]
    fn fake_keyboard(state: &str);
    #[wasm_bindgen(js_name = restoreKeyboard)]
    fn restore_keyboard();
    #[wasm_bindgen(js_name = blockCanvasFocusForwarding)]
    fn block_canvas_focus_forwarding(blocked: bool);
    #[wasm_bindgen(js_name = insertIntoEditor)]
    fn insert_into_editor(text: &str);
    #[wasm_bindgen(js_name = tapWithCompatibilityEvents)]
    fn tap_with_compatibility_events(x: f64, y: f64);
    #[wasm_bindgen(js_name = undoInEditor)]
    fn undo_in_editor() -> String;
    #[wasm_bindgen(js_name = keyboardShows)]
    fn keyboard_shows() -> u32;
    #[wasm_bindgen(js_name = countEditorFocus)]
    fn count_editor_focus();
    #[wasm_bindgen(js_name = editorFocusCount)]
    fn editor_focus_count() -> u32;
    #[wasm_bindgen(js_name = editorBlurCount)]
    fn editor_blur_count() -> u32;
    fn frames(count: u32) -> js_sys::Promise;
    #[wasm_bindgen(js_name = blurAll)]
    fn blur_all();
    #[wasm_bindgen(js_name = editorHints)]
    fn editor_hints() -> String;
    #[wasm_bindgen(js_name = editorCorrection)]
    fn editor_correction() -> String;
}

/// What one dispatched drag event did.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DragAnswer {
    prevented: bool,
    reached: bool,
    drop_effect: String,
}

fn drag_answer(kind: &str) -> DragAnswer {
    serde_json::from_str(&drag_at(kind, 40.0, 30.0)).expect("the page's answer")
}

/// What the canvas's source stored at the browser's `dragstart`.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    prevented: bool,
    stored: std::collections::BTreeMap<String, String>,
    effect_allowed: String,
}

fn started() -> Started {
    serde_json::from_str(&drag_start()).expect("the page's answer")
}

/// What one dispatched DOM event did.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Dispatched {
    prevented: bool,
    reached_ancestor: bool,
    time_ns: u64,
}

fn dispatched(values: &[f64]) -> Dispatched {
    Dispatched {
        prevented: values[0] != 0.0,
        reached_ancestor: values[1] != 0.0,
        time_ns: (values[2] * 1_000_000.0) as u64,
    }
}

async fn wait(millis: u32) {
    js_sys::futures::JsFuture::from(sleep(millis))
        .await
        .expect("a timeout resolves");
}

/// A window on the page's canvas whose input callback records every input
/// and answers with `answer`; closed when dropped, so the next test can own
/// the canvas.
struct CanvasWindow {
    window: Rc<dyn HostWindow>,
    handler: Rc<RecordingWindowHandler>,
    platform: WebPlatform,
    seen: Rc<RefCell<Vec<PlatformInput>>>,
    answer: Rc<Cell<DispatchEventResult>>,
    /// What a drop target under every hover would accept, as a runtime
    /// caches it while the hover is dispatched.
    accept: Rc<Cell<Option<TransferActions>>>,
    /// Run inside the dispatch of the next pointer press.
    on_press: PressHook,
}

/// Code a test runs inside the dispatch of the next pointer press.
type PressHook = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

impl CanvasWindow {
    fn open() -> Self {
        // A failed test aborts without closing its window: each test opens
        // the backend on a canvas of its own, which it creates.
        fresh_canvas();
        let platform = WebPlatform::try_new().expect("a browser platform");
        let handler = RecordingWindowHandler::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let answer = Rc::new(Cell::new(DispatchEventResult::resolved(false, true)));
        let accept = Rc::new(Cell::new(None));
        let on_press: PressHook = Rc::default();
        let (record, reply, accepting, pressed) = (
            Rc::clone(&seen),
            Rc::clone(&answer),
            Rc::clone(&accept),
            Rc::clone(&on_press),
        );
        let transfer = platform.data_transfer();
        handler.react_to_input(move |input| {
            let hovered = match &input {
                PlatformInput::DragDrop(DragDropEvent::Entered { offer, .. }) => Some(offer.id()),
                PlatformInput::DragDrop(DragDropEvent::Moved { id, .. }) => Some(*id),
                _ => None,
            };
            if let Some(id) = hovered {
                transfer.update_drop_feedback(
                    id,
                    DropFeedback {
                        accept: accepting.get(),
                    },
                );
            }
            if matches!(input, PlatformInput::Pointer(PointerEvent::Down(_))) {
                let run = pressed.borrow_mut().take();
                if let Some(run) = run {
                    run();
                }
            }
            record.borrow_mut().push(input);
            reply.get()
        });
        let window = platform
            .open_window(WindowOptions::default(), handler.clone())
            .expect("the page's canvas opens");
        Self {
            window,
            handler,
            platform,
            seen,
            answer,
            accept,
            on_press,
        }
    }

    /// The drag-and-drop events seen so far, taken.
    fn drag_events(&self) -> Vec<DragDropEvent> {
        self.seen
            .borrow_mut()
            .drain(..)
            .filter_map(|input| match input {
                PlatformInput::DragDrop(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    fn answer(&self, propagate: bool, prevent: bool) {
        self.answer
            .set(DispatchEventResult::resolved(propagate, prevent));
    }

    /// The pointer events seen so far, taken.
    fn pointer_events(&self) -> Vec<PointerEvent> {
        self.seen
            .borrow_mut()
            .drain(..)
            .filter_map(|input| match input {
                PlatformInput::Pointer(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    /// The trackpad availability the window reported.
    fn trackpad_gestures(&self) -> Option<Availability> {
        self.seen.borrow().iter().find_map(|input| match input {
            PlatformInput::Capabilities(capabilities) => Some(capabilities.trackpad_gestures()),
            _ => None,
        })
    }
}

impl Drop for CanvasWindow {
    fn drop(&mut self) {
        self.window.close_for_cleanup();
    }
}

/// A pan-zoom's phases as names, with the scale and rotation each update
/// carries.
fn pan_zoom(events: &[PointerEvent]) -> Vec<(String, f64, f64, u64)> {
    events
        .iter()
        .map(|event| match event {
            PointerEvent::PanZoom(event) => {
                assert!(
                    matches!(event.pointer().kind, PointerKind::Trackpad),
                    "a pinch is a trackpad's: {event:?}"
                );
                let (name, scale, rotation) = match event.phase {
                    PanZoomPhase::Start => ("start", 1.0, 0.0),
                    PanZoomPhase::Update(transform) => {
                        ("update", transform.scale(), transform.rotation())
                    }
                    PanZoomPhase::End => ("end", 0.0, 0.0),
                    PanZoomPhase::Cancelled => ("cancelled", 0.0, 0.0),
                    _ => ("other", 0.0, 0.0),
                };
                (name.to_owned(), scale, rotation, event.time.as_nanos())
            }
            other => (format!("{other:?}"), 0.0, 0.0, 0),
        })
        .collect()
}

fn names(phases: &[(String, f64, f64, u64)]) -> Vec<&str> {
    phases.iter().map(|(name, ..)| name.as_str()).collect()
}

/// A wheel scroll is answered with the dispatch result while the browser
/// waits on it: its default (page scrolling) is cancelled only when the
/// application prevents it, and it reaches the canvas's ancestors only when
/// the application lets it propagate.
/// A touch that moves on the canvas stays the application's, on a canvas the
/// backend created and on one the page styled: where `touch-action` lets
/// it, the browser takes a touch past its slop for its own panning and
/// cancels the pointer, and a swipe, drag or pinch dies a few pixels in.
#[wasm_bindgen_test]
fn the_browser_leaves_every_touch_on_the_canvas_to_the_application() {
    for (canvas, place) in [
        ("created by the backend", fresh_canvas as fn()),
        ("the page's own", page_canvas),
    ] {
        place();
        let platform = WebPlatform::try_new().expect("a browser platform");
        let window = platform
            .open_window(WindowOptions::default(), RecordingWindowHandler::new())
            .expect("the page's canvas opens");
        assert_eq!(touch_action(), "none", "the canvas {canvas}");
        window.close_now();
    }
}

/// A redraw is served by the page's next animation frame, never inside the
/// call: a frame that asks for the next one, as a running animation does,
/// gets it on a later animation frame rather than at once, at the same frame
/// time, in a loop that holds the page's only thread until the animation
/// ends, if it ever does.
#[wasm_bindgen_test]
async fn a_redraw_waits_for_the_next_animation_frame() {
    // An animation's frames, each asking for the next; bounded, so a backend
    // that runs them inline fails here rather than hanging the page.
    const ANIMATION: usize = 100;
    let canvas = HostedCanvas::open();
    let frames = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (counted, window) = (Arc::clone(&frames), std::rc::Rc::clone(&canvas.window));
    canvas.handler.react_to_frame(move |_| {
        if counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < ANIMATION {
            window.request_redraw();
        }
    });
    canvas.window.request_redraw();
    let inline = frames.load(std::sync::atomic::Ordering::SeqCst);
    wait(200).await;
    let later = frames.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(inline, 0, "frames run inside the request");
    assert!(
        (1..ANIMATION).contains(&later),
        "{later} frames in 200 ms of animation frames"
    );
}

#[wasm_bindgen_test]
fn a_wheel_scroll_is_answered_with_its_dispatch_result() {
    let window = CanvasWindow::open();
    for (propagate, prevent) in [(true, false), (true, true), (false, false), (false, true)] {
        window.answer(propagate, prevent);
        let event = dispatched(&wheel(false, 0, 0.0, 40.0, 30.0, 30.0));
        assert_eq!(
            (event.prevented, event.reached_ancestor),
            (prevent, propagate),
            "answer propagate={propagate} prevent={prevent}"
        );
        assert!(
            matches!(
                window.pointer_events().as_slice(),
                [PointerEvent::Scroll(_)]
            ),
            "the scroll reached the application"
        );
    }
}

/// A Control-held wheel event with a fractional pixel delta is a trackpad
/// pinch: the application sees a `PanZoom` from the trackpad whose scale
/// inverts Chromium's encoding, about the event's position, and its End
/// comes at the deadline after the last step, stamped on the event timeline
/// with that deadline, without any further input.
#[wasm_bindgen_test]
async fn a_ctrl_wheel_pinch_reaches_the_application_as_a_pan_zoom() {
    let window = CanvasWindow::open();
    assert_eq!(window.trackpad_gestures(), Some(Availability::Emulated));
    let first = dispatched(&wheel(true, 0, 0.0, -100.0 * 1.25_f64.ln(), 40.0, 30.0));
    let last = dispatched(&wheel(true, 0, 0.0, -100.0 * 1.2_f64.ln(), 41.0, 30.0));
    assert!(
        first.prevented && last.prevented,
        "the browser does not zoom a pinch the application takes"
    );
    let events = window.pointer_events();
    let [left, top] = canvas_origin()[..] else {
        panic!("an origin");
    };
    let PointerEvent::PanZoom(start) = &events[0] else {
        panic!("a pan-zoom: {events:?}");
    };
    let focal = start.position.get();
    assert!(
        (focal.x - (40.0 - left)).abs() < 1e-9 && (focal.y - (30.0 - top)).abs() < 1e-9,
        "about the event's position on the canvas: {focal:?}"
    );
    let steps = pan_zoom(&events);
    assert_eq!(names(&steps), ["start", "update", "update"]);
    assert!((steps[2].1 - 1.5).abs() < 1e-9, "1.25 × 1.2: {steps:?}");
    wait(400).await;
    let end = pan_zoom(&window.pointer_events());
    assert_eq!(
        names(&end),
        ["end"],
        "the End came from the deadline's timer"
    );
    assert_eq!(
        end[0].3,
        last.time_ns + 120_000_000,
        "stamped with the deadline: the last step plus the gap"
    );
}

/// The application refuses a pinch by not preventing its first event's
/// default: the pinch it started is cancelled, and the rest of the sequence
/// is left to the browser (its page zoom), reaching nobody.
#[wasm_bindgen_test]
fn a_refused_pinch_is_left_to_the_browser() {
    let window = CanvasWindow::open();
    window.answer(true, false);
    let first = dispatched(&wheel(true, 0, 0.0, -3.5, 40.0, 30.0));
    assert!(!first.prevented, "the browser keeps the zoom");
    assert_eq!(
        names(&pan_zoom(&window.pointer_events())),
        ["start", "cancelled"]
    );
    window.answer(false, true);
    let rest = dispatched(&wheel(true, 0, 0.0, -3.5, 40.0, 30.0));
    assert!(!rest.prevented, "the sequence stays the browser's");
    assert!(window.pointer_events().is_empty(), "and reaches nobody");
}

/// A Control-held mouse wheel (whole pixels) stays a scroll carrying
/// Control, so a zoom-on-Control-wheel control still sees it.
#[wasm_bindgen_test]
fn a_ctrl_held_mouse_wheel_stays_a_scroll() {
    let window = CanvasWindow::open();
    let event = dispatched(&wheel(true, 0, 0.0, 100.0, 40.0, 30.0));
    assert!(event.prevented);
    let events = window.pointer_events();
    let [PointerEvent::Scroll(scroll)] = events.as_slice() else {
        panic!("a scroll: {events:?}");
    };
    assert!(
        scroll
            .modifiers
            .contains(flui_platform::input::keyboard::Modifiers::CONTROL)
    );
}

/// WebKit's gesture events reach the application as one `PanZoom` with the
/// per-step scale and the rotation in clockwise radians, and their default
/// (Safari's page zoom) is cancelled when the application takes them.
#[wasm_bindgen_test]
fn webkit_gesture_events_reach_the_application_as_a_pan_zoom() {
    let window = CanvasWindow::open();
    let start = dispatched(&gesture("gesturestart", 1.0, 0.0, 50.0, 40.0));
    let change = dispatched(&gesture("gesturechange", 2.0, 90.0, 52.0, 40.0));
    let end = dispatched(&gesture("gestureend", 2.0, 90.0, 52.0, 40.0));
    assert!(start.prevented && change.prevented && end.prevented);
    let steps = pan_zoom(&window.pointer_events());
    assert_eq!(names(&steps).first(), Some(&"start"));
    assert_eq!(names(&steps).last(), Some(&"end"));
    let (_, scale, rotation, _) = steps
        .iter()
        .rev()
        .find(|(name, ..)| name == "update")
        .expect("an update");
    assert!((scale - 2.0).abs() < 1e-9, "{steps:?}");
    assert!(
        (rotation - core::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "{steps:?}"
    );
}

/// The pointer events as verbs with their kind and pointer id.
fn pointer_verbs(events: &[PointerEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            PointerEvent::Enter(signal) => {
                format!(
                    "enter {:?} {}",
                    signal.pointer.kind,
                    signal.pointer.id.get()
                )
            }
            PointerEvent::Leave(signal) => {
                format!(
                    "leave {:?} {}",
                    signal.pointer.kind,
                    signal.pointer.id.get()
                )
            }
            PointerEvent::Move(movement) => format!(
                "move {:?} {}",
                movement.pointer.kind,
                movement.pointer.id.get()
            ),
            PointerEvent::Down(press) => {
                format!("down {:?} {}", press.pointer.kind, press.pointer.id.get())
            }
            PointerEvent::Up(release) => {
                format!("up {:?} {}", release.pointer.kind, release.pointer.id.get())
            }
            PointerEvent::Cancel(cancel) => format!(
                "cancel {:?} {} {:?}",
                cancel.pointer.kind,
                cancel.pointer.id.get(),
                cancel.reason
            ),
            PointerEvent::Scroll(scroll) => {
                format!(
                    "scroll {:?} {}",
                    scroll.pointer.kind,
                    scroll.pointer.id.get()
                )
            }
            PointerEvent::PanZoom(gesture) => format!("panzoom {}", gesture.pointer().id.get()),
            other => format!("{other:?}"),
        })
        .collect()
}

/// A page that loses the OS focus (Alt+Tab) or is hidden mid-press may never
/// see the release: the press is cancelled then, so the next press is a
/// press and not a button the window still holds.
#[wasm_bindgen_test]
fn a_press_the_page_loses_is_cancelled() {
    let window = CanvasWindow::open();
    pointer_at("pointerdown", 1, "mouse", 0, 1, 40.0, 30.0);
    page_blur();
    pointer_at("pointerdown", 1, "mouse", 0, 1, 40.0, 30.0);
    assert_eq!(
        pointer_verbs(&window.pointer_events()),
        ["down Mouse 1", "cancel Mouse 1 FocusLost", "down Mouse 1"]
    );
    pointer_at("pointerup", 1, "mouse", 0, 0, 40.0, 30.0);
    window.pointer_events();
    pointer_at("pointerdown", 5, "touch", 0, 1, 40.0, 30.0);
    hide_page(true);
    hide_page(false);
    pointer_at("pointerdown", 5, "touch", 0, 1, 40.0, 30.0);
    let verbs = pointer_verbs(&window.pointer_events());
    let [first, cancel, again] = verbs.as_slice() else {
        panic!("a touch, its cancel and a new touch: {verbs:?}");
    };
    assert!(first.starts_with("down Touch") && again.starts_with("down Touch"));
    assert!(cancel.starts_with("cancel Touch") && cancel.ends_with("FocusLost"));
}

/// A wheel event is the mouse's: it scrolls as the pointer the mouse's own
/// events carry, never as the identity a touch or pen took.
#[wasm_bindgen_test]
fn a_wheel_scrolls_as_the_mouse_and_never_as_a_touch() {
    let window = CanvasWindow::open();
    pointer_at("pointerdown", 2, "touch", 0, 1, 40.0, 30.0);
    pointer_at("pointerup", 2, "touch", 0, 0, 40.0, 30.0);
    pointer_at("pointermove", 1, "mouse", -1, 0, 50.0, 30.0);
    wheel(false, 0, 0.0, 40.0, 50.0, 30.0);
    let verbs = pointer_verbs(&window.pointer_events());
    let touch = verbs
        .iter()
        .find(|verb| verb.starts_with("down Touch"))
        .expect("a touch");
    let mouse = verbs
        .iter()
        .find(|verb| verb.starts_with("move Mouse"))
        .expect("the mouse");
    let scroll = verbs
        .iter()
        .find(|verb| verb.starts_with("scroll"))
        .expect("a scroll");
    assert_eq!(scroll, &mouse.replacen("move", "scroll", 1), "{verbs:?}");
    assert_ne!(
        touch.rsplit(' ').next(),
        scroll.rsplit(' ').next(),
        "{verbs:?}"
    );
}

/// A mouse or pen over the canvas enters and leaves it; a pen that leaves
/// is out of range and its contact ends, so the next pen in range is a new
/// contact.
#[wasm_bindgen_test]
fn a_hovering_pen_ends_where_it_leaves() {
    let window = CanvasWindow::open();
    pointer_at("pointerenter", 7, "pen", -1, 0, 40.0, 30.0);
    pointer_at("pointermove", 7, "pen", -1, 0, 42.0, 30.0);
    pointer_at("pointerleave", 7, "pen", -1, 0, 42.0, 30.0);
    pointer_at("pointerenter", 8, "pen", -1, 0, 40.0, 30.0);
    pointer_at("pointerenter", 1, "mouse", -1, 0, 40.0, 30.0);
    pointer_at("pointerleave", 1, "mouse", -1, 0, 40.0, 30.0);
    let verbs = pointer_verbs(&window.pointer_events());
    let pen = |verb: &str| {
        verbs
            .iter()
            .filter(|seen| seen.starts_with(verb) && seen.contains("Pen"))
            .count()
    };
    assert_eq!(
        (pen("enter"), pen("move"), pen("leave")),
        (2, 1, 1),
        "{verbs:?}"
    );
    assert_ne!(verbs[0], verbs[3], "a new pen is a new contact: {verbs:?}");
    assert_eq!(&verbs[4..], ["enter Mouse 1", "leave Mouse 1"]);
    // The page losing focus ends what is still over the canvas, the second
    // pen, and not the mouse that already left: one leave per enter.
    page_blur();
    let after = pointer_verbs(&window.pointer_events());
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(after[0].starts_with("leave Pen"), "{after:?}");
}

/// WebKit on a touch screen reports a two-finger pinch as gesture events
/// beside the touches the application already has: they are left to the
/// touches, so the application does not scale twice.
#[wasm_bindgen_test]
fn a_touch_pinch_is_delivered_once() {
    let window = CanvasWindow::open();
    pointer_at("pointerdown", 2, "touch", 0, 1, 40.0, 30.0);
    pointer_at("pointerdown", 3, "touch", 0, 1, 60.0, 30.0);
    gesture("gesturestart", 1.0, 0.0, 50.0, 30.0);
    gesture("gesturechange", 2.0, 0.0, 50.0, 30.0);
    gesture("gestureend", 2.0, 0.0, 50.0, 30.0);
    let verbs = pointer_verbs(&window.pointer_events());
    assert!(
        verbs.iter().all(|verb| !verb.starts_with("panzoom")),
        "{verbs:?}"
    );
    assert_eq!(verbs.len(), 2, "the two touches: {verbs:?}");
}

/// The browser's context menu over the canvas is answered as the application
/// answered the secondary press that opened it; one no press opened (the
/// menu key) stays the application's.
#[wasm_bindgen_test]
fn the_context_menu_is_answered_as_its_secondary_press() {
    let window = CanvasWindow::open();
    for (propagate, prevent) in [(true, false), (true, true), (false, false), (false, true)] {
        window.answer(propagate, prevent);
        pointer_at("pointerdown", 1, "mouse", 2, 2, 40.0, 30.0);
        let menu = dispatched(&context_menu(2, false));
        pointer_at("pointerup", 1, "mouse", 2, 0, 40.0, 30.0);
        assert_eq!(
            (menu.prevented, menu.reached_ancestor),
            (prevent, propagate),
            "a press answered propagate={propagate} prevent={prevent}"
        );
    }
    // macOS opens it from a Control-held primary click: that press answers
    // it, as a secondary one does.
    window.answer(false, false);
    control_click("pointerdown", 40.0, 30.0);
    let menu = dispatched(&context_menu(0, true));
    control_click("pointerup", 40.0, 30.0);
    assert_eq!(
        (menu.prevented, menu.reached_ancestor),
        (false, false),
        "a Control-held click's menu"
    );
    window.answer(true, false);
    assert!(
        dispatched(&context_menu(0, false)).prevented,
        "the menu key"
    );
}

/// A page hidden mid-press ends the press before its visibility is
/// reported, so a callback that closes the window on it does not leave the
/// press without its end.
#[wasm_bindgen_test]
fn a_hidden_page_ends_its_presses_before_a_visibility_callback_closes_it() {
    let window = CanvasWindow::open();
    let closing = std::rc::Rc::downgrade(&window.window);
    window.handler.react_to_state_change(move || {
        if let Some(window) = closing.upgrade()
            && window.state().occluded
        {
            window.close_now();
        }
    });
    pointer_at("pointerdown", 5, "touch", 0, 1, 40.0, 30.0);
    hide_page(true);
    hide_page(false);
    let verbs = pointer_verbs(&window.pointer_events());
    assert!(
        verbs
            .last()
            .is_some_and(|verb| verb.starts_with("cancel Touch") && verb.ends_with("FocusLost")),
        "{verbs:?}"
    );
}

/// A pen whose browser reports only the tilt planes keeps its orientation.
#[wasm_bindgen_test]
fn a_pen_reporting_only_tilt_planes_keeps_its_orientation() {
    let window = CanvasWindow::open();
    tilted_pen(40.0, 30.0, 30.0, 0.0);
    let events = window.pointer_events();
    let down = events
        .iter()
        .find_map(|event| match event {
            PointerEvent::Down(down) => Some(down),
            _ => None,
        })
        .expect("a down");
    let orientation = down.sample.orientation.expect("the tilt is kept");
    let altitude = orientation.altitude().expect("an altitude");
    assert!(
        (altitude - 60.0_f64.to_radians()).abs() < 1e-6,
        "30° of tilt along x is 60° of altitude: {orientation:?}"
    );
}

/// An HTML drag over the canvas is offered by its types and files, a target
/// that accepts makes `dragover` accept the drop with its action, and the
/// drop copies every representation out of the `DataTransfer` before it
/// returns, so the target's requests are served after the event: strings at
/// once, files read asynchronously.
#[wasm_bindgen_test]
async fn an_html_drag_over_the_canvas_is_offered_and_dropped() {
    let window = CanvasWindow::open();
    window.accept.set(Some(TransferActions::COPY));
    begin_drag(
        r#"[["text/plain","hello"],["text/uri-list","https://a.example/"]]"#,
        r##"[["notes.txt","text/markdown","# notes"],["shot.png","image/png","png"]]"##,
        "copyMove",
    );
    let enter = drag_answer("dragenter");
    assert!(enter.prevented, "the canvas becomes the drag's target");
    let over = drag_answer("dragover");
    assert!(over.prevented, "the drop is accepted");
    assert_eq!(over.drop_effect, "copy");
    let drop = drag_answer("drop");
    assert!(drop.prevented, "the browser does not open the dropped link");
    let events = window.drag_events();
    let [
        DragDropEvent::Entered { offer, allowed, .. },
        DragDropEvent::Moved { .. },
        DragDropEvent::Dropped { id, action, .. },
    ] = events.as_slice()
    else {
        panic!("enter, move, drop: {events:?}");
    };
    assert_eq!(*allowed, TransferActions::COPY | TransferActions::MOVE);
    assert_eq!(*action, TransferActions::COPY);
    assert_eq!(offer.id(), *id);
    let transfer = window.platform.data_transfer();
    let TransferPayload::Text(text) = transfer
        .request(*id, text_index(offer), TransferLimits::default())
        .await
        .expect("the text was copied at the drop")
    else {
        panic!("text");
    };
    assert_eq!(text, "hello");
    let file = offer
        .find(&TransferFormat::Custom {
            mime: Mime::new("text/markdown"),
        })
        .expect("the file is offered by its type");
    let TransferPayload::Custom { bytes, .. } = transfer
        .request(*id, file, TransferLimits::default())
        .await
        .expect("the file is read")
    else {
        panic!("bytes");
    };
    assert_eq!(&*bytes, b"# notes");
    let image = offer
        .find(&TransferFormat::Image {
            mime: Mime::new("image/png"),
        })
        .expect("the image file is offered as an image");
    let TransferPayload::Custom { mime, bytes } = transfer
        .request(*id, image, TransferLimits::default())
        .await
        .expect("the image is read")
    else {
        panic!("an image file resolves as bytes with its container type");
    };
    assert_eq!((mime.as_str(), &*bytes), ("image/png", &b"png"[..]));
    assert!(matches!(
        transfer
            .request(*id, file, TransferLimits::default().with_max_bytes(3))
            .await,
        Err(TransferError::TooLarge { .. })
    ));
    transfer.conclude_drop(*id);
    assert!(matches!(
        transfer
            .request(*id, text_index(offer), TransferLimits::default())
            .await,
        Err(TransferError::StaleOffer(_))
    ));
}

fn text_index(offer: &DataTransferOffer) -> RepresentationIndex {
    offer.find(&TransferFormat::Text).expect("text offered")
}

/// A target that declines leaves `dragover` uncancelled with no drop effect,
/// so the browser refuses the drop; leaving retires the offer.
#[wasm_bindgen_test]
async fn a_declined_drag_is_refused_and_its_leave_retires_the_offer() {
    let window = CanvasWindow::open();
    begin_drag(r#"[["text/plain","hello"]]"#, "[]", "copy");
    assert!(drag_answer("dragenter").prevented);
    let over = drag_answer("dragover");
    assert!(!over.prevented && over.drop_effect == "none", "{over:?}");
    window.answer(false, true);
    let leave = drag_answer("dragleave");
    assert!(!leave.reached, "the answer stops the leave at the canvas");
    let events = window.drag_events();
    let [
        DragDropEvent::Entered { offer, .. },
        DragDropEvent::Moved { .. },
        DragDropEvent::Exited { id },
    ] = events.as_slice()
    else {
        panic!("enter, move, exit: {events:?}");
    };
    assert_eq!(offer.id(), *id);
    let request =
        window
            .platform
            .data_transfer()
            .request(*id, text_index(offer), TransferLimits::default());
    assert!(matches!(request.await, Err(TransferError::StaleOffer(_))));
}

/// An embedder whose callback does not prevent `dragenter` keeps the
/// browser's own handling: the canvas does not become the target.
#[wasm_bindgen_test]
fn an_unprevented_dragenter_leaves_the_drag_to_the_page() {
    let window = CanvasWindow::open();
    window.answer(true, false);
    window.accept.set(Some(TransferActions::COPY));
    begin_drag(r#"[["text/plain","hello"]]"#, "[]", "copy");
    let enter = drag_answer("dragenter");
    assert!(!enter.prevented && enter.reached, "{enter:?}");
    assert!(!drag_answer("dragover").prevented);
}

fn os_drag_request(ended: &Rc<RefCell<Vec<OsDragEnd>>>) -> OsDragRequest {
    let key = DragSessionKey::new(
        ProcessNonce::from_raw(std::num::NonZeroU64::new(5).expect("nonzero")),
        UiRuntimeId::new_gen(0, flui_foundation::generation::Generation::FIRST),
        DragSessionId::from_wire(std::num::NonZeroU64::new(3).expect("nonzero")),
    );
    let text = RepresentationDescriptor {
        format: TransferFormat::Text,
        declared_len: None,
    };
    let ended = Rc::clone(ended);
    let terms = OsDragTerms::try_new(
        key,
        vec![text],
        TransferActions::COPY | TransferActions::MOVE,
    )
    .expect("one text representation under COPY and MOVE");
    OsDragRequest::new(
        terms,
        None,
        Box::new(|index| {
            Ok(if index.0 == 0 {
                TransferPayload::Text("dragged".to_owned())
            } else {
                TransferPayload::Custom {
                    mime: Mime::new(flui_platform::transfer::drag::IDENTITY_ESSENCE),
                    bytes: Arc::from(Vec::new()),
                }
            })
        }),
        Box::new(move |end| ended.borrow_mut().push(end)),
    )
}

/// The canvas is draggable, and the browser's `dragstart` starts the HTML
/// drag only for a session the runtime started on this press: its
/// representations, the identity among them, are stored then, and the end
/// is read from `dragend`. Without one the `dragstart` is cancelled, and a
/// session started later in that press stays in its window.
#[wasm_bindgen_test]
fn dragstart_runs_a_waiting_session_and_refuses_otherwise() {
    let window = CanvasWindow::open();
    let source = window
        .window
        .drag_source()
        .expect("the web starts HTML drags");
    assert!(matches!(
        source.start_timing(),
        OsDragStart::AtStart { eager_data: true }
    ));
    let ended = Rc::new(RefCell::new(Vec::new()));

    // A runtime starts its session at its quiescent point, still inside the
    // press's dispatch: the source already knows of the press there.
    let starting = Rc::new(RefCell::new(None));
    {
        let (source, ended, starting) =
            (Arc::clone(&source), Rc::clone(&ended), Rc::clone(&starting));
        *window.on_press.borrow_mut() = Some(Box::new(move || {
            *starting.borrow_mut() = Some(source.start(os_drag_request(&ended)));
        }));
    }
    press_mouse("pointerdown", 40.0, 30.0);
    assert_eq!(
        starting.borrow_mut().take(),
        Some(Ok(())),
        "a session started inside the press's dispatch waits for dragstart"
    );
    let start = started();
    assert!(!start.prevented, "the HTML drag runs");
    assert_eq!(
        start.stored.get("text/plain").map(String::as_str),
        Some("dragged")
    );
    assert!(
        start
            .stored
            .keys()
            .any(|dom_type| dom_type.starts_with(flui_platform::transfer::drag::IDENTITY_ESSENCE)),
        "the identity rides in its type: {start:?}"
    );
    assert_eq!(start.effect_allowed, "copyMove");
    press_mouse("pointercancel", 40.0, 30.0);
    drag_end("move");
    assert_eq!(*ended.borrow(), [OsDragEnd::Dropped(TransferActions::MOVE)]);

    // No session on this press: the browser's drag is cancelled.
    press_mouse("pointerdown", 40.0, 30.0);
    assert!(started().prevented);
    assert_eq!(
        source.start(os_drag_request(&ended)),
        Err(DragError::Unsupported(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::Unavailable
        ))),
        "too late for this press: the session stays in its window"
    );
    press_mouse("pointerup", 40.0, 30.0);

    // A session no dragstart took is cancelled at the release.
    press_mouse("pointerdown", 40.0, 30.0);
    source
        .start(os_drag_request(&ended))
        .expect("a press can start a drag");
    press_mouse("pointerup", 40.0, 30.0);
    assert_eq!(ended.borrow().last(), Some(&OsDragEnd::Cancelled));
    assert_eq!(
        source.start(os_drag_request(&ended)),
        Err(DragError::Unsupported(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::Unavailable
        ))),
        "no press"
    );
}

/// A window opened through the owner platform, as a runner opens one, with
/// the text store host its owner reads, the keys it delivered and the
/// focus it reported.
struct HostedCanvas {
    owner: flui_native::OwnerPlatform,
    window: Rc<dyn HostWindow>,
    handler: Rc<RecordingWindowHandler>,
    host: Rc<dyn TextStoreHost>,
    keys: Rc<RefCell<Vec<KeyEvent>>>,
    active: Arc<std::sync::Mutex<Vec<bool>>>,
}

impl HostedCanvas {
    fn open() -> Self {
        Self::open_on(fresh_canvas)
    }

    /// The window on the canvas `place` leaves in the page: none, so the
    /// window makes its own, or the page's.
    fn open_on(place: fn()) -> Self {
        place();
        let slot = Rc::new(RefCell::new(None));
        let out = Rc::clone(&slot);
        Box::new(WebPlatform::try_new().expect("a browser platform"))
            .run(crate::app::on_ready(move |owner| {
                let handler = RecordingWindowHandler::new();
                let keys = Rc::new(RefCell::new(Vec::new()));
                let record = Rc::clone(&keys);
                handler.react_to_input(move |input| {
                    if let PlatformInput::Keyboard(key) = input {
                        record.borrow_mut().push(key);
                    }
                    // Nothing handles a key: the browser keeps its default.
                    DispatchEventResult::resolved(true, false)
                });
                let flui_native::WindowOpen::Ready(window) =
                    owner.open_window(WindowOptions::default(), handler.clone())?
                else {
                    return Err("the web opens its window at once".into());
                };
                let host = owner
                    .text_store_host(&window)
                    .ok_or("the web window serves text stores")?;
                let active = Arc::new(std::sync::Mutex::new(Vec::new()));
                let seen = Arc::clone(&active);
                let observed = std::rc::Rc::downgrade(&window);
                let last = Cell::new(window.state().focus.is_focused());
                handler.react_to_state_change(move || {
                    let Some(window) = observed.upgrade() else {
                        return;
                    };
                    let focused = window.state().focus.is_focused();
                    if focused != last.replace(focused) {
                        seen.lock().expect("focus log").push(focused);
                    }
                });
                *out.borrow_mut() = Some(Self {
                    owner,
                    window,
                    handler,
                    host,
                    keys,
                    active,
                });
                Ok(())
            }))
            .expect("the platform runs");
        slot.take().expect("the window opened during on_ready")
    }

    fn last_key(&self) -> KeyEvent {
        self.keys.borrow().last().cloned().expect("a key")
    }
}

impl Drop for HostedCanvas {
    fn drop(&mut self) {
        self.window.close_for_cleanup();
        self.owner.quit();
    }
}

fn as_store(store: &Rc<InMemoryTextStore>) -> Rc<dyn TextStore> {
    Rc::clone(store) as Rc<dyn TextStore>
}

/// Where the browser has `EditContext`, the focused field's store is served
/// through one on the canvas: it starts with the store's text and
/// selection, the browser's text updates edit the store, the update made
/// while composing is the store's composition until `compositionend`, and
/// the application's own edit reaches the browser's copy. Unfocusing the
/// field takes the context off the canvas.
#[wasm_bindgen_test]
fn an_edit_context_serves_the_focused_field() {
    for application_commit in [false, true] {
        let canvas = HostedCanvas::open();
        assert!(focus_canvas());
        let store = InMemoryTextStore::new("hello");
        canvas.host.focus_store(Some(as_store(&store)));
        assert_eq!(
            edit_context_text().as_deref(),
            Some("hello"),
            "Chrome has EditContext"
        );

        text_update(5, 5, " world", 11, 11);
        assert_eq!(store.text(), "hello world");
        assert_eq!(store.selection().range().start().get(), 11);

        edit_context_composition("compositionstart");
        text_update(11, 11, "に", 12, 12);
        let composing = store.composition().expect("the update composes");
        assert_eq!(
            (composing.range.start().get(), composing.range.end().get()),
            (11, 12)
        );
        if application_commit {
            store.app_replace(
                Utf16Range::try_new(Utf16Offset::new(11), Utf16Offset::new(12))
                    .expect("composition range"),
                "に",
            );
        } else {
            edit_context_composition("compositionend");
        }
        assert_eq!(store.composition(), None, "committed");
        assert_eq!(store.text(), "hello worldに");
        key_at_focus("Escape", None);
        assert_eq!(
            canvas.last_key().composition,
            ImeComposition::Inactive,
            "application_commit={application_commit}"
        );

        store.app_replace(
            Utf16Range::try_new(Utf16Offset::ZERO, Utf16Offset::new(5)).expect("a range"),
            "HELLO",
        );
        assert_eq!(edit_context_text().as_deref(), Some("HELLO worldに"));

        canvas.host.focus_store(None);
        assert_eq!(edit_context_text(), None, "the context leaves the canvas");
    }
}

/// A key the input method processes is delivered marked composing, so no
/// shortcut, focus traversal or drag cancellation takes it: while the field
/// composes (whatever the event says), and whenever the event carries the
/// input method's key code (WebKit's Escape after `compositionend`). Its
/// default, the input method's, is kept.
#[wasm_bindgen_test]
fn escape_while_the_field_composes_is_the_input_methods() {
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let store = InMemoryTextStore::new("");
    canvas.host.focus_store(Some(as_store(&store)));

    edit_context_composition("compositionstart");
    assert!(
        !key_at_focus("Escape", None),
        "the input method's default stays"
    );
    assert_eq!(canvas.last_key().composition, ImeComposition::Active);
    edit_context_composition("compositionend");
    key_at_focus("Escape", None);
    assert_eq!(canvas.last_key().composition, ImeComposition::Inactive);
    key_at_focus("Escape", Some(229));
    assert_eq!(canvas.last_key().composition, ImeComposition::Active);
}

/// Restores `EditContext` however the test ends.
struct EditContextHidden;

impl EditContextHidden {
    fn hide() -> Self {
        hide_edit_context();
        Self
    }
}

impl Drop for EditContextHidden {
    fn drop(&mut self) {
        restore_edit_context();
    }
}

/// What the hidden textarea holds.
#[derive(Debug, serde::Deserialize)]
struct EditorState {
    value: String,
    start: u32,
    end: u32,
    focused: bool,
    /// Its computed `font-size` in CSS pixels.
    #[serde(rename = "fontSize")]
    font_size: f64,
}

fn editor() -> Option<EditorState> {
    serde_json::from_str(&editor_state()).expect("the page's answer")
}

/// Where the browser has no `EditContext`, a hidden textarea beside the
/// canvas serves the field: focus moves into it without the window losing
/// focus, what the input method types there edits the store (composing until
/// `compositionend`), the application's edits reach it, its keys reach the
/// window as the canvas's do, and unfocusing the field gives focus back to
/// the canvas.
#[wasm_bindgen_test]
fn a_textarea_serves_the_field_without_edit_context() {
    for application_commit in [false, true] {
        let _hidden = EditContextHidden::hide();
        let canvas = HostedCanvas::open();
        assert!(focus_canvas());
        let store = InMemoryTextStore::new("ab");
        let reported = canvas.active.lock().expect("focus log").len();
        canvas.host.focus_store(Some(as_store(&store)));
        let state = editor().expect("a textarea in the text-input host after the canvas");
        assert_eq!((state.value.as_str(), state.focused), ("ab", true));
        assert!(
            state.font_size >= 16.0,
            "iOS Safari zooms into a focused field below 16 px: {}",
            state.font_size
        );
        assert!(
            canvas.active.lock().expect("focus log")[reported..].is_empty(),
            "focus moved within the canvas's area: no window focus change"
        );
        assert!(canvas.window.state().focus.is_focused());

        editor_composition("compositionstart");
        type_into_editor("abに", 3, true);
        assert_eq!(store.text(), "abに");
        assert!(store.composition().is_some(), "the inserted text composes");
        if application_commit {
            store.app_replace(
                Utf16Range::try_new(Utf16Offset::new(2), Utf16Offset::new(3))
                    .expect("composition range"),
                "に",
            );
        } else {
            editor_composition("compositionend");
        }
        assert_eq!(store.composition(), None);
        key_at_focus("Escape", None);
        assert_eq!(
            canvas.last_key().composition,
            ImeComposition::Inactive,
            "application_commit={application_commit}"
        );
        type_into_editor("abにc", 4, false);
        assert_eq!(store.text(), "abにc");
        assert_eq!(store.selection().range().start().get(), 4);

        store.app_replace(
            Utf16Range::try_new(Utf16Offset::ZERO, Utf16Offset::new(2)).expect("a range"),
            "AB",
        );
        let state = editor().expect("the textarea");
        assert_eq!(state.value, "ABにc");
        let _ = (state.start, state.end);

        key_at_focus("Enter", None);
        assert_eq!(
            canvas.last_key().key,
            flui_platform::input::keyboard::Key::Named(
                flui_platform::input::keyboard::NamedKey::Enter
            )
        );

        canvas.host.focus_store(None);
        let state = editor().expect("the window keeps its one editor");
        assert!(
            !state.focused && state.value.is_empty(),
            "released: unfocused and empty"
        );
        assert!(
            canvas.window.state().focus.is_focused(),
            "focus went back to the canvas"
        );
    }
}

/// A tap with no field served focuses the window's editor in the tap's
/// handler with `inputmode="none"` and `aria-hidden`, so a tap on a button
/// opens no keyboard and shows a screen reader no field; the field the
/// application focuses for that tap after the handler (at its next frame)
/// takes the editor over with its focus and writes its own hints.
///
/// The test driver has no trusted input from the page, so the taps are
/// constructed events; what a real tap opens on iOS Safari and Android is a
/// device run.
#[wasm_bindgen_test]
fn a_tap_focuses_an_editor_the_field_attached_after_it_takes_over() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    blur_all();
    touch_stroke(5.0, 5.0, 5.0, 5.0);
    assert_eq!(page_focus(), "editor", "focused in the tap's handler");
    assert_eq!(editor_hints(), r#"["none","off",null]"#, "no keyboard yet");
    assert_eq!(editor_aria_hidden().as_deref(), Some("true"));
    let store = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&store)));
    let state = editor().expect("the editor the tap focused");
    assert_eq!((state.value.as_str(), state.focused), ("ab", true));
    assert_eq!(
        editor_hints(),
        r#"["text","off","enter"]"#,
        "the field's hints"
    );
    assert_eq!(editor_aria_hidden(), None);
    canvas.host.focus_store(None);
}

/// A tap no field follows within two animation frames gives the focus back
/// to the canvas, having opened no keyboard; a second tap inside the first
/// one's two frames keeps its own two.
#[wasm_bindgen_test]
async fn a_tap_no_field_follows_gives_the_focus_back_to_the_canvas() {
    let _hidden = EditContextHidden::hide();
    let _canvas = HostedCanvas::open();
    blur_all();
    touch_stroke(200.0, 100.0, 200.0, 100.0);
    assert_eq!(page_focus(), "editor");
    assert_eq!(editor_hints(), r#"["none","off",null]"#, "no keyboard");
    js_sys::futures::JsFuture::from(frames(1))
        .await
        .expect("a frame");
    touch_stroke(200.0, 100.0, 200.0, 100.0);
    js_sys::futures::JsFuture::from(frames(1))
        .await
        .expect("a frame");
    assert_eq!(page_focus(), "editor", "the second tap's frames still run");
    js_sys::futures::JsFuture::from(frames(2))
        .await
        .expect("two frames");
    assert_eq!(page_focus(), "canvas");
}

/// Moving from one field to the next keeps the window's one editor and its
/// focus, so a phone's keyboard stays up; releasing the last field gives the
/// focus back to the canvas.
#[wasm_bindgen_test]
fn moving_from_one_field_to_the_next_keeps_the_editor_and_its_focus() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let first = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&first)));
    assert_eq!(page_focus(), "editor");
    mark_editor();
    let second = InMemoryTextStore::new("xyz");
    canvas.host.focus_store(Some(as_store(&second)));
    let state = editor().expect("the editor");
    assert_eq!((state.value.as_str(), state.focused), ("xyz", true));
    assert!(editor_is_marked(), "the same element");
    assert_eq!(textarea_count(), 1);
    canvas.host.focus_store(None);
    assert_eq!(page_focus(), "canvas");
}

/// A tap on the field whose editor kept its focus moves focus only when the
/// browser reports the keyboard hidden (`navigator.virtualKeyboard`'s empty
/// rectangle; Android's Back keeps the focus): the editor gives its focus to
/// the canvas and takes it back inside the tap's handler, the second focus
/// its own, so the editor ends focused with the canvas's forwarding
/// stopped. With the keyboard reported up (typing, perhaps
/// mid-composition), with an inset of zero inferred from the visual
/// viewport (Safari reads that with its keyboard up) or with nothing to
/// read, it changes no focus. `navigator.virtualKeyboard`'s `show()` is
/// never called.
#[wasm_bindgen_test]
fn a_tap_on_a_field_that_kept_its_focus_reopens_only_a_hidden_keyboard() {
    struct Keyboard;
    impl Drop for Keyboard {
        fn drop(&mut self) {
            block_canvas_focus_forwarding(false);
            restore_keyboard();
        }
    }
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let store = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&store)));
    assert_eq!(page_focus(), "editor");
    let _keyboard = Keyboard;
    for (state, forwarding, focuses) in [
        ("up", true, 0),
        ("inferred", true, 0),
        ("unknown", true, 0),
        ("hidden", true, 1),
        ("hidden", false, 1),
    ] {
        block_canvas_focus_forwarding(!forwarding);
        fake_keyboard(state);
        count_editor_focus();
        touch_stroke(5.0, 5.0, 5.0, 5.0);
        let case = format!("keyboard {state}, forwarding {forwarding}");
        assert_eq!(editor_focus_count(), focuses, "{case}");
        assert_eq!(page_focus(), "editor", "{case}");
        assert_eq!(keyboard_shows(), 0, "show() is never relied on");
    }
    block_canvas_focus_forwarding(false);
    fake_keyboard("hidden");
    count_editor_focus();
    touch_stroke(200.0, 100.0, 200.0, 100.0);
    assert_eq!(editor_focus_count(), 0, "a tap off the field does nothing");
    canvas.host.focus_store(None);
}

/// A tap with nothing served, as Chrome on a phone delivers it (the pointer
/// events, then the compatibility `mousedown`, which focuses the canvas):
/// the canvas hands the focus on to the editor the tap focused, so the field
/// attached after it finds the editor focused rather than the canvas.
#[wasm_bindgen_test]
fn a_tap_s_compatibility_mousedown_leaves_the_focus_on_the_editor() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    blur_all();
    tap_with_compatibility_events(5.0, 5.0);
    assert_eq!(page_focus(), "editor", "handed on from the canvas");
    let store = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&store)));
    let state = editor().expect("the editor");
    assert_eq!((state.value.as_str(), state.focused), ("ab", true));
    canvas.host.focus_store(None);
}

/// The window's editor's attributes follow its state on every change: a
/// served field's hints with no `aria-hidden`; between fields `aria-hidden`,
/// `inputmode="none"`, `autocomplete="off"` and no value, so a password
/// field's text and hints do not linger. What each assertion pins:
/// `idle.value == ""` the idle state's clearing in `set_hidden_state`; the
/// undo assertion that the next field's value is replaced from script, in
/// `EditorElement::show` (the password, typed through the browser's
/// editing, is in the textarea's undo history, which a script write
/// empties). The undo assertion alone would stay green with the idle
/// clearing reverted: `show`'s write empties the history too.
#[wasm_bindgen_test]
fn the_hidden_editor_s_attributes_follow_its_state() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let password = InMemoryTextStore::new("");
    password.set_protected(true);
    canvas.host.focus_store(Some(as_store(&password)));
    assert_eq!(editor_hints(), r#"["text","new-password","enter"]"#);
    assert_eq!(editor_aria_hidden(), None);
    insert_into_editor("secret");
    assert_eq!(password.text(), "secret", "typed through the browser");
    canvas.host.focus_store(None);
    let idle = editor().expect("the window's editor");
    assert_eq!(idle.value, "", "no value between fields");
    assert_eq!(editor_hints(), r#"["none","off",null]"#);
    assert_eq!(editor_aria_hidden().as_deref(), Some("true"));
    let plain = InMemoryTextStore::new("ab");
    plain.set_text_input(flui_platform::text::text_store::TextInputOptions::new(
        flui_platform::text::text_store::TextInputPurpose::Search,
    ));
    canvas.host.focus_store(Some(as_store(&plain)));
    assert_eq!(editor_hints(), r#"["search","off","search"]"#);
    assert_eq!(editor_aria_hidden(), None);
    let undone = undo_in_editor();
    assert!(
        !undone.contains("secret"),
        "the browser's undo does not bring the password back: {undone:?}"
    );
    canvas.host.focus_store(None);
}

/// A composition still live in one field when another takes the editor is
/// committed in place to the first, and the browser's late composition
/// events land in no store: the second field keeps its text.
#[wasm_bindgen_test]
fn a_composition_left_in_one_field_lands_in_no_other() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let first = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&first)));
    editor_composition("compositionstart");
    type_into_editor("abに", 3, true);
    assert!(first.composition().is_some(), "the first field composes");
    let second = InMemoryTextStore::new("xyz");
    canvas.host.focus_store(Some(as_store(&second)));
    assert_eq!(first.composition(), None, "committed in place");
    assert_eq!(first.text(), "abに");
    editor_composition("compositionend");
    let state = editor().expect("the editor");
    type_into_editor(&state.value, 3, false);
    assert_eq!(
        second.text(),
        "xyz",
        "nothing of the first field's composition"
    );
    canvas.host.focus_store(None);
}

/// An application that completes the composition in the hidden editor ends
/// the browser's by rewriting the editor's value from script, with no blur
/// or focus that would close a phone's keyboard outside a tap, and keeps
/// showing the store's text, which the application then commits in place.
#[wasm_bindgen_test]
fn completing_a_composition_in_the_hidden_editor_moves_no_focus() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let store = InMemoryTextStore::new("ab");
    let served = as_store(&store);
    canvas.host.focus_store(Some(Rc::clone(&served)));
    editor_composition("compositionstart");
    type_into_editor("abに", 3, true);
    assert!(store.composition().is_some(), "the field composes");
    count_editor_focus();
    let end = canvas
        .host
        .complete_composition(&served)
        .expect("the focused store");
    assert_eq!(end, CompositionEnd::Abandoned, "the browser's discarded");
    assert_eq!(
        (editor_blur_count(), editor_focus_count()),
        (0, 0),
        "no focus move"
    );
    assert_eq!(page_focus(), "editor");
    // The application commits the store's composition in place, as
    // `Abandoned` asks.
    let committed = flui_platform::text::text_store::commit_composition_in_place(&*served, |_| {})
        .expect("the store commits");
    assert_eq!(committed, LockOutcome::Granted, "committed now");
    assert_eq!(store.composition(), None);
    assert_eq!(store.text(), "abに");
    let state = editor().expect("the editor");
    assert_eq!(state.value, "abに");
    // The browser's own end of the composition, after the rewrite, adds
    // nothing.
    editor_composition("compositionend");
    type_into_editor(&state.value, 3, false);
    assert_eq!(store.text(), "abに");
    canvas.host.focus_store(None);
}

/// A store that refuses its next `refusals` asynchronous locks (the
/// browser's edits), as one whose frame transaction cannot take them, so
/// the host republishes the store to the editor.
struct RefusingStore {
    inner: Rc<InMemoryTextStore>,
    refusals: Cell<usize>,
}

impl TextStore for RefusingStore {
    fn status(&self) -> TextStoreStatus {
        self.inner.status()
    }

    fn press_target_at(
        &self,
        point: flui_foundation::geometry::Point<f64>,
    ) -> Result<
        flui_platform::text::text_store::PressTarget,
        flui_platform::text::text_store::TextStoreError,
    > {
        self.inner.press_target_at(point)
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        if self.refusals.get() > 0 && timing == LockTiming::Async {
            self.refusals.set(self.refusals.get() - 1);
            return Err(TextStoreError::DeferredQueueFull);
        }
        self.inner.request_lock(grant, timing)
    }

    fn run_deferred_grants(&self) -> usize {
        self.inner.run_deferred_grants()
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        self.inner.set_commit_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        self.inner.set_observer(observer);
    }
}

/// A composition live when the host republishes the store (an edit the
/// store refused) is committed in place once, and the browser's end of it
/// in the synchronous order (its `compositionend` and `input` after the
/// value reset) lands the composed text once. An input method that commits
/// asynchronously after the reset is not covered here (the parity source
/// keeps it owed).
#[wasm_bindgen_test]
async fn a_composition_live_during_a_resync_lands_once() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let inner = InMemoryTextStore::new("ab");
    let store = Rc::new(RefusingStore {
        inner: Rc::clone(&inner),
        refusals: Cell::new(0),
    });
    canvas
        .host
        .focus_store(Some(Rc::clone(&store) as Rc<dyn TextStore>));
    editor_composition("compositionstart");
    type_into_editor("abに", 3, true);
    assert!(inner.composition().is_some(), "the field composes");
    store.refusals.set(1);
    type_into_editor("abにほ", 4, true);
    // The republish runs once the task that refused the edit ends.
    js_sys::futures::JsFuture::from(frames(1))
        .await
        .expect("a frame");
    assert_eq!(inner.composition(), None, "committed in place");
    assert_eq!(inner.text(), "abに");
    let state = editor().expect("the editor");
    assert_eq!(state.value, "abに", "the store republished");
    editor_composition("compositionend");
    type_into_editor(&state.value, 3, false);
    assert_eq!(inner.text(), "abに", "the composed text once");
    canvas.host.focus_store(None);
}

/// With a field served and its keyboard hidden (the textarea blurred), a tap
/// on the field focuses the textarea again; a tap elsewhere, a drag over
/// the field (the end of a scroll) and a mouse leave it unfocused.
#[wasm_bindgen_test]
fn a_hidden_keyboard_opens_again_on_a_tap_on_the_field_alone() {
    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    // The kit's field "ab" lies over (0, 0)–(20, 20) of the canvas.
    let store = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&store)));
    assert_eq!(page_focus(), "editor");
    blur_all();
    for (what, stroke) in [
        ("a tap off the field", [200.0, 100.0, 200.0, 100.0]),
        ("a drag ending on the field", [5.0, 60.0, 5.0, 5.0]),
    ] {
        touch_stroke(stroke[0], stroke[1], stroke[2], stroke[3]);
        assert_ne!(page_focus(), "editor", "{what}");
    }
    press_mouse("pointerdown", 5.0, 5.0);
    press_mouse("pointerup", 5.0, 5.0);
    assert_ne!(page_focus(), "editor", "a mouse");
    touch_stroke(5.0, 5.0, 6.0, 6.0);
    assert_eq!(page_focus(), "editor", "a tap on the field");
    canvas.host.focus_store(None);
}

/// The hidden editor tells the browser, for each purpose, each enter action
/// and protection, which keyboard to show (`inputmode`), what it may fill the
/// field with (`autocomplete`: nothing beside the canvas, `new-password` for a
/// protected field), how to label Return (`enterkeyhint`: the action the
/// field names, else a search for a query, else `enter`, never left to the
/// browser) and how to capitalize and correct.
#[wasm_bindgen_test]
fn the_hidden_editor_states_each_purpose_and_enter_action() {
    use flui_platform::text::text_store::TextInputPurpose::{Email, Phone, Search, Text, Url};
    use flui_platform::text::text_store::{
        Capitalization, Correction, EnterAction, NumberFormat, TextInputOptions, TextInputPurpose,
    };

    let _hidden = EditContextHidden::hide();
    let canvas = HostedCanvas::open();
    assert!(focus_canvas());
    let number = TextInputPurpose::Number(NumberFormat::WHOLE);
    let decimal = TextInputPurpose::Number(NumberFormat {
        signed: false,
        decimal: true,
    });
    // (purpose, single line, protected, [inputmode, autocomplete, enterkeyhint])
    let cases = [
        (Text, true, false, r#"["text","off","enter"]"#),
        (Text, false, false, r#"["text","off","enter"]"#),
        (Email, true, false, r#"["email","off","enter"]"#),
        (Email, false, false, r#"["email","off","enter"]"#),
        (Url, true, false, r#"["url","off","enter"]"#),
        (Phone, true, false, r#"["tel","off","enter"]"#),
        (number, true, false, r#"["numeric","off","enter"]"#),
        (decimal, true, false, r#"["decimal","off","enter"]"#),
        (Search, true, false, r#"["search","off","search"]"#),
        (Search, false, false, r#"["search","off","search"]"#),
        (Text, true, true, r#"["text","new-password","enter"]"#),
        (number, true, true, r#"["numeric","new-password","enter"]"#),
    ];
    for (purpose, single_line, protected, expected) in cases {
        let store = InMemoryTextStore::new("ab");
        store.set_text_input(TextInputOptions::new(purpose));
        store.set_single_line(single_line);
        store.set_protected(protected);
        canvas.host.focus_store(Some(as_store(&store)));
        let case = format!("{purpose:?}, single line {single_line}, protected {protected}");
        assert_eq!(editor_hints(), expected, "attached: {case}");
        canvas.host.focus_store(None);
    }
    // A status that changes while the field is served reaches the element.
    let store = InMemoryTextStore::new("ab");
    canvas.host.focus_store(Some(as_store(&store)));
    assert_eq!(
        editor_correction(),
        r#"["off","on","true"]"#,
        "free text is corrected and spell-checked"
    );
    store.set_text_input(TextInputOptions::new(Search));
    assert_eq!(editor_hints(), r#"["search","off","search"]"#, "refreshed");
    store.set_text_input(
        TextInputOptions::TEXT
            .enter_action(EnterAction::SendMessage)
            .capitalization(Capitalization::Sentences)
            .correction(Correction::Suggestions),
    );
    store.set_single_line(false);
    assert_eq!(editor_hints(), r#"["text","off","send"]"#, "refreshed");
    assert_eq!(editor_correction(), r#"["sentences","off","true"]"#);
    store.set_protected(true);
    assert_eq!(
        editor_correction(),
        r#"["off","off","false"]"#,
        "a password is neither shifted nor corrected"
    );
    canvas.host.focus_store(None);
    assert_eq!(editor_correction(), r#"["off","off","false"]"#, "idle");
}

/// An `EditContext` serving a field on the page's own canvas writes its
/// `inputmode` and `enterkeyhint`, and the page's own values come back once
/// the field goes; closing the window leaves a canvas that had no `style`
/// attribute with none.
#[wasm_bindgen_test]
fn a_page_canvas_gets_back_the_inputmode_and_the_style_it_had() {
    use flui_platform::text::text_store::{EnterAction, TextInputOptions, TextInputPurpose};

    let canvas = HostedCanvas::open_on(bare_canvas);
    let store = InMemoryTextStore::new("ab");
    store.set_text_input(
        TextInputOptions::new(TextInputPurpose::Email).enter_action(EnterAction::Next),
    );
    canvas.host.focus_store(Some(as_store(&store)));
    assert_eq!(
        canvas_attribute("inputmode").as_deref(),
        Some("email"),
        "the field's"
    );
    assert_eq!(canvas_attribute("enterkeyhint").as_deref(), Some("next"));
    canvas.host.focus_store(None);
    assert_eq!(
        canvas_attribute("inputmode").as_deref(),
        Some("decimal"),
        "the page's"
    );
    assert_eq!(canvas_attribute("enterkeyhint"), None, "the page had none");
    canvas.window.close_now();
    assert_eq!(canvas_attribute("style"), None, "no style attribute");
    assert_eq!(canvas_attribute("tabindex"), None);
    assert_eq!(canvas_attribute("width"), None);
    drop(canvas);
    fresh_canvas();
}

/// A window on a page with no canvas puts one root in the page holding the
/// canvas, the text-input host and the semantics host, the semantics host
/// last; on the page's own canvas the two hosts follow it. Closing the window
/// takes out what it added and gives the page's canvas back its own `style`
/// and `tabindex`.
#[wasm_bindgen_test]
fn the_window_keeps_its_canvas_and_hosts_in_order_and_takes_them_out() {
    for (canvas, place, expected) in [
        (
            "made by the window",
            fresh_canvas as fn(),
            r#"{"root":true,"order":["canvas","flui-text-input","flui-semantics"],"last":true}"#,
        ),
        (
            "the page's own",
            page_canvas,
            r#"{"root":false,"order":["canvas","flui-text-input","flui-semantics"],"last":null}"#,
        ),
    ] {
        place();
        let page = canvas_siblings().map(|_| canvas_attributes());
        let platform = WebPlatform::try_new().expect("a browser platform");
        let window = platform
            .open_window(WindowOptions::default(), RecordingWindowHandler::new())
            .expect("the canvas opens");
        assert_eq!(canvas_siblings().as_deref(), Some(expected), "{canvas}");
        window.close_now();
        assert_eq!(
            host_elements_in_page(),
            0,
            "{canvas}: what the window added"
        );
        if let Some(page) = page {
            assert_eq!(canvas_attributes(), page, "{canvas}: the page's attributes");
        }
    }
    fresh_canvas();
}

/// A frame request carries the browser's own frame time, not the moment the callback ran: a frame
/// the page's thread was held for before the backend's callback sees a vsync that old. Each frame asks
/// for the next, as a running animation does: the backend produces a frame only when one is asked for.
#[wasm_bindgen_test]
async fn an_animation_frame_request_is_stamped_with_the_browsers_frame_time() {
    const HELD_MS: f64 = 25.0;
    let canvas = HostedCanvas::open();
    // `(requests seen, how long before the callback ran the second one's vsync was)`.
    let seen = Arc::new(std::sync::Mutex::new((0_usize, Duration::ZERO)));
    let (record, window) = (Arc::clone(&seen), Rc::clone(&canvas.window));
    canvas.handler.react_to_frame(move |request| {
        let now = flui_platform::EventTime::from_nanos(flui_native::event_timestamp_ns());
        let mut seen = record.lock().expect("the frame record");
        seen.0 += 1;
        if seen.0 == 2 {
            seen.1 = now.saturating_duration_since(request.vsync);
        }
        if seen.0 == 1 {
            // Registered inside the callback, before the request for the next frame, so it runs
            // ahead of the backend's own callback for that frame and holds the thread between the
            // frame's start and that callback.
            hold_next_frame(HELD_MS);
            window.request_redraw();
        }
        assert!(
            request.target_present.is_none(),
            "the browser names no present time"
        );
    });
    canvas.window.request_redraw();
    wait(300).await;
    let (requests, age) = *seen.lock().expect("the frame record");
    assert!(requests >= 2, "{requests} frame requests in 300 ms");
    assert!(
        age >= Duration::from_millis(20),
        "the frame after a {HELD_MS} ms hold was stamped {age:?} before its callback"
    );
}
