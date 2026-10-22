//! The handlers the platform calls: the application's and each window's.
//!
//! Two levels of callbacks:
//! - [`PlatformHandlers`]: Global platform-level callbacks (quit, reopen, etc.)
//! - [`WindowCallbacks`]: Per-window callbacks (input, resize, close, etc.)
//!
//! Every one of them is called through the call point
//! (`call_point::call`), which holds no lock or borrow while
//! it runs, and every replaced one is retired outside the locks
//! (`call_point::install`).

use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use flui_foundation::IdMint;
use flui_foundation::geometry::{Size, WindowMetrics};
use parking_lot::Mutex;

use flui_platform::input_capabilities::InputCapabilities;

use flui_platform::{
    DispatchEventResult, PlatformInput, SessionEndAnswer, SessionEndPhase, WindowEvent,
    WindowExecutionState,
};

use super::call_point::{Called, HandlerSlot, call, retire};
use super::panic_boundary::contain_owner_callback;

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

/// The application's handlers: what the platform calls on its own events
/// (quit, a window event, the session ending, …).
///
/// A host keeps this set behind its own lock or `RefCell` and reaches each
/// field as a `call_point::Field` slot: a handler is called
/// through the call point with that lock released, and one being replaced is
/// dropped after it.
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
    /// drain-before-decide rule). A backend that never consults it is
    /// unaffected by this field at all. With none set, winit, Win32 and
    /// AppKit allow the exit (the pre-#555 unconditional "last window closed
    /// -> exit" default), and headless never quits, so an embedder that never
    /// registers a hook sees no behavior change.
    pub exit_policy: Option<Box<dyn Fn() -> bool + Send>>,

    /// Consulted once per idle event-loop iteration for the earliest
    /// wall-clock instant the loop should wake at (issue #556's wall-clock
    /// wake seam) — `None` (unset, or the hook itself answers `None`) keeps
    /// the unconditional `ControlFlow::Wait` behavior exactly as before this
    /// field existed. A backend that never reads this field at all is
    /// unaffected by it.
    ///
    /// `Arc`, not `Box`: the hook itself re-enters `flui-app` (it walks
    /// every hosted UI runtime and takes gesture-arena locks) and may be
    /// asked again from inside that walk, so the call point calls a clone
    /// (`call_point::SharedField`) with the lock
    /// released (ADR-0038 §5) and the hook stays installed while it runs.
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
            session_end: None,
            exit_policy: None,
            wake_deadline: None,
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
            .field("session_end", &self.session_end.is_some())
            .field("exit_policy", &self.exit_policy.is_some())
            .field("wake_deadline", &self.wake_deadline.is_some())
            .finish()
    }
}

// ============================================================================
// Per-Window Callbacks
// ============================================================================

/// A window whose callbacks its loop dispatches to: the window feature's, which
/// a loop holds as a trait object without naming it (ADR-0151 §10). Each
/// loop's window hook extends it with what that loop asks of a window.
#[cfg(any(
    target_os = "macos",
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "linux")
    )
))]
pub(crate) trait CallbackWindow: crate::host_window::HostWindow {
    /// The window's callbacks.
    fn callbacks(&self) -> &WindowCallbacks;
}

/// The windows a loop holds through its hook, as one object: the window
/// feature's. Each loop's hook extends it with what that loop asks of them.
pub(crate) trait OpenWindows: Send + Sync {
    /// Whether no window is open.
    fn is_empty(&self) -> bool;
}

#[expect(clippy::type_complexity)]
/// Per-window callback storage with one causal reentry queue.
///
/// Each callback is stored in a `Mutex<Option<Box<dyn FnMut/FnOnce + Send>>>`.
/// The dispatch pattern ensures reentrancy safety and ordering:
/// 1. enqueue the typed event in the window FIFO;
/// 2. one caller becomes the drain owner;
/// 3. call the callback through the call point
///    (`call_point::call`): taken out, called with its slot
///    unlocked, put back;
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
    on_input: crate::owner::slot::OwnerSlot<InputCallback>,
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
    /// `windowDidChangeOcclusionState:` — rules in `window::visibility`.
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
    /// [`flui_platform::PlatformWindow::on_surface_status_change`] for the contract and for
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
    /// out of its slot for a call ([`Latched`]) runs with its slot empty, so
    /// a `clear()` that lands while a callback is out finds nothing to take
    /// there — and without this flag, the call point would then restore that
    /// callback into the slot `clear()` just emptied the instant the call
    /// returns, resurrecting a callback (and everything it owns — a frame
    /// closure, a renderer, a surface, the window's own `Arc`) whose window
    /// no longer exists. A slot reads the flag when its callback comes back
    /// instead of restoring unconditionally.
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
                contain_owner_callback(|| drop(event));
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

