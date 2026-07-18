//! Platform callback handlers
//!
//! Registry pattern for platform callbacks, allowing the framework to register
//! event handlers without tight coupling to platform implementations.
//!
//! Two levels of callbacks:
//! - [`PlatformHandlers`]: Global platform-level callbacks (quit, reopen, etc.)
//! - [`WindowCallbacks`]: Per-window callbacks (input, resize, close, etc.)

use std::collections::VecDeque;

use flui_types::geometry::{Pixels, Size};
use parking_lot::Mutex;

use crate::traits::{DispatchEventResult, PlatformInput, WindowEvent};

/// Platform callback handlers registry
///
/// This struct stores all registered callbacks from the framework.
/// Platform implementations invoke these callbacks when events occur.
///
/// # Design Pattern
///
/// This is the callback registry pattern from GPUI - it decouples the framework
/// from platform implementations. The framework registers handlers, and the
/// platform invokes them at appropriate times.
///
/// # Thread Safety
///
/// All callbacks are `Send` but not `Sync`, as they're typically invoked from
/// the main thread only.
pub struct PlatformHandlers {
    /// Called when the application should quit
    pub quit: Option<Box<dyn FnMut() + Send>>,

    /// Called when the application is reopened (macOS dock click)
    pub reopen: Option<Box<dyn FnMut() + Send>>,

    /// Called when a window event occurs
    pub window_event: Option<Box<dyn FnMut(WindowEvent) + Send>>,

    /// Called when URLs are opened (e.g., from file manager, browser)
    pub open_urls: Option<Box<dyn FnMut(Vec<String>) + Send>>,

    /// Called when keyboard layout changes
    pub keyboard_layout_changed: Option<Box<dyn FnMut() + Send>>,
}

impl PlatformHandlers {
    /// Create new empty handler registry
    pub fn new() -> Self {
        Self {
            quit: None,
            reopen: None,
            window_event: None,
            open_urls: None,
            keyboard_layout_changed: None,
        }
    }

    /// Invoke the quit callback if registered
    #[inline]
    pub fn invoke_quit(&mut self) {
        if let Some(ref mut handler) = self.quit {
            handler();
        }
    }

    /// Invoke the reopen callback if registered
    #[inline]
    pub fn invoke_reopen(&mut self) {
        if let Some(ref mut handler) = self.reopen {
            handler();
        }
    }

    /// Invoke the window event callback if registered
    #[inline]
    pub fn invoke_window_event(&mut self, event: WindowEvent) {
        if let Some(ref mut handler) = self.window_event {
            handler(event);
        }
    }

    /// Invoke the open URLs callback if registered
    #[inline]
    pub fn invoke_open_urls(&mut self, urls: Vec<String>) {
        if let Some(ref mut handler) = self.open_urls {
            handler(urls);
        }
    }

    /// Invoke the keyboard layout changed callback if registered
    #[inline]
    pub fn invoke_keyboard_layout_changed(&mut self) {
        if let Some(ref mut handler) = self.keyboard_layout_changed {
            handler();
        }
    }
}

impl Default for PlatformHandlers {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PlatformHandlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformHandlers")
            .field("quit", &self.quit.is_some())
            .field("reopen", &self.reopen.is_some())
            .field("window_event", &self.window_event.is_some())
            .field("open_urls", &self.open_urls.is_some())
            .field(
                "keyboard_layout_changed",
                &self.keyboard_layout_changed.is_some(),
            )
            .finish()
    }
}

// ============================================================================
// Per-Window Callbacks
// ============================================================================

