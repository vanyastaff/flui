//! One `addEventListener` that removes itself when dropped, for the observers
//! on the page and its screen (`display::web::events`, `platforms::web::page_events`).

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Event, EventTarget};

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
