//! Window-owned DOM subscriptions. Only the exact canvas may have a live owner.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use wasm_bindgen::{JsCast, prelude::*};

use super::window::DeviceSize;
use crate::owner::OpenWindowError;

#[wasm_bindgen(inline_js = "
const ownerKey = Symbol.for('flui.canvas.input-owner');
export function claimCanvas(canvas) {
    if (canvas[ownerKey]) throw new Error('canvas already has a live FLUI input owner');
    const token = { pointerAdmissions: new Set() };
    Object.defineProperty(canvas, ownerKey, { value: token, configurable: true });
    return token;
}
export function releaseCanvas(canvas, token) {
    if (canvas[ownerKey] === token) delete canvas[ownerKey];
}
export function effectiveCanvasFocus(canvas) {
    if (!canvas.ownerDocument?.hasFocus()) return false;
    return isOwnedFocus(canvas, canvas.getRootNode().activeElement);
}
// The canvas, or the text editor its owner registered beside it.
export function isOwnedFocus(canvas, node) {
    if (node === canvas) return true;
    const editor = canvas[ownerKey]?.editor;
    return !!node && node === editor;
}
export function setCanvasEditor(canvas, token, editor) {
    if (canvas[ownerKey] === token) token.editor = editor;
}
// Observe the device-pixel content box where the browser has it (Chrome,
// Firefox); Safari rejects the box, and its content box is observed instead.
// True when the device-pixel box is.
export function observeCanvas(observer, canvas) {
    try {
        observer.observe(canvas, { box: 'device-pixel-content-box' });
        return true;
    } catch {
        observer.observe(canvas);
        return false;
    }
}
export function observeBox(observer, canvas, box) { observer.observe(canvas, { box }); }
// The last entry's device-pixel content box as [width, height]; [] when the
// entry has none, or when the canvas has any padding, a fraction of a pixel
// included: the window's size is the padding box (`clientWidth`), which the
// content box does not measure. The box sizes are logical: a vertical writing
// mode puts the width in blockSize.
export function devicePixelSize(entries) {
    const entry = entries[entries.length - 1];
    const box = entry?.devicePixelContentBoxSize?.[0];
    if (!box) return [];
    const style = globalThis.getComputedStyle?.(entry.target);
    if (!style) return [];
    const sides = [style.paddingTop, style.paddingRight, style.paddingBottom, style.paddingLeft];
    if (sides.some(side => parseFloat(side) !== 0)) return [];
    const vertical = /^(vertical|sideways)/.test(style.writingMode ?? '');
    const width = vertical ? box.blockSize : box.inlineSize;
    const height = vertical ? box.inlineSize : box.blockSize;
    const valid = size => Number.isInteger(size) && size > 0;
    return valid(width) && valid(height) ? [width, height] : [];
}
export function disconnectObserver(observer) { observer.disconnect(); }
export function admitPointerCapture(canvas, token, id) {
    if (canvas[ownerKey] !== token) return false;
    token.pointerAdmissions.add(id);
    return true;
}
export function retirePointerCapture(token, id) { token.pointerAdmissions.delete(id); }
export function releasePointerCapture(canvas, token, id) {
    const belongsToNewOwner = () => {
        const current = canvas[ownerKey];
        return current && current !== token && current.pointerAdmissions.has(id);
    };
    if (belongsToNewOwner()) return;
    const captured = canvas.hasPointerCapture(id);
    // The host query itself can synchronously close/reopen the registration.
    if (captured && !belongsToNewOwner()) canvas.releasePointerCapture(id);
}
")]
extern "C" {
    #[wasm_bindgen(catch, js_name = claimCanvas)]
    fn claim_canvas(canvas: &web_sys::HtmlCanvasElement) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = releaseCanvas)]
    fn release_canvas(canvas: &web_sys::HtmlCanvasElement, token: &JsValue) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = effectiveCanvasFocus)]
    pub(crate) fn effective_focus(canvas: &web_sys::HtmlCanvasElement) -> bool;
    #[wasm_bindgen(js_name = isOwnedFocus)]
    pub(super) fn is_owned_focus(canvas: &web_sys::HtmlCanvasElement, node: &JsValue) -> bool;
    #[wasm_bindgen(js_name = setCanvasEditor)]
    fn set_canvas_editor(canvas: &web_sys::HtmlCanvasElement, token: &JsValue, editor: &JsValue);
    #[wasm_bindgen(catch, js_name = observeCanvas)]
    fn observe_canvas(
        observer: &web_sys::ResizeObserver,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<bool, JsValue>;
    #[wasm_bindgen(catch, js_name = observeBox)]
    fn observe_box(
        observer: &web_sys::ResizeObserver,
        canvas: &web_sys::HtmlCanvasElement,
        r#box: &str,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = devicePixelSize)]
    fn device_pixel_size(entries: &JsValue) -> Vec<u32>;
    #[wasm_bindgen(catch, js_name = disconnectObserver)]
    fn disconnect_observer(observer: &web_sys::ResizeObserver) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = releasePointerCapture)]
    fn release_capture(
        canvas: &web_sys::HtmlCanvasElement,
        token: &JsValue,
        id: i32,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name = admitPointerCapture)]
    fn admit_capture(canvas: &web_sys::HtmlCanvasElement, token: &JsValue, id: i32) -> bool;
    #[wasm_bindgen(js_name = retirePointerCapture)]
    fn retire_capture(token: &JsValue, id: i32);
}