#[allow(clippy::type_complexity)]
/// Per-window callback storage with one causal reentry queue.
///
/// Each callback is stored in a `Mutex<Option<Box<dyn FnMut/FnOnce + Send>>>`.
/// The dispatch pattern ensures reentrancy safety and ordering:
/// 1. enqueue the typed event in the window FIFO;
/// 2. one caller becomes the drain owner;
/// 3. take callback → unlock → call → restore;
/// 4. drain all nested window events in causal order, including transitions
///    between kinds such as input → resize → frame.
///
/// This prevents deadlocks when a callback tries to interact with the window
/// (which would require the same lock if stored differently).
pub struct WindowCallbacks {
    /// Called when an input event (pointer, keyboard) is delivered to this
    /// window. Returns `DispatchEventResult` indicating if the event was
    /// consumed.
    pub on_input: Mutex<Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the platform requests a new frame to be rendered.
    pub on_request_frame: Mutex<Option<Box<dyn FnMut() + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the window is resized. Parameters: new size (logical), scale
    /// factor.
    pub on_resize: Mutex<Option<Box<dyn FnMut(Size<Pixels>, f32) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the window is moved.
    pub on_moved: Mutex<Option<Box<dyn FnMut() + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the window is about to be destroyed. Only fires once
    /// (FnOnce).
    pub on_close: Mutex<Option<Box<dyn FnOnce() + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called to ask if the window should close. Return `false` to veto.
    pub on_should_close: Mutex<Option<Box<dyn FnMut() -> bool + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the window gains or loses focus. Parameter: is_active.
    pub on_active_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the window's visibility (occlusion) changes. Parameter:
    /// is_visible (`true` when the window becomes visible/unoccluded).
    ///
    /// Distinct from `on_active_status_change`: a window can be visible but
    /// unfocused (Flutter's `AppLifecycleState::Inactive`), or focused but
    /// not visible (unusual, but not excluded). Feeds the `AppLifecycleState`
    /// derivation `ADR-0035` documents; winit's `WindowEvent::Occluded`
    /// drives it on desktop. Wayland compositors deliver occlusion via the
    /// xdg-shell v6 `suspended` state, a compositor-conditional extension;
    /// where a compositor never sends it, this callback simply never fires
    /// — the window is treated as always visible (the same behavior as
    /// before this callback existed).
    pub on_visibility_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the mouse enters or leaves the window. Parameter:
    /// is_hovered.
    pub on_hover_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the system appearance (light/dark) changes.
    pub on_appearance_changed: Mutex<Option<Box<dyn FnMut() + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    event_dispatch: Mutex<DispatchState<WindowCallbackEvent>>,
    should_close_dispatching: Mutex<bool>,
}

enum WindowCallbackEvent {
    Input(PlatformInput),
    RequestFrame,
    Resize(Size<Pixels>, f32),
    Moved,
    Close,
    Active(bool),
    Visibility(bool),
    Hover(bool),
    AppearanceChanged,
}

struct DispatchState<E> {
    pending: VecDeque<E>,
    dispatching: bool,
}

impl<E> DispatchState<E> {
    const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            dispatching: false,
        }
    }
}

/// Owns one FIFO drain. On normal exhaustion it clears `dispatching` under
/// the queue lock, closing the enqueue-vs-finish race. On unwind, `Drop`
/// clears it and discards nested work from the aborted callback transaction.
struct DispatchDrain<'a, E> {
    state: &'a Mutex<DispatchState<E>>,
    active: bool,
}

impl<'a, E> DispatchDrain<'a, E> {
    fn begin(state: &'a Mutex<DispatchState<E>>, event: E) -> Option<Self> {
        let mut guard = state.lock();
        guard.pending.push_back(event);
        if guard.dispatching {
            return None;
        }
        guard.dispatching = true;
        Some(Self {
            state,
            active: true,
        })
    }

    fn next(&mut self) -> Option<E> {
        let mut state = self.state.lock();
        if let Some(event) = state.pending.pop_front() {
            return Some(event);
        }
        state.dispatching = false;
        self.active = false;
        None
    }
}

