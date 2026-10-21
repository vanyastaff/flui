//! Platform callback handlers
//!
//! Registry pattern for platform callbacks, allowing the framework to register
//! event handlers without tight coupling to platform implementations.
//!
//! Two levels of callbacks:
//! - [`PlatformHandlers`]: Global platform-level callbacks (quit, reopen, etc.)
//! - [`WindowCallbacks`]: Per-window callbacks (input, resize, close, etc.)

use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use flui_foundation::IdMint;
use flui_foundation::geometry::{Size, WindowMetrics};
use parking_lot::Mutex;

use flui_platform::input_capabilities::InputCapabilities;

use crate::traits::{
    DispatchEventResult, PlatformInput, SessionEndAnswer, SessionEndPhase, WindowEvent,
    WindowExecutionState,
};

/// The callback a platform asks at each phase of the user's session ending.
pub type SessionEndHook = Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>;

/// A window's input callback.
pub type InputCallback = Box<dyn FnMut(PlatformInput) -> DispatchEventResult>;

/// Asks the backend for an owner-thread turn that calls
/// [`WindowCallbacks::resume_input_delivery`].
pub type InputWake = Arc<dyn Fn() + Send + Sync>;

/// Failed passes over one registration's capability snapshot after which
/// delivery stops, with no further wakes, until the callback is registered
/// again or the window closes (ADR-0213 §3).
const SNAPSHOT_ATTEMPTS: u8 = 3;

/// The most input a window holds behind its snapshot or a failed pass's
/// tail: the runtime holds as many pointer events while it waits for a tree.
/// Admission past it evicts the oldest ordinary input.
const HELD_INPUT_CAPACITY: usize = 256;

/// Keys remembered as answered "not handled" while queued; a platform sends
/// its follow-up right after the answer, so a few suffice.
const UNANSWERED_KEYS: usize = 8;

/// The input a window holds behind its capability snapshot (ADR-0213 §3).
struct InputFeed {
    /// The capabilities in effect before the first held entry: the snapshot
    /// a registration receives first.
    front: InputCapabilities,
    /// The current registration has not received its snapshot.
    snapshot_pending: bool,
    /// An input callback has been registered.
    registered: bool,
    /// The current registration, so a snapshot delivered to an outgoing
    /// callback is not credited to its replacement; zero before the first.
    registration: u64,
    /// Issues every registration's identity, never one twice.
    registrations: IdMint<NonZeroU64>,
    /// Failed passes over the current registration's snapshot.
    failed_snapshots: u8,
    /// Input admitted behind the snapshot or behind a failed pass's tail, in
    /// admission order, capability changes included.
    held: VecDeque<PlatformInput>,
    /// Older input a FIFO drain moved behind a pending snapshot during this
    /// drain; it goes ahead of `held`'s newer entries.
    redirected: usize,
    /// Input lost to the bound since the window opened: evicted from a full
    /// queue, or refused while nothing can deliver it. Reported once.
    lost: u64,
}

impl InputFeed {
    const fn new() -> Self {
        Self {
            front: InputCapabilities::UNAVAILABLE,
            snapshot_pending: true,
            registered: false,
            registration: 0,
            registrations: IdMint::new(),
            failed_snapshots: 0,
            held: VecDeque::new(),
            redirected: 0,
            lost: 0,
        }
    }

    /// Whether ordinary input must wait behind the snapshot or a tail.
    fn holds(&self) -> bool {
        self.snapshot_pending || !self.held.is_empty()
    }

    /// Whether nothing can deliver held input until the callback is
    /// registered again: the registration's snapshot failed every pass.
    fn stalled(&self) -> bool {
        self.registered && self.snapshot_pending && self.failed_snapshots >= SNAPSHOT_ATTEMPTS
    }

    /// Hold `input` at `at` (clamped to the queue), evicting the oldest
    /// ordinary input when the queue is full. A queue of capability changes
    /// only evicts its head instead, which becomes the snapshot's baseline.
    /// Returns where `input` went and the evicted entry, for the caller to
    /// drop outside the lock.
    fn hold_at(&mut self, at: usize, input: PlatformInput) -> (usize, Option<PlatformInput>) {
        let mut at = at.min(self.held.len());
        let evicted = if self.held.len() >= HELD_INPUT_CAPACITY {
            let oldest = self
                .held
                .iter()
                .position(|held| !matches!(held, PlatformInput::Capabilities(_)))
                .unwrap_or(0);
            let evicted = self.held.remove(oldest);
            if let Some(PlatformInput::Capabilities(capabilities)) = &evicted {
                self.front = *capabilities;
            }
            if oldest < at {
                at -= 1;
            }
            if oldest < self.redirected {
                self.redirected -= 1;
            }
            self.count_lost();
            evicted
        } else {
            None
        };
        self.held.insert(at, input);
        (at, evicted)
    }

    /// Hold `input` behind everything held.
    fn hold(&mut self, input: PlatformInput) -> Option<PlatformInput> {
        self.hold_at(self.held.len(), input).1
    }

    fn count_lost(&mut self) {
        if self.lost == 0 {
            tracing::warn!(
                capacity = HELD_INPUT_CAPACITY,
                "window input is being lost: its callback has not taken the held input"
            );
        }
        self.lost = self.lost.saturating_add(1);
    }
}

/// What one step of an input pass delivers.
enum HeldEntry {
    Snapshot(InputCapabilities, u64),
    Input(PlatformInput),
}

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

    /// Asked at each phase of the user's session ending; its answer to
    /// [`SessionEndPhase::Query`] decides whether the platform asks the user
    /// to wait.
    pub session_end: Option<SessionEndHook>,

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
    /// every hosted UI runtime and takes gesture-arena locks), so a caller
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

/// A session-end hook while it runs, back in its slot when the guard drops.
struct RunningSessionEndHook<'a> {
    handlers: &'a Mutex<PlatformHandlers>,
    hook: Option<SessionEndHook>,
}

