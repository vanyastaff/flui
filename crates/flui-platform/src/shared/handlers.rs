//! Platform callback handlers
//!
//! Registry pattern for platform callbacks, allowing the framework to register
//! event handlers without tight coupling to platform implementations.
//!
//! Two levels of callbacks:
//! - [`PlatformHandlers`]: Global platform-level callbacks (quit, reopen, etc.)
//! - [`WindowCallbacks`]: Per-window callbacks (input, resize, close, etc.)

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_types::geometry::{Pixels, Size};
use parking_lot::Mutex;

use crate::traits::{DispatchEventResult, PlatformInput, WindowEvent, WindowExecutionState};

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
#[non_exhaustive]
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

    /// Consulted when this platform's own window bookkeeping believes every
    /// window it tracks has just closed, before it decides whether to end
    /// the run loop (winit) or mark itself not-running (headless).
    ///
    /// `true` allows the exit; `false` vetoes it (e.g. because the embedder
    /// has an "open another window" request queued outside this platform's
    /// own window map — issue #555's `AppRuntime::should_exit`
    /// drain-before-decide rule). A backend that never calls
    /// [`PlatformHandlers::invoke_exit_policy`] is unaffected by this field
    /// at all; a backend that does but finds it unset gets `true` (the
    /// pre-#555 unconditional "last window closed -> exit" default), so an
    /// embedder that never registers a hook sees no behavior change.
    pub exit_policy: Option<Box<dyn Fn() -> bool + Send>>,

    /// Consulted once per idle event-loop iteration for the earliest
    /// wall-clock instant the loop should wake at (issue #556's wall-clock
    /// wake seam) — `None` (unset, or the hook itself answers `None`) keeps
    /// the unconditional `ControlFlow::Wait` behavior exactly as before this
    /// field existed. A backend that never reads this field at all is
    /// unaffected by it.
    ///
    /// `Arc`, not `Box`: the hook itself re-enters `flui-app` (it walks
    /// every hosted realm and takes gesture-arena locks), so a caller
    /// holding this platform's own state lock must clone the `Arc` out and
    /// invoke it AFTER releasing that lock (ADR-0038 §5's discipline) —
    /// a `Box` would force either an in-lock call or a full field swap.
    ///
    /// The winit backend's `about_to_wait` is this field's one live
    /// consumer today, and it reads the field DIRECTLY —
    /// `state.handlers.wake_deadline.clone()` inside `with_state`, then
    /// calls the cloned `Arc` after the lock guard has dropped — rather
    /// than going through [`PlatformHandlers::invoke_wake_deadline`]. That
    /// method takes `&self` and would need the lock held for its own
    /// entire call, invoking the re-entrant hook while the lock is still
    /// live and violating the exact discipline described above;
    /// `invoke_wake_deadline` stays a convenience method for the
    /// storage-level test that exercises this field in isolation, not a
    /// production call site.
    pub wake_deadline: Option<Arc<dyn Fn() -> Option<web_time::Instant> + Send + Sync>>,
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
            exit_policy: None,
            wake_deadline: None,
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

    /// Consult the exit-policy hook (see [`Self::exit_policy`]'s doc). `true`
    /// when unset — the pre-#555 unconditional "last window closed -> exit"
    /// default a real native backend (winit) had. Only a storage-level test
    /// calls this directly today: winit's own `CloseRequested` handling
    /// leases the hook out of the lock first (`WinitPlatform::
    /// lease_exit_policy_hook`) rather than calling this while the lock is
    /// held. The headless backend does NOT use this method at all — its own
    /// pre-#555 default is the OPPOSITE ("no hook -> never quit", matching
    /// every headless test/consumer that predates this mechanism, none of
    /// which expects closing a mock window to spontaneously call `quit`) —
    /// see `HeadlessPlatform`'s own `notify_closed` for that backend's
    /// inline equivalent.
    #[inline]
    pub fn invoke_exit_policy(&self) -> bool {
        self.exit_policy.as_ref().is_none_or(|hook| hook())
    }

    /// Consult the wake-deadline hook (see [`Self::wake_deadline`]'s doc).
    /// `None` when unset — the previously-unconditional-`Wait` default. A
    /// public method with no production caller: the winit backend's
    /// `about_to_wait` (this field's one live production consumer) clones
    /// the `Arc` directly out of [`Self::wake_deadline`] instead, because
    /// this method takes `&self` and calling it would keep the platform
    /// state lock held for the hook's entire re-entrant call — see
    /// [`Self::wake_deadline`]'s own doc for why that ordering matters.
    /// Only a storage-level test calls this directly today
    /// (`set_wake_deadline_hook_installs_into_the_shared_handler_slot`,
    /// `flui-platform/src/platforms/winit/platform.rs`), the same posture
    /// [`Self::invoke_exit_policy`] documents for itself above.
    #[inline]
    pub fn invoke_wake_deadline(&self) -> Option<web_time::Instant> {
        self.wake_deadline.as_ref().and_then(|hook| hook())
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
            .field("exit_policy", &self.exit_policy.is_some())
            .field("wake_deadline", &self.wake_deadline.is_some())
            .finish()
    }
}

