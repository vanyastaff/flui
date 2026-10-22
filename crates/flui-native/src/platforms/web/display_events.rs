//! The browser events that change what the display snapshot reports.
//!
//! Every observer here only records that something changed and wakes the owner
//! through the shared [`OwnerSignal`]: the owner's next turn reads
//! `display_snapshot`, which samples the browser again. None of them runs
//! owner code, and none reads or writes frame state. The one exception is a
//! window's own pixel-ratio watch ([`PixelRatioWatch::for_layout`]), which
//! brings that window's backing store to the new ratio and runs its callbacks.
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
use web_sys::{Event, MediaQueryList};

use super::display::{WebDisplays, event_target, get};
use crate::owner::signal::OwnerSignal;
use crate::owner::web::listener::{Listener, warn_failure};
use crate::shared::dppx_query::DppxWatch;
use crate::shared::web_feature::{GetScreenDetails, MatchMedia, PermissionsQuery, WebFeature};

/// The listeners that keep the display snapshot current; dropping it removes
/// them.
pub(crate) struct DisplayObservers {
    /// Cleared first on drop, so a callback already queued in the browser is
    /// inert.
    active: Rc<Cell<bool>>,
    _pixel_ratio: Option<PixelRatioWatch>,
    _visibility: Option<Listener>,
    _screen_change: Option<Listener>,
    _resize: Option<Listener>,
    _screens: Option<Rc<ScreenAccess>>,
}

impl DisplayObservers {
    /// Start observing for `displays`; every change wakes `signal`.
    pub(crate) fn start(displays: &Arc<WebDisplays>, signal: &Arc<OwnerSignal>) -> Self {
        let active = Rc::new(Cell::new(true));
        let pixel_ratio = displays.features.match_media.map(|proof| {
            let displays = Arc::downgrade(displays);
            let signal = Arc::downgrade(signal);
            // A new scale is a new snapshot and a new cadence to measure.
            PixelRatioWatch::start(proof, &active, move || {
                if let Some(displays) = displays.upgrade() {
                    displays.invalidate();
                    displays.restart_estimate();
                    wake(&signal);
                }
            })
        });
        let visibility = visibility_listener(displays, &active);
        let ratio = pixel_ratio.as_ref().map(PixelRatioWatch::handle);
        let resize = ratio
            .clone()
            .and_then(|ratio| resize_listener(ratio, &active));
        let screen_change = screen_change_listener(displays, signal, &active, ratio);
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
            _pixel_ratio: pixel_ratio,
            _visibility: visibility,
            _screen_change: screen_change,
            _resize: resize,
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
            warn_failure("visibility observation unavailable", &error);
            None
        }
    }
}

// ==================== The screen object ====================

/// `window.screen` raises `change` when its size, pixel ratio or position
/// changes, without any permission, where the browser has the event.
fn screen_change_listener(
    displays: &Arc<WebDisplays>,
    signal: &Arc<OwnerSignal>,
    active: &Rc<Cell<bool>>,
    ratio: Option<std::rc::Weak<PixelRatioShared>>,
) -> Option<Listener> {
    let target = displays
        .features
        .screen_change?
        .target(&web_sys::window()?)?;
    let displays = Arc::downgrade(displays);
    let signal = Arc::downgrade(signal);
    let active = Rc::clone(active);
    match Listener::add(&target, "change", move |_| {
        if active.get()
            && let Some(displays) = displays.upgrade()
        {
            displays.screen_changed();
            // A scale change that arrived while no query was armed shows up
            // with the screen's: arm now rather than at the next retry.
            if let Some(ratio) = ratio.as_ref().and_then(std::rc::Weak::upgrade) {
                ratio.arm_if_unarmed();
            }
            wake(&signal);
        }
    }) {
        Ok(listener) => Some(listener),
        Err(error) => {
            warn_failure("screen change observation unavailable", &error);
            None
        }
    }
}

/// Page zoom fires `resize` on the window: a pixel-ratio watch that is not
/// armed arms itself from it.
fn resize_listener(
    ratio: std::rc::Weak<PixelRatioShared>,
    active: &Rc<Cell<bool>>,
) -> Option<Listener> {
    let window = web_sys::window()?;
    let active = Rc::clone(active);
    Listener::add(window.as_ref(), "resize", move |_| {
        if active.get()
            && let Some(ratio) = ratio.upgrade()
        {
            ratio.arm_if_unarmed();
        }
    })
    .inspect_err(|error| warn_failure("resize observation unavailable", error))
    .ok()
}

// ==================== Pixel ratio ====================

/// A closure called with an animation frame timestamp.
type FrameCallback = Closure<dyn FnMut(f64)>;