fn backend(error: JsValue) -> OpenWindowError {
    OpenWindowError::Backend {
        message: format!("DOM registration failed: {error:?}"),
    }
}

fn report(error: JsValue) {
    crate::owner::panic_boundary::contain_owner_callback(|| {
        tracing::warn!(
            ?error,
            "DOM subscription retirement failed; delivery remains fenced"
        );
    });
}

pub(super) struct CanvasClaim {
    canvas: web_sys::HtmlCanvasElement,
    token: JsValue,
}

impl CanvasClaim {
    pub(super) fn acquire(canvas: &web_sys::HtmlCanvasElement) -> Result<Self, OpenWindowError> {
        Ok(Self {
            canvas: canvas.clone(),
            token: claim_canvas(canvas).map_err(backend)?,
        })
    }

    fn capture_owner(&self) -> PointerCaptureOwner {
        PointerCaptureOwner {
            canvas: self.canvas.clone(),
            token: self.token.clone(),
        }
    }
}

/// Native capture custody belongs to a registration, not its cached buttons.
/// A retired body may still be returning from a host call after a new owner
/// has admitted the same pointer on this canvas.
#[derive(Clone)]
pub(super) struct PointerCaptureOwner {
    canvas: web_sys::HtmlCanvasElement,
    token: JsValue,
}

impl PointerCaptureOwner {
    pub(super) fn admit(&self, id: i32) -> bool {
        admit_capture(&self.canvas, &self.token, id)
    }
    pub(super) fn forget(&self, id: i32) {
        retire_capture(&self.token, id);
    }
    pub(super) fn release(&self, id: i32) {
        self.forget(id);
        if let Err(error) = release_capture(&self.canvas, &self.token, id) {
            report(error);
        }
    }
}

impl Drop for CanvasClaim {
    fn drop(&mut self) {
        if let Err(error) = release_canvas(&self.canvas, &self.token) {
            report(error);
        }
    }
}

pub(crate) struct Listener {
    target: web_sys::EventTarget,
    kind: &'static str,
    capture: bool,
    closure: Option<Closure<dyn Fn(web_sys::Event)>>,
    // The JS wrapper has only a Weak reference. Even a refused detach cannot
    // retain application captures or invoke the retired body.
    _body: Rc<dyn Fn(web_sys::Event)>,
}