impl Drop for RunningSessionEndHook<'_> {
    fn drop(&mut self) {
        let Some(hook) = self.hook.take() else { return };
        let displaced = {
            let mut handlers = self.handlers.lock();
            if handlers.session_end.is_none() {
                handlers.session_end = Some(hook);
                None
            } else {
                Some(hook)
            }
        };
        // Outside the lock: a capture's destructor is user code.
        drop(displaced);
    }
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
            session_end: None,
            exit_policy: None,
            wake_deadline: None,
        }
    }

    /// Ask the registered session-end hook about `phase`, answering
    /// [`SessionEndAnswer::Proceed`] when none is registered.
    ///
    /// The hook runs with no lock of `handlers` held, so it may register
    /// another hook or call back into the platform. It is out of its slot while
    /// it runs, and is put back afterwards even when it panics, unless a hook
    /// registered in the meantime took the slot: then the older one is dropped.
    #[must_use]
    pub fn run_session_end(handlers: &Mutex<Self>, phase: SessionEndPhase) -> SessionEndAnswer {
        let Some(hook) = handlers.lock().session_end.take() else {
            return SessionEndAnswer::Proceed;
        };
        let mut running = RunningSessionEndHook {
            handlers,
            hook: Some(hook),
        };
        running
            .hook
            .as_mut()
            .map_or(SessionEndAnswer::Proceed, |hook| hook(phase))
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
    /// default a real native backend (winit) had. Nothing calls this
    /// directly: winit's own `CloseRequested` handling
    /// leases the hook out of the lock first (`WinitPlatform::
    /// lease_exit_policy_hook`) rather than calling this while the lock is
    /// held, and AppKit and Win32 likewise take the hook out of their handler
    /// set before running it, with the same "no hook allows the exit"
    /// default. The headless backend does NOT use this method at all — its own
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
    /// Nothing calls this directly, the same posture
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
            .field("session_end", &self.session_end.is_some())
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
    /// consumed. Installed only through [`Self::set_input_callback`], which
    /// delivers the capability snapshot first.
    /// Owner-thread-only (ADR-0082 §4): the callback need not be `Send`.
    on_input: super::owner_slot::OwnerSlot<InputCallback>,
    input_feed: Mutex<InputFeed>,
    input_wake: Mutex<Option<InputWake>>,
    /// Keys answered "not handled" while queued behind a running callback,
    /// newest last, whose platform follow-up must not be delivered again.
    unanswered_keys: Mutex<VecDeque<flui_platform::keyboard::KeyEvent>>,

    /// Called when the platform requests a new frame to be rendered.
    pub on_request_frame: Mutex<Option<Box<dyn FnMut() + Send>>>,

    /// Called when the window is resized. Parameters: new size (logical), scale
    /// factor.
    pub on_resize: Mutex<Option<Box<dyn FnMut(WindowMetrics) + Send>>>,

    /// Called when the window state may differ from the one last read.
    pub on_state_changed: Mutex<Option<Box<dyn FnMut() + Send>>>,

    /// Called when the window is about to be destroyed. Only fires once
    /// (FnOnce).
    pub on_close: Mutex<Option<Box<dyn FnOnce() + Send>>>,

    /// Called to ask if the window should close. Return `false` to veto.
    pub on_should_close: Mutex<Option<Box<dyn FnMut() -> bool + Send>>>,

    /// Called when the window gains or loses focus. Parameter: is_active.
    pub on_active_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,

    /// Called when the window's visibility (occlusion) changes. Parameter:
    /// is_visible (`true` when the window becomes visible/unoccluded).
    ///
    /// Distinct from `on_active_status_change`: a window can be visible but
    /// unfocused (the `Inactive` lifecycle state), or focused but
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
    /// The native Web backend samples the canvas document's `hidden` value
    /// and observes Document `visibilitychange`, separately from canvas focus.
    /// Where no signal is ever delivered, this callback simply never fires
    /// — the window is treated as always visible (the same behavior as
    /// before this callback existed).
    pub on_visibility_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,

    /// Called when the mouse enters or leaves the window. Parameter:
    /// is_hovered.
    pub on_hover_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,

    /// Called when the system appearance (light/dark) changes.
    pub on_appearance_changed: Mutex<Option<Box<dyn FnMut() + Send>>>,

    /// Called when the display this window is on may have changed.
    pub on_display_changed: Mutex<Option<Box<dyn FnMut() + Send>>>,

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
    pub on_surface_status_change: Mutex<Option<Box<dyn FnMut(bool) + Send>>>,

    safe_area_dispatch: Mutex<DispatchState<flui_foundation::geometry::EdgeInsets>>,
    on_safe_area_change:
        Mutex<Option<Box<dyn FnMut(flui_foundation::geometry::EdgeInsets) + Send>>>,
    on_execution_state_change: Mutex<Option<Box<dyn FnMut(WindowExecutionState) + Send>>>,
    event_dispatch: Mutex<DispatchState<WindowCallbackEvent>>,
    /// Incremented each time a registered frame callback runs, nested
    /// requests drained later included; a request with no callback to run
    /// leaves it unchanged. Shareable so a backend can count frames across
    /// every window it opens ([`Self::counting_frames`]).
    frames_run: Arc<AtomicU64>,
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

/// Run by a queued clear once it has retired every callback; see
/// [`WindowCallbacks::clear_then`].
type RetirementNotice = Box<dyn FnOnce() + Send>;

type StatusCallback = Box<dyn FnMut(bool) + Send>;

enum WindowCallbackEvent {
    Input(PlatformInput),
    RequestFrame,
    Resize(WindowMetrics),
    StateChanged,
    Close,
    Active(bool, Option<Box<dyn FnOnce() + Send>>),
    Execution(WindowExecutionState),
    Visibility(bool, Option<Box<dyn FnOnce() + Send>>),
    Hover(bool),
    AppearanceChanged,
    DisplayChanged,
    SurfaceStatus(bool),
    /// A deferred [`WindowCallbacks::clear`]: queued instead of run inline
    /// when `clear()` is called while this window's FIFO is already
    /// draining (a `close()` issued from inside one of this window's own
    /// callbacks — see `clear()`'s own doc for why draining it in order
    /// matters).
    Clear(Option<RetirementNotice>),
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

    /// Become the drain owner without queueing an event; `None` while another
    /// drain runs.
    fn acquire(state: &'a Mutex<DispatchState<E>>) -> Option<Self> {
        let mut guard = state.lock();
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
            for event in pending {
                super::panic_boundary::contain_owner_callback(|| drop(event));
            }
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

#[cfg(target_os = "ios")]
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
            if std::thread::panicking() {
                // An invocation failure remains authoritative. Retire this
                // individual outgoing capture after releasing its slot guard;
                // its destructor must not abort the original unwind.
                super::panic_boundary::contain_owner_callback(|| drop(callback));
            } else {
                drop(callback);
            }
            return;
        }
        *slot = Some(callback);
    }
}

/// The input callback taken out of its owner-thread slot for one call; on
/// drop it goes back unless the window cleared or a replacement was
/// registered meanwhile, in which case it retires here. Every step runs on
/// the owner thread, so no lock orders the restore against a clear.
struct InputLease<'a> {
    slot: &'a super::owner_slot::OwnerSlot<InputCallback>,
    cleared: &'a AtomicBool,
    callback: Option<InputCallback>,
}

impl<'a> InputLease<'a> {
    fn take(
        slot: &'a super::owner_slot::OwnerSlot<InputCallback>,
        cleared: &'a AtomicBool,
    ) -> Self {
        Self {
            slot,
            cleared,
            callback: slot.take(),
        }
    }

    fn callback_mut(&mut self) -> Option<&mut InputCallback> {
        self.callback.as_mut()
    }

    /// Put the callback back; the outgoing one when the window cleared or a
    /// replacement was registered meanwhile.
    fn release(&mut self) -> Option<InputCallback> {
        let callback = self.callback.take()?;
        if self.cleared.load(Ordering::SeqCst) {
            return Some(callback);
        }
        self.slot.restore(callback).err()
    }

    /// End the lease after a call; `failed` when the call panicked and the
    /// panic was caught. An outgoing callback then is retained, not dropped:
    /// its captures stay out of an operation that already failed (ADR-0127
    /// §2). Otherwise it retires here, its capture failure contained.
    fn retire(mut self, failed: bool) {
        let outgoing = self.release();
        if failed {
            std::mem::forget(outgoing);
        } else {
            super::panic_boundary::contain_owner_callback(|| drop(outgoing));
        }
    }
}

impl Drop for InputLease<'_> {
    fn drop(&mut self) {
        let Some(outgoing) = self.release() else {
            return;
        };
        if std::thread::panicking() {
            // The invocation's failure is unwinding: the outgoing callback is
            // retained, so its captures cannot fail a second time (ADR-0127 §2).
            std::mem::forget(outgoing);
        } else {
            drop(outgoing);
        }
    }
}

/// Invoke a leased input callback with `input`, containing a panic; whether
/// it returned normally. The payload is forgotten, not dropped: its own
/// destructor may panic.
fn invoke_contained(lease: &mut InputLease<'_>, input: PlatformInput) -> bool {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(callback) = lease.callback_mut() {
            callback(input);
        }
    }));
    match outcome {
        Ok(()) => true,
        Err(payload) => {
            std::mem::forget(payload);
            super::panic_boundary::contain_owner_callback(|| {
                tracing::error!("input callback panicked; the queued input stays in order");
            });
            false
        }
    }
}