/// The `(resolution: Xdppx)` query for the ratio now in effect.
pub(super) struct PixelRatioWatch {
    shared: Rc<PixelRatioShared>,
    /// The one JS function every query listens with; it is never replaced, so
    /// the callback that is running can arm its successor without freeing
    /// itself.
    callback: Option<Closure<dyn FnMut(Event)>>,
}

struct PixelRatioShared {
    proof: WebFeature<MatchMedia>,
    rule: RefCell<DppxWatch>,
    query: RefCell<Option<MediaQueryList>>,
    function: RefCell<Option<js_sys::Function>>,
    /// What a changed ratio means to whoever watches it.
    on_change: Box<dyn Fn()>,
    active: Rc<Cell<bool>>,
    /// The callback that arms again on a frame after an arming that failed; it
    /// is owned here and never replaced while a frame may call it.
    retry: RefCell<Option<FrameCallback>>,
    /// The pending animation frame of that callback.
    retry_frame: Cell<Option<i32>>,
    /// Retries since the last successful arming; each waits longer than the
    /// one before, up to [`MAX_RETRY_WAIT`] frames, and they never stop: a
    /// watch that is not armed misses every zoom and scale change.
    retries: Cell<u32>,
    /// Frames the pending retry callback still lets pass before it tries.
    wait: Cell<u32>,
}

/// How many times arming reads the ratio again when it changed between the
/// read and the query listening: a query that does not match the ratio now
/// would never fire for a change that already happened.
const ARM_ATTEMPTS: usize = 4;

/// The longest wait, in frames, between two attempts to arm after the browser
/// refused the query.
const MAX_RETRY_WAIT: u32 = 120;

impl PixelRatioShared {
    /// Listen to the query for the ratio in effect now, instead of the spent
    /// one. `true` when the ratio differs from the one the spent query
    /// described, or when no query could be armed (a change may have gone
    /// unseen).
    fn rearm(&self) -> bool {
        let Some(window) = web_sys::window() else {
            return false;
        };
        let Some(function) = self.function.borrow().clone() else {
            return false;
        };
        let spent = self.query.borrow_mut().take();
        if let Some(spent) = spent
            && let Err(error) = spent.remove_event_listener_with_callback("change", &function)
        {
            warn_failure("pixel ratio query removal failed", &error);
        }
        let mut changed = false;
        for _ in 0..ARM_ATTEMPTS {
            let rearm = self.rule.borrow_mut().arm(window.device_pixel_ratio());
            changed |= rearm.changed;
            let Some(text) = rearm.query else { break };
            let query = match self.proof.query(&window, &text) {
                Ok(Some(query)) => query,
                Ok(None) => break,
                Err(error) => {
                    warn_failure("pixel ratio query failed", &error);
                    break;
                }
            };
            if let Err(error) = query.add_event_listener_with_callback("change", &function) {
                warn_failure("pixel ratio query registration failed", &error);
                break;
            }
            if query.matches() {
                *self.query.borrow_mut() = Some(query);
                self.retries.set(0);
                return changed;
            }
            // The ratio moved between reading it and listening: this query
            // describes a ratio that is already gone. Read it again.
            changed = true;
            if let Err(error) = query.remove_event_listener_with_callback("change", &function) {
                warn_failure("pixel ratio query removal failed", &error);
            }
        }
        self.retry_on_next_frame(&window);
        true
    }

    /// Arm again after a growing wait (one frame, then two, four, and so on up
    /// to [`MAX_RETRY_WAIT`]), for as long as the browser refuses. A retry is
    /// already pending: nothing changes.
    fn retry_on_next_frame(&self, window: &web_sys::Window) {
        if !self.active.get() || self.retry_frame.get().is_some() {
            return;
        }
        let retries = self.retries.get();
        self.retries.set(retries.saturating_add(1));
        let delay = (1_u32 << retries.min(7)).min(MAX_RETRY_WAIT);
        self.wait.set(delay - 1);
        self.request_retry_frame(window);
    }

    fn request_retry_frame(&self, window: &web_sys::Window) {
        let retry = self.retry.borrow();
        let Some(retry) = retry.as_ref() else { return };
        match window.request_animation_frame(retry.as_ref().unchecked_ref()) {
            Ok(frame) => self.retry_frame.set(Some(frame)),
            Err(error) => warn_failure("pixel ratio retry unavailable", &error),
        }
    }

    /// A zoom or a screen change that is already known to the page: arm now if
    /// the watch is not armed, without waiting for a retry frame.
    fn arm_if_unarmed(&self) {
        if self.active.get() && self.query.borrow().is_none() && self.rearm() {
            self.announce_change();
        }
    }

    fn announce_change(&self) {
        (self.on_change)();
    }
}

