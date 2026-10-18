//! The browser events that change what the display snapshot reports.
//!
//! Every observer here only records that something changed and wakes the owner
//! through the shared [`OwnerSignal`]: the owner's next turn reads
//! `display_snapshot`, which samples the browser again. None of them runs
//! owner code, and none reads or writes frame state.
//!
//! - the page's pixel ratio, through a `(resolution: Xdppx)` media query that
//!   is armed again after each change ([`crate::shared::dppx_query`]);
//! - the tab becoming visible, after which the refresh period is measured
//!   again;
//! - Window Management access: when its permission is already granted the
//!   screen list and its changes are followed. The permission is only read,
//!   never requested.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Event, EventTarget, MediaQueryList};

use super::display::{WebDisplays, event_target, get};
use crate::shared::dppx_query::DppxWatch;
use crate::shared::owner_signal::OwnerSignal;
use crate::shared::web_feature::{GetScreenDetails, PermissionsQuery, WebFeature};

/// The listeners that keep the display snapshot current; dropping it removes
/// them.
pub(super) struct DisplayObservers {
    /// Cleared first on drop, so a callback already queued in the browser is
    /// inert.
    active: Rc<Cell<bool>>,
    _pixel_ratio: Option<PixelRatioWatch>,
    _visibility: Option<Listener>,
    _screens: Option<Rc<ScreenAccess>>,
}

impl DisplayObservers {
    /// Start observing for `displays`; every change wakes `signal`.
    pub(super) fn start(displays: &Arc<WebDisplays>, signal: &Arc<OwnerSignal>) -> Self {
        let active = Rc::new(Cell::new(true));
        let pixel_ratio = displays.features.match_media.map(|_| {
            PixelRatioWatch::start(Arc::downgrade(displays), Arc::downgrade(signal), &active)
        });
        let visibility = visibility_listener(displays, &active);
        let screens = displays
            .features
            .permissions
            .zip(displays.features.screen_details)
            .map(|(permissions, screen_details)| {
                ScreenAccess::start(
                    permissions,
                    screen_details,
                    Arc::downgrade(displays),
                    Arc::downgrade(signal),
                    &active,
                )
            });
        Self {
            active,
            _pixel_ratio: pixel_ratio.flatten(),
            _visibility: visibility,
            _screens: screens,
        }
    }
}

impl Drop for DisplayObservers {
    fn drop(&mut self) {
        self.active.set(false);
    }
}

fn wake(signal: &Weak<OwnerSignal>) {
    if let Some(signal) = signal.upgrade() {
        // A failed wake leaves the signal's retry debt for the next turn.
        let _ = signal.wake();
    }
}

fn log_failure(what: &str, error: &JsValue) {
    // A tracing subscriber is application code: contain it.
    crate::shared::panic_boundary::contain_owner_callback(|| {
        tracing::warn!(?error, "{what}");
    });
}

// ==================== One event listener ====================

/// One registered `addEventListener`, removed when dropped.
struct Listener {
    target: EventTarget,
    event: &'static str,
    callback: Option<Closure<dyn FnMut(Event)>>,
}

impl Listener {
    fn add(
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
            log_failure("browser display listener removal failed", &error);
        }
    }
}

// ==================== Visibility ====================

fn visibility_listener(displays: &Arc<WebDisplays>, active: &Rc<Cell<bool>>) -> Option<Listener> {
    let document = web_sys::window()?.document()?;
    let displays = Arc::downgrade(displays);
    let active = Rc::clone(active);
    let watched = document.clone();
    let registered = Listener::add(document.as_ref(), "visibilitychange", move |_| {
        if active.get()
            && !watched.hidden()
            && let Some(displays) = displays.upgrade()
        {
            // Frames did not run while the tab was hidden: the cadence is
            // measured again from the first one after it returns.
            displays.restart_estimate();
        }
    });
    match registered {
        Ok(listener) => Some(listener),
        Err(error) => {
            log_failure("visibility observation unavailable", &error);
            None
        }
    }
}

// ==================== Pixel ratio ====================

/// The `(resolution: Xdppx)` query for the ratio now in effect.
struct PixelRatioWatch {
    shared: Rc<PixelRatioShared>,
    /// The one JS function every query listens with; it is never replaced, so
    /// the callback that is running can arm its successor without freeing
    /// itself.
    callback: Option<Closure<dyn FnMut(Event)>>,
}

struct PixelRatioShared {
    rule: RefCell<DppxWatch>,
    query: RefCell<Option<MediaQueryList>>,
    function: RefCell<Option<js_sys::Function>>,
}