impl<E> Drop for DispatchDrain<'_, E> {
    fn drop(&mut self) {
        if self.active {
            let pending = {
                let mut state = self.state.lock();
                state.dispatching = false;
                // The causal parent callback aborted, so its nested events no
                // longer have a valid completion point. Discard that aborted
                // transaction instead of making the next unrelated caller
                // receive a previous event's return value. Drop payloads only
                // after releasing the mutex in case their destructors re-enter.
                std::mem::take(&mut state.pending)
            };
            drop(pending);
        }
    }
}

struct BooleanDispatchGuard<'a> {
    dispatching: &'a Mutex<bool>,
}

impl Drop for BooleanDispatchGuard<'_> {
    fn drop(&mut self) {
        *self.dispatching.lock() = false;
    }
}

/// Temporarily removes one `FnMut` callback without holding its mutex while
/// user code runs, then restores it even if that code unwinds.
struct CallbackLease<'a, T> {
    slot: &'a Mutex<Option<T>>,
    callback: Option<T>,
}

impl<'a, T> CallbackLease<'a, T> {
    fn take(slot: &'a Mutex<Option<T>>) -> Self {
        let callback = slot.lock().take();
        Self { slot, callback }
    }

    fn callback_mut(&mut self) -> Option<&mut T> {
        self.callback.as_mut()
    }
}

impl<T> Drop for CallbackLease<'_, T> {
    fn drop(&mut self) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        let mut slot = self.slot.lock();
        if slot.is_none() {
            *slot = Some(callback);
        }
    }
}

impl WindowCallbacks {
    /// Create a new empty callback set
    pub fn new() -> Self {
        Self {
            on_input: Mutex::new(None),
            on_request_frame: Mutex::new(None),
            on_resize: Mutex::new(None),
            on_moved: Mutex::new(None),
            on_close: Mutex::new(None),
            on_should_close: Mutex::new(None),
            on_active_status_change: Mutex::new(None),
            on_visibility_status_change: Mutex::new(None),
            on_hover_status_change: Mutex::new(None),
            on_appearance_changed: Mutex::new(None),
            event_dispatch: Mutex::new(DispatchState::new()),
            should_close_dispatching: Mutex::new(false),
        }
    }

    fn drain_events(
        &self,
        mut drain: DispatchDrain<'_, WindowCallbackEvent>,
    ) -> Option<DispatchEventResult> {
        let mut input_result = None;
        while let Some(event) = drain.next() {
            match event {
                WindowCallbackEvent::Input(event) => {
                    let mut lease = CallbackLease::take(&self.on_input);
                    let result = lease
                        .callback_mut()
                        .map_or_else(DispatchEventResult::default, |callback| callback(event));
                    input_result.get_or_insert(result);
                }
                WindowCallbackEvent::RequestFrame => {
                    let mut lease = CallbackLease::take(&self.on_request_frame);
                    if let Some(callback) = lease.callback_mut() {
                        callback();
                    }
                }
                WindowCallbackEvent::Resize(size, scale_factor) => {
                    let mut lease = CallbackLease::take(&self.on_resize);
                    if let Some(callback) = lease.callback_mut() {
                        callback(size, scale_factor);
                    }
                }
                WindowCallbackEvent::Moved => {
                    let mut lease = CallbackLease::take(&self.on_moved);
                    if let Some(callback) = lease.callback_mut() {
                        callback();
                    }
                }
                WindowCallbackEvent::Close => {
                    let callback = self.on_close.lock().take();
                    if let Some(callback) = callback {
                        callback();
                    }
                }
                WindowCallbackEvent::Active(is_active) => {
                    let mut lease = CallbackLease::take(&self.on_active_status_change);
                    if let Some(callback) = lease.callback_mut() {
                        callback(is_active);
                    }
                }
                WindowCallbackEvent::Visibility(is_visible) => {
                    let mut lease = CallbackLease::take(&self.on_visibility_status_change);
                    if let Some(callback) = lease.callback_mut() {
                        callback(is_visible);
                    }
                }
                WindowCallbackEvent::Hover(is_hovered) => {
                    let mut lease = CallbackLease::take(&self.on_hover_status_change);
                    if let Some(callback) = lease.callback_mut() {
                        callback(is_hovered);
                    }
                }
                WindowCallbackEvent::AppearanceChanged => {
                    let mut lease = CallbackLease::take(&self.on_appearance_changed);
                    if let Some(callback) = lease.callback_mut() {
                        callback();
                    }
                }
            }
        }
        input_result
    }

