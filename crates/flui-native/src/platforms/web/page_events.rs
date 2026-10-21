//! The browser events that are about the page, not about a window.
//!
//! `pagehide` is the last event a page is certain to get before the browser
//! may discard it: closing the tab, navigating away, or moving the page into
//! the back/forward cache. `beforeunload` and `unload` are not used for this —
//! either one makes a page ineligible for that cache, and mobile browsers often
//! never raise them. A page restored from that cache raises `pageshow` with
//! `persisted` set, and that undoes the ending.

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};
use wasm_bindgen::JsValue;

use super::listener::{Listener, warn_failure};
use super::platform::WebLoop;
use crate::host;

/// The listeners on the page itself; removed when dropped.
pub struct PageObservers {
    /// Cleared first on drop: a browser that refuses to remove a listener
    /// leaves it calling, and a late call must reach nothing.
    active: Rc<Cell<bool>>,
    _page_hide: Option<Listener>,
    _page_show: Option<Listener>,
}

impl PageObservers {
    /// Call `on_phase` with [`SessionEndPhase::Ending`] each time the page is
    /// hidden, and with [`SessionEndPhase::Cancelled`] when a page the browser
    /// kept in the back/forward cache is shown again. The first can repeat
    /// across such a round trip, so `on_phase` must be safe to repeat.
    pub(super) fn start(on_phase: impl Fn(SessionEndPhase) + 'static) -> Self {
        let active = Rc::new(Cell::new(true));
        let on_phase = Rc::new(on_phase);
        let Some(window) = web_sys::window() else {
            return Self {
                active,
                _page_hide: None,
                _page_show: None,
            };
        };
        let (hide, hide_active) = (Rc::clone(&on_phase), Rc::clone(&active));
        let page_hide = listen(&window, "pagehide", move |_| {
            if hide_active.get() {
                hide(SessionEndPhase::Ending);
            }
        });
        let show_active = Rc::clone(&active);
        let page_show = listen(&window, "pageshow", move |event| {
            // Only a page that was cached and is back; a first load also raises
            // `pageshow`, with `persisted` false.
            if show_active.get() && persisted(&event) {
                on_phase(SessionEndPhase::Cancelled);
            }
        });
        Self {
            active,
            _page_hide: page_hide,
            _page_show: page_show,
        }
    }
}

impl std::fmt::Debug for PageObservers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageObservers").finish_non_exhaustive()
    }
}

impl Drop for PageObservers {
    fn drop(&mut self) {
        self.active.set(false);
    }
}

impl host::Lifecycle<WebLoop> for PageObservers {
    fn on_session_end(
        owner: &WebLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        let previous = owner.with_state(|state| state.handlers.session_end.replace(callback));
        drop(previous);
    }

    fn on_host_notice(_: &WebLoop, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        // A page is told of none of sleep, wake, session lock or memory
        // pressure, so nothing calls the callback.
        drop(callback);
    }

    fn release_session_end(_: &WebLoop) {
        // A page cannot be asked to wait, so the hook's answer is never read
        // and no session end is held open.
    }

    fn on_reopen(_: &WebLoop, callback: Box<dyn FnMut() + Send>) {
        // Only AppKit reports a reopen.
        drop(callback);
    }

    fn activate(_: &WebLoop, _: bool) {
        // A page cannot bring its tab to the front.
    }
}

/// One listener on `window`; an unavailable one is logged and left out.
fn listen(
    window: &web_sys::Window,
    kind: &'static str,
    handler: impl FnMut(web_sys::Event) + 'static,
) -> Option<Listener> {
    Listener::add(window.as_ref(), kind, handler)
        .inspect_err(|error| warn_failure("page lifecycle observation unavailable", error))
        .ok()
}

/// `PageTransitionEvent.persisted`; absent on any other event.
fn persisted(event: &web_sys::Event) -> bool {
    js_sys::Reflect::get(event, &JsValue::from_str("persisted"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}