impl PixelRatioShared {
    /// Listen to the query for the ratio in effect now, instead of the spent
    /// one. `true` when the ratio differs from the one the spent query
    /// described.
    fn rearm(&self) -> bool {
        let Some(window) = web_sys::window() else {
            return false;
        };
        let Some(function) = self.function.borrow().clone() else {
            return false;
        };
        let rearm = self.rule.borrow_mut().arm(window.device_pixel_ratio());
        let spent = self.query.borrow_mut().take();
        if let Some(spent) = spent
            && let Err(error) = spent.remove_event_listener_with_callback("change", &function)
        {
            log_failure("pixel ratio query removal failed", &error);
        }
        if let Some(text) = rearm.query {
            match window.match_media(&text) {
                Ok(Some(query)) => match query
                    .add_event_listener_with_callback("change", &function)
                {
                    Ok(()) => *self.query.borrow_mut() = Some(query),
                    Err(error) => log_failure("pixel ratio query registration failed", &error),
                },
                Ok(None) => {}
                Err(error) => log_failure("pixel ratio query failed", &error),
            }
        }
        rearm.changed
    }
}

impl PixelRatioWatch {
    fn start(
        displays: Weak<WebDisplays>,
        signal: Weak<OwnerSignal>,
        active: &Rc<Cell<bool>>,
    ) -> Option<Self> {
        let shared = Rc::new(PixelRatioShared {
            rule: RefCell::new(DppxWatch::new()),
            query: RefCell::new(None),
            function: RefCell::new(None),
        });
        let handler_shared = Rc::clone(&shared);
        let active = Rc::clone(active);
        let callback = Closure::<dyn FnMut(Event)>::new(move |_| {
            if !active.get() {
                return;
            }
            let changed = handler_shared.rearm();
            if changed && let Some(displays) = displays.upgrade() {
                // A new scale is a new snapshot and a new cadence to measure.
                displays.invalidate();
                displays.restart_estimate();
                wake(&signal);
            }
        });
        *shared.function.borrow_mut() = Some(callback.as_ref().unchecked_ref::<js_sys::Function>().clone());
        shared.rearm();
        if shared.query.borrow().is_none() {
            // Nothing is armed: the browser cannot evaluate the query.
            return None;
        }
        Some(Self {
            shared,
            callback: Some(callback),
        })
    }
}

impl Drop for PixelRatioWatch {
    fn drop(&mut self) {
        let query = self.shared.query.borrow_mut().take();
        let function = self.shared.function.borrow_mut().take();
        let Some(callback) = self.callback.take() else {
            return;
        };
        if let (Some(query), Some(function)) = (query, function)
            && let Err(error) = query.remove_event_listener_with_callback("change", &function)
        {
            callback.forget();
            log_failure("pixel ratio query removal failed", &error);
        }
    }
}

// ==================== Window Management ====================

/// Follows the screen list while the Window Management permission is granted.
struct ScreenAccess {
    permissions: WebFeature<PermissionsQuery>,
    screen_details: WebFeature<GetScreenDetails>,
    displays: Weak<WebDisplays>,
    signal: Weak<OwnerSignal>,
    active: Rc<Cell<bool>>,
    /// The `PermissionStatus` and its `change` listener.
    permission: RefCell<Option<(JsValue, Listener)>>,
    /// The listeners on the `ScreenDetails` object.
    details: RefCell<Vec<Listener>>,
    /// Counts refreshes; an older one that finishes late discards its result.
    generation: Cell<u64>,
}

/// The permission names Chrome has used for Window Management, current first.
const PERMISSION_NAMES: [&str; 2] = ["window-management", "window-placement"];

impl ScreenAccess {
    fn start(
        permissions: WebFeature<PermissionsQuery>,
        screen_details: WebFeature<GetScreenDetails>,
        displays: Weak<WebDisplays>,
        signal: Weak<OwnerSignal>,
        active: &Rc<Cell<bool>>,
    ) -> Rc<Self> {
        let access = Rc::new(Self {
            permissions,
            screen_details,
            displays,
            signal,
            active: Rc::clone(active),
            permission: RefCell::new(None),
            details: RefCell::new(Vec::new()),
            generation: Cell::new(0),
        });
        Self::refresh(&access);
        access
    }

    /// Read the permission and bring the screen list in line with it.
    fn refresh(this: &Rc<Self>) {
        let generation = this.generation.get() + 1;
        this.generation.set(generation);
        let access = Rc::downgrade(this);
        wasm_bindgen_futures::spawn_local(async move {
            let Some(access) = access.upgrade() else {
                return;
            };
            access.refresh_async(&access, generation).await;
        });
    }