// ============================================================================
// Per-Window Callbacks
// ============================================================================

#[expect(clippy::type_complexity)]
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
    /// drives it on desktop, but only on X11 (Xlib's
    /// `VisibilityFullyObscured` — full obscuration only), macOS, iOS, and
    /// Web — winit 0.30 has no Wayland emitter for this event at all
    /// ("Android / Wayland / Windows / Orbital: Unsupported", per winit's
    /// own `WindowEvent::Occluded` doc). The native Win32 backend derives
    /// its own signal from `WM_SIZE` minimize/restore plus `WM_SHOWWINDOW`
    /// hide/show, and the native AppKit backend from
    /// `windowDidChangeOcclusionState:` — rules in `shared::visibility`.
    /// Where no signal is ever delivered, this callback simply never fires
    /// — the window is treated as always visible (the same behavior as
    /// before this callback existed).
    pub on_visibility_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the mouse enters or leaves the window. Parameter:
    /// is_hovered.
    pub on_hover_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the system appearance (light/dark) changes.
    pub on_appearance_changed: Mutex<Option<Box<dyn FnMut() + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    /// Called when the GPU surface's availability changes. Parameter:
    /// `has_surface` (`false` means the surface must be released before this
    /// callback returns).
    ///
    /// Distinct from the lifecycle callbacks above: a window can be resumed
    /// and still have no usable swapchain, and Android's `Pause` does not
    /// clear `AndroidApp::native_window()` at all. See
    /// [`crate::traits::PlatformWindow::on_surface_status_change`] for the contract and for
    /// why a backend that never emits it is harmless while one that emits only
    /// `false` is not.
    pub on_surface_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>, // PORT-CHECK-OK-SP6: PlatformHandlers callback storage; FR-029 #5 sanctioned; SP-6 lock-placement tracked

    on_execution_state_change: Mutex<Option<Box<dyn FnMut(WindowExecutionState) + Send>>>,
    event_dispatch: Mutex<DispatchState<WindowCallbackEvent>>,
    should_close_dispatching: Mutex<bool>,

    /// One-shot latch set by [`Self::clear`] before it takes any slot.
    ///
    /// Closes the #919-class hazard from the *other* direction: a callback
    /// leased out via [`CallbackLease::take`] runs with its slot empty, so
    /// a `clear()` that lands while a callback is out finds nothing to take
    /// there — and without this flag, [`CallbackLease::drop`] would then
    /// restore that callback into the slot `clear()` just emptied the
    /// instant the leased call returns, resurrecting a callback (and
    /// everything it owns — a frame closure, a renderer, a surface, the
    /// window's own `Arc`) whose window no longer exists. Every lease reads
    /// the flag on drop instead of restoring unconditionally.
    cleared: AtomicBool,
    // Immediate lifecycle delivery cannot overtake a queued general-FIFO clear.
    lifecycle_closed: AtomicBool,
}

enum WindowCallbackEvent {
    Input(PlatformInput),
    RequestFrame,
    Resize(Size<Pixels>, f32),
    Moved,
    Close,
    Active(bool),
    Execution(WindowExecutionState),
    Visibility(bool),
    Hover(bool),
    AppearanceChanged,
    SurfaceStatus(bool),
    /// A deferred [`WindowCallbacks::clear`]: queued instead of run inline
    /// when `clear()` is called while this window's FIFO is already
    /// draining (a `close()` issued from inside one of this window's own
    /// callbacks — see `clear()`'s own doc for why draining it in order
    /// matters).
    Clear,
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

#[cfg(any(target_os = "ios", test))]
pub(crate) enum LifecycleEvent {
    Execution(WindowExecutionState),
    Focus(bool),
    Visibility(bool),
    Surface(bool),
}

/// Temporarily removes one `FnMut` callback without holding its mutex while
/// user code runs, then restores it even if that code unwinds — unless the
/// window closed while the callback was out (see `cleared`'s doc), in which
/// case it is dropped instead.
struct CallbackLease<'a, T> {
    slot: &'a Mutex<Option<T>>,
    cleared: &'a AtomicBool,
    callback: Option<T>,
}