impl WindowCallbacks {
    /// Refuse registrations once native retirement has started.
    #[cfg(target_os = "windows")]
    pub(crate) fn accepts_registration(&self) -> bool {
        !self.lifecycle_closed.load(Ordering::SeqCst)
    }
    /// Create a new empty callback set
    pub fn new() -> Self {
        Self::counting_frames(Arc::default())
    }

    /// Create an empty callback set that increments `frames_run` each time
    /// its frame callback runs. A frame request with no callback to run (none
    /// registered yet, or the set already cleared) does not count, so a
    /// backend sharing one counter across its windows can tell a request a
    /// frame serviced from one that reached nothing.
    pub fn counting_frames(frames_run: Arc<AtomicU64>) -> Self {
        Self {
            frames_run,
            on_execution_state_change: Mutex::new(None),
            on_safe_area_change: Mutex::new(None),
            safe_area_dispatch: Mutex::new(DispatchState::new()),
            on_input: super::owner_slot::OwnerSlot::new(),
            input_feed: Mutex::new(InputFeed::new()),
            input_wake: Mutex::new(None),
            unanswered_keys: Mutex::new(VecDeque::new()),
            on_request_frame: Mutex::new(None),
            on_resize: Mutex::new(None),
            on_state_changed: Mutex::new(None),
            on_close: Mutex::new(None),
            on_should_close: Mutex::new(None),
            on_active_status_change: Mutex::new(None),
            on_visibility_status_change: Mutex::new(None),
            on_hover_status_change: Mutex::new(None),
            on_appearance_changed: Mutex::new(None),
            on_display_changed: Mutex::new(None),
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
    /// crates/flui-native/src` finds every call site that reaches this
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
        self.clear_with(None);
    }

    /// [`Self::clear`], then `on_retired` once every slot's callback has
    /// actually been dropped. When this window's FIFO is draining (a close
    /// issued from inside one of its own callbacks), that is after the
    /// running callback returns and the queued clear runs, not when this call
    /// returns: a backend uses it to order a notice after the release of what
    /// those callbacks own (a renderer and its surface). If the running
    /// callback unwinds, terminal clear and retirement notices still finish;
    /// accepted native focus commits and notifications also finish, while
    /// ordinary nested events from the aborted transaction are discarded.
    pub fn clear_then(&self, on_retired: impl FnOnce() + Send + 'static) {
        self.clear_with(Some(Box::new(on_retired)));
    }

    fn clear_with(&self, retired: Option<RetirementNotice>) {
        self.lifecycle_closed.store(true, Ordering::SeqCst);
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Clear(retired))
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
        let on_safe_area_change = self.on_safe_area_change.lock().take();
        let on_execution_state_change = self.on_execution_state_change.lock().take();
        // Off the owner thread this reaches nothing; the callback then
        // retires at the owner's next lease, which sees `cleared`.
        let on_input = self.on_input.take();
        let held = std::mem::take(&mut self.input_feed.lock().held);
        let input_wake = self.input_wake.lock().take();
        let on_request_frame = self.on_request_frame.lock().take();
        let on_resize = self.on_resize.lock().take();
        let on_state_changed = self.on_state_changed.lock().take();
        let on_close = self.on_close.lock().take();
        let on_should_close = self.on_should_close.lock().take();
        let on_active_status_change = self.on_active_status_change.lock().take();
        let on_visibility_status_change = self.on_visibility_status_change.lock().take();
        let on_hover_status_change = self.on_hover_status_change.lock().take();
        let on_appearance_changed = self.on_appearance_changed.lock().take();
        let on_display_changed = self.on_display_changed.lock().take();
        let on_surface_status_change = self.on_surface_status_change.lock().take();
        // Each slot retires independently: distinct panicking destructors must
        // never meet during the same unwind. No slot lock is held here.
        super::panic_boundary::contain_owner_callback(|| drop(on_safe_area_change));
        super::panic_boundary::contain_owner_callback(|| drop(on_execution_state_change));
        super::panic_boundary::contain_owner_callback(|| drop(on_input));
        for input in held {
            super::panic_boundary::contain_owner_callback(|| drop(input));
        }
        super::panic_boundary::contain_owner_callback(|| drop(input_wake));
        super::panic_boundary::contain_owner_callback(|| drop(on_request_frame));
        super::panic_boundary::contain_owner_callback(|| drop(on_resize));
        super::panic_boundary::contain_owner_callback(|| drop(on_state_changed));
        super::panic_boundary::contain_owner_callback(|| drop(on_close));
        super::panic_boundary::contain_owner_callback(|| drop(on_should_close));
        super::panic_boundary::contain_owner_callback(|| drop(on_active_status_change));
        super::panic_boundary::contain_owner_callback(|| drop(on_visibility_status_change));
        super::panic_boundary::contain_owner_callback(|| drop(on_hover_status_change));
        super::panic_boundary::contain_owner_callback(|| drop(on_appearance_changed));
        super::panic_boundary::contain_owner_callback(|| drop(on_display_changed));
        super::panic_boundary::contain_owner_callback(|| drop(on_surface_status_change));
    }