impl PixelRatioWatch {
    /// Watch the ratio for a window's layout: `on_change` runs each time it
    /// changes while `active` holds. A window dragged to a display of another
    /// scale keeps its CSS box, so neither a `ResizeObserver` of that box nor
    /// the window's `resize` event reports it, and one of its device-pixel
    /// box misses it while the rounded size stays. `None` in a browser
    /// without media queries.
    pub(super) fn for_layout(
        active: &Rc<Cell<bool>>,
        on_change: impl Fn() + 'static,
    ) -> Option<Self> {
        let proof = super::features::BrowserFeatures::detect().match_media?;
        Some(Self::start(proof, active, on_change))
    }

    /// Arm now if no query is armed (the browser refused the last one), and
    /// report a change that went unseen meanwhile.
    pub(super) fn arm_if_unarmed(&self) {
        self.shared.arm_if_unarmed();
    }

    fn handle(&self) -> std::rc::Weak<PixelRatioShared> {
        Rc::downgrade(&self.shared)
    }

    fn start(
        proof: WebFeature<MatchMedia>,
        active: &Rc<Cell<bool>>,
        on_change: impl Fn() + 'static,
    ) -> Self {
        let shared = Rc::new(PixelRatioShared {
            proof,
            rule: RefCell::new(DppxWatch::new()),
            query: RefCell::new(None),
            function: RefCell::new(None),
            on_change: Box::new(on_change),
            active: Rc::clone(active),
            retry: RefCell::new(None),
            retry_frame: Cell::new(None),
            retries: Cell::new(0),
            wait: Cell::new(0),
        });
        let weak = Rc::downgrade(&shared);
        let callback = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Some(shared) = weak.upgrade()
                && shared.active.get()
                && shared.rearm()
            {
                shared.announce_change();
            }
        });
        *shared.function.borrow_mut() = Some(
            callback
                .as_ref()
                .unchecked_ref::<js_sys::Function>()
                .clone(),
        );
        let weak = Rc::downgrade(&shared);
        *shared.retry.borrow_mut() = Some(Closure::new(move |_: f64| {
            let Some(shared) = weak.upgrade() else { return };
            shared.retry_frame.set(None);
            if !shared.active.get() || shared.query.borrow().is_some() {
                return;
            }
            let waiting = shared.wait.get();
            if waiting > 0 {
                shared.wait.set(waiting - 1);
                if let Some(window) = web_sys::window() {
                    shared.request_retry_frame(&window);
                }
                return;
            }
            shared.arm_if_unarmed();
        }));
        shared.rearm();
        Self {
            shared,
            callback: Some(callback),
        }
    }
}

impl Drop for PixelRatioWatch {
    fn drop(&mut self) {
        if let Some(frame) = self.shared.retry_frame.take()
            && let Some(window) = web_sys::window()
        {
            window.cancel_animation_frame(frame).ok();
        }
        let query = self.shared.query.borrow_mut().take();
        let function = self.shared.function.borrow_mut().take();
        let Some(callback) = self.callback.take() else {
            return;
        };
        if let (Some(query), Some(function)) = (query, function)
            && let Err(error) = query.remove_event_listener_with_callback("change", &function)
        {
            callback.forget();
            warn_failure("pixel ratio query removal failed", &error);
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
    /// The `PermissionStatus` and its `change` listener, when one could be
    /// registered; without it the state is still read, but a later change is
    /// missed.
    permission: RefCell<Option<(JsValue, Option<Listener>)>>,
    /// The `ScreenDetails` object being followed and the screen it named as
    /// current when last looked at.
    followed: RefCell<Option<Followed>>,
    /// The listeners on the `ScreenDetails` object.
    details: RefCell<Vec<Listener>>,
    /// The listeners on each listed screen: its size or pixel ratio changed.
    screens: RefCell<Vec<Listener>>,
    /// Counts refreshes; an older one that finishes late discards its result.
    generation: Cell<u64>,
}

/// The permission names Chrome has used for Window Management, current first.
const PERMISSION_NAMES: [&str; 2] = ["window-management", "window-placement"];

/// A `ScreenDetails` object and the `currentScreen` it last named.
struct Followed {
    details: JsValue,
    current: Option<JsValue>,
}

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
            followed: RefCell::new(None),
            details: RefCell::new(Vec::new()),
            screens: RefCell::new(Vec::new()),
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
        if self.followed.borrow().is_none() {
            let Some(details) = self.get_screen_details().await else {
                return;
            };
            if !self.current(generation) {
                return;
            }
            self.watch_details(this, &details);
            self.set_details(Some(details));
        }
    }