    fn current(&self, generation: u64) -> bool {
        self.active.get() && self.generation.get() == generation
    }

    async fn refresh_async(&self, this: &Rc<Self>, generation: u64) {
        if self.permission.borrow().is_none() {
            let Some(status) = self.query_permission().await else {
                return;
            };
            if !self.current(generation) {
                return;
            }
            self.watch_permission(this, status);
        }
        let granted = self
            .permission
            .borrow()
            .as_ref()
            .is_some_and(|(status, _)| permission_granted(status));
        if !granted {
            self.set_details(None);
            return;
        }
        if self.details.borrow().is_empty() {
            let Some(details) = self.get_screen_details().await else {
                return;
            };
            if !self.current(generation) {
                return;
            }
            self.watch_details(&details);
            self.set_details(Some(details));
        }
    }

    /// The `PermissionStatus` for Window Management, read without prompting.
    async fn query_permission(&self) -> Option<JsValue> {
        // Proof that `navigator.permissions.query` exists.
        let _ = self.permissions;
        let window = web_sys::window()?;
        let permissions = get(window.navigator().as_ref(), "permissions")?;
        let query: js_sys::Function = get(&permissions, "query")?.dyn_into().ok()?;
        for name in PERMISSION_NAMES {
            let descriptor = js_sys::Object::new();
            js_sys::Reflect::set(&descriptor, &"name".into(), &name.into()).ok()?;
            // An unknown name rejects; the older name is tried next.
            let Ok(promise) = query.call1(&permissions, &descriptor) else {
                continue;
            };
            let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
                continue;
            };
            if let Ok(status) = wasm_bindgen_futures::JsFuture::from(promise).await {
                return Some(status);
            }
        }
        None
    }

    /// `getScreenDetails()`. Called only after the permission read `granted`,
    /// when it does not prompt.
    async fn get_screen_details(&self) -> Option<JsValue> {
        // Proof that `getScreenDetails` exists.
        let _ = self.screen_details;
        let window = web_sys::window()?;
        let function: js_sys::Function = get(window.as_ref(), "getScreenDetails")?.dyn_into().ok()?;
        let promise: js_sys::Promise = function.call0(window.as_ref()).ok()?.dyn_into().ok()?;
        match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(details) => Some(details),
            Err(error) => {
                log_failure("screen details unavailable", &error);
                None
            }
        }
    }

    /// Re-read the permission whenever the user changes it.
    fn watch_permission(&self, this: &Rc<Self>, status: JsValue) {
        let Some(target) = event_target(&status) else {
            return;
        };
        let access = Rc::downgrade(this);
        let listener = Listener::add(&target, "change", move |_| {
            if let Some(access) = access.upgrade()
                && access.active.get()
            {
                Self::refresh(&access);
            }
        });
        match listener {
            Ok(listener) => *self.permission.borrow_mut() = Some((status, listener)),
            Err(error) => {
                // The state is still readable once; later changes are missed.
                *self.permission.borrow_mut() = None;
                log_failure("permission observation unavailable", &error);
            }
        }
    }

    /// Follow the screen list and the screen the window is on.
    fn watch_details(&self, details: &JsValue) {
        let Some(target) = event_target(details) else {
            return;
        };
        let mut listeners = Vec::new();
        for event in ["screenschange", "currentscreenchange"] {
            let displays = self.displays.clone();
            let signal = self.signal.clone();
            let active = Rc::clone(&self.active);
            let registered = Listener::add(&target, event, move |_| {
                if !active.get() {
                    return;
                }
                if let Some(displays) = displays.upgrade() {
                    if event == "currentscreenchange" {
                        // Another screen has another cadence to measure.
                        displays.forget_estimate();
                    } else {
                        displays.invalidate();
                    }
                    wake(&signal);
                }
            });
            match registered {
                Ok(listener) => listeners.push(listener),
                Err(error) => log_failure("screen change observation unavailable", &error),
            }
        }
        *self.details.borrow_mut() = listeners;
    }

    /// Install or drop the `ScreenDetails` object and wake the owner.
    fn set_details(&self, details: Option<JsValue>) {
        let Some(displays) = self.displays.upgrade() else {
            return;
        };
        let dropping = details.is_none();
        if dropping {
            let old = std::mem::take(&mut *self.details.borrow_mut());
            drop(old);
        }
        let previous = displays.set_details(details);
        let unchanged = dropping && previous.is_none();
        drop(previous);
        if !unchanged {
            wake(&self.signal);
        }
    }
}

fn permission_granted(status: &JsValue) -> bool {
    get(status, "state").and_then(|state| state.as_string()).as_deref() == Some("granted")
}