    fn drain_events(
        &self,
        mut drain: DispatchDrain<'_, WindowCallbackEvent>,
    ) -> Option<DispatchEventResult> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.drain_event_loop(&mut drain)
        })) {
            Ok(result) => result,
            Err(failure) => {
                // Keep the FIFO admitted during recovery: reentrant native
                // observations must follow the already accepted tail.
                while let Some(event) = drain.next() {
                    match event {
                        WindowCallbackEvent::Active(focused, Some(commit)) => {
                            // The backend has already accepted this observation.
                            // Dropping it would lose both its getter commit and
                            // notification, even after a duplicate native signal.
                            super::panic_boundary::contain_owner_callback(|| {
                                self.deliver_status_observation(
                                    &self.on_active_status_change,
                                    focused,
                                    Some(commit),
                                );
                            });
                        }
                        WindowCallbackEvent::Visibility(visible, Some(commit)) => {
                            super::panic_boundary::contain_owner_callback(|| {
                                self.deliver_status_observation(
                                    &self.on_visibility_status_change,
                                    visible,
                                    Some(commit),
                                );
                            });
                        }
                        WindowCallbackEvent::Close
                            if self.lifecycle_closed.load(Ordering::SeqCst) =>
                        {
                            // Closing the native registration is irreversible.
                            // Its application cancellation hook survives an
                            // aborted causal callback, before capture retirement.
                            let callback = self.on_close.lock().take();
                            if let Some(callback) = callback {
                                super::panic_boundary::contain_owner_callback(callback);
                            }
                        }
                        WindowCallbackEvent::Clear(retired) => {
                            self.clear_now();
                            if let Some(retired) = retired {
                                super::panic_boundary::contain_owner_callback(retired);
                            }
                        }
                        event => super::panic_boundary::contain_owner_callback(|| drop(event)),
                    }
                }
                if self.lifecycle_closed.load(Ordering::SeqCst) {
                    self.clear_now();
                }
                std::panic::resume_unwind(failure);
            }
        }
    }

    fn drain_event_loop(
        &self,
        drain: &mut DispatchDrain<'_, WindowCallbackEvent>,
    ) -> Option<DispatchEventResult> {
        let mut input_result = None;
        while let Some(event) = drain.next() {
            match event {
                WindowCallbackEvent::Input(event) => {
                    // Admitted before a re-registration: the new callback's
                    // snapshot still comes first.
                    if !self.deliver_pending_snapshot() {
                        self.redirect_behind_snapshot(event);
                        input_result.get_or_insert(DispatchEventResult::DEFERRED);
                        continue;
                    }
                    let mut lease = InputLease::take(&self.on_input, &self.cleared);
                    let result = lease
                        .callback_mut()
                        .map_or_else(DispatchEventResult::default, |callback| callback(event));
                    // An outgoing callback replaced during its call retires
                    // here, after its result; its capture failure is contained.
                    lease.retire(false);
                    input_result.get_or_insert(result);
                }
                WindowCallbackEvent::RequestFrame => {
                    let mut lease = CallbackLease::take(&self.on_request_frame, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        // Counted before the call, so a frame that unwinds
                        // still counts as one that ran.
                        self.frames_run.fetch_add(1, Ordering::Relaxed);
                        callback();
                    }
                }
                WindowCallbackEvent::Resize(metrics) => {
                    // Queued delivery is already fenced by `clear_now` having
                    // emptied this slot before the event runs, so this lease
                    // answers to `cleared`. The immediate iOS
                    // `dispatch_metrics_resize` leases the same slot against
                    // `lifecycle_closed` instead: its fence has to close at the
                    // close REQUEST, which can arrive while a drain is still
                    // holding `clear_now` back.
                    let mut lease = CallbackLease::take(&self.on_resize, &self.cleared);
                    if let Some(callback) = lease.callback_mut() {
                        callback(metrics);
                    }
                }
                WindowCallbackEvent::StateChanged => {
                    let mut lease = CallbackLease::take(&self.on_state_changed, &self.cleared);
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
                WindowCallbackEvent::Active(is_active, commit) => {
                    self.deliver_status_observation(
                        &self.on_active_status_change,
                        is_active,
                        commit,
                    );
                }
                WindowCallbackEvent::Visibility(is_visible, commit) => {
                    self.deliver_status_observation(
                        &self.on_visibility_status_change,
                        is_visible,
                        commit,
                    );
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
                WindowCallbackEvent::DisplayChanged => {
                    let mut lease = CallbackLease::take(&self.on_display_changed, &self.cleared);
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
                WindowCallbackEvent::Clear(retired) => {
                    self.clear_now();
                    if let Some(retired) = retired {
                        super::panic_boundary::contain_owner_callback(retired);
                    }
                }
            }
        }
        input_result
    }

    fn deliver_status_observation(
        &self,
        slot: &Mutex<Option<StatusCallback>>,
        value: bool,
        commit: Option<Box<dyn FnOnce() + Send>>,
    ) {
        if let Some(commit) = commit {
            commit();
        }
        let mut lease = CallbackLease::take(slot, &self.cleared);
        if let Some(callback) = lease.callback_mut() {
            callback(value);
        }
    }

    /// Dispatch an input event.
    ///
    /// The outer drain returns the callback result for its own event. A nested
    /// dispatch is queued, never dropped, and answers at once because its
    /// callback result is not known until the outer callback returns
    /// (ADR-0207 §7): a key gets [`DispatchEventResult::DEFERRED_UNHANDLED`],
    /// so its platform default runs now, and the platform's follow-up for it
    /// (AppKit resends an unhandled key equivalent as `keyDown:`) is not
    /// delivered again; any other input gets [`DispatchEventResult::DEFERRED`],
    /// which suppresses a default (Win32 mouse promotion) its queued delivery
    /// replaces.
    ///
    /// Until the input callback has received its capability snapshot, and
    /// while a failed pass's tail is still queued, the event waits behind
    /// them ([`Self::set_input_callback`]) and the result is
    /// [`DispatchEventResult::DEFERRED`] (the default result while no callback
    /// was ever registered: nothing claims the event yet). At most 256 entries
    /// are held; past that the oldest ordinary input is evicted. Once a
    /// registration's snapshot has failed every pass, nothing can deliver
    /// until the callback is registered again, so the event is refused with
    /// the default result instead of held. A [`PlatformInput::Capabilities`]
    /// is a capability change ([`Self::set_input_capabilities`]).
    pub fn dispatch_input(&self, event: PlatformInput) -> DispatchEventResult {
        if let PlatformInput::Capabilities(capabilities) = event {
            self.set_input_capabilities(capabilities);
            return DispatchEventResult::DEFERRED;
        }
        // The platform's follow-up for a key answered "not handled" while a
        // callback ran (AppKit resends it as `keyDown:`): that key is already
        // queued or delivered, so it is not delivered again.
        let key = match &event {
            PlatformInput::Keyboard(key) => Some(key.clone()),
            _ => None,
        };
        if let Some(key) = &key {
            let mut unanswered = self.unanswered_keys.lock();
            if let Some(index) = unanswered.iter().position(|marker| marker == key) {
                unanswered.remove(index);
                return DispatchEventResult::default();
            }
        }
        {
            let mut feed = self.input_feed.lock();
            if feed.stalled() {
                // Nothing delivers held input until the callback is
                // registered again: the event is refused, not held, and its
                // platform default runs.
                feed.count_lost();
                return DispatchEventResult::default();
            }
            if feed.holds() {
                let evicted = feed.hold(event);
                // With no application callback yet nothing claims the event,
                // so the platform default is not suppressed on its behalf.
                let result = if feed.registered {
                    DispatchEventResult::DEFERRED
                } else {
                    DispatchEventResult::default()
                };
                drop(feed);
                super::panic_boundary::contain_owner_callback(|| drop(evicted));
                self.resume_input_delivery();
                return result;
            }
        }
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Input(event))
        else {
            // Queued behind the running callback. A key cannot wait for its
            // answer: it is answered "not handled" so the platform default
            // runs now, and its follow-up is recognized above.
            let Some(key) = key else {
                return DispatchEventResult::DEFERRED;
            };
            let mut unanswered = self.unanswered_keys.lock();
            if unanswered.len() == UNANSWERED_KEYS {
                unanswered.pop_front();
            }
            unanswered.push_back(key);
            return DispatchEventResult::DEFERRED_UNHANDLED;
        };
        self.drain_events(drain).unwrap_or_default()
    }

    /// Install the window's input callback (ADR-0213 §3).
    ///
    /// The registration is committed first; the outgoing callback is retired
    /// after the slot's guard is released, its capture failure contained.
    /// The new callback then receives the window's capability snapshot
    /// ([`PlatformInput::Capabilities`]) before any other input, followed by
    /// the input held while no snapshot had been delivered, in admission
    /// order. Delivery runs now unless this window's events are already
    /// draining (a registration from inside a callback), in which case the
    /// backend's wake ([`Self::set_input_wake`]) resumes it. After the window
    /// closes, a registration is dropped.
    ///
    /// The callback is held for the thread that created this window, which
    /// delivers its input (ADR-0082 §4): a registration from any other thread
    /// is refused and the callback dropped on the thread that made it.
    pub fn set_input_callback(&self, callback: InputCallback) {
        let outgoing = if self.cleared.load(Ordering::SeqCst) {
            Some(callback)
        } else {
            let registration = self.input_feed.lock().registrations.next();
            match registration {
                Ok(registration) => match self.on_input.replace(callback) {
                    Ok(outgoing) => {
                        let mut feed = self.input_feed.lock();
                        feed.snapshot_pending = true;
                        feed.registered = true;
                        feed.failed_snapshots = 0;
                        feed.registration = registration.get();
                        outgoing
                    }
                    Err(refused) => {
                        tracing::error!(
                            "input callback refused: a window's input callback is registered \
                             on its owner thread; the callback was dropped on the calling thread"
                        );
                        Some(refused)
                    }
                },
                Err(_exhausted) => {
                    tracing::error!(
                        "input callback refused: this window issued every registration it can"
                    );
                    Some(callback)
                }
            }
        };
        super::panic_boundary::contain_owner_callback(|| drop(outgoing));
        self.resume_input_delivery();
    }

    /// Record the window's input capabilities: before the input callback has
    /// its snapshot (and while no ordinary input waits), the snapshot it will
    /// receive; after that, a change delivered in order with the input around
    /// it. A backend sets the initial value when it builds the window,
    /// reflecting the devices already attached.
    pub fn set_input_capabilities(&self, capabilities: InputCapabilities) {
        {
            let mut feed = self.input_feed.lock();
            if !feed.held.is_empty() {
                let evicted = feed.hold(PlatformInput::Capabilities(capabilities));
                drop(feed);
                super::panic_boundary::contain_owner_callback(|| drop(evicted));
                self.resume_input_delivery();
                return;
            }
            feed.front = capabilities;
            if feed.snapshot_pending {
                return;
            }
        }
        if let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Input(PlatformInput::Capabilities(capabilities)),
        ) {
            self.drain_events(drain);
        }
    }

    /// The window's current input capabilities, as the latest snapshot or
    /// change admitted describes them.
    #[must_use]
    pub fn input_capabilities(&self) -> InputCapabilities {
        let feed = self.input_feed.lock();
        feed.held
            .iter()
            .rev()
            .find_map(|input| match input {
                PlatformInput::Capabilities(capabilities) => Some(*capabilities),
                _ => None,
            })
            .unwrap_or(feed.front)
    }

    /// Install the backend's owner-thread wake: after a contained callback
    /// failure, or when input arrived during a pass, the backend schedules an
    /// owner turn that calls [`Self::resume_input_delivery`], so a queued
    /// tail does not wait for unrelated input.
    pub fn set_input_wake(&self, wake: InputWake) {
        let outgoing = self.input_wake.lock().replace(wake);
        super::panic_boundary::contain_owner_callback(|| drop(outgoing));
    }

    /// Deliver the pending snapshot and held input to the input callback.
    ///
    /// One pass delivers only the entries present when it started; input
    /// admitted during it waits for the next pass, which the backend's wake
    /// schedules. A callback panic is contained here: the failed snapshot
    /// stays at the head and is retried (at most three failed passes per
    /// registration), a failed ordinary entry is not re-delivered, and the
    /// rest stays queued in order. Called from the backend's wake on the
    /// owner thread; while this window's events drain it only asks for
    /// another wake.
    pub fn resume_input_delivery(&self) {
        let Some(drain) = DispatchDrain::acquire(&self.event_dispatch) else {
            self.wake_input();
            return;
        };
        self.input_feed.lock().redirected = 0;
        let rewake = self.input_pass();
        // Window events admitted while the pass ran drain in their order.
        self.drain_events(drain);
        if rewake {
            self.wake_input();
        }
    }

    /// One bounded pass; whether another pass should be woken.
    fn input_pass(&self) -> bool {
        let budget = {
            let feed = self.input_feed.lock();
            feed.held.len() + usize::from(feed.snapshot_pending)
        };
        for _ in 0..budget {
            let mut lease = InputLease::take(&self.on_input, &self.cleared);
            if lease.callback.is_none() {
                // Held until a registration; a snapshot is never emitted
                // into an absent callback.
                return false;
            }
            let entry = {
                let mut feed = self.input_feed.lock();
                if feed.snapshot_pending {
                    if feed.failed_snapshots >= SNAPSHOT_ATTEMPTS {
                        drop(feed);
                        drop(lease);
                        return false;
                    }
                    HeldEntry::Snapshot(feed.front, feed.registration)
                } else if let Some(input) = feed.held.pop_front() {
                    if let PlatformInput::Capabilities(capabilities) = &input {
                        feed.front = *capabilities;
                    }
                    HeldEntry::Input(input)
                } else {
                    drop(feed);
                    drop(lease);
                    return false;
                }
            };
            let (input, snapshot) = match entry {
                HeldEntry::Snapshot(capabilities, registration) => (
                    PlatformInput::Capabilities(capabilities),
                    Some(registration),
                ),
                HeldEntry::Input(input) => (input, None),
            };
            let delivered = invoke_contained(&mut lease, input);
            lease.retire(!delivered);
            match (delivered, snapshot) {
                (true, Some(registration)) => self.snapshot_delivered(registration),
                (true, None) => {}
                (false, Some(registration)) => return self.snapshot_failed(registration),
                // The entry is not re-delivered; the tail stays queued.
                (false, None) => return true,
            }
        }
        // Input admitted during the pass waits for the next one.
        self.input_feed.lock().holds() && self.on_input.is_occupied() == Some(true)
    }

    fn snapshot_delivered(&self, registration: u64) {
        let mut feed = self.input_feed.lock();
        if feed.registration == registration {
            feed.snapshot_pending = false;
            feed.failed_snapshots = 0;
        }
    }

    /// Count a failed snapshot; whether it is retried.
    fn snapshot_failed(&self, registration: u64) -> bool {
        let mut feed = self.input_feed.lock();
        if feed.registration != registration {
            // A replacement registered during the call; its snapshot is due.
            return true;
        }
        feed.failed_snapshots = feed.failed_snapshots.saturating_add(1);
        feed.failed_snapshots < SNAPSHOT_ATTEMPTS
    }

    /// Inside a FIFO drain: deliver the snapshot a re-registration owes before
    /// the drain's next input. `false` when it could not be delivered.
    fn deliver_pending_snapshot(&self) -> bool {
        let (capabilities, registration) = {
            let feed = self.input_feed.lock();
            if !feed.snapshot_pending {
                return true;
            }
            if feed.failed_snapshots >= SNAPSHOT_ATTEMPTS {
                return false;
            }
            (feed.front, feed.registration)
        };
        let mut lease = InputLease::take(&self.on_input, &self.cleared);
        if lease.callback.is_none() {
            return false;
        }
        let delivered = invoke_contained(&mut lease, PlatformInput::Capabilities(capabilities));
        lease.retire(!delivered);
        if delivered {
            self.snapshot_delivered(registration);
        } else if self.snapshot_failed(registration) {
            self.wake_input();
        }
        delivered
    }

    /// Hold an input a FIFO drain could not deliver behind the snapshot,
    /// ahead of newer held input.
    fn redirect_behind_snapshot(&self, input: PlatformInput) {
        let mut feed = self.input_feed.lock();
        let at = feed.redirected;
        let (at, evicted) = feed.hold_at(at, input);
        feed.redirected = at + 1;
        drop(feed);
        super::panic_boundary::contain_owner_callback(|| drop(evicted));
    }

    fn wake_input(&self) {
        let wake = self.input_wake.lock().clone();
        if let Some(wake) = wake {
            super::panic_boundary::contain_owner_callback(|| wake());
        }
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

    /// Dispatch a resize event with the window's new metrics.
    pub fn dispatch_resize(&self, metrics: WindowMetrics) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::Resize(metrics))
        else {
            return;
        };
        self.drain_events(drain);
    }

    /// [`Self::dispatch_resize`] for a backend that reports a logical size: the device size is
    /// the logical size at `scale_factor`, rounded. A report that makes no metrics (a zero or
    /// non-finite size, a non-positive or non-finite scale) is not delivered.
    pub fn dispatch_logical_resize(&self, size: Size<f64>, scale_factor: f64) {
        match WindowMetrics::from_logical(size, scale_factor) {
            Ok(metrics) => self.dispatch_resize(metrics),
            Err(reason) => tracing::debug!(%reason, "resize report dropped"),
        }
    }

    /// Dispatch a window state change.
    pub fn dispatch_state_changed(&self) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::StateChanged)
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
    #[cfg(target_os = "ios")]
    pub(crate) fn dispatch_lifecycle_immediate(&self, event: LifecycleEvent) {
        use LifecycleEvent::{Execution, Focus, Surface, Visibility};
        match event {
            Execution(value) => self.invoke_lifecycle(&self.on_execution_state_change, value),
            Focus(value) => self.invoke_lifecycle(&self.on_active_status_change, value),
            Visibility(value) => self.invoke_lifecycle(&self.on_visibility_status_change, value),
            Surface(value) => self.invoke_lifecycle(&self.on_surface_status_change, value),
        }
    }

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
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Active(is_active, None),
        ) else {
            return;
        };
        self.drain_events(drain);
    }

    /// Publish a backend observation at its causal delivery point, so getters
    /// inside a queued focus callback describe that callback's own observation.
    #[cfg(any(test, target_arch = "wasm32"))]
    pub(crate) fn dispatch_focus_observation(
        &self,
        focused: bool,
        commit: impl FnOnce() + Send + 'static,
    ) {
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Active(focused, Some(Box::new(commit))),
        ) else {
            return;
        };
        self.drain_events(drain);
    }

    /// Dispatch visibility status change (occlusion gained/lost).
    pub fn dispatch_visibility_status_change(&self, is_visible: bool) {
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Visibility(is_visible, None),
        ) else {
            return;
        };
        self.drain_events(drain);
    }

    /// Commit an accepted visibility observation immediately before its callback.
    #[cfg(any(test, target_arch = "wasm32"))]
    pub(crate) fn dispatch_visibility_observation(
        &self,
        visible: bool,
        commit: impl FnOnce() + Send + 'static,
    ) {
        let Some(drain) = DispatchDrain::begin(
            &self.event_dispatch,
            WindowCallbackEvent::Visibility(visible, Some(Box::new(commit))),
        ) else {
            return;
        };
        self.drain_events(drain);
    }

    /// Replace a safe-area observer without holding storage through capture Drop.
    pub fn set_safe_area_callback(
        &self,
        callback: Box<dyn FnMut(flui_foundation::geometry::EdgeInsets) + Send>,
    ) {
        let old = {
            let mut slot = self.on_safe_area_change.lock();
            if self.lifecycle_closed.load(Ordering::SeqCst) {
                drop(slot);
                super::panic_boundary::contain_owner_callback(|| drop(callback));
                return;
            }
            slot.replace(callback)
        };
        super::panic_boundary::contain_owner_callback(|| drop(old));
    }

    /// Immediate metrics delivery, independently leased from the input FIFO.
    pub fn dispatch_safe_area_change(&self, insets: flui_foundation::geometry::EdgeInsets) {
        if self.lifecycle_closed.load(Ordering::SeqCst) {
            return;
        }
        let Some(mut drain) = DispatchDrain::begin(&self.safe_area_dispatch, insets) else {
            return;
        };
        while let Some(insets) = drain.next() {
            self.invoke_lifecycle(&self.on_safe_area_change, insets);
        }
    }

    #[cfg(target_os = "ios")]
    pub(crate) fn dispatch_metrics_resize(&self, size: Size<f64>, scale: f64) {
        if self.lifecycle_closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(metrics) = WindowMetrics::from_logical(size, scale) else {
            return;
        };
        // Latched on `lifecycle_closed`, deliberately not the `cleared` flag
        // the queued Resize arm leases against (see `drain_events`'s own arm):
        // this is an IMMEDIATE delivery, so it is fenced the moment a close is
        // requested — `clear()` sets this latch on entry — rather than at slot
        // teardown, which can sit behind a still-running drain.
        let mut lease = CallbackLease::take(&self.on_resize, &self.lifecycle_closed);
        super::panic_boundary::contain_owner_callback(|| {
            if let Some(callback) = lease.callback_mut() {
                callback(metrics);
            }
        });
        super::panic_boundary::contain_owner_callback(|| drop(lease));
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

    /// Dispatch a display change hint (the window may be on another display, or
    /// its display changed).
    pub fn dispatch_display_changed(&self) {
        let Some(drain) =
            DispatchDrain::begin(&self.event_dispatch, WindowCallbackEvent::DisplayChanged)
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
                dyn FnMut($crate::traits::PlatformInput) -> $crate::traits::DispatchEventResult,
            >,
        ) {
            self.$callbacks_field.set_input_callback(callback);
        }

        fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
            *self.$callbacks_field.on_request_frame.lock() = Some(callback);
        }

        fn on_resize(
            &self,
            callback: Box<dyn FnMut(::flui_foundation::geometry::WindowMetrics) + Send>,
        ) {
            *self.$callbacks_field.on_resize.lock() = Some(callback);
        }

        fn on_state_changed(&self, callback: Box<dyn FnMut() + Send>) {
            *self.$callbacks_field.on_state_changed.lock() = Some(callback);
        }

        fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
            *self.$callbacks_field.on_close.lock() = Some(callback);
        }

        fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
            *self.$callbacks_field.on_should_close.lock() = Some(callback);
        }

        fn on_safe_area_change(
            &self,
            callback: Box<dyn FnMut(flui_foundation::geometry::EdgeInsets) + Send>,
        ) {
            self.$callbacks_field.set_safe_area_callback(callback);
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

        fn on_display_changed(&self, callback: Box<dyn FnMut() + Send>) {
            *self.$callbacks_field.on_display_changed.lock() = Some(callback);
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
            .field("on_input", &self.on_input.is_occupied())
            .field("on_request_frame", &self.on_request_frame.lock().is_some())
            .field("on_resize", &self.on_resize.lock().is_some())
            .field("on_state_changed", &self.on_state_changed.lock().is_some())
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
                "on_display_changed",
                &self.on_display_changed.lock().is_some(),
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
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, AtomicUsize};

    use crate::shared::{hwnd_affinity, owner_signal};

    use super::*;

    #[test]
    fn session_end_hook_runs_without_the_lock_and_returns_to_its_slot() {
        let handlers = Mutex::new(PlatformHandlers::new());
        assert_eq!(
            PlatformHandlers::run_session_end(&handlers, SessionEndPhase::Ending),
            SessionEndAnswer::Proceed,
            "no hook registered"
        );

        let phases = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&phases);
        let handlers = Arc::new(handlers);
        let locked = Arc::clone(&handlers);
        handlers.lock().session_end = Some(Box::new(move |phase| {
            recorded.lock().push(phase);
            // Not held while the hook runs: this would deadlock otherwise.
            drop(locked.lock());
            SessionEndAnswer::Block
        }));

        for phase in [SessionEndPhase::Query, SessionEndPhase::Ending] {
            assert_eq!(
                PlatformHandlers::run_session_end(&handlers, phase),
                SessionEndAnswer::Block
            );
        }
        assert_eq!(
            *phases.lock(),
            [SessionEndPhase::Query, SessionEndPhase::Ending],
            "the hook came back after each run"
        );
    }

    #[test]
    fn session_end_hook_survives_its_own_panic() {
        let handlers = Mutex::new(PlatformHandlers::new());
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&runs);
        handlers.lock().session_end = Some(Box::new(move |_| {
            assert!(
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0,
                "the first run fails"
            );
            SessionEndAnswer::Proceed
        }));

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            PlatformHandlers::run_session_end(&handlers, SessionEndPhase::Ending)
        }));
        assert!(outcome.is_err());
        assert_eq!(
            PlatformHandlers::run_session_end(&handlers, SessionEndPhase::Ending),
            SessionEndAnswer::Proceed
        );
        assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn session_end_hook_registered_while_running_replaces_the_running_one() {
        let handlers = Arc::new(Mutex::new(PlatformHandlers::new()));
        let registering = Arc::clone(&handlers);
        handlers.lock().session_end = Some(Box::new(move |_| {
            registering.lock().session_end = Some(Box::new(|_| SessionEndAnswer::Block));
            SessionEndAnswer::Proceed
        }));

        assert_eq!(
            PlatformHandlers::run_session_end(&handlers, SessionEndPhase::Ending),
            SessionEndAnswer::Proceed
        );
        assert_eq!(
            PlatformHandlers::run_session_end(&handlers, SessionEndPhase::Ending),
            SessionEndAnswer::Block,
            "the newer hook kept the slot"
        );
    }

    fn accepted_focus_observations_survive_dispatch_unwind() {
        accepted_status_observations_survive_dispatch_unwind(false);
    }

    fn accepted_visibility_observations_survive_dispatch_unwind() {
        accepted_status_observations_survive_dispatch_unwind(true);
    }

    fn accepted_status_observations_survive_dispatch_unwind(visibility: bool) {
        use std::sync::atomic::AtomicBool;

        let dispatch: fn(&WindowCallbacks, bool, Box<dyn FnOnce() + Send>) = if visibility {
            WindowCallbacks::dispatch_visibility_observation
        } else {
            WindowCallbacks::dispatch_focus_observation
        };
        for (opposing, reenter, fail_notification, expected) in [
            (false, false, false, &[false][..]),
            (true, false, false, &[false, true][..]),
            (true, true, false, &[false, true, false][..]),
            (true, true, true, &[false, true, false][..]),
        ] {
            let callbacks = Arc::new(WindowCallbacks::new());
            let focused = Arc::new(AtomicBool::new(true));
            let notifications = Arc::new(Mutex::new(Vec::new()));
            let nested = Arc::downgrade(&callbacks);
            let state = Arc::clone(&focused);
            let delivered = Arc::clone(&notifications);
            let first = AtomicBool::new(true);
            let slot = if visibility {
                &callbacks.on_visibility_status_change
            } else {
                &callbacks.on_active_status_change
            };
            *slot.lock() = Some(Box::new(move |value| {
                assert_eq!(state.load(Ordering::SeqCst), value, "causal status getter");
                delivered.lock().push(value);
                if first.swap(false, Ordering::SeqCst) {
                    if reenter {
                        let state = Arc::clone(&state);
                        dispatch(
                            &nested.upgrade().expect("live owner"),
                            false,
                            Box::new(move || {
                                state.store(false, Ordering::SeqCst);
                            }),
                        );
                    }
                    assert!(!fail_notification, "competing status failure");
                }
            }));
            let changed = Arc::new(AtomicBool::new(false));
            let delivered = Arc::clone(&changed);
            *callbacks.on_state_changed.lock() = Some(Box::new(move || {
                delivered.store(true, Ordering::SeqCst);
            }));
            let nested = Arc::downgrade(&callbacks);
            let state = Arc::clone(&focused);
            callbacks.set_input_callback(Box::new(move |input| {
                if matches!(input, PlatformInput::Capabilities(_)) {
                    return DispatchEventResult::default();
                }
                let owner = nested.upgrade().expect("live owner");
                let state_lost = Arc::clone(&state);
                dispatch(
                    &owner,
                    false,
                    Box::new(move || {
                        state_lost.store(false, Ordering::SeqCst);
                    }),
                );
                if opposing {
                    let state_restored = Arc::clone(&state);
                    dispatch(
                        &owner,
                        true,
                        Box::new(move || {
                            state_restored.store(true, Ordering::SeqCst);
                        }),
                    );
                }
                owner.dispatch_state_changed();
                panic!("original dispatch failure");
            }));
            let failure = catch_unwind(AssertUnwindSafe(|| {
                callbacks.dispatch_input(PlatformInput::Ime(flui_platform::ImeEvent::Enabled));
            }))
            .expect_err("original callback failure");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some("original dispatch failure")
            );
            assert_eq!(&*notifications.lock(), expected, "accepted status delivery");
            assert_eq!(
                focused.load(Ordering::SeqCst),
                *expected.last().expect("status")
            );
            assert!(
                !changed.load(Ordering::SeqCst),
                "aborted ordinary event was replayed"
            );
            let state = Arc::clone(&focused);
            let delivered = notifications.lock().len();
            dispatch(
                &callbacks,
                true,
                Box::new(move || state.store(true, Ordering::SeqCst)),
            );
            assert_eq!(notifications.lock().len(), delivered + 1, "next delivery");
            assert_eq!(
                notifications.lock().last(),
                Some(&true),
                "next healthy observation"
            );
            callbacks.clear();
        }
    }

    fn focus_recovery_finishes_a_reentrant_close() {
        use std::sync::atomic::AtomicBool;

        let callbacks = Arc::new(WindowCallbacks::new());
        let closed = Arc::new(AtomicBool::new(false));
        let notice = Arc::clone(&closed);
        *callbacks.on_close.lock() = Some(Box::new(move || {
            notice.store(true, Ordering::SeqCst);
        }));
        let nested = Arc::downgrade(&callbacks);
        *callbacks.on_active_status_change.lock() = Some(Box::new(move |_| {
            let owner = nested.upgrade().expect("live owner");
            owner.dispatch_close();
            owner.clear();
        }));
        let nested = Arc::downgrade(&callbacks);
        callbacks.set_input_callback(Box::new(move |input| {
            if matches!(input, PlatformInput::Capabilities(_)) {
                return DispatchEventResult::default();
            }
            nested
                .upgrade()
                .expect("live owner")
                .dispatch_focus_observation(false, || {});
            panic!("original input failure");
        }));
        let failure = catch_unwind(AssertUnwindSafe(|| {
            callbacks.dispatch_input(PlatformInput::Ime(flui_platform::ImeEvent::Enabled));
        }))
        .expect_err("original input failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("original input failure")
        );
        assert!(
            closed.load(Ordering::SeqCst),
            "required terminal hook was lost"
        );
        callbacks.dispatch_active_status_change(true);
    }

    fn clear_multiple_panicking_captures(queued: bool) {
        struct Capture(Arc<AtomicU32>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("distinct callback capture");
            }
        }
        let callbacks = Arc::new(WindowCallbacks::new());
        let drops = Arc::new(AtomicU32::new(0));
        let first = Capture(drops.clone());
        let second = Capture(drops.clone());
        *callbacks.on_resize.lock() = Some(Box::new(move |_| {
            let _ = &first;
        }));
        *callbacks.on_state_changed.lock() = Some(Box::new(move || {
            let _ = &second;
        }));
        let closed = Arc::new(AtomicU32::new(0));
        let seen = closed.clone();
        *callbacks.on_close.lock() = Some(Box::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
        }));
        let inner = callbacks.clone();
        if queued {
            *callbacks.on_request_frame.lock() = Some(Box::new(move || {
                inner.dispatch_close();
                inner.clear();
            }));
            callbacks.dispatch_request_frame();
        } else {
            callbacks.dispatch_close();
            callbacks.clear();
        }
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(callbacks.cleared.load(Ordering::SeqCst));
        assert!(callbacks.on_resize.lock().is_none());
        assert!(callbacks.on_state_changed.lock().is_none());
    }

    fn owner_callback_and_capture_panics_do_not_replace_an_existing_unwind() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Hostile;
        impl Drop for Hostile {
            fn drop(&mut self) {
                panic!("hostile payload drop");
            }
        }
        struct Capture(Arc<AtomicUsize>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(Hostile);
            }
        }
        struct Guard(Option<Box<dyn FnMut() + Send>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                crate::shared::panic_boundary::invoke_and_drop_owner_callback(
                    self.0.take().expect("callback"),
                );
            }
        }
        for already_unwinding in [false, true] {
            let invoked = Arc::new(AtomicUsize::new(0));
            let dropped = Arc::new(AtomicUsize::new(0));
            let capture = Capture(Arc::clone(&dropped));
            let observed = Arc::clone(&invoked);
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let guard = Guard(Some(Box::new(move || {
                    let _ = &capture;
                    observed.fetch_add(1, Ordering::SeqCst);
                    std::panic::panic_any(Hostile);
                })));
                assert!(!already_unwinding, "original bootstrap panic");
                drop(guard);
            }));
            if already_unwinding {
                assert_eq!(
                    *outcome
                        .expect_err("original panic")
                        .downcast::<&str>()
                        .expect("original identity"),
                    "original bootstrap panic"
                );
            } else {
                assert!(outcome.is_ok());
            }
            assert_eq!(invoked.load(Ordering::SeqCst), 1);
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
    }

    fn off_owner_retention_case(fail_report: bool, already_unwinding: bool) {
        struct Resource(Arc<AtomicUsize>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("owner-affine resource retired on a foreign thread");
            }
        }
        struct Guard {
            resource: Option<(Resource, Resource)>,
            report_drops: Arc<AtomicUsize>,
            fail_report: bool,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                let report_drops = Arc::clone(&self.report_drops);
                let fail_report = self.fail_report;
                crate::shared::panic_boundary::retain_off_owner_resource(
                    self.resource.take().expect("resource"),
                    || {
                        if fail_report {
                            std::panic::panic_any((
                                Resource(Arc::clone(&report_drops)),
                                Resource(report_drops),
                            ));
                        }
                    },
                );
            }
        }
        let resource_drops = Arc::new(AtomicUsize::new(0));
        let report_drops = Arc::new(AtomicUsize::new(0));
        let result = catch_unwind(AssertUnwindSafe(|| {
            let guard = Guard {
                resource: Some((
                    Resource(Arc::clone(&resource_drops)),
                    Resource(Arc::clone(&resource_drops)),
                )),
                report_drops: Arc::clone(&report_drops),
                fail_report,
            };
            assert!(!already_unwinding, "original teardown failure");
            drop(guard);
        }));
        assert_eq!(resource_drops.load(Ordering::SeqCst), 0);
        assert_eq!(report_drops.load(Ordering::SeqCst), 0);
        if already_unwinding {
            assert_eq!(
                *result
                    .expect_err("original failure")
                    .downcast::<&str>()
                    .expect("original payload"),
                "original teardown failure"
            );
        } else {
            assert!(result.is_ok(), "report failure must remain contained");
        }
        let next = AtomicUsize::new(0);
        crate::shared::panic_boundary::contain_owner_callback(|| {
            next.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(next.load(Ordering::SeqCst), 1);
    }

    fn off_owner_retention_precedes_normal_diagnostics() {
        off_owner_retention_case(false, false);
    }
    fn off_owner_retention_contains_opaque_diagnostic_failure() {
        off_owner_retention_case(true, false);
    }
    fn off_owner_retention_preserves_an_unwind_with_normal_diagnostics() {
        off_owner_retention_case(false, true);
    }
    fn off_owner_retention_preserves_the_existing_unwind() {
        off_owner_retention_case(true, true);
    }
    fn clear_multiple_panicking_captures_directly() {
        clear_multiple_panicking_captures(false);
    }

    const HW_OWNER: u32 = 7;
    const HW_FOREIGN: u32 = 8;

    fn refusal_precedence_is_gone_then_foreign_thread_then_class_then_slot() {
        // All four conditions failing at once report the highest-precedence
        // refusal, walking down as each earlier condition is repaired.
        assert_eq!(
            hwnd_affinity::classify_user_data_access(
                hwnd_affinity::OWNER_GONE,
                HW_FOREIGN,
                false,
                0
            ),
            hwnd_affinity::UserDataVerdict::Refuse(hwnd_affinity::UserDataRefusal::WindowGone)
        );
        assert_eq!(
            hwnd_affinity::classify_user_data_access(HW_OWNER, HW_FOREIGN, false, 0),
            hwnd_affinity::UserDataVerdict::Refuse(hwnd_affinity::UserDataRefusal::ForeignThread)
        );
        assert_eq!(
            hwnd_affinity::classify_user_data_access(HW_OWNER, HW_OWNER, false, 0),
            hwnd_affinity::UserDataVerdict::Refuse(hwnd_affinity::UserDataRefusal::ForeignClass)
        );
        assert_eq!(
            hwnd_affinity::classify_user_data_access(HW_OWNER, HW_OWNER, true, 0),
            hwnd_affinity::UserDataVerdict::Refuse(hwnd_affinity::UserDataRefusal::EmptySlot)
        );
    }

    fn worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = owner_signal::OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&calls);
        let weak = Arc::downgrade(&signal);
        signal
            .register(Box::new(move || {
                if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                    let signal = weak.upgrade().expect("live");
                    signal.wake().expect("reentrant wake");
                    assert!(!signal.drive());
                }
            }))
            .expect("register");
        signal.start().expect("start");
        let worker = Arc::clone(&signal);
        std::thread::spawn(move || {
            for _ in 0..100 {
                worker.wake().expect("wake");
            }
        })
        .join()
        .expect("worker");
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        signal.close();
    }

    /// The owner-thread machinery's contracts: hostile callback captures and
    /// payloads are contained (a direct clear drops each capture without
    /// unwinding; an owner callback's own and capture panics never replace an
    /// existing unwind), the HWND user-data refusal precedence holds, and the
    /// owner signal coalesces a burst and gives a re-entrant wake a later turn.
    #[test]
    fn the_owner_thread_machinery_honours_its_contracts() {
        let cases: &[(&str, fn())] = &[
            (
                "accepted focus observations survive dispatch unwind",
                accepted_focus_observations_survive_dispatch_unwind,
            ),
            (
                "accepted visibility observations survive dispatch unwind",
                accepted_visibility_observations_survive_dispatch_unwind,
            ),
            (
                "focus recovery finishes a reentrant close",
                focus_recovery_finishes_a_reentrant_close,
            ),
            #[cfg(target_os = "windows")]
            (
                "window identity exhaustion",
                crate::platforms::windows::WindowsPlatform::window_identity_exhaustion_permanently_refuses_retries,
            ),
            #[cfg(target_os = "windows")]
            (
                "platform identity exhaustion",
                crate::platforms::windows::WindowsPlatform::platform_identity_exhaustion_acquires_nothing,
            ),
            (
                "clear multiple panicking captures",
                clear_multiple_panicking_captures_directly,
            ),
            (
                "owner callback and capture panic priority",
                owner_callback_and_capture_panics_do_not_replace_an_existing_unwind,
            ),
            (
                "refusal precedence",
                refusal_precedence_is_gone_then_foreign_thread_then_class_then_slot,
            ),
            (
                "worker burst and reentrant wake",
                worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn,
            ),
            (
                "off-owner normal diagnostics",
                off_owner_retention_precedes_normal_diagnostics,
            ),
            (
                "off-owner diagnostic failure",
                off_owner_retention_contains_opaque_diagnostic_failure,
            ),
            (
                "off-owner unwind with normal diagnostics",
                off_owner_retention_preserves_an_unwind_with_normal_diagnostics,
            ),
            (
                "off-owner existing unwind",
                off_owner_retention_preserves_the_existing_unwind,
            ),
        ];
        let mut failures = Vec::new();
        for &(name, case) in cases {
            if catch_unwind(AssertUnwindSafe(case)).is_err() {
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "failed owner machinery cases: {failures:?}"
        );
    }
}