impl Listener {
    /// A nonpassive listener for `kind` on `target`, outside a
    /// registration's installation: one the window's text input attaches to
    /// its surface while the window lives. Its body runs only while
    /// `accepting` holds; dropping it detaches it.
    pub(crate) fn install(
        target: &web_sys::EventTarget,
        kind: &'static str,
        accepting: &Rc<Cell<bool>>,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<Self, OpenWindowError> {
        Self::install_in_phase(target, kind, accepting, false, body)
    }

    /// [`Self::install`] in the capture phase when `capture` holds, which
    /// sees the events of descendants that do not bubble (`scroll`).
    pub(crate) fn install_in_phase(
        target: &web_sys::EventTarget,
        kind: &'static str,
        accepting: &Rc<Cell<bool>>,
        capture: bool,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<Self, OpenWindowError> {
        let body: Rc<dyn Fn(web_sys::Event)> = Rc::new(body);
        let weak = Rc::downgrade(&body);
        let accepting = Rc::clone(accepting);
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |event| {
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body(event);
            }
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(false);
        options.set_capture(capture);
        // Own the token before calling the host: a replaced add method might
        // install a listener and then throw. Dropping it still attempts removal.
        let listener = Self {
            target: target.clone(),
            kind,
            capture,
            closure: Some(closure),
            _body: body,
        };
        target
            .add_event_listener_with_callback_and_add_event_listener_options(
                kind,
                listener
                    .closure
                    .as_ref()
                    .expect("BUG: live listener owns closure")
                    .as_ref()
                    .unchecked_ref(),
                &options,
            )
            .map_err(backend)?;
        Ok(listener)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let Some(closure) = self.closure.take() else {
            return;
        };
        if let Err(error) = self.target.remove_event_listener_with_callback_and_bool(
            self.kind,
            closure.as_ref().unchecked_ref(),
            self.capture,
        ) {
            // A host-replaced removal method can leave JS referring to this
            // function. Transfer the weak-only wrapper to wasm-bindgen's GC
            // lifetime instead of invalidating a still-referenced trampoline.
            let _ = closure.into_js_value();
            report(error);
        }
    }
}

struct ResizeSubscription {
    observer: web_sys::ResizeObserver,
    closure: Option<Closure<dyn Fn(JsValue)>>,
    _body: Rc<dyn Fn(Option<DeviceSize>)>,
}

impl Drop for ResizeSubscription {
    fn drop(&mut self) {
        if let Err(error) = disconnect_observer(&self.observer) {
            if let Some(closure) = self.closure.take() {
                let _ = closure.into_js_value();
            }
            report(error);
        }
    }
}

/// A registration-owned `setTimeout` slot: one pending deadline at a time,
/// cleared when re-armed and when the registration retires. Its body runs
/// only while the registration accepts, through a weak reference like a
/// listener's.
pub(super) struct DeadlineTimer {
    closure: Closure<dyn Fn()>,
    pending: Rc<Cell<Option<i32>>>,
    _body: Rc<dyn Fn()>,
}

impl DeadlineTimer {
    /// Run the body once `delay` has passed, replacing a pending run.
    pub(super) fn arm(&self, delay: std::time::Duration) {
        let Some(window) = web_sys::window() else {
            return;
        };
        if let Some(id) = self.pending.take() {
            window.clear_timeout_with_handle(id);
        }
        let millis = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
        match window.set_timeout_with_callback_and_timeout_and_arguments_0(
            self.closure.as_ref().unchecked_ref(),
            millis,
        ) {
            Ok(id) => self.pending.set(Some(id)),
            Err(error) => report(error),
        }
    }

    /// Clear the pending run, if any.
    pub(super) fn cancel(&self) {
        if let (Some(id), Some(window)) = (self.pending.take(), web_sys::window()) {
            window.clear_timeout_with_handle(id);
        }
    }
}

impl Drop for DeadlineTimer {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(crate) struct Registration {
    pub(super) accepting: Rc<Cell<bool>>,
    pub(super) pointers: Rc<RefCell<HashMap<i32, u16>>>,
    canvas: web_sys::HtmlCanvasElement,
    listeners: Vec<Listener>,
    observers: Vec<ResizeSubscription>,
    timers: Vec<Rc<DeadlineTimer>>,
    claim: CanvasClaim,
}

impl Registration {
    pub(super) fn new(canvas: &web_sys::HtmlCanvasElement, claim: CanvasClaim) -> Self {
        Self {
            accepting: Rc::new(Cell::new(true)),
            pointers: Rc::default(),
            canvas: canvas.clone(),
            listeners: Vec::new(),
            observers: Vec::new(),
            timers: Vec::new(),
            claim,
        }
    }

    /// A deadline timer whose body runs only while this registration
    /// accepts; retirement clears it.
    pub(super) fn timer(&mut self, body: impl Fn() + 'static) -> Rc<DeadlineTimer> {
        let body: Rc<dyn Fn()> = Rc::new(body);
        let weak = Rc::downgrade(&body);
        let accepting = Rc::clone(&self.accepting);
        let pending: Rc<Cell<Option<i32>>> = Rc::default();
        let fired = Rc::clone(&pending);
        let closure = Closure::<dyn Fn()>::new(move || {
            fired.set(None);
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body();
            }
        });
        let timer = Rc::new(DeadlineTimer {
            closure,
            pending,
            _body: body,
        });
        self.timers.push(Rc::clone(&timer));
        timer
    }

    pub(crate) fn listen(
        &mut self,
        target: &web_sys::EventTarget,
        kind: &'static str,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<(), OpenWindowError> {
        // A refused add drops its token at once, which attempts removal; the
        // installed prefix is removed when the registration rolls back.
        let listener = Listener::install(target, kind, &self.accepting, body)?;
        self.listeners.push(listener);
        Ok(())
    }

    /// Observe the canvas's size; `body` gets each report's device-pixel
    /// size where the browser gives one and it can be used. Each observer
    /// reports only a change of its own box, so three watch the canvas: the
    /// device-pixel content box where the browser has it (Chrome, Firefox),
    /// else the CSS content box (Safari); the CSS content box beside a
    /// device-pixel one, as padding inside a `border-box` canvas or a reflow
    /// can move it while the rounded device box stays; and the border box,
    /// as padding around a `content-box` canvas grows the window's padding
    /// box (`clientWidth`) and neither content box. Every entry carries the
    /// device-pixel size where the browser has it, so any report replaces the
    /// last. A browser without `ResizeObserver` is left to the window's
    /// `resize` subscription.
    pub(super) fn observe(
        &mut self,
        body: impl Fn(Option<DeviceSize>) + 'static,
    ) -> Result<(), OpenWindowError> {
        let body: Rc<dyn Fn(Option<DeviceSize>)> = Rc::new(body);
        let Some(device) = self.subscribe(&body) else {
            return Ok(());
        };
        let css_boxes: &[&str] = if observe_canvas(&device, &self.canvas).map_err(backend)? {
            &["content-box", "border-box"]
        } else {
            &["border-box"]
        };
        for &css_box in css_boxes {
            if let Some(observer) = self.subscribe(&body) {
                observe_box(&observer, &self.canvas, css_box).map_err(backend)?;
            }
        }
        Ok(())
    }

    /// A `ResizeObserver` that calls `body` with each report's device-pixel
    /// size while this registration accepts; `None` in a browser without one.
    fn subscribe(
        &mut self,
        body: &Rc<dyn Fn(Option<DeviceSize>)>,
    ) -> Option<web_sys::ResizeObserver> {
        let weak = Rc::downgrade(body);
        let accepting = Rc::clone(&self.accepting);
        let closure = Closure::<dyn Fn(JsValue)>::new(move |entries: JsValue| {
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body(match device_pixel_size(&entries)[..] {
                    [width, height] => Some(DeviceSize { width, height }),
                    _ => None,
                });
            }
        });
        let observer = match web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref()) {
            Ok(observer) => observer,
            Err(error) => {
                crate::owner::panic_boundary::contain_owner_callback(|| {
                    tracing::warn!(
                        ?error,
                        "ResizeObserver unavailable; using window resize subscription"
                    );
                });
                return None;
            }
        };
        self.observers.push(ResizeSubscription {
            observer: observer.clone(),
            closure: Some(closure),
            _body: Rc::clone(body),
        });
        Some(observer)
    }