impl<'a, T> CallbackLease<'a, T> {
    fn take(slot: &'a Mutex<Option<T>>, cleared: &'a AtomicBool) -> Self {
        let callback = slot.lock().take();
        Self {
            slot,
            cleared,
            callback,
        }
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
        // Read the latch UNDER the slot lock, not before it. `clear_now()`
        // (see `WindowCallbacks::clear`'s doc) stores this flag, in program
        // order on one thread, strictly before it takes ANY slot — so a
        // `false` observed HERE, synchronized against that store through
        // this very slot's mutex, means `clear_now()`'s own take of this
        // exact slot (if it ever runs) is still ahead of us: restoring is
        // safe, because `clear_now()` visits every slot unconditionally and
        // will still find and drop whatever we put back. A `true` observed
        // here means `clear_now()` has already started, so this slot must
        // end up empty regardless of whether its take of this slot already
        // ran (and found nothing, because we were holding the callback) or
        // is still to come. Checking before acquiring the lock instead would
        // race: `clear_now()` could store the flag and take this slot
        // (finding it empty, since we're holding the callback) entirely
        // between our read and our lock, and we would restore a callback
        // `clear_now()` already believes it dropped.
        if self.cleared.load(Ordering::SeqCst) || slot.is_some() {
            // Release the guard before the callback's destructor runs — it
            // may re-enter platform code that locks this same
            // (non-reentrant) mutex.
            drop(slot);
            drop(callback);
            return;
        }
        *slot = Some(callback);
    }
}

impl WindowCallbacks {
    /// Create a new empty callback set
    pub fn new() -> Self {
        Self {
            on_execution_state_change: Mutex::new(None),
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
            on_surface_status_change: Mutex::new(None),
            event_dispatch: Mutex::new(DispatchState::new()),
            should_close_dispatching: Mutex::new(false),
            cleared: AtomicBool::new(false),
            lifecycle_closed: AtomicBool::new(false),
        }
    }

