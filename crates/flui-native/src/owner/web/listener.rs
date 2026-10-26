//! `addEventListener`s that remove themselves when dropped: one for the
//! observers on the page and its screen (`display::web::events`,
//! `lifecycle::web`), and one whose body runs only while a canvas's
//! registration accepts, for what a window and the features beside it listen
//! for on and around the canvas.

use std::rc::Rc;

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Event, EventTarget};

use super::Accepting;
use crate::owner::OpenWindowError;

/// Log a failed browser call; a tracing subscriber is application code, so a
/// panic in it is contained.
pub(crate) fn warn_failure(what: &str, error: &JsValue) {
    crate::owner::panic_boundary::contain_owner_callback(|| {
        tracing::warn!(?error, "{what}");
    });
}

/// One registered `addEventListener`, removed when dropped.
pub(crate) struct Listener {
    target: EventTarget,
    event: &'static str,
    callback: Option<Closure<dyn FnMut(Event)>>,
}

impl Listener {
    pub(crate) fn add(
        target: &EventTarget,
        event: &'static str,
        handler: impl FnMut(Event) + 'static,
    ) -> Result<Self, JsValue> {
        let callback = Closure::new(handler);
        target.add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())?;
        Ok(Self {
            target: target.clone(),
            event,
            callback: Some(callback),
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        if let Err(error) = self
            .target
            .remove_event_listener_with_callback(self.event, callback.as_ref().unchecked_ref())
        {
            // Keep the JS trampoline valid if native removal failed; its
            // captures are inert once the observers are inactive.
            callback.forget();
            warn_failure("browser listener removal failed", &error);
        }
    }
}

/// A nonpassive `addEventListener` on or around a canvas whose body runs only
/// while the canvas's registration accepts; dropping it detaches it.
pub(crate) struct CanvasListener {
    target: EventTarget,
    kind: &'static str,
    capture: bool,
    closure: Option<Closure<dyn Fn(Event)>>,
    // The JS wrapper has only a Weak reference. Even a refused detach cannot
    // retain application captures or invoke the retired body.
    _body: Rc<dyn Fn(Event)>,
}

impl CanvasListener {
    /// A listener for `kind` on `target`, outside a registration's
    /// installation: one a window's text input attaches to its editor while
    /// the window lives. Its body runs only while `accepting` holds.
    ///
    /// # Errors
    /// What the browser reports when the listener cannot be added.
    pub(crate) fn install(
        target: &EventTarget,
        kind: &'static str,
        accepting: &Accepting,
        body: impl Fn(Event) + 'static,
    ) -> Result<Self, OpenWindowError> {
        Self::install_in_phase(target, kind, accepting, false, body)
    }

    /// [`Self::install`] in the capture phase when `capture` holds, which
    /// sees the events of descendants that do not bubble (`scroll`).
    ///
    /// # Errors
    /// What the browser reports when the listener cannot be added.
    pub(crate) fn install_in_phase(
        target: &EventTarget,
        kind: &'static str,
        accepting: &Accepting,
        capture: bool,
        body: impl Fn(Event) + 'static,
    ) -> Result<Self, OpenWindowError> {
        Self::install_body(target, kind, accepting, capture, Rc::new(body))
    }

    /// [`Self::install_in_phase`] for a body handed over behind its `Rc`, of
    /// which the listener keeps the only strong reference.
    ///
    /// # Errors
    /// What the browser reports when the listener cannot be added.
    pub(crate) fn install_body(
        target: &EventTarget,
        kind: &'static str,
        accepting: &Accepting,
        capture: bool,
        body: Rc<dyn Fn(Event)>,
    ) -> Result<Self, OpenWindowError> {
        debug_assert_eq!(
            Rc::strong_count(&body),
            1,
            "BUG: a listener keeps the only strong reference to its body"
        );
        let weak = Rc::downgrade(&body);
        let accepting = accepting.clone();
        let closure = Closure::<dyn Fn(Event)>::new(move |event| {
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
            .map_err(|error| OpenWindowError::Backend {
                message: format!("DOM registration failed: {error:?}"),
            })?;
        Ok(listener)
    }
}

impl Drop for CanvasListener {
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
            warn_failure(
                "DOM subscription retirement failed; delivery remains fenced",
                &error,
            );
        }
    }
}
