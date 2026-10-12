//! Window-owned DOM subscriptions. Only the exact canvas may have a live owner.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use wasm_bindgen::{JsCast, prelude::*};

use crate::traits::OpenWindowError;

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
    return !!canvas.ownerDocument?.hasFocus() && canvas.getRootNode().activeElement === canvas;
}
export function observeCanvas(observer, canvas) { observer.observe(canvas); }
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
    pub(super) fn effective_focus(canvas: &web_sys::HtmlCanvasElement) -> bool;
    #[wasm_bindgen(catch, js_name = observeCanvas)]
    fn observe_canvas(
        observer: &web_sys::ResizeObserver,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<(), JsValue>;
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
    crate::shared::panic_boundary::contain_owner_callback(|| {
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

struct Listener {
    target: web_sys::EventTarget,
    kind: &'static str,
    closure: Option<Closure<dyn Fn(web_sys::Event)>>,
    // The JS wrapper has only a Weak reference. Even a refused detach cannot
    // retain application captures or invoke the retired body.
    _body: Rc<dyn Fn(web_sys::Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let Some(closure) = self.closure.take() else {
            return;
        };
        if let Err(error) = self
            .target
            .remove_event_listener_with_callback(self.kind, closure.as_ref().unchecked_ref())
        {
            // A host-replaced removal method can leave JS referring to this
            // function. Transfer the weak-only wrapper to wasm-bindgen's GC
            // lifetime instead of invalidating a still-referenced trampoline.
            let _ = closure.into_js_value();
            report(error);
        }
    }
}

struct Observer {
    observer: web_sys::ResizeObserver,
    closure: Option<Closure<dyn Fn()>>,
    _body: Rc<dyn Fn()>,
}

impl Drop for Observer {
    fn drop(&mut self) {
        if let Err(error) = disconnect_observer(&self.observer) {
            if let Some(closure) = self.closure.take() {
                let _ = closure.into_js_value();
            }
            report(error);
        }
    }
}

pub(super) struct Registration {
    pub(super) accepting: Rc<Cell<bool>>,
    pub(super) pointers: Rc<RefCell<HashMap<i32, u16>>>,
    canvas: web_sys::HtmlCanvasElement,
    listeners: Vec<Listener>,
    observer: Option<Observer>,
    claim: CanvasClaim,
}

impl Registration {
    pub(super) fn new(canvas: &web_sys::HtmlCanvasElement, claim: CanvasClaim) -> Self {
        Self {
            accepting: Rc::new(Cell::new(true)),
            pointers: Rc::default(),
            canvas: canvas.clone(),
            listeners: Vec::new(),
            observer: None,
            claim,
        }
    }

    pub(super) fn listen(
        &mut self,
        target: &web_sys::EventTarget,
        kind: &'static str,
        body: impl Fn(web_sys::Event) + 'static,
    ) -> Result<(), OpenWindowError> {
        let body: Rc<dyn Fn(web_sys::Event)> = Rc::new(body);
        let weak = Rc::downgrade(&body);
        let accepting = Rc::clone(&self.accepting);
        let closure = Closure::<dyn Fn(web_sys::Event)>::new(move |event| {
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body(event);
            }
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(false);
        // Own the token before calling the host: a replaced add method might
        // install a listener and then throw. Rollback still attempts removal.
        self.listeners.push(Listener {
            target: target.clone(),
            kind,
            closure: Some(closure),
            _body: body,
        });
        let listener = self
            .listeners
            .last()
            .expect("BUG: listener token just inserted");
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
            .map_err(backend)
    }

    pub(super) fn observe(&mut self, body: impl Fn() + 'static) -> Result<(), OpenWindowError> {
        let body: Rc<dyn Fn()> = Rc::new(body);
        let weak = Rc::downgrade(&body);
        let accepting = Rc::clone(&self.accepting);
        let closure = Closure::<dyn Fn()>::new(move || {
            if accepting.get()
                && let Some(body) = weak.upgrade()
            {
                body();
            }
        });
        let observer = match web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref()) {
            Ok(observer) => observer,
            Err(error) => {
                crate::shared::panic_boundary::contain_owner_callback(|| {
                    tracing::warn!(
                        ?error,
                        "ResizeObserver unavailable; using window resize subscription"
                    )
                });
                return Ok(());
            }
        };
        self.observer = Some(Observer {
            observer: observer.clone(),
            closure: Some(closure),
            _body: body,
        });
        observe_canvas(&observer, &self.canvas).map_err(backend)
    }

    pub(super) fn fence(&self) {
        self.accepting.set(false);
    }

    pub(super) fn capture_owner(&self) -> PointerCaptureOwner {
        self.claim.capture_owner()
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
            crate::shared::panic_boundary::contain_owner_callback(|| owner.release(id));
        }
        let observer = self.observer.take();
        let listeners = std::mem::take(&mut self.listeners);
        drop(observer);
        for listener in listeners {
            drop(listener);
        }
    }
}