/// One window callback's slot for a call, closed by a latch of the window
/// (`cleared` for delivery through the FIFO, `lifecycle_closed` for immediate
/// delivery): a callback that returns from its call after the latch was set
/// is not put back, and the call point drops it instead.
struct Latched<'a, T> {
    slot: &'a Mutex<Option<T>>,
    closed: &'a AtomicBool,
}

impl<T> HandlerSlot for Latched<'_, T> {
    type Handler = T;

    fn take(&self) -> Option<T> {
        self.slot.lock().take()
    }

    fn restore(&self, callback: T) -> Option<T> {
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
        // ran (and found nothing, because the call held the callback) or is
        // still to come. Checking before acquiring the lock instead would
        // race: `clear_now()` could store the flag and take this slot
        // (finding it empty) entirely between our read and our lock, and we
        // would restore a callback `clear_now()` already believes it dropped.
        if self.closed.load(Ordering::SeqCst) || slot.is_some() {
            return Some(callback);
        }
        *slot = Some(callback);
        None
    }

    fn replace(&self, callback: T) -> Option<T> {
        self.slot.lock().replace(callback)
    }
}

/// The input callback's owner-thread slot for a call, closed by the window's
/// `cleared` latch. Every step runs on the owner thread, so no lock orders
/// the restore against a clear.
struct InputSlot<'a> {
    slot: &'a crate::owner::slot::OwnerSlot<InputCallback>,
    cleared: &'a AtomicBool,
}

impl HandlerSlot for InputSlot<'_> {
    type Handler = InputCallback;

    fn take(&self) -> Option<InputCallback> {
        self.slot.take()
    }

    fn restore(&self, callback: InputCallback) -> Option<InputCallback> {
        if self.cleared.load(Ordering::SeqCst) {
            return Some(callback);
        }
        self.slot.restore(callback).err()
    }

    fn replace(&self, callback: InputCallback) -> Option<InputCallback> {
        self.slot.replace(callback).unwrap_or_else(Some)
    }
}

/// The close callback's slot for its one call: the callback is `FnOnce`, so
/// it comes out in an envelope the call empties, and nothing goes back.
struct Closing<'a>(&'a Mutex<Option<Box<dyn FnOnce() + Send>>>);

impl HandlerSlot for Closing<'_> {
    type Handler = Option<Box<dyn FnOnce() + Send>>;

    fn take(&self) -> Option<Self::Handler> {
        self.0.lock().take().map(Some)
    }

    fn restore(&self, envelope: Self::Handler) -> Option<Self::Handler> {
        Some(envelope)
    }

    fn replace(&self, envelope: Self::Handler) -> Option<Self::Handler> {
        envelope.and_then(|callback| self.0.lock().replace(callback).map(Some))
    }
}

/// Run the close callback out of its envelope; `FnOnce` consumes its
/// captures in the call.
fn run_close(envelope: &mut Option<Box<dyn FnOnce() + Send>>) {
    if let Some(close) = envelope.take() {
        close();
    }
}

impl WindowCallbacks {
    /// A callback's slot for delivery through this window's FIFO.
    fn queued<'a, T>(&'a self, slot: &'a Mutex<Option<T>>) -> Latched<'a, T> {
        Latched {
            slot,
            closed: &self.cleared,
        }
    }

    /// A callback's slot for immediate delivery, fenced at the close request.
    fn immediate<'a, T>(&'a self, slot: &'a Mutex<Option<T>>) -> Latched<'a, T> {
        Latched {
            slot,
            closed: &self.lifecycle_closed,
        }
    }