    pub(super) fn fence(&self) {
        self.accepting.set(false);
    }

    pub(super) fn capture_owner(&self) -> PointerCaptureOwner {
        self.claim.capture_owner()
    }

    /// The canvas's claim, as the text input names its editor on it.
    pub(super) fn editor_slot(&self) -> EditorSlot {
        EditorSlot {
            canvas: self.claim.canvas.clone(),
            token: self.claim.token.clone(),
        }
    }
}

/// Where a canvas's owner names the text editor beside it, so the canvas's
/// focus area includes it.
#[derive(Clone)]
pub(crate) struct EditorSlot {
    canvas: web_sys::HtmlCanvasElement,
    token: JsValue,
}

impl EditorSlot {
    /// `editor` is now part of the canvas's focus area; `None` when none is.
    pub(crate) fn set(&self, editor: Option<&web_sys::HtmlElement>) {
        let editor = editor.map_or(JsValue::NULL, |editor| JsValue::from(editor.clone()));
        set_canvas_editor(&self.canvas, &self.token, &editor);
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.fence();
        let pointers = std::mem::take(&mut *self.pointers.borrow_mut());
        let owner = self.capture_owner();
        for &id in pointers.keys() {
            owner.forget(id);
        }
        // Admission ends before release, which can synchronously dispatch loss.
        for id in pointers.into_keys() {
            crate::owner::panic_boundary::contain_owner_callback(|| owner.release(id));
        }
        let observers = std::mem::take(&mut self.observers);
        let listeners = std::mem::take(&mut self.listeners);
        for timer in std::mem::take(&mut self.timers) {
            timer.cancel();
        }
        drop(observers);
        for listener in listeners {
            drop(listener);
        }
    }
}