    /// The `PermissionStatus` for Window Management, read without prompting.
    async fn query_permission(&self) -> Option<JsValue> {
        let window = web_sys::window()?;
        for name in PERMISSION_NAMES {
            // An unknown name throws or rejects; the older name is tried next.
            let Some(promise) = self.permissions.query(&window, name) else {
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
        let window = web_sys::window()?;
        let promise = self.screen_details.request(&window)?;
        match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(details) => Some(details),
            Err(error) => {
                warn_failure("screen details unavailable", &error);
                None
            }
        }
    }

    /// Re-read the permission whenever the user changes it.
    fn watch_permission(&self, this: &Rc<Self>, status: JsValue) {
        let access = Rc::downgrade(this);
        let listener = event_target(&status).and_then(|target| {
            let listener = Listener::add(&target, "change", move |_| {
                if let Some(access) = access.upgrade()
                    && access.active.get()
                {
                    Self::refresh(&access);
                }
            });
            // Without the listener the state is still readable; a later
            // change is missed.
            listener
                .inspect_err(|error| warn_failure("permission observation unavailable", error))
                .ok()
        });
        *self.permission.borrow_mut() = Some((status, listener));
    }

    /// Follow the screen list and the screen the window is on.
    fn watch_details(&self, this: &Rc<Self>, details: &JsValue) {
        *self.followed.borrow_mut() = Some(Followed {
            details: details.clone(),
            current: get(details, "currentScreen"),
        });
        self.watch_screens();
        let Some(target) = event_target(details) else {
            return;
        };
        let mut listeners = Vec::new();
        for event in ["screenschange", "currentscreenchange"] {
            let access = Rc::downgrade(this);
            let registered = Listener::add(&target, event, move |_| {
                if let Some(access) = access.upgrade()
                    && access.active.get()
                {
                    if event == "screenschange" {
                        access.screens_changed();
                    } else {
                        access.current_screen_changed();
                    }
                }
            });
            match registered {
                Ok(listener) => listeners.push(listener),
                Err(error) => warn_failure("screen change observation unavailable", &error),
            }
        }
        *self.details.borrow_mut() = listeners;
    }

    /// Listen to `change` on every listed screen: one that changes size or
    /// pixel ratio without the list changing is otherwise never seen.
    fn watch_screens(&self) {
        let listed = self
            .followed
            .borrow()
            .as_ref()
            .and_then(|followed| get(&followed.details, "screens"))
            .map(|screens| js_sys::Array::from(&screens).to_vec())
            .unwrap_or_default();
        let listeners: Vec<Listener> = listed
            .iter()
            .filter_map(event_target)
            .filter_map(|target| {
                let displays = self.displays.clone();
                let signal = self.signal.clone();
                let active = Rc::clone(&self.active);
                Listener::add(&target, "change", move |_| {
                    if active.get()
                        && let Some(displays) = displays.upgrade()
                    {
                        displays.invalidate();
                        wake(&signal);
                    }
                })
                .inspect_err(|error| warn_failure("screen observation unavailable", error))
                .ok()
            })
            .collect();
        // Replacing drops the old listeners outside the borrow.
        let old = std::mem::replace(&mut *self.screens.borrow_mut(), listeners);
        drop(old);
    }

    /// The list of screens changed: follow the new ones and sample again.
    fn screens_changed(&self) {
        self.watch_screens();
        if let Some(displays) = self.displays.upgrade() {
            displays.invalidate();
            wake(&self.signal);
        }
    }

    /// `currentscreenchange`: the window's screen changed only if the object
    /// `currentScreen` names is another one; otherwise the same screen's
    /// metrics changed and the cadence it was measured at still holds.
    fn current_screen_changed(&self) {
        let moved = {
            let mut followed = self.followed.borrow_mut();
            let Some(followed) = followed.as_mut() else {
                return;
            };
            let now = get(&followed.details, "currentScreen");
            let moved = match (&followed.current, &now) {
                (Some(before), Some(now)) => !js_sys::Object::is(before, now),
                (None, None) => false,
                _ => true,
            };
            followed.current = now;
            moved
        };
        if let Some(displays) = self.displays.upgrade() {
            if moved {
                // Another screen has another cadence to measure.
                displays.forget_estimate();
            } else {
                displays.invalidate();
            }
            wake(&self.signal);
        }
    }

    /// Install or drop the `ScreenDetails` object and wake the owner.
    fn set_details(&self, details: Option<JsValue>) {
        let Some(displays) = self.displays.upgrade() else {
            return;
        };
        let dropping = details.is_none();
        if dropping {
            let old = (
                std::mem::take(&mut *self.details.borrow_mut()),
                std::mem::take(&mut *self.screens.borrow_mut()),
                self.followed.borrow_mut().take(),
            );
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
    get(status, "state")
        .and_then(|state| state.as_string())
        .as_deref()
        == Some("granted")
}