    /// The input callback's slot.
    fn input(&self) -> InputSlot<'_> {
        InputSlot {
            slot: &self.on_input,
            cleared: &self.cleared,
        }
    }

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
            on_input: crate::owner::slot::OwnerSlot::new(),
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
    /// (`owner/winit.rs`, the primary in-loop path) and its
    /// `WinitApp::release_open_window_callbacks` (same file, the quit route
    /// `complete_window_close` never runs for — every window still tracked
    /// when `event_loop.run_app` returns), `WinitWindow::drop`
    /// (`window/winit.rs`, a last-resort guarantee for a window
    /// whose final `Arc` unwinds anywhere else), the headless backend's
    /// `complete_close` (`window/headless.rs`), Win32's
    /// `WM_DESTROY` arm (`window/windows/proc.rs`), AppKit's
    /// `handle_close` (`window/apple/macos/delegate.rs`, reached from the
    /// `windowWillClose:` delegate), and the Android backend's loop exit
    /// (`owner/android/looper.rs`, `AndroidLoop::run` after its `loop`,
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
    /// every slot permanently inert.** The ten slots called through a
    /// `Latched` slot (every one except `on_close`) still accept a fresh
    /// registration after this call returns: that callback runs exactly once,
    /// the next time its event is dispatched, and only then does its slot
    /// see `cleared` set and hand it back to be dropped instead of restoring
    /// it — so it cannot run a second time. `on_close` is untouched by the
    /// latch entirely: it is `FnOnce`, so `drain_events`'s `Close` arm takes
    /// and calls it with nothing to restore, meaning a callback registered on it after
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
        // Set BEFORE taking any slot: a callback out for a call right now
        // (its slot already empty, user code running) checks this flag when
        // it comes back, which happens strictly after this store — the only
        // way to stop it from restoring itself into a slot this call is
        // about to empty. See `Latched::restore`.
        self.cleared.store(true, Ordering::SeqCst);
        let on_safe_area_change = self.on_safe_area_change.lock().take();
        let on_execution_state_change = self.on_execution_state_change.lock().take();
        // Off the owner thread this reaches nothing; the callback then
        // retires at the owner's next call, whose slot sees `cleared`.
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
        contain_owner_callback(|| drop(on_safe_area_change));
        contain_owner_callback(|| drop(on_execution_state_change));
        contain_owner_callback(|| drop(on_input));
        for input in held {
            contain_owner_callback(|| drop(input));
        }
        contain_owner_callback(|| drop(input_wake));
        contain_owner_callback(|| drop(on_request_frame));
        contain_owner_callback(|| drop(on_resize));
        contain_owner_callback(|| drop(on_state_changed));
        contain_owner_callback(|| drop(on_close));
        contain_owner_callback(|| drop(on_should_close));
        contain_owner_callback(|| drop(on_active_status_change));
        contain_owner_callback(|| drop(on_visibility_status_change));
        contain_owner_callback(|| drop(on_hover_status_change));
        contain_owner_callback(|| drop(on_appearance_changed));
        contain_owner_callback(|| drop(on_display_changed));
        contain_owner_callback(|| drop(on_surface_status_change));
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
                            contain_owner_callback(|| {
                                self.deliver_status_observation(
                                    &self.on_active_status_change,
                                    focused,
                                    Some(commit),
                                );
                            });
                        }
                        WindowCallbackEvent::Visibility(visible, Some(commit)) => {
                            contain_owner_callback(|| {
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
                            // aborted causal callback, before capture retirement;
                            // its own failure does not replace the first.
                            call(&Closing(&self.on_close), run_close)
                                .or_contain((), "a window's close callback");
                        }
                        WindowCallbackEvent::Clear(retired) => {
                            self.clear_now();
                            if let Some(retired) = retired {
                                contain_owner_callback(retired);
                            }
                        }
                        event => contain_owner_callback(|| drop(event)),
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
                    // An outgoing callback replaced during its call retires
                    // after its result; its capture failure is contained.
                    let result = call(&self.input(), |callback| callback(event))
                        .or_resume(DispatchEventResult::default());
                    input_result.get_or_insert(result);
                }
                WindowCallbackEvent::RequestFrame => {
                    call(&self.queued(&self.on_request_frame), |callback| {
                        // Counted before the call, so a frame that unwinds
                        // still counts as one that ran.
                        self.frames_run.fetch_add(1, Ordering::Relaxed);
                        callback();
                    })
                    .or_resume(());
                }
                WindowCallbackEvent::Resize(metrics) => {
                    // Queued delivery is already fenced by `clear_now` having
                    // emptied this slot before the event runs, so this slot
                    // answers to `cleared`. The immediate iOS
                    // `dispatch_metrics_resize` calls the same slot against
                    // `lifecycle_closed` instead: its fence has to close at the
                    // close REQUEST, which can arrive while a drain is still
                    // holding `clear_now` back.
                    call(&self.queued(&self.on_resize), |callback| callback(metrics)).or_resume(());
                }
                WindowCallbackEvent::StateChanged => {
                    call(&self.queued(&self.on_state_changed), |callback| callback()).or_resume(());
                }
                WindowCallbackEvent::Close => {
                    call(&Closing(&self.on_close), run_close).or_resume(());
                }
                WindowCallbackEvent::Execution(state) => {
                    call(&self.queued(&self.on_execution_state_change), |callback| {
                        callback(state);
                    })
                    .or_resume(());
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
                    call(&self.queued(&self.on_hover_status_change), |callback| {
                        callback(is_hovered);
                    })
                    .or_resume(());
                }
                WindowCallbackEvent::AppearanceChanged => {
                    call(&self.queued(&self.on_appearance_changed), |callback| {
                        callback();
                    })
                    .or_resume(());
                }
                WindowCallbackEvent::DisplayChanged => {
                    call(&self.queued(&self.on_display_changed), |callback| {
                        callback();
                    })
                    .or_resume(());
                }
                WindowCallbackEvent::SurfaceStatus(has_surface) => {
                    call(&self.queued(&self.on_surface_status_change), |callback| {
                        callback(has_surface);
                    })
                    .or_resume(());
                }
                WindowCallbackEvent::Clear(retired) => {
                    self.clear_now();
                    if let Some(retired) = retired {
                        contain_owner_callback(retired);
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
        call(&self.queued(slot), |callback| callback(value)).or_resume(());
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
                contain_owner_callback(|| drop(evicted));
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
        retire(outgoing);
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
                contain_owner_callback(|| drop(evicted));
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
        retire(outgoing);
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
            // The entry is read once the callback is out of its slot, so
            // nothing leaves the queue for an absent callback.
            let mut snapshot = None;
            let called = call(&self.input(), |callback| {
                let input = match self.next_held_entry()? {
                    HeldEntry::Snapshot(capabilities, registration) => {
                        snapshot = Some(registration);
                        PlatformInput::Capabilities(capabilities)
                    }
                    HeldEntry::Input(input) => input,
                };
                callback(input);
                Some(())
            });
            let delivered = match called {
                // Held until a registration; a snapshot is never emitted
                // into an absent callback.
                Called::Vacant | Called::Returned(None) => return false,
                Called::Returned(Some(())) => true,
                Called::Panicked(failure) => {
                    failure.contain("a window's input callback; the queued input stays in order");
                    false
                }
            };
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

    /// The next entry a pass delivers: the snapshot a registration is owed,
    /// or the oldest held input. `None` when nothing is held, or the snapshot
    /// failed every pass it gets.
    fn next_held_entry(&self) -> Option<HeldEntry> {
        let mut feed = self.input_feed.lock();
        if feed.snapshot_pending {
            return (feed.failed_snapshots < SNAPSHOT_ATTEMPTS)
                .then(|| HeldEntry::Snapshot(feed.front, feed.registration));
        }
        let input = feed.held.pop_front()?;
        if let PlatformInput::Capabilities(capabilities) = &input {
            feed.front = *capabilities;
        }
        Some(HeldEntry::Input(input))
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
        let delivered = match call(&self.input(), |callback| {
            callback(PlatformInput::Capabilities(capabilities));
        }) {
            Called::Vacant => return false,
            Called::Returned(()) => true,
            Called::Panicked(failure) => {
                failure.contain("a window's input callback; the queued input stays in order");
                false
            }
        };
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
        contain_owner_callback(|| drop(evicted));
    }

    fn wake_input(&self) {
        let wake = self.input_wake.lock().clone();
        if let Some(wake) = wake {
            contain_owner_callback(|| wake());
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
        // Default: allow close.
        call(&self.queued(&self.on_should_close), |callback| callback()).or_resume(true)
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
        if self.lifecycle_closed.load(Ordering::SeqCst) {
            return;
        }
        call(&self.immediate(slot), |callback| callback(value))
            .or_contain((), "a window's lifecycle callback");
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
        let retired = {
            let mut slot = self.on_safe_area_change.lock();
            if self.lifecycle_closed.load(Ordering::SeqCst) {
                Some(callback)
            } else {
                slot.replace(callback)
            }
        };
        retire(retired);
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
        // the queued Resize arm calls against (see `drain_events`'s own arm):
        // this is an IMMEDIATE delivery, so it is fenced the moment a close is
        // requested — `clear()` sets this latch on entry — rather than at slot
        // teardown, which can sit behind a still-running drain.
        call(&self.immediate(&self.on_resize), |callback| {
            callback(metrics);
        })
        .or_contain((), "a window's resize callback");
    }

    /// Replace an execution observer, disposing captures outside the storage lock.
    pub fn set_execution_state_callback(
        &self,
        callback: Box<dyn FnMut(WindowExecutionState) + Send>,
    ) {
        let retired = {
            let mut slot = self.on_execution_state_change.lock();
            if self.cleared.load(Ordering::SeqCst) {
                Some(callback)
            } else {
                slot.replace(callback)
            }
        };
        retire(retired);
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
    /// [`flui_platform::PlatformWindow::on_surface_status_change`] for the full contract.
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
/// and every setter is the same one-liner: install the boxed callback in its
/// slot through the call point, which retires any previous registration
/// after the slot's lock is released. Invoke inside an
/// `impl PlatformWindow for ...` block, naming the field that holds the
/// [`WindowCallbacks`]:
///
/// ```ignore
/// impl PlatformWindow for MyWindow {
///     crate::owner::handlers::impl_window_callback_setters!(callbacks);
///     // ... the rest of the impl ...
/// }
/// ```
///
/// The signatures below must match the `PlatformWindow` trait exactly; types
/// and paths are spelled absolutely so the expansion never depends on the
/// invoking module's imports.
macro_rules! impl_window_callback_setters {
    ($callbacks_field:ident) => {
        fn on_input(
            &self,
            callback: Box<
                dyn FnMut(::flui_platform::PlatformInput) -> ::flui_platform::DispatchEventResult,
            >,
        ) {
            self.$callbacks_field.set_input_callback(callback);
        }

        fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
            $crate::owner::call_point::install(&self.$callbacks_field.on_request_frame, callback);
        }

        fn on_resize(
            &self,
            callback: Box<dyn FnMut(::flui_foundation::geometry::WindowMetrics) + Send>,
        ) {
            $crate::owner::call_point::install(&self.$callbacks_field.on_resize, callback);
        }

        fn on_state_changed(&self, callback: Box<dyn FnMut() + Send>) {
            $crate::owner::call_point::install(&self.$callbacks_field.on_state_changed, callback);
        }

        fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
            $crate::owner::call_point::install(&self.$callbacks_field.on_close, callback);
        }

        fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
            $crate::owner::call_point::install(&self.$callbacks_field.on_should_close, callback);
        }

        fn on_safe_area_change(
            &self,
            callback: Box<dyn FnMut(flui_foundation::geometry::EdgeInsets) + Send>,
        ) {
            self.$callbacks_field.set_safe_area_callback(callback);
        }

        fn on_execution_state_change(
            &self,
            callback: Box<dyn FnMut(::flui_platform::WindowExecutionState) + Send>,
        ) {
            self.$callbacks_field.set_execution_state_callback(callback);
        }
        fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            $crate::owner::call_point::install(
                &self.$callbacks_field.on_active_status_change,
                callback,
            );
        }

        fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            $crate::owner::call_point::install(
                &self.$callbacks_field.on_visibility_status_change,
                callback,
            );
        }

        fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            $crate::owner::call_point::install(
                &self.$callbacks_field.on_hover_status_change,
                callback,
            );
        }

        fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
            $crate::owner::call_point::install(
                &self.$callbacks_field.on_appearance_changed,
                callback,
            );
        }

        fn on_display_changed(&self, callback: Box<dyn FnMut() + Send>) {
            $crate::owner::call_point::install(&self.$callbacks_field.on_display_changed, callback);
        }

        fn on_surface_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
            $crate::owner::call_point::install(
                &self.$callbacks_field.on_surface_status_change,
                callback,
            );
        }
    };
}
// Textual-scope escape: `pub(crate) use` gives the macro a normal path
// (`crate::owner::handlers::impl_window_callback_setters`) without `#[macro_export]`,
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

    use crate::owner::signal::OwnerSignal;

    use super::*;

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
        // A teardown that runs a one-shot callback from a destructor, as the
        // Win32 run guard runs the quit callback.
        struct Guard(Mutex<Option<Box<dyn FnMut() + Send>>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                call(&crate::owner::call_point::Once(&self.0), |callback| {
                    callback();
                })
                .or_contain((), "the guarded callback");
            }
        }
        for already_unwinding in [false, true] {
            let invoked = Arc::new(AtomicUsize::new(0));
            let dropped = Arc::new(AtomicUsize::new(0));
            let capture = Capture(Arc::clone(&dropped));
            let observed = Arc::clone(&invoked);
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let guard = Guard(Mutex::new(Some(Box::new(move || {
                    let _ = &capture;
                    observed.fetch_add(1, Ordering::SeqCst);
                    std::panic::panic_any(Hostile);
                }))));
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
            // The call failed: its captures are retained, never dropped into
            // a second failure (ADR-0127 §2).
            assert_eq!(dropped.load(Ordering::SeqCst), 0);
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
                crate::owner::panic_boundary::retain_off_owner_resource(
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
        contain_owner_callback(|| {
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

    fn worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
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

    /// A loop that reads what wakes asked of it begins the turn, reads, then
    /// drives. A wake that lands while it reads posts the next turn, and the
    /// loop reads again there: a change recorded during the first read is not
    /// left for an unrelated later wake.
    fn a_wake_while_the_loop_reads_gets_a_turn_that_reads_again() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        signal
            .register(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        signal.start().expect("start");
        // What a background source records for the loop to read, and wakes
        // the owner for.
        let changes = Arc::new(Mutex::new(Vec::<u32>::new()));
        changes.lock().push(1);
        signal.wake().expect("wake");
        assert_eq!(posts.load(Ordering::SeqCst), 1);

        let mut read = Vec::new();
        let mut turns = 0;
        // The loop: one pass per posted turn.
        while turns < posts.load(Ordering::SeqCst) {
            turns += 1;
            let turn = signal.begin_turn();
            read.extend(std::mem::take(&mut *changes.lock()));
            if turns == 1 {
                // The source records another change while the loop reads.
                changes.lock().push(2);
                signal.wake().expect("wake");
            }
            assert!(!turn.drive());
        }
        assert_eq!(
            read,
            [1, 2],
            "the change made during the read was never read"
        );
        assert_eq!(turns, 2);
        // The wake during the read was the first turn's to run: the second
        // turn only read.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        signal.close();
    }

    /// A turn begun and dropped before it was driven (a read that unwinds, a
    /// loop that returns early) posts the next turn for the work still owed,
    /// so the application's turn never waits for an unrelated wake.
    fn an_undriven_turn_posts_the_next_one() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        signal
            .register(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        signal.start().expect("start");
        signal.wake().expect("wake");
        assert_eq!(posts.load(Ordering::SeqCst), 1);

        let unwound = catch_unwind(AssertUnwindSafe(|| {
            let _turn = signal.begin_turn();
            panic!("a read panics before the turn is driven");
        }));
        assert!(unwound.is_err());
        assert_eq!(
            posts.load(Ordering::SeqCst),
            2,
            "the undriven turn posted no turn for the work it left owed"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the posted turn runs it");
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        signal.close();
    }

    /// A read inside a turn that finds work for the application's turn owes
    /// it to that turn: the drive runs it, and nothing is posted for it.
    #[cfg(all(windows, feature = "winit-backend"))]
    fn a_turn_owed_by_its_read_posts_nothing() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        signal
            .register(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        signal.start().expect("start");

        let turn = signal.begin_turn();
        turn.owe();
        assert!(!turn.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            posts.load(Ordering::SeqCst),
            0,
            "an owed turn posted a turn of its own"
        );
        signal.close();
    }

    /// The owner-thread machinery's contracts: hostile callback captures and
    /// payloads are contained (a direct clear drops each capture without
    /// unwinding; an owner callback's own and capture panics never replace an
    /// existing unwind), and the owner signal coalesces a burst and gives a
    /// re-entrant wake a later turn, gives a wake that lands while the loop
    /// reads a turn that reads again, posts the next turn when a begun turn is
    /// dropped undriven, and runs work a read owes to its turn without posting
    /// another.
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
                crate::owner::windows::WindowsLoop::window_identity_exhaustion_permanently_refuses_retries,
            ),
            #[cfg(target_os = "windows")]
            (
                "platform identity exhaustion",
                crate::owner::windows::WindowsLoop::platform_identity_exhaustion_acquires_nothing,
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
                "worker burst and reentrant wake",
                worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn,
            ),
            (
                "a wake while the loop reads",
                a_wake_while_the_loop_reads_gets_a_turn_that_reads_again,
            ),
            (
                "an undriven turn",
                an_undriven_turn_posts_the_next_one,
            ),
            #[cfg(all(windows, feature = "winit-backend"))]
            (
                "a turn owed by its read",
                a_turn_owed_by_its_read_posts_nothing,
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