    /// Dispatch an input event.
    ///
    /// The outer drain returns the callback result for its own event. A nested
    /// dispatch is queued and returns [`DispatchEventResult::DEFERRED`]
    /// immediately because its callback result cannot be synchronously known
    /// until the outer callback returns. The conservative deferred value
    /// suppresses native default handling until FLUI consumes the queued event.
    pub fn dispatch_input(&self, event: PlatformInput) -> DispatchEventResult {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Input(event))
        else {
            return DispatchEventResult::DEFERRED;
        };
        self.drain_events(drain).unwrap_or_default()
    }

    /// Dispatch a frame request.
    pub fn dispatch_request_frame(&self) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::RequestFrame)
        else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch a resize event with new logical size and scale factor.
    pub fn dispatch_resize(&self, size: Size<Pixels>, scale_factor: f32) {
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Resize(size, scale_factor),
        ) else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch a window moved event.
    pub fn dispatch_moved(&self) {
        let Some(drain) = DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Moved)
        else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch close event. Consumes the callback (FnOnce).
    pub fn dispatch_close(&self) {
        let Some(drain) = DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Close)
        else {
            return;
        };
        self.drain_events(drain);
    }

    /// Query whether the window should close.
    ///
    /// Returns `true` if no callback is registered. Same-kind reentry cannot
    /// produce a causally valid synchronous answer, so a nested query returns
    /// `false` (conservative veto) and is not recursively invoked. The outer
    /// query remains authoritative and its callback is restored on unwind.
    pub fn dispatch_should_close(&self) -> bool {
        {
            let mut dispatching = self.should_close_dispatching.lock();
            if *dispatching {
                return false;
            }
            *dispatching = true;
        }
        let _dispatch_guard = BooleanDispatchGuard {
            dispatching: &self.should_close_dispatching,
        };
        let mut lease = CallbackLease::take(&self.on_should_close);
        if let Some(callback) = lease.callback_mut() {
            callback()
        } else {
            true // Default: allow close
        }
    }

    /// Dispatch active status change (focus gained/lost).
    pub fn dispatch_active_status_change(&self, is_active: bool) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Active(is_active))
        else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch visibility status change (occlusion gained/lost).
    pub fn dispatch_visibility_status_change(&self, is_visible: bool) {
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Visibility(is_visible),
        ) else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch hover status change (mouse enter/leave).
    pub fn dispatch_hover_status_change(&self, is_hovered: bool) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Hover(is_hovered))
        else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch appearance change (system theme changed).
    pub fn dispatch_appearance_changed(&self) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::AppearanceChanged)
        else {
            return;
        };
        self.drain_events(drain);
    }
}

impl Default for WindowCallbacks {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WindowCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowCallbacks")
            .field("on_input", &self.on_input.lock().is_some())
            .field("on_request_frame", &self.on_request_frame.lock().is_some())
            .field("on_resize", &self.on_resize.lock().is_some())
            .field("on_moved", &self.on_moved.lock().is_some())
            .field("on_close", &self.on_close.lock().is_some())
            .field("on_should_close", &self.on_should_close.lock().is_some())
            .field(
                "on_active_status_change",
                &self.on_active_status_change.lock().is_some(),
            )
            .field(
                "on_visibility_status_change",
                &self.on_visibility_status_change.lock().is_some(),
            )
            .field(
                "on_hover_status_change",
                &self.on_hover_status_change.lock().is_some(),
            )
            .field(
                "on_appearance_changed",
                &self.on_appearance_changed.lock().is_some(),
            )
            .finish_non_exhaustive()
    }
}