    /// Takes and drops every registered callback.
    ///
    /// Registered callbacks are the platform's only owning references to
    /// presentation-scoped embedder state — in `flui-app`'s wiring the
    /// frame callback owns the window's GPU renderer, whose `wgpu::Surface`
    /// is built from the `Arc<dyn PlatformWindow>` clone the renderer owns
    /// (ADR-0063) and must therefore be destroyed while the native window
    /// behind that clone is still alive. `rg -n
    /// "callbacks\(\)\.clear\(\)|callbacks\.clear\(\)"
    /// crates/flui-platform/src` finds every call site that reaches this
    /// method at window close: winit's `complete_window_close`
    /// (`platforms/winit/platform.rs`, the primary in-loop path) and its
    /// `WinitApp::release_open_window_callbacks` (same file, the quit route
    /// `complete_window_close` never runs for — every window still tracked
    /// when `event_loop.run_app` returns), `WinitWindow::drop`
    /// (`platforms/winit/window.rs`, a last-resort guarantee for a window
    /// whose final `Arc` unwinds anywhere else), the headless backend's
    /// `complete_close` (`platforms/headless/platform.rs`), Win32's
    /// `WM_DESTROY` arm (`platforms/windows/platform.rs`), AppKit's
    /// `handle_close` (`platforms/macos/window.rs`, reached from the
    /// `windowWillClose:` delegate), and the Android backend's loop exit
    /// (`platforms/android/mod.rs`, `AndroidPlatform::run` after its `loop`,
    /// reached by `Destroy`, `quit()` and a failed bootstrap; a panic out of
    /// `run` skips it, by decision) — so that destruction order is pinned
    /// deterministically instead of left to struct field order.
    ///
    /// **Drain-ordered, not immediate, when called while this window's FIFO
    /// is already draining.** A close requested from inside one of this
    /// window's own callbacks (e.g. `on_input` — a widget calling
    /// `window.close()` from its own gesture handler) reaches one of the
    /// call sites above SYNCHRONOUSLY, while that callback's own dispatch
    /// is still on the stack — so `dispatch_close()` finds the FIFO already
    /// dispatching and only QUEUES `Close` rather than invoking `on_close`
    /// inline (see `DispatchDrain::begin`). If `clear()` took every slot
    /// immediately at that point, it would take `on_close` before the
    /// queued `Close` event ever gets a chance to run it — dropping the
    /// callback silently instead of firing it, which is how a widget's
    /// `close_this_window` wiring would leak the window from `flui-app`'s
    /// registry forever (issue #1043). So `clear()` itself goes through the
    /// same FIFO: it queues a `WindowCallbackEvent::Clear` exactly like any
    /// other event, and either drains it immediately (no dispatch was in
    /// flight) or lets the ALREADY-running drain reach it in causal order —
    /// after the `Close` queued ahead of it, and after every other event
    /// queued ahead of that. The actual take-and-drop body lives in
    /// `clear_now`, called either directly below or from `drain_events`'s
    /// own `Clear` arm.
    ///
    /// **The `cleared` latch never resets once set, but that does not make
    /// every slot permanently inert.** The ten slots `CallbackLease`
    /// dispatches through (every one except `on_close`) still accept a
    /// fresh registration after this call returns: that callback runs
    /// exactly once, the next time its event is dispatched, and only then
    /// does its lease's `Drop` see `cleared` set and discard it instead of
    /// restoring it (`CallbackLease::drop`) — so it cannot run a second
    /// time. `on_close` is untouched by the latch entirely: it is `FnOnce`,
    /// so `drain_events`'s `Close` arm takes and calls it directly with no
    /// lease to gate a restore, meaning a callback registered on it after
    /// `clear()` returns would fire normally if `Close` were ever
    /// dispatched again — a case no current call site produces (nothing
    /// dispatches `Close` twice), but not prevented by this type either.
    /// None of this is a registration path any backend should use: a
    /// window that reopens must construct a fresh `WindowCallbacks`, never
    /// reuse one that has already been cleared.
    pub fn clear(&self) {
        self.lifecycle_closed.store(true, Ordering::SeqCst);
        let Some(drain) = DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Clear)
        else {
            // Already draining: the running drain's own loop will reach
            // this queued `Clear` in FIFO order and call `clear_now` from
            // `drain_events`'s own arm.
            return;
        };
        self.drain_events(drain);
    }

    /// The actual latch-then-take-all body behind [`Self::clear`]. Called
    /// either directly by `clear()` (no dispatch was in flight) or from
    /// `drain_events`'s `Clear` arm (a dispatch was already draining this
    /// window's FIFO, so this runs once every event queued ahead of it —
    /// including a reentrant `close()`'s own `Close` — has already fired).
    /// Payloads are collected first and dropped only after every slot's
    /// lock has been released, since a callback's destructor may re-enter
    /// platform code.
    fn clear_now(&self) {
        // Set BEFORE taking any slot: a callback leased out right now (its
        // slot already empty, user code running) checks this flag when its
        // lease drops, which happens strictly after this store — the only
        // way to stop it from restoring itself into a slot this call is
        // about to empty. See `CallbackLease::drop`.
        self.cleared.store(true, Ordering::SeqCst);
        let dropped = (
            self.on_execution_state_change.lock().take(),
            self.on_input.lock().take(),
            self.on_request_frame.lock().take(),
            self.on_resize.lock().take(),
            self.on_moved.lock().take(),
            self.on_close.lock().take(),
            self.on_should_close.lock().take(),
            self.on_active_status_change.lock().take(),
            self.on_visibility_status_change.lock().take(),
            self.on_hover_status_change.lock().take(),
            self.on_appearance_changed.lock().take(),
            self.on_surface_status_change.lock().take(),
        );
        drop(dropped);
    }

    fn drain_events(
        &self,
        mut drain: DispatchDrain<'_, WindowCallbackEvent>,
    ) -> Option<DispatchEventResult> {
        let mut input_result = None;
        while let Some(event) = drain.next() {
            match event {
                WindowCallbackEvent::Input(event) => {
                    let mut lease = CallbackLease::take(&self.on_input, &self.cleared);
                    let result = lease
                        .callback_mut()
                        .map_or_else(DispatchEventResult::default, |callback| callback(event));
                    input_result.get_or_insert(result);
                }
                WindowCallbackEvent::RequestFrame => {
                    let mut lease = CallbackLease::take(&self.on_request_frame, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback();
                    }
                }
                WindowCallbackEvent::Resize(size, scale_factor) => {
                    let mut lease = CallbackLease::take(&self.on_resize, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(size, scale_factor);
                    }
                }
                WindowCallbackEvent::Moved => {
                    let mut lease = CallbackLease::take(&self.on_moved, &self.cleared);
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
                WindowCallbackEvent::Execution(state) => {
                    let mut lease =
                        CallbackLease::take(&self.on_execution_state_change, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(state);
                    }
                }
                WindowCallbackEvent::Active(is_active) => {
                    let mut lease =
                        CallbackLease::take(&self.on_active_status_change, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(is_active);
                    }
                }
                WindowCallbackEvent::Visibility(is_visible) => {
                    let mut lease =
                        CallbackLease::take(&self.on_visibility_status_change, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(is_visible);
                    }
                }
                WindowCallbackEvent::Hover(is_hovered) => {
                    let mut lease =
                        CallbackLease::take(&self.on_hover_status_change, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(is_hovered);
                    }
                }
                WindowCallbackEvent::AppearanceChanged => {
                    let mut lease = CallbackLease::take(&self.on_appearance_changed, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback();
                    }
                }
                WindowCallbackEvent::SurfaceStatus(has_surface) => {
                    let mut lease =
                        CallbackLease::take(&self.on_surface_status_change, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(has_surface);
                    }
                }
                WindowCallbackEvent::Clear => {
                    self.clear_now();
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
        let mut lease = CallbackLease::take(&self.on_should_close, &self.cleared);
        if let Some(callback) = lease.callback_mut() {
            callback()
        } else {
            true // Default: allow close
        }
    }

    /// Deliver iOS lifecycle effects in its own serialized transaction, even when
    /// an input/frame callback currently owns the general event FIFO. The caller
    /// must serialize these effects, including callback capture destruction.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn dispatch_lifecycle_immediate(&self, event: LifecycleEvent) {
        use LifecycleEvent::{Execution, Focus, Surface, Visibility};
        match event {
            Execution(value) => self.invoke_lifecycle(&self.on_execution_state_change, value),
            Focus(value) => self.invoke_lifecycle(&self.on_active_status_change, value),
            Visibility(value) => self.invoke_lifecycle(&self.on_visibility_status_change, value),
            Surface(value) => self.invoke_lifecycle(&self.on_surface_status_change, value),
        }
    }

    #[cfg(any(target_os = "ios", test))]
    fn invoke_lifecycle<T, F: FnMut(T)>(&self, slot: &Mutex<Option<F>>, value: T) {
        use super::panic_boundary::contain_owner_callback;
        if self.lifecycle_closed.load(Ordering::SeqCst) {
            return;
        }
        let mut lease = CallbackLease::take(slot, &self.lifecycle_closed);
        contain_owner_callback(|| {
            if let Some(callback) = lease.callback_mut() {
                callback(value);
            }
        });
        // Invocation has finished unwinding before a retired capture can panic.
        contain_owner_callback(|| drop(lease));
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

    /// Replace an execution observer, disposing captures outside the storage lock.
    pub fn set_execution_state_callback(
        &self,
        callback: Box<dyn FnMut(WindowExecutionState) + Send>,
    ) {
        let old = {
            let mut slot = self.on_execution_state_change.lock();
            if self.cleared.load(Ordering::SeqCst) {
                drop(slot);
                drop(callback);
                return;
            }
            slot.replace(callback)
        };
        drop(old);
    }

    /// Deliver an owner-thread execution observation through the window FIFO.
    pub fn dispatch_execution_state_change(&self, state: WindowExecutionState) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Execution(state))
        else {
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

    /// Dispatch a GPU-surface availability change. Parameter: `has_surface`
    /// (`false` asks the owner to release its surface before returning).
    ///
    /// The `false` edge is the one with a real deadline behind it: it runs
    /// while the platform has the native window in hand and is about to lose
    /// it, so the callback it reaches must drop the surface synchronously
    /// rather than schedule the drop for later. See
    /// [`crate::traits::PlatformWindow::on_surface_status_change`] for the full contract.
    pub fn dispatch_surface_status_change(&self, has_surface: bool) {
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::SurfaceStatus(has_surface),
        ) else {
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

/// Emits the eleven `PlatformWindow` `on_*` callback-registration trait methods.
///
/// Every backend window stores its callbacks in a [`WindowCallbacks`] — either
/// as a bare field or behind an `Arc` (auto-deref makes one body cover both) —
/// and every setter is the same one-liner: store the boxed callback in its
/// slot, replacing any previous registration. Invoke inside an
/// `impl PlatformWindow for ...` block, naming the field that holds the
/// [`WindowCallbacks`]:
///
/// ```ignore
/// impl PlatformWindow for MyWindow {
///     crate::shared::impl_window_callback_setters!(callbacks);
///     // ... the rest of the impl ...
/// }
/// ```
///
/// The signatures below must match the `PlatformWindow` trait exactly; types
/// are spelled with absolute paths so the expansion never depends on the
/// invoking module's imports.
macro_rules! impl_window_callback_setters {
    ($callbacks_field:ident) => {
        fn on_input(
            &self,
            callback: Box<
                dyn FnMut($crate::traits::PlatformInput) -> $crate::traits::DispatchEventResult
                    + Send,
            >,
        ) {
            *self.$callbacks_field.on_input.lock() = Some(callback);
        }

        fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
            *self.$callbacks_field.on_request_frame.lock() = Some(callback);
        }

        fn on_resize(
            &self,
            callback: Box<
                dyn FnMut(::flui_types::geometry::Size<::flui_types::geometry::Pixels>, f32) + Send,
            >,
        ) {
            *self.$callbacks_field.on_resize.lock() = Some(callback);
        }

        fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
            *self.$callbacks_field.on_moved.lock() = Some(callback);
        }

        fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
            *self.$callbacks_field.on_close.lock() = Some(callback);
        }

        fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
            *self.$callbacks_field.on_should_close.lock() = Some(callback);
        }

        fn on_execution_state_change(
            &self,
            callback: Box<dyn FnMut($crate::WindowExecutionState) + Send>,
        ) {
            self.$callbacks_field.set_execution_state_callback(callback);
        }
        fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            *self.$callbacks_field.on_active_status_change.lock() = Some(callback);
        }

        fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            *self.$callbacks_field.on_visibility_status_change.lock() = Some(callback);
        }

        fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            *self.$callbacks_field.on_hover_status_change.lock() = Some(callback);
        }

        fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
            *self.$callbacks_field.on_appearance_changed.lock() = Some(callback);
        }

        fn on_surface_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            *self.$callbacks_field.on_surface_status_change.lock() = Some(callback);
        }
    };
}
// Textual-scope escape: `pub(crate) use` gives the macro a normal path
// (`crate::shared::impl_window_callback_setters`) without `#[macro_export]`,
// which would put an intra-crate implementation detail on the public API.
pub(crate) use impl_window_callback_setters;

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
            .field(
                "on_surface_status_change",
                &self.on_surface_status_change.lock().is_some(),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    use super::*;

    /// A keyboard event: the cheapest concrete `PlatformInput` to construct
    /// for a test that only cares about triggering the `on_input` drain.
    fn keyboard_event() -> PlatformInput {
        PlatformInput::Keyboard(ui_events::keyboard::KeyboardEvent {
            state: ui_events::keyboard::KeyState::Down,
            key: keyboard_types::Key::Named(keyboard_types::NamedKey::Enter),
            code: ui_events::keyboard::Code::Unidentified,
            location: ui_events::keyboard::Location::Standard,
            modifiers: keyboard_types::Modifiers::empty(),
            repeat: false,
            is_composing: false,
        })
    }

    #[test]
    fn lifecycle_delivery_survives_frame_origin_unwind_and_clear() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let callbacks = Arc::new(WindowCallbacks::new());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let history = Arc::clone(&seen);
        callbacks.set_execution_state_callback(Box::new(move |state| history.lock().push(state)));
        let inner = Arc::clone(&callbacks);
        *callbacks.on_request_frame.lock() = Some(Box::new(move || {
            inner.dispatch_lifecycle_immediate(LifecycleEvent::Execution(
                WindowExecutionState::Suspended,
            ));
            panic!("frame caller panic after lifecycle notification");
        }));
        assert!(catch_unwind(AssertUnwindSafe(|| callbacks.dispatch_request_frame())).is_err());
        assert_eq!(*seen.lock(), [WindowExecutionState::Suspended]);
        callbacks.clear();
        callbacks
            .dispatch_lifecycle_immediate(LifecycleEvent::Execution(WindowExecutionState::Running));
        assert_eq!(*seen.lock(), [WindowExecutionState::Suspended]);
    }

    #[test]
    fn lifecycle_clear_inside_frame_fences_immediate_followups() {
        let callbacks = Arc::new(WindowCallbacks::new());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let inner = Arc::clone(&callbacks);
        let history = Arc::clone(&seen);
        callbacks.set_execution_state_callback(Box::new(move |state| {
            history.lock().push(state);
            inner.clear();
        }));
        let inner = Arc::clone(&callbacks);
        *callbacks.on_request_frame.lock() = Some(Box::new(move || {
            inner.dispatch_lifecycle_immediate(LifecycleEvent::Execution(
                WindowExecutionState::Suspended,
            ));
            inner.dispatch_lifecycle_immediate(LifecycleEvent::Execution(
                WindowExecutionState::Running,
            ));
        }));
        callbacks.dispatch_request_frame();
        assert_eq!(*seen.lock(), [WindowExecutionState::Suspended]);
        assert!(callbacks.on_execution_state_change.lock().is_none());
    }

    #[test]
    fn lifecycle_invocation_and_retired_capture_panics_are_separate() {
        use std::sync::atomic::AtomicUsize;
        struct Capture(Arc<AtomicUsize>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("retired capture panic");
            }
        }
        let callbacks = Arc::new(WindowCallbacks::new());
        let dropped = Arc::new(AtomicUsize::new(0));
        let replacement = Arc::new(AtomicUsize::new(0));
        let capture = Capture(Arc::clone(&dropped));
        let inner = Arc::clone(&callbacks);
        let seen = Arc::clone(&replacement);
        callbacks.set_execution_state_callback(Box::new(move |_| {
            let _ = &capture;
            let seen = Arc::clone(&seen);
            inner.set_execution_state_callback(Box::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
            }));
            panic!("observer panic");
        }));
        callbacks.dispatch_lifecycle_immediate(LifecycleEvent::Execution(
            WindowExecutionState::Suspended,
        ));
        callbacks
            .dispatch_lifecycle_immediate(LifecycleEvent::Execution(WindowExecutionState::Running));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(replacement.load(Ordering::SeqCst), 1);
        // Exercise all immediate effect slots with the same clear/lease contract.
        callbacks.dispatch_lifecycle_immediate(LifecycleEvent::Focus(false));
        callbacks.dispatch_lifecycle_immediate(LifecycleEvent::Visibility(false));
        callbacks.dispatch_lifecycle_immediate(LifecycleEvent::Surface(false));
    }

    #[test]
    fn execution_callback_replacement_and_reentrant_delivery_keep_fifo_and_close_fence() {
        let callbacks = Arc::new(WindowCallbacks::new());
        let history = Arc::new(Mutex::new(Vec::new()));
        let inner = Arc::clone(&callbacks);
        let observed = Arc::clone(&history);
        callbacks.set_execution_state_callback(Box::new(move |state| {
            observed.lock().push(state);
            let replacement_history = Arc::clone(&observed);
            inner.set_execution_state_callback(Box::new(move |state| {
                replacement_history.lock().push(state);
            }));
            inner.dispatch_execution_state_change(WindowExecutionState::Running);
        }));
        callbacks.dispatch_execution_state_change(WindowExecutionState::Suspended);
        assert_eq!(
            *history.lock(),
            [
                WindowExecutionState::Suspended,
                WindowExecutionState::Running
            ]
        );
        callbacks.clear();
        callbacks.set_execution_state_callback(Box::new(|_| panic!("closed callback admitted")));
        callbacks.dispatch_execution_state_change(WindowExecutionState::Detached);
        assert_eq!(history.lock().len(), 2);
    }

    #[test]
    fn replacing_execution_callback_drops_capture_outside_storage_lock() {
        struct Reenter(Arc<WindowCallbacks>);
        impl Drop for Reenter {
            fn drop(&mut self) {
                self.0.set_execution_state_callback(Box::new(|_| {}));
            }
        }
        let callbacks = Arc::new(WindowCallbacks::new());
        let capture = Reenter(Arc::clone(&callbacks));
        callbacks.set_execution_state_callback(Box::new(move |_| {
            let _retain = &capture;
        }));
        callbacks.set_execution_state_callback(Box::new(|_| {}));
        callbacks.clear();
    }

    /// A close requested from inside a callback the FIFO is already
    /// draining (issue #1043): a widget closing its own window from inside
    /// a currently-dispatched callback (here, `on_input`) reaches
    /// `dispatch_close()` while that outer dispatch is still on the stack,
    /// so `dispatch_close()` only QUEUES `Close` instead of invoking
    /// `on_close` inline. `clear()` called right after it (as every
    /// real close arm does) must not take `on_close` out from under that
    /// still-queued event — `on_close` must still fire exactly once, and
    /// every slot must still end up empty once the drain finishes. Goes red
    /// if `clear()` reverts to taking every slot immediately instead of
    /// queuing a `Clear` event behind the pending `Close`.
    #[test]
    fn close_from_inside_a_drained_callback_still_fires_on_close() {
        let callbacks = Arc::new(WindowCallbacks::new());
        let close_count = Arc::new(AtomicU32::new(0));

        let count_for_close = Arc::clone(&close_count);
        callbacks.on_close.lock().replace(Box::new(move || {
            count_for_close.fetch_add(1, Ordering::SeqCst);
        }));

        let inner = Arc::clone(&callbacks);
        callbacks.on_input.lock().replace(Box::new(move |_event| {
            // Simulate a widget synchronously closing its own window from
            // inside an input callback — exactly the native shape (a
            // gesture handler calling `window.close()`, which reaches
            // `dispatch_close()` then `clear()` on the owning thread
            // before this callback returns).
            inner.dispatch_close();
            inner.clear();
            DispatchEventResult::default()
        }));

        callbacks.dispatch_input(keyboard_event());

        assert_eq!(
            close_count.load(Ordering::SeqCst),
            1,
            "on_close must still fire exactly once even when close() is \
             requested from inside a callback the FIFO is already draining"
        );
        assert!(callbacks.on_input.lock().is_none());
        assert!(callbacks.on_close.lock().is_none());
        assert!(callbacks.on_request_frame.lock().is_none());
        assert!(callbacks.on_resize.lock().is_none());
        assert!(callbacks.on_moved.lock().is_none());
        assert!(callbacks.on_should_close.lock().is_none());
        assert!(callbacks.on_active_status_change.lock().is_none());
        assert!(callbacks.on_visibility_status_change.lock().is_none());
        assert!(callbacks.on_hover_status_change.lock().is_none());
        assert!(callbacks.on_appearance_changed.lock().is_none());
        assert!(callbacks.on_surface_status_change.lock().is_none());
    }

    /// The #919-class hazard from the other direction: `close()` requested
    /// from inside a callback that is currently leased out. Without the
    /// `cleared` latch, `CallbackLease::drop` would restore this very
    /// callback into the slot `clear()` just emptied the instant this
    /// closure returns — this test goes red if that latch is removed.
    #[test]
    fn close_from_inside_a_leased_callback_does_not_resurrect_it() {
        let callbacks = Arc::new(WindowCallbacks::new());
        let inner = Arc::clone(&callbacks);
        callbacks.on_should_close.lock().replace(Box::new(move || {
            inner.clear();
            true
        }));

        assert!(callbacks.dispatch_should_close());
        assert!(
            callbacks.on_should_close.lock().is_none(),
            "a callback that clears its own window's callbacks from inside \
             itself must not be resurrected by its own lease's Drop"
        );
    }

    /// The ordinary case: a lease taken and dropped with no `clear()` in
    /// between still restores its callback, so the new latch does not break
    /// normal reentrant dispatch.
    #[test]
    fn a_lease_taken_before_any_clear_restores_normally() {
        let callbacks = WindowCallbacks::new();
        let _prev = callbacks.on_should_close.lock().replace(Box::new(|| true));

        assert!(callbacks.dispatch_should_close());
        assert!(
            callbacks.on_should_close.lock().is_some(),
            "an ordinary dispatch outside any clear() must restore its callback"
        );
    }

    /// `clear()`'s own doc states this precisely: the `cleared` latch does
    /// not refuse a fresh registration outright. `CallbackLease::take`
    /// never consults the latch, only `CallbackLease::drop` does — so a
    /// callback registered on a leased slot after `clear()` has already run
    /// still fires exactly once, the next time its event is dispatched,
    /// and only then does it get discarded instead of restored.
    #[test]
    fn a_callback_registered_after_clear_runs_once_then_its_lease_drops_it() {
        let callbacks = WindowCallbacks::new();
        callbacks.clear();

        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran_in_callback = Arc::clone(&ran);
        callbacks.on_request_frame.lock().replace(Box::new(move || {
            ran_in_callback.store(true, Ordering::SeqCst);
        }));

        callbacks.dispatch_request_frame();

        assert!(
            ran.load(Ordering::SeqCst),
            "a callback registered after clear() must still run the one time \
             its event is dispatched"
        );
        assert!(
            callbacks.on_request_frame.lock().is_none(),
            "and must not survive past that one dispatch"
        );
    }

    /// The surface-status slot carries its parameter through the FIFO to the
    /// callback, and — like every other leased slot — survives the dispatch
    /// that ran it, so a resume/release cycle can repeat without the owner
    /// re-registering.
    #[test]
    fn surface_status_change_reaches_its_callback_with_the_parameter() {
        let callbacks = WindowCallbacks::new();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));

        let seen_in_callback = Arc::clone(&seen);
        callbacks
            .on_surface_status_change
            .lock()
            .replace(Box::new(move |has_surface| {
                seen_in_callback
                    .lock()
                    .expect("BUG: test-only mutex is never poisoned")
                    .push(has_surface);
            }));

        callbacks.dispatch_surface_status_change(false);
        callbacks.dispatch_surface_status_change(true);

        assert_eq!(
            seen.lock()
                .expect("BUG: test-only mutex is never poisoned")
                .clone(),
            vec![false, true],
            "both edges reach the callback in order, with the released edge first"
        );
        assert!(
            callbacks.on_surface_status_change.lock().is_some(),
            "an ordinary dispatch outside any clear() must restore this callback too"
        );
    }

    /// The `cleared` latch covers this slot as well: a release callback that
    /// ends up clearing its own window (a close racing a suspend) must not be
    /// resurrected by its own lease's `Drop`. This goes red if
    /// `on_surface_status_change` is left out of `clear_now`'s take-all list.
    #[test]
    fn surface_status_change_cleared_from_inside_is_not_resurrected() {
        let callbacks = Arc::new(WindowCallbacks::new());
        let inner = Arc::clone(&callbacks);
        callbacks
            .on_surface_status_change
            .lock()
            .replace(Box::new(move |_has_surface| {
                inner.clear();
            }));

        callbacks.dispatch_surface_status_change(false);

        assert!(
            callbacks.on_surface_status_change.lock().is_none(),
            "a surface-status callback that clears its window from inside \
             itself must not be resurrected by its own lease's Drop"
        );
    }

    /// The sequence the Android backend's exit path performs, pinned on the
    /// primitive it calls: both cycle-closing slots (`on_request_frame` and
    /// `on_surface_status_change` own the raster lane in `flui-app`'s
    /// wiring) are dispatched at the top level during the loop, their leases
    /// restore them, `dispatch_close` consumes `on_close`, and then `clear()`
    /// must drop both closures and with them everything they own. Two probes,
    /// one per slot, so a `clear_now` that forgets either slot fails on a
    /// named assertion. Slot emptiness is not the claim: a `clear_now` that
    /// took every slot and then `mem::forget` the tuple would leave every
    /// slot `None` and every capture alive, and only these probes see that.
    #[test]
    fn clear_after_top_level_dispatches_releases_what_the_cycle_closing_slots_own() {
        let callbacks = WindowCallbacks::new();
        let frame_owned = Arc::new(());
        let frame_weak = Arc::downgrade(&frame_owned);
        callbacks.on_request_frame.lock().replace(Box::new(move || {
            let _ = &frame_owned;
        }));
        let surface_owned = Arc::new(());
        let surface_weak = Arc::downgrade(&surface_owned);
        callbacks
            .on_surface_status_change
            .lock()
            .replace(Box::new(move |_has_surface| {
                let _ = &surface_owned;
            }));

        callbacks.dispatch_request_frame();
        callbacks.dispatch_surface_status_change(false);
        callbacks.dispatch_close();
        assert!(
            frame_weak.upgrade().is_some(),
            "an ordinary dispatch restores the frame callback"
        );
        assert!(
            surface_weak.upgrade().is_some(),
            "an ordinary dispatch restores the surface callback"
        );

        callbacks.clear();

        assert!(
            frame_weak.upgrade().is_none(),
            "clear() must drop the frame callback and what it owns"
        );
        assert!(
            surface_weak.upgrade().is_none(),
            "clear() must drop the surface callback and what it owns"
        );
    }
}
