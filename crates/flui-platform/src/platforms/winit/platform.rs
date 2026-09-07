//! Winit-based platform implementation
//!
//! Cross-platform implementation using winit for window management — the
//! primary desktop backend on Linux until a native Wayland/X11 backend lands
//! (roadmap Cross.P); also covers Windows and macOS as a fallback.
//!
//! # Architecture
//!
//! Unlike GPUI which uses native platform APIs (Win32, Cocoa, Wayland/X11),
//! FLUI uses winit as a cross-platform abstraction. This trade-off provides:
//! - Simpler implementation and maintenance
//! - Faster multi-platform support
//! - Good-enough performance for most use cases
//!
//! The architecture uses winit 0.30's `ApplicationHandler` trait to manage
//! the event loop without consuming ownership.
//!
//! # Same-thread window creation during `on_ready`
//!
//! winit 0.30 requires a live `ActiveEventLoop` to create a window, and that
//! is only reachable from inside an `ApplicationHandler` callback running on
//! the event-loop thread. `Platform::run`'s `on_ready` callback is invoked
//! synchronously from [`WinitApp::resumed`] — one such callback — so a call
//! to `open_window` made from inside `on_ready` cannot go through the
//! cross-thread owner-control lane: that lane's only consumer is a *later*
//! dispatch on the same thread and cannot run
//! until `resumed` (and therefore `on_ready`) returns, which would deadlock
//! forever. [`ACTIVE_EVENT_LOOP`] publishes the live `ActiveEventLoop` for
//! the exact duration of the `on_ready` call so `open_window` can create the
//! window directly instead.
//!
//! # Programmatic close runs the owner's close teardown
//!
//! A window leaves this backend by exactly one body,
//! [`WinitApp::complete_window_close`], whichever way it was asked to go:
//! the compositor's `CloseRequested` (after the should-close veto) runs it
//! in the event arm, and [`PlatformWindow::close`] — a decision the embedder
//! already made, so no veto — posts a per-window request on the owner
//! control lane ([`ControlSender::request_close_window`]) that the owner
//! runs on its next turn. It cannot run in place: the teardown ends the
//! loop when the last window goes, which needs the live `ActiveEventLoop`
//! only an owner-turn callback holds, and the `on_close` callback must fire
//! on the owner thread whatever thread asked. Before this (issue #919)
//! `close()` hid the window and never left the tracking map, so the exit
//! policy — consulted only against that map — never saw the last window
//! go and the process lingered with nothing on screen.
//!
//! # Vsync pacing
//!
//! [`WinitApp::about_to_wait`] pins the event loop's control flow every
//! iteration rather than trusting winit's own default: `ControlFlow::Wait`
//! when nothing needs a wall-clock wake, `ControlFlow::WaitUntil(deadline)`
//! when the registered wake-deadline hook (issue #556) answers `Some`. This
//! is deliberate, not decorative: `flui-app`'s frame loop is wake-driven (a
//! redraw is requested only from `UiRealm::wake_frame`/`request_redraw`,
//! never polled), and steady-state pacing for a frame that DOES present
//! comes entirely from the GPU-side blocking Fifo present in
//! `flui-engine`'s `Renderer::render_scene` (see the frame-pacing ADR). If
//! a future winit release changed its own default away from `Wait` (e.g.
//! to `Poll`), that pacing model would silently regress into a busy-spin
//! with no compile or CI signal — pinning the value here turns an upstream
//! default change into a one-line diff to review instead of a surprise.
//!
//! # Wall-clock wake actuation
//!
//! `ControlFlow::WaitUntil` alone does not run a pump: when the deadline
//! expires, winit calls [`WinitApp::new_events`] with
//! `StartCause::ResumeTimeReached`, but nothing about that call delivers a
//! `WindowEvent` — no window is dirty, nothing requested a redraw. Without
//! an explicit actuator, `about_to_wait` would run again immediately,
//! re-consult the SAME hook (nothing has changed, so it answers the same
//! now-past deadline), and re-arm `WaitUntil` at an instant already behind
//! `Instant::now()` — winit fires it again on the very next loop
//! iteration, forever: a wall-clock wake with no actuator on the other
//! side degenerates into an unbounded busy-spin, not a wake. `new_events`
//! closes this: on `ResumeTimeReached` it calls `request_redraw()` on
//! every window this platform currently tracks, which queues a REAL
//! `WindowEvent::RedrawRequested` for the next iteration — the exact same
//! `dispatch_request_frame`/`on_request_frame`/`wake_action` path every
//! other wake in this backend already goes through, so a deadline that
//! resolves nothing new (nothing was actually due, or the realm has no
//! other demand) still costs at most one extra `wake_action::Skip`, never
//! a second spin.

use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use flui_foundation::{ClaimOutcome, ClaimSlot};
use keyboard_types::Modifiers as KeyboardModifiers;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent as WinitWindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{WindowAttributes, WindowId as WinitWindowId},
};

use flui_types::geometry::{Pixels, Point, px};

use super::window::WinitWindow;
use super::{
    clipboard::ArboardClipboard,
    control::{
        ControlCommand, ControlReceiver, ControlSendError, ControlSender, OpenWindowResult,
        control_lane,
    },
    data_transfer::WinitDataTransfer,
    display::WinitDisplay,
    events as winit_events,
};
use crate::{
    data_transfer::DataTransferSource,
    error::{BootstrapError, PlatformError},
    executor::BackgroundExecutor,
    shared::PlatformHandlers,
    traits::{
        Clipboard, DesktopCapabilities, OpenWindowError, OwnerPlatform, PendingWindow, Platform,
        PlatformCapabilities, PlatformDisplay, PlatformExecutor, PlatformInput,
        PlatformReadyCallback, PlatformWindow, ProxySendError, WindowEvent, WindowId, WindowOpen,
        WindowOptions,
        owner::{OwnerHooks, ProxyTransport},
    },
};

/// Convert a tracked physical cursor position to logical pixels.
fn logical_cursor_point(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
) -> Point<Pixels> {
    Point::new(
        px((position.x / scale_factor) as f32),
        px((position.y / scale_factor) as f32),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenWindowStateError {
    NotRunning,
    Starting,
    OwnerWouldBlock,
    Stopped,
}

enum WinitRunState {
    New {
        quit_requested: bool,
    },
    Starting {
        owner_thread: ThreadId,
        control: ControlSender,
    },
    Running {
        owner_thread: ThreadId,
        control: ControlSender,
    },
    Stopped,
}

impl WinitRunState {
    fn control_for_open_window(
        &self,
        caller_thread: ThreadId,
    ) -> Result<ControlSender, OpenWindowStateError> {
        match self {
            Self::New { .. } => Err(OpenWindowStateError::NotRunning),
            Self::Starting { owner_thread, .. } if *owner_thread == caller_thread => {
                Err(OpenWindowStateError::OwnerWouldBlock)
            }
            Self::Starting { .. } => Err(OpenWindowStateError::Starting),
            Self::Running { owner_thread, .. } if *owner_thread == caller_thread => {
                Err(OpenWindowStateError::OwnerWouldBlock)
            }
            Self::Running { control, .. } => Ok(control.clone()),
            Self::Stopped => Err(OpenWindowStateError::Stopped),
        }
    }
}

/// Winit-based platform implementation
///
/// This is the primary cross-platform implementation using winit for window
/// management. It supports Windows, macOS, and Linux desktop environments.
///
/// # Usage
///
/// ```rust,ignore
/// let platform = WinitPlatform::new();
/// platform.run(Box::new(|owner| {
///     // `open_window` is guaranteed `Ready` here — see the module-level
///     // doc on same-thread window creation.
///     let _window = owner.open_window(Default::default())?.try_ready()?;
///     Ok(())
/// }))?;
/// ```
pub struct WinitPlatform {
    /// Platform capabilities descriptor. `DesktopCapabilities` is a
    /// zero-sized, immutable-after-construction marker, so it lives directly
    /// on `WinitPlatform` (not inside the `Mutex`-guarded state) — that lets
    /// `capabilities()` return `&dyn PlatformCapabilities` borrowed straight
    /// from `&self` instead of from a `MutexGuard` temporary.
    capabilities: DesktopCapabilities,

    state: Arc<Mutex<WinitPlatformState>>,
}

impl std::fmt::Debug for WinitPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `WinitPlatformState` holds boxed callbacks and channel endpoints
        // that don't implement `Debug`; print the immutable descriptor only.
        f.debug_struct("WinitPlatform")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

/// Internal state for WinitPlatform
struct WinitPlatformState {
    /// Callback handlers
    handlers: PlatformHandlers,

    /// Background executor
    background_executor: Arc<BackgroundExecutor>,

    /// Clipboard
    clipboard: Arc<ArboardClipboard>,

    /// The one data-transfer source of this platform instance (ADR-0038):
    /// constructed once here, cloned out by `Platform::data_transfer()`,
    /// never reconstructed — it owns the single offer table, so ids stay
    /// redeemable across every clone.
    data_transfer: Arc<WinitDataTransfer>,

    /// Map of winit window IDs to platform window IDs
    window_id_map: HashMap<WinitWindowId, WindowId>,

    /// Map of platform window IDs to WinitWindow wrappers
    windows: HashMap<WindowId, Arc<WinitWindow>>,

    /// Cached displays
    displays: Vec<Arc<WinitDisplay>>,

    /// Active window ID
    active_window: Option<WindowId>,

    /// Next window ID to allocate
    next_window_id: u64,

    /// Event-loop ownership and control-lane lifecycle.
    run_state: WinitRunState,

    /// Current cursor position per window (physical)
    cursor_positions: HashMap<WindowId, winit::dpi::PhysicalPosition<f64>>,

    /// Current keyboard modifiers
    current_modifiers: KeyboardModifiers,

    /// Mouse buttons currently held, tracked as RAW winit buttons from
    /// `MouseInput` transitions — winit's `CursorMoved` carries no button
    /// state of its own, and a move with an empty button set is a HOVER to
    /// the gesture layer, so without this the pan recognizer never receives
    /// drag updates (live drag-scrolling did nothing).
    ///
    /// Raw, not normalized: `convert_mouse_button` folds the unbounded
    /// `MouseButton::Other(_)` id space onto ui-events' finite exotic
    /// button band, so two distinct vendor buttons can normalize to the
    /// same `PointerButton` — a normalized set would let releasing one
    /// clear the shared bit while the other is still down. The normalized
    /// set is DERIVED per event by [`held_pointer_buttons`].
    pressed_buttons: std::collections::HashSet<winit::event::MouseButton>,
    /// Stable pointer ids for live touch contacts, keyed by the winit
    /// `(device, contact)` pair — two touch devices commonly both report
    /// contact 0, and the interaction binding keys routes, pending moves and
    /// gesture arenas solely by pointer id, so a shared id would let either
    /// contact tear down the other's sequence. Entries are removed on the
    /// terminal phases (Ended/Cancelled).
    touch_contacts: std::collections::HashMap<(winit::event::DeviceId, u64), u64>,
    /// Buttons whose PRESS was dropped for lack of a tracked cursor
    /// position: the matching release must be suppressed too, or consumers
    /// receive an orphan Up (and, in between, moves that look like a drag
    /// for a Down that never existed). Entries clear on the release.
    suppressed_buttons: std::collections::HashSet<winit::event::MouseButton>,
    /// Next pointer id to hand a new touch contact. Starts past
    /// `PointerId::PRIMARY` (the mouse) and only grows — contact ids are
    /// never reused within a session, which keeps a late event for a dead
    /// contact from aliasing a live one.
    next_touch_pointer_id: u64,
}

/// The normalized W3C button set for the currently held raw buttons.
/// Resolve a stable pointer id for one touch contact, allocating on the
/// first sighting and releasing on the terminal phases. See the
/// `touch_contacts` field doc for why identity is the `(device, contact)`
/// pair and why ids are never reused.
fn resolve_touch_pointer_id(
    contacts: &mut std::collections::HashMap<(winit::event::DeviceId, u64), u64>,
    next_id: &mut u64,
    key: (winit::event::DeviceId, u64),
    phase: winit::event::TouchPhase,
) -> u64 {
    use winit::event::TouchPhase;
    let pointer_id = *contacts.entry(key).or_insert_with(|| {
        let id = *next_id;
        *next_id += 1;
        id
    });
    if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
        contacts.remove(&key);
    }
    pointer_id
}

fn held_pointer_buttons(
    pressed: &std::collections::HashSet<winit::event::MouseButton>,
) -> ui_events::pointer::PointerButtons {
    let mut buttons = ui_events::pointer::PointerButtons::default();
    for raw in pressed {
        buttons.insert(winit_events::convert_mouse_button(*raw));
    }
    buttons
}

impl WinitPlatformState {
    fn new() -> Self {
        // Initialize clipboard (may fail in headless environments).
        // `inert()` explicitly, not `default()`: init just failed, and
        // `default()` would retry the same doomed backend init before
        // falling back — the fallback for a KNOWN-failed init is the inert
        // clipboard directly.
        let clipboard = ArboardClipboard::new().map_or_else(
            |err| {
                tracing::warn!(?err, "Failed to initialize clipboard, using inert fallback");
                Arc::new(ArboardClipboard::inert())
            },
            Arc::new,
        );

        Self {
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(BackgroundExecutor::new()),
            clipboard,
            data_transfer: Arc::new(WinitDataTransfer::new()),
            window_id_map: HashMap::new(),
            windows: HashMap::new(),
            displays: Vec::new(),
            active_window: None,
            next_window_id: 1,
            run_state: WinitRunState::New {
                quit_requested: false,
            },
            cursor_positions: HashMap::new(),
            current_modifiers: KeyboardModifiers::empty(),
            pressed_buttons: std::collections::HashSet::new(),
            touch_contacts: std::collections::HashMap::new(),
            suppressed_buttons: std::collections::HashSet::new(),
            next_touch_pointer_id: 2,
        }
    }

    fn allocate_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    fn register_window(
        &mut self,
        winit_id: WinitWindowId,
        platform_id: WindowId,
        window: Arc<WinitWindow>,
    ) {
        self.window_id_map.insert(winit_id, platform_id);
        self.windows.insert(platform_id, window);

        if self.active_window.is_none() {
            self.active_window = Some(platform_id);
        }
    }

    fn get_platform_window_id(&self, winit_id: WinitWindowId) -> Option<WindowId> {
        self.window_id_map.get(&winit_id).copied()
    }

    fn init_displays(&mut self, event_loop: &ActiveEventLoop) {
        let monitors: Vec<_> = event_loop.available_monitors().collect();
        let primary_monitor = event_loop.primary_monitor();

        self.displays = monitors
            .into_iter()
            .enumerate()
            .map(|(idx, monitor)| {
                let is_primary = primary_monitor
                    .as_ref()
                    .map_or(idx == 0, |pm| pm.name() == monitor.name());

                Arc::new(WinitDisplay::new(monitor, idx as u64, is_primary))
            })
            .collect();

        tracing::info!(count = self.displays.len(), "Initialized displays");
    }
}

impl Default for WinitPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl WinitPlatform {
    /// Create a new winit platform
    pub fn new() -> Self {
        Self {
            capabilities: DesktopCapabilities,
            state: Arc::new(Mutex::new(WinitPlatformState::new())),
        }
    }

    /// Get mutable access to state (for internal use)
    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WinitPlatformState) -> R,
    {
        let mut state = self.state.lock();
        f(&mut state)
    }

    /// Create and run the event loop
    ///
    /// This is the main entry point for the platform. It creates the event
    /// loop, initializes the platform, and runs until quit is requested.
    ///
    /// # Errors
    /// Same contract as [`Platform::run`]: [`PlatformError::Bootstrap`] for
    /// an `on_ready` failure, [`PlatformError::EventLoop`] for a loop-level
    /// failure (including a second `run` on the same platform).
    pub fn run_event_loop(
        self: Arc<Self>,
        on_ready: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        tracing::info!("Creating winit event loop");

        let event_loop =
            EventLoop::builder()
                .build()
                .map_err(|error| PlatformError::EventLoop {
                    message: error.to_string(),
                })?;
        let event_loop_proxy = event_loop.create_proxy();
        let wake_owner = Arc::new(move || {
            if event_loop_proxy.send_event(()).is_err() {
                tracing::trace!("winit event loop closed before its wake was delivered");
            }
        });
        let (control, receiver) = control_lane(wake_owner);
        let owner_thread = thread::current().id();
        let quit_requested = self.install_control_lane(owner_thread, control.clone())?;

        let mut app = WinitApp {
            platform: Arc::clone(&self),
            on_ready: Some(on_ready),
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: self_close_deadline_from_env(),
            self_close_route: self_close_route_from_env(),
        };
        if quit_requested {
            control.request_quit();
        }

        let result = event_loop
            .run_app(&mut app)
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            });
        // Taken before `finish_shutdown` only so the borrow shape stays
        // simple — `finish_shutdown` does not touch this field.
        let bootstrap_error = app.bootstrap_error.take();
        app.finish_shutdown();

        combine_shutdown_result(bootstrap_error, result)
    }

    fn install_control_lane(
        &self,
        owner_thread: ThreadId,
        control: ControlSender,
    ) -> Result<bool, PlatformError> {
        self.with_state(|state| {
            let previous = std::mem::replace(&mut state.run_state, WinitRunState::Stopped);
            match previous {
                WinitRunState::New { quit_requested } => {
                    state.run_state = WinitRunState::Starting {
                        owner_thread,
                        control,
                    };
                    Ok(quit_requested)
                }
                other => {
                    state.run_state = other;
                    Err(PlatformError::EventLoop {
                        message: "the winit event loop can only be started once".to_string(),
                    })
                }
            }
        })
    }

    fn mark_running(&self) {
        self.with_state(|state| {
            let previous = std::mem::replace(&mut state.run_state, WinitRunState::Stopped);
            state.run_state = match previous {
                WinitRunState::Starting {
                    owner_thread,
                    control,
                } => WinitRunState::Running {
                    owner_thread,
                    control,
                },
                other => other,
            };
        });
    }

    fn mark_stopped(&self) {
        self.with_state(|state| state.run_state = WinitRunState::Stopped);
    }

    /// Create a winit window immediately using a live `ActiveEventLoop` and
    /// register it in platform state. Shared by the cross-thread control lane
    /// and `open_window`'s same-thread
    /// fast path (see the module-level doc on same-thread window creation).
    fn create_window_now(
        &self,
        event_loop: &ActiveEventLoop,
        options: WindowOptions,
    ) -> Result<WindowId, OpenWindowError> {
        // Resolve the lane BEFORE creating the native window: a window whose
        // programmatic close could reach no owner is the shape of issue #919,
        // and a live `ActiveEventLoop` without an installed lane cannot
        // happen (`run_event_loop` installs it before the loop starts) — but
        // the signature already speaks `OpenWindowError`, so the impossible
        // case is a typed refusal, not a panic holding a native window.
        let Some(close_lane) = self.control_sender() else {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(options),
            });
        };
        let mut attributes = WindowAttributes::default()
            .with_title(options.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                options.size.width.0,
                options.size.height.0,
            ))
            .with_resizable(options.resizable)
            .with_decorations(options.decorated)
            .with_visible(options.visible);

        if let Some(min) = options.min_size {
            attributes = attributes
                .with_min_inner_size(winit::dpi::LogicalSize::new(min.width.0, min.height.0));
        }
        if let Some(max) = options.max_size {
            attributes = attributes
                .with_max_inner_size(winit::dpi::LogicalSize::new(max.width.0, max.height.0));
        }

        let raw_window = Arc::new(event_loop.create_window(attributes).map_err(|error| {
            OpenWindowError::Backend {
                message: format!("winit window creation failed: {error}"),
            }
        })?);
        let winit_id = raw_window.id();
        // Allocate the platform identity before constructing the wrapper so
        // `WinitWindow` can carry its own `id()` from the start, rather than
        // being registered under an identity it cannot itself report.
        let platform_id = self.with_state(WinitPlatformState::allocate_window_id);
        let winit_window = Arc::new(WinitWindow::new(platform_id, raw_window, close_lane));

        self.with_state(|state| {
            state.register_window(winit_id, platform_id, winit_window.clone());
        });

        tracing::info!(?platform_id, "Created window");

        Ok(platform_id)
    }

    /// Look up a previously-created window by [`WindowId`] and return the
    /// exact stored allocation as a [`PlatformWindow`]. Named `window_by_id`
    /// (not `window_handle`) to avoid colliding with the unrelated
    /// [`PlatformWindow::window_handle`] raw GPU-handle accessor.
    fn window_by_id(
        &self,
        window_id: WindowId,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
        self.with_state(|state| {
            state
                .windows
                .get(&window_id)
                .ok_or_else(|| OpenWindowError::Backend {
                    message: "Window not found in state".to_string(),
                })
                .map(|window| Arc::clone(window) as Arc<dyn PlatformWindow>)
        })
    }

    /// Takes the global window-event handler out from under the state lock
    /// so it can be invoked outside it (ADR-0039). Before this hoist,
    /// `invoke_window_event` ran while `with_state`'s lock was held, so a
    /// handler that called an owner-thread platform method (e.g. a
    /// deferred `OwnerPlatform::open_window`) would re-enter this same
    /// non-reentrant lock and deadlock. Restores the handler on drop, only
    /// if nothing fresher was registered meanwhile — the same take/invoke/
    /// restore-if-none contract as `shared::handlers::CallbackLease`,
    /// reimplemented here (not reused directly) because this handler lives
    /// nested inside the platform's single big state `Mutex`, not its own
    /// standalone `Mutex<Option<T>>`.
    fn lease_window_event_handler(&self) -> WindowEventHandlerLease<'_> {
        let handler = self.with_state(|state| state.handlers.window_event.take());
        WindowEventHandlerLease {
            platform: self,
            handler,
        }
    }

    /// Takes the exit-policy hook out from under the state lock so it can be
    /// consulted outside it (ADR-0039) — the same take/invoke/restore-if-none
    /// discipline [`Self::lease_window_event_handler`] uses, for the same
    /// reason: the hook's own body (`flui-app`'s `AppRuntime::should_exit`)
    /// drops removed realm state, whose destructors (a dispose hook opening
    /// another window, say) may call back into this platform. Calling the
    /// hook while still holding this non-reentrant lock would deadlock the
    /// instant such a callback re-entered any `with_state`-guarded method.
    fn lease_exit_policy_hook(&self) -> ExitPolicyHookLease<'_> {
        let hook = self.with_state(|state| state.handlers.exit_policy.take());
        ExitPolicyHookLease {
            platform: self,
            hook,
        }
    }

    /// The owner control lane, while a loop is starting or running; `None`
    /// before `run_event_loop` installs one or after the loop stopped.
    fn control_sender(&self) -> Option<ControlSender> {
        self.with_state(|state| match &state.run_state {
            WinitRunState::Starting { control, .. } | WinitRunState::Running { control, .. } => {
                Some(control.clone())
            }
            WinitRunState::New { .. } | WinitRunState::Stopped => None,
        })
    }
}

/// See [`WinitPlatform::lease_window_event_handler`].
struct WindowEventHandlerLease<'a> {
    platform: &'a WinitPlatform,
    handler: Option<Box<dyn FnMut(WindowEvent) + Send>>,
}

impl WindowEventHandlerLease<'_> {
    /// Invokes the leased handler, if one is registered, outside the
    /// platform state lock.
    fn invoke(&mut self, event: WindowEvent) {
        if let Some(handler) = self.handler.as_mut() {
            handler(event);
        }
    }
}

impl Drop for WindowEventHandlerLease<'_> {
    fn drop(&mut self) {
        let Some(handler) = self.handler.take() else {
            return;
        };
        self.platform.with_state(|state| {
            if state.handlers.window_event.is_none() {
                state.handlers.window_event = Some(handler);
            }
        });
    }
}

/// See [`WinitPlatform::lease_exit_policy_hook`].
struct ExitPolicyHookLease<'a> {
    platform: &'a WinitPlatform,
    hook: Option<Box<dyn Fn() -> bool + Send>>,
}

impl ExitPolicyHookLease<'_> {
    /// Consults the leased hook outside the platform state lock. `true`
    /// (allow the exit) when no hook is installed — the pre-#555
    /// unconditional default.
    fn invoke(&self) -> bool {
        self.hook.as_ref().is_none_or(|hook| hook())
    }
}

impl Drop for ExitPolicyHookLease<'_> {
    fn drop(&mut self) {
        let Some(hook) = self.hook.take() else {
            return;
        };
        self.platform.with_state(|state| {
            if state.handlers.exit_policy.is_none() {
                state.handlers.exit_policy = Some(hook);
            }
        });
    }
}

/// Combines `event_loop.run_app`'s own result with a stashed `on_ready`
/// bootstrap failure (`WinitApp::bootstrap_error`).
///
/// The bootstrap error is the root cause the whole fallible-`on_ready`
/// design exists to surface — it is checked and returned FIRST, never
/// shadowed by a loop-level error that arrives alongside it.
/// Checking `result` first (the bug this function fixes) would silently
/// drop the actual bootstrap failure exactly when
/// [`WinitApp::request_exit`]'s own `event_loop.exit()` call produces a
/// winit-level error of its own — the one scenario this whole design
/// exists to get right. When both are present, the loop error rides along
/// in [`PlatformError::Bootstrap`]'s `loop_error` field on top of the
/// bootstrap error, so neither is lost: the bootstrap error's own chain is
/// still reachable via `Error::source()` on the returned value.
fn combine_shutdown_result(
    bootstrap_error: Option<BootstrapError>,
    result: Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    match bootstrap_error {
        Some(source) => Err(PlatformError::Bootstrap {
            source,
            loop_error: result.err().map(|error| error.to_string()),
        }),
        None => result,
    }
}

thread_local! {
    /// Published only while `on_ready` executes synchronously inside
    /// [`WinitApp::resumed`], on the winit event-loop thread. See the
    /// module-level doc on same-thread window creation.
    static ACTIVE_EVENT_LOOP: Cell<Option<NonNull<ActiveEventLoop>>> = const { Cell::new(None) };
}

/// Publishes `event_loop` to [`ACTIVE_EVENT_LOOP`] for the duration of `f`,
/// then un-publishes it unconditionally (including if `f` panics), so a
/// publication never outlives the call that set it.
///
/// Callers must only pass an `event_loop` that stays valid for the entire
/// call to `f` — `resumed`'s `&ActiveEventLoop` parameter satisfies this
/// because it outlives the whole `resumed` call, which fully contains `f`.
fn with_active_event_loop<R>(event_loop: &ActiveEventLoop, f: impl FnOnce() -> R) -> R {
    // Save/restore rather than unconditionally clearing to `None`: today
    // `on_ready` is the only caller and never nests, but restoring whatever
    // was published before this call (instead of clobbering it to `None`)
    // keeps a hypothetical future nested publication on this thread correct
    // for free.
    struct RestoreOnDrop(Option<NonNull<ActiveEventLoop>>);
    impl Drop for RestoreOnDrop {
        fn drop(&mut self) {
            ACTIVE_EVENT_LOOP.with(|cell| cell.set(self.0));
        }
    }

    let previous = ACTIVE_EVENT_LOOP.with(|cell| cell.replace(Some(NonNull::from(event_loop))));
    let _restore = RestoreOnDrop(previous);
    f()
}

/// Reads the harness self-close deadline from `FLUI_SELF_CLOSE_AFTER_MS`
/// (milliseconds from now). See [`WinitApp::self_close_deadline`] for why
/// this hook exists. A present-but-unparsable value is a harness
/// misconfiguration worth surfacing, not silently ignoring.
fn self_close_deadline_from_env() -> Option<Instant> {
    let raw = std::env::var("FLUI_SELF_CLOSE_AFTER_MS").ok()?;
    match raw.parse::<u64>() {
        Ok(ms) => {
            tracing::info!(after_ms = ms, "harness self-close armed");
            Some(Instant::now() + Duration::from_millis(ms))
        }
        Err(error) => {
            tracing::warn!(%raw, %error, "FLUI_SELF_CLOSE_AFTER_MS is not a millisecond count");
            None
        }
    }
}

/// Which close route the harness self-close drives — see
/// [`WinitApp::self_close_route`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SelfCloseRoute {
    /// Synthesize `WindowEvent::CloseRequested`: the compositor/user path,
    /// veto and all.
    #[default]
    Compositor,
    /// Call [`PlatformWindow::close`] on the tracked window: the
    /// programmatic path, exactly as an application closing its own window
    /// does (issue #919).
    Programmatic,
}

/// Reads the harness self-close route from `FLUI_SELF_CLOSE_ROUTE`
/// (`compositor`, the default, or `programmatic`). Only meaningful with a
/// deadline armed. An unknown value is a harness misconfiguration worth
/// surfacing, not silently defaulting.
fn self_close_route_from_env() -> SelfCloseRoute {
    let Ok(raw) = std::env::var("FLUI_SELF_CLOSE_ROUTE") else {
        return SelfCloseRoute::default();
    };
    match raw.as_str() {
        "compositor" => SelfCloseRoute::Compositor,
        "programmatic" => SelfCloseRoute::Programmatic,
        _ => {
            tracing::warn!(
                %raw,
                "FLUI_SELF_CLOSE_ROUTE is neither `compositor` nor `programmatic`; using compositor"
            );
            SelfCloseRoute::Compositor
        }
    }
}

/// How long an `about_to_wait` decision parks the loop for, in whole
/// milliseconds from now — `None` for an open-ended `Wait`. Trace-only:
/// the number a pacing investigation needs next to the frame callback's own
/// timing, without which "the loop was idle" and "the loop was blocked in
/// the GPU" are indistinguishable in a log.
fn control_flow_wait_ms(control_flow: ControlFlow) -> Option<u64> {
    match control_flow {
        ControlFlow::WaitUntil(deadline) => Some(
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u64,
        ),
        ControlFlow::Wait | ControlFlow::Poll => None,
    }
}

/// The earlier of two optional wall-clock deadlines — `None` only when both
/// are `None`, so an armed deadline is never lost to an idle `Wait`.
fn earliest_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (deadline @ Some(_), None) | (None, deadline @ Some(_)) => deadline,
        (None, None) => None,
    }
}

/// Application handler for winit event loop
///
/// Implements `ApplicationHandler` to receive events from winit without
/// consuming the event loop.
struct WinitApp {
    platform: Arc<WinitPlatform>,
    on_ready: Option<PlatformReadyCallback>,
    control: ControlReceiver,
    quit_notified: bool,
    /// Claim slots whose delivery already landed (`Ok`), kept alive past
    /// dequeue so a *later* abandonment (the requester claimed delivery,
    /// then dropped without reading it) can still be swept and unwound —
    /// see [`WinitApp::sweep_settled_replies`]. Bounded by lane capacity
    /// for queued entries; a delivered-but-unclaimed entry persists here
    /// until the requester claims or drops it (ADR-0039 §3's honest
    /// registry-occupancy statement — not admission-time bounded).
    in_flight_replies: Vec<(WindowId, ClaimSlot<OpenWindowResult>)>,
    /// Set when `on_ready` returns `Err` (a fallible bootstrap failure —
    /// window creation, GPU init, root-widget attach — has no other return
    /// path back to `Platform::run`'s caller). `resumed` stashes it
    /// here and requests exit; `run_event_loop` propagates it out of `run`
    /// (wrapped as [`PlatformError::Bootstrap`]) once the loop has actually
    /// unwound, rather than swallowing it into a bare `tracing::error!`
    /// while the loop keeps pumping a half-built app.
    bootstrap_error: Option<BootstrapError>,
    /// Harness self-close deadline, armed from `FLUI_SELF_CLOSE_AFTER_MS` at
    /// loop start. When it expires, [`WinitApp::fire_self_close_if_due`]
    /// synthesizes `WindowEvent::CloseRequested` for one tracked window —
    /// the exact arm a compositor close takes. This exists for the Wayland
    /// live-smoke harness: unlike X11 (where XTEST + a `WM_DELETE_WINDOW`
    /// client message can drive a real close from outside, see
    /// `tools/live-smoke`), Wayland has no protocol a third-party client can
    /// use to close another client's toplevel — `xdg_toplevel.close` is
    /// compositor→client only — so the close-path teardown ordering is
    /// untestable on Wayland without this in-process trigger. Unset (the
    /// default, and always in production) this is `None` and nothing here
    /// runs.
    self_close_deadline: Option<Instant>,
    /// Which close route the armed self-close drives
    /// (`FLUI_SELF_CLOSE_ROUTE`): the synthesized compositor
    /// `CloseRequested` by default, or [`PlatformWindow::close`] on the
    /// tracked window — so the live-smoke harnesses can prove the
    /// programmatic route's teardown (issue #919) against a real app, real
    /// GPU surface, and the embedder's real exit-policy hook, which no
    /// in-crate test can host. Irrelevant while `self_close_deadline` is
    /// `None`.
    self_close_route: SelfCloseRoute,
}

/// Completes a dequeued window request even if owner-side processing
/// unwinds. Wraps the claim-slot owner half (ADR-0039 §3) rather than the
/// pre-ADR-0039 buffered `sync_channel(1)` one-shot.
struct OpenWindowReplyGuard {
    reply: Option<ClaimSlot<OpenWindowResult>>,
}

impl OpenWindowReplyGuard {
    fn new(reply: ClaimSlot<OpenWindowResult>) -> Self {
        Self { reply: Some(reply) }
    }

    /// Delivers `result`. Returns the still-live `ClaimSlot` when delivery
    /// landed on `Pending` (the owner should track it in
    /// `in_flight_replies` for a possible later abandonment); `None` when
    /// the requester had already abandoned the slot (the owner should
    /// unwind instead of tracking).
    fn complete(mut self, result: OpenWindowResult) -> Option<ClaimSlot<OpenWindowResult>> {
        let reply = self
            .reply
            .take()
            .expect("BUG: complete called after this guard already completed once");
        match reply.deliver(result) {
            Ok(()) => Some(reply),
            Err(_already_abandoned) => None,
        }
    }
}

impl Drop for OpenWindowReplyGuard {
    fn drop(&mut self) {
        if let Some(reply) = self.reply.take() {
            // `OwnerGone`, not `Backend`: this guard fires when the owner
            // stops (panics, or drops the guard) before ever completing the
            // request -- consistent with `reject_pending_commands`'
            // identical shutdown-path delivery and with `OwnerGone`'s own
            // documented semantics ("the loop died after the request was
            // already accepted"). `Backend` means the backend tried and
            // failed to create the window; that never happened here.
            let _ = reply.deliver(Err(OpenWindowError::OwnerGone { rejected: None }));
        }
    }
}

impl ApplicationHandler for WinitApp {
    /// Actuates a wall-clock wake (issue #556): `ControlFlow::WaitUntil`
    /// expiring delivers `StartCause::ResumeTimeReached` here with no
    /// accompanying `WindowEvent` — nothing else in this backend turns that
    /// bare wake into a pump. Poke every currently tracked window's
    /// `request_redraw()`, which queues a real
    /// `WindowEvent::RedrawRequested` for the very next iteration and so
    /// re-enters `dispatch_request_frame`/`on_request_frame`/`wake_action`
    /// exactly like any other wake — never a second, parallel produce path
    /// (see the module doc's "Wall-clock wake actuation" section for why an
    /// un-actuated `WaitUntil` degenerates into an unbounded spin instead).
    /// Every other `StartCause` (`Init`, `Poll`, `WaitCancelled`) is a
    /// documented no-op here — `Poll`/`WaitCancelled` never fire without a
    /// caller setting `ControlFlow::Poll`, which this backend never does.
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.platform.with_state(|state| {
                for window in state.windows.values() {
                    window.inner().request_redraw();
                }
            });
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        tracing::info!("Application resumed");

        // Initialize displays on first resume
        self.platform.with_state(|state| {
            if state.displays.is_empty() {
                state.init_displays(event_loop);
            }
        });

        // Call on_ready callback once. `ACTIVE_EVENT_LOOP` is published for
        // this exact nested call so `open_window` can create windows
        // directly instead of deadlocking on the cross-thread lane (see the
        // module-level doc). Mint the owner-thread capability (ADR-0039)
        // fresh for this call: `!Send + !Sync`, so it cannot outlive this
        // stack frame except by `on_ready` stashing it itself (e.g.
        // `flui-app`'s `OWNER_PLATFORM_HOST` TLS slot).
        if let Some(on_ready) = self.on_ready.take() {
            tracing::info!("Calling on_ready callback");
            let owner_thread = thread::current().id();
            let platform: Arc<dyn Platform> = Arc::clone(&self.platform) as Arc<dyn Platform>;
            let hooks: Arc<dyn OwnerHooks> = Arc::new(WinitOwnerHooks {
                platform: Arc::clone(&self.platform),
                owner_thread,
            });
            let owner = OwnerPlatform::new(platform, hooks);
            if let Err(error) = with_active_event_loop(event_loop, || on_ready(owner)) {
                // A fallible bootstrap (window creation, GPU init,
                // root-widget attach) failed with no return path back to
                // `Platform::run`'s caller except through this stash and an
                // immediate exit — never continue the loop with a
                // half-built app. `finish_shutdown` (via `request_exit`)
                // fires quit handlers and closes the owner lane exactly as
                // any other shutdown path does.
                //
                // Logged once, at `error` level, from `Platform::run`'s own
                // `inspect_err` once `run_event_loop` has combined this with
                // whatever `event_loop.run_app` itself returns -- not here,
                // to avoid double-logging the same failure under two
                // different (and, at the `run()` site, previously
                // misleading) messages.
                tracing::debug!(
                    "on_ready bootstrap failed; requesting event-loop exit \
                     (see Platform::run's propagated error for the failure)"
                );
                self.bootstrap_error = Some(error);
                self.request_exit(event_loop);
                return;
            }
        }

        self.platform.mark_running();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WinitWindowId,
        event: WinitWindowEvent,
    ) {
        let (platform_id, window) = self.platform.with_state(|state| {
            let pid = state.get_platform_window_id(window_id);
            let win = pid.and_then(|id| state.windows.get(&id).cloned());
            (pid, win)
        });

        let Some(platform_id) = platform_id else {
            tracing::warn!("Received event for unknown window");
            return;
        };

        match event {
            WinitWindowEvent::CloseRequested => {
                tracing::info!(?platform_id, "Window close requested");

                // Ask the window if it should close
                if window
                    .as_ref()
                    .is_some_and(|win| !win.callbacks().dispatch_should_close())
                {
                    return; // Vetoed
                }

                // The global "the user asked and nothing vetoed" event belongs
                // to this route alone (a programmatic close was never a
                // request); leased and invoked OUTSIDE the state lock
                // (ADR-0039) like every global-handler call here.
                self.platform
                    .lease_window_event_handler()
                    .invoke(WindowEvent::CloseRequested {
                        window_id: platform_id,
                    });

                self.complete_window_close(event_loop, platform_id, window.as_ref());
            }
            WinitWindowEvent::Resized(physical_size) => {
                use flui_types::geometry::{Size, device_px, px};

                let size = Size::new(
                    device_px(physical_size.width as i32),
                    device_px(physical_size.height as i32),
                );

                tracing::debug!(?platform_id, ?size, "Window resized");

                // Dispatch per-window resize callback
                if let Some(ref win) = window {
                    let scale = win.scale_factor() as f32;
                    let logical = Size::new(
                        px(physical_size.width as f32 / scale),
                        px(physical_size.height as f32 / scale),
                    );
                    win.callbacks().dispatch_resize(logical, scale);
                }

                // Notify platform handler (leased outside the state lock).
                self.platform
                    .lease_window_event_handler()
                    .invoke(WindowEvent::Resized {
                        window_id: platform_id,
                        size,
                    });
            }
            WinitWindowEvent::RedrawRequested => {
                tracing::trace!(?platform_id, "Redraw requested");

                // Dispatch per-window frame request callback
                if let Some(ref win) = window {
                    win.callbacks().dispatch_request_frame();
                }

                // Notify platform handler (leased outside the state lock).
                self.platform
                    .lease_window_event_handler()
                    .invoke(WindowEvent::RedrawRequested {
                        window_id: platform_id,
                    });
            }
            WinitWindowEvent::Focused(focused) => {
                tracing::debug!(?platform_id, ?focused, "Window focus changed");

                // Update WinitWindow focus state
                if let Some(ref win) = window {
                    win.set_focused(focused);
                    win.callbacks().dispatch_active_status_change(focused);
                }

                // Active-window bookkeeping stays under the lock; only the
                // handler invocation moves outside it.
                self.platform.with_state(|state| {
                    if focused {
                        state.active_window = Some(platform_id);
                    } else {
                        if state.active_window == Some(platform_id) {
                            state.active_window = None;
                        }
                        // A release delivered while unfocused never reaches
                        // `MouseInput`, so a set left as-is would replay the
                        // stale hold on refocus and misclassify the next
                        // cursor move as a drag.
                        state.pressed_buttons.clear();
                    }
                });

                self.platform
                    .lease_window_event_handler()
                    .invoke(WindowEvent::FocusChanged {
                        window_id: platform_id,
                        focused,
                    });
            }
            WinitWindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::debug!(?platform_id, ?scale_factor, "Scale factor changed");

                self.platform.lease_window_event_handler().invoke(
                    WindowEvent::ScaleFactorChanged {
                        window_id: platform_id,
                        scale_factor,
                    },
                );
                // Deliberately NO resize dispatch here. winit applies the
                // OS-suggested inner size right after this event ("By
                // default, the window is resized to the value suggested by
                // the OS" — winit::event::WindowEvent::ScaleFactorChanged),
                // so a Resized always follows, and THAT arm reads the
                // post-change scale factor — the realm's device-pixel ratio
                // updates through the proven path with the correct new
                // size. Dispatching here would divide the still-unchanged
                // physical size by the new scale and publish a transiently
                // wrong logical size. Backends without winit's guarantee
                // must dispatch the resize themselves (the headless mock's
                // simulate_scale_factor_change pins that contract).
            }
            WinitWindowEvent::CursorMoved { position, .. } => {
                let (modifiers, held_buttons) = self.platform.with_state(|state| {
                    state.cursor_positions.insert(platform_id, position);
                    (
                        state.current_modifiers,
                        held_pointer_buttons(&state.pressed_buttons),
                    )
                });

                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input =
                        winit_events::cursor_moved_event(position, scale, modifiers, held_buttons);
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::MouseInput { state, button, .. } => {
                let (modifiers, cursor_pos, held_buttons, suppressed) =
                    self.platform.with_state(|s| {
                        let cursor_pos = s.cursor_positions.get(&platform_id).copied();
                        // A press with no tracked cursor position cannot be
                        // delivered anywhere real (the old (0,0) stand-in
                        // actuated the top-left widget) — suppress the WHOLE
                        // sequence: the press never enters the held set, and
                        // the matching release is swallowed below, so
                        // consumers never see a drag or an orphan Up for a
                        // Down that never existed.
                        let suppressed = match state {
                            winit::event::ElementState::Pressed => {
                                if cursor_pos.is_none() {
                                    s.suppressed_buttons.insert(button);
                                    true
                                } else {
                                    // Track the RAW transition: the emitted
                                    // event's `buttons` is the set held AFTER
                                    // it, and raw tracking keeps aliased
                                    // buttons from clearing each other's bits.
                                    s.pressed_buttons.insert(button);
                                    false
                                }
                            }
                            winit::event::ElementState::Released => {
                                if s.suppressed_buttons.remove(&button) {
                                    true
                                } else {
                                    s.pressed_buttons.remove(&button);
                                    false
                                }
                            }
                        };
                        (
                            s.current_modifiers,
                            cursor_pos,
                            held_pointer_buttons(&s.pressed_buttons),
                            suppressed,
                        )
                    });

                if suppressed {
                    tracing::debug!(
                        ?platform_id,
                        ?state,
                        "suppressing a button event from an unpositioned press sequence"
                    );
                    return;
                }
                let Some(cursor_pos) = cursor_pos else {
                    // A release whose press WAS delivered but whose position
                    // tracking has since been lost (focus loss cleared it):
                    // nothing sane to deliver at — drop, traced. The raw set
                    // was already updated above.
                    tracing::debug!(
                        ?platform_id,
                        "dropping a mouse button event with no tracked cursor position"
                    );
                    return;
                };
                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input = winit_events::mouse_button_event(
                        button,
                        state,
                        cursor_pos,
                        scale,
                        modifiers,
                        held_buttons,
                    );
                    win.callbacks().dispatch_input(input);
                }
            }
            // NaN (documented possible) folds to None in the shared
            // conversion; the guard drops the tick without touching the
            // rest of this handler (the trailing wildcard arm covers it).
            WinitWindowEvent::PinchGesture { delta, .. }
                if crate::shared::gestures::pinch(delta).is_some() =>
            {
                let gesture = crate::shared::gestures::pinch(delta)
                    .expect("BUG: the match guard just checked Some");
                let (modifiers, cursor_pos) = self.platform.with_state(|s| {
                    (
                        s.current_modifiers,
                        s.cursor_positions.get(&platform_id).copied(),
                    )
                });
                // Same no-made-up-origin contract as clicks and wheels: a
                // gesture with no tracked cursor position is dropped.
                let Some(cursor_pos) = cursor_pos else {
                    tracing::debug!(
                        ?platform_id,
                        "dropping a trackpad gesture with no tracked cursor position"
                    );
                    return;
                };
                if let Some(ref win) = window {
                    let input = winit_events::trackpad_gesture_event(
                        gesture,
                        cursor_pos,
                        win.scale_factor(),
                        modifiers,
                    );
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::RotationGesture { delta, .. } => {
                let (modifiers, cursor_pos) = self.platform.with_state(|s| {
                    (
                        s.current_modifiers,
                        s.cursor_positions.get(&platform_id).copied(),
                    )
                });
                // Same no-made-up-origin contract as clicks and wheels: a
                // gesture with no tracked cursor position is dropped.
                let Some(cursor_pos) = cursor_pos else {
                    tracing::debug!(
                        ?platform_id,
                        "dropping a trackpad gesture with no tracked cursor position"
                    );
                    return;
                };
                if let Some(ref win) = window {
                    let input = winit_events::trackpad_gesture_event(
                        crate::shared::gestures::rotation_ccw_degrees(delta),
                        cursor_pos,
                        win.scale_factor(),
                        modifiers,
                    );
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::Touch(touch) => {
                let (modifiers, pointer_id) = self.platform.with_state(|s| {
                    let pointer_id = resolve_touch_pointer_id(
                        &mut s.touch_contacts,
                        &mut s.next_touch_pointer_id,
                        (touch.device_id, touch.id),
                        touch.phase,
                    );
                    (s.current_modifiers, pointer_id)
                });
                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input = winit_events::touch_event(touch, pointer_id, scale, modifiers);
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::MouseWheel { delta, .. } => {
                tracing::debug!(?delta, "MouseWheel");
                let (modifiers, cursor_pos) = self.platform.with_state(|s| {
                    (
                        s.current_modifiers,
                        s.cursor_positions.get(&platform_id).copied(),
                    )
                });
                // Same untracked-position posture as the button arm above.
                let Some(cursor_pos) = cursor_pos else {
                    tracing::debug!(
                        ?platform_id,
                        "dropping a wheel event with no tracked cursor position"
                    );
                    return;
                };

                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input =
                        winit_events::mouse_wheel_event(delta, cursor_pos, scale, modifiers);
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // winit synthesizes press events for every key held when a
                // window gains focus, and release events for every key held
                // when it loses focus (X11 and Windows only — the
                // `is_synthetic` doc in winit 0.30 `src/event.rs`). Both are
                // keyboard-state synchronization, not user keystrokes, but
                // they get opposite treatment:
                //
                // - Synthetic PRESSES are dropped. Every framework key
                //   consumer — `EditableText` typing, `Shortcuts`, focus
                //   traversal — actuates on `KeyState::Down`, so dispatching
                //   one would re-type and re-trigger every held key each
                //   time the window regains focus; and no consumer needs
                //   the press for state (modifier state rides
                //   `ModifiersChanged`, which winit emits on focus change
                //   in its own right).
                // - Synthetic RELEASES are dispatched. When the physical
                //   release happens while the window is unfocused, this
                //   synthetic release is the ONLY key-up the window will
                //   ever receive — an app-level `Focus::on_key_event`
                //   handler latching a key on Down (held-key movement, a
                //   push-to-talk toggle) would otherwise stay stuck after
                //   an Alt-Tab. A stray Up is inert to every Down-actuated
                //   consumer.
                //
                // Flutter draws the same line, from the other side: its
                // embedders deliver state-sync as `synthesized` events that
                // update key state without being treated as the user's
                // direct action (`hardware_keyboard.dart`). FLUI's
                // `KeyboardEvent` carries no such flag, so the press half —
                // which WOULD actuate — cannot be forwarded safely and is
                // dropped instead.
                if is_synthetic && event.state == winit::event::ElementState::Pressed {
                    tracing::trace!(
                        ?event,
                        "dropping synthetic key press (focus-change state sync)"
                    );
                    return;
                }
                let modifiers = self.platform.with_state(|s| s.current_modifiers);

                if let Some(ref win) = window {
                    let input = winit_events::keyboard_event(&event, modifiers);
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::Ime(event) => {
                let input = winit_events::ime_event(&event);
                if let Some(ref win) = window {
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::ModifiersChanged(new_modifiers) => {
                self.platform.with_state(|state| {
                    state.current_modifiers = winit_events::convert_modifiers(new_modifiers);
                });
            }
            WinitWindowEvent::CursorEntered { .. } => {
                if let Some(ref win) = window {
                    win.callbacks().dispatch_hover_status_change(true);
                }
            }
            WinitWindowEvent::CursorLeft { .. } => {
                if let Some(ref win) = window {
                    win.callbacks().dispatch_hover_status_change(false);
                }
            }
            WinitWindowEvent::Moved(_) => {
                if let Some(ref win) = window {
                    win.callbacks().dispatch_moved();
                }
            }
            WinitWindowEvent::ThemeChanged(_) => {
                if let Some(ref win) = window {
                    win.callbacks().dispatch_appearance_changed();
                }
            }
            WinitWindowEvent::Occluded(occluded) => {
                tracing::debug!(?platform_id, ?occluded, "Window occlusion changed");

                // `occluded == true` means fully covered/not visible;
                // `PlatformWindow::on_visibility_status_change`'s contract
                // is `is_visible`, so this is the negation. Wayland
                // delivery rides the xdg-shell v6 `suspended` state (a
                // compositor-conditional extension); on a compositor that
                // never sends it, this arm simply never fires, matching
                // the pre-existing always-visible behavior.
                if let Some(ref win) = window {
                    win.set_visible(!occluded);
                    win.callbacks().dispatch_visibility_status_change(!occluded);
                }
            }
            WinitWindowEvent::HoveredFile(path) => {
                self.handle_drag_drop_path(platform_id, window.as_ref(), path, DragPath::Hovered);
            }
            WinitWindowEvent::DroppedFile(path) => {
                self.handle_drag_drop_path(platform_id, window.as_ref(), path, DragPath::Dropped);
            }
            WinitWindowEvent::HoveredFileCancelled => {
                let transfer = self
                    .platform
                    .with_state(|state| Arc::clone(&state.data_transfer));
                let event = transfer.note_hover_cancelled(platform_id);
                // Lock discipline (ADR-0038 §5): both the platform-state and
                // transfer locks are released before dispatch, mirroring the
                // CursorMoved arm.
                if let (Some(event), Some(win)) = (event, window.as_ref()) {
                    win.callbacks()
                        .dispatch_input(PlatformInput::DragDrop(event));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Drop-burst boundary (ADR-0038): winit delivers one `DroppedFile`
        // per file with no end-of-burst marker, so the first `about_to_wait`
        // after at least one `DroppedFile` freezes the accumulated list as
        // the offer's payload and emits `Dropped`. A straggler after this
        // point mints a new defensive session — two complete drops, never
        // one truncated one (documented winit approximation ceiling).
        self.freeze_completed_drops();

        // Wall-clock wake: consult the registered
        // wake-deadline hook (issue #556 — `flui-app` computes the earliest
        // instant any hosted realm's pending gesture/timer deadline needs
        // the loop to wake at, across every realm on this thread) fresh on
        // EVERY iteration, not just once — a deadline that fires, a new one
        // getting armed, or every deadline clearing must all be reflected
        // immediately, not stale from whenever the hook was installed.
        // `Some(deadline)` -> `ControlFlow::WaitUntil`; `None` (no hook
        // installed, or the hook itself has nothing pending) falls back to
        // the previously-unconditional `Wait` — re-asserted every iteration
        // so an upstream winit default change can't silently turn the
        // wake-driven frame loop into a busy poll.
        //
        // Lock discipline (ADR-0038 §5, the same rule this module's
        // `CursorMoved`/`HoveredFileCancelled` arms already follow): the
        // hook itself re-enters `flui-app`, walking every hosted realm and
        // taking gesture-arena locks — it must never run while this
        // platform's own state mutex is held. Clone the `Arc` out, drop the
        // lock, THEN call it.
        let wake_deadline_hook = self
            .platform
            .with_state(|state| state.handlers.wake_deadline.clone());
        let wake_deadline = wake_deadline_hook.and_then(|hook| hook());

        // Harness self-close (see the `self_close_deadline` field doc): fire
        // once the deadline passes and a window exists, then let its own
        // deadline participate in the wait below so an idle loop still wakes
        // to fire it (`new_events`' `ResumeTimeReached` actuation pumps the
        // loop back here).
        self.fire_self_close_if_due(event_loop);

        let control_flow = match earliest_deadline(wake_deadline, self.self_close_deadline) {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        };
        tracing::trace!(
            wait_until_ms = control_flow_wait_ms(control_flow),
            "about to wait"
        );
        event_loop.set_control_flow(control_flow);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        self.process_control(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        // A close posted after the last drain but before admission closed
        // (from inside the exit-policy hook that ended the loop, say) still
        // gets its teardown — the loop is exiting, the window is not.
        self.complete_pending_closes(event_loop);
        self.finish_shutdown();
    }
}

/// Which winit file-drag arm produced a path.
#[derive(Debug, Clone, Copy)]
enum DragPath {
    /// `WindowEvent::HoveredFile` — the drag is still in flight.
    Hovered,
    /// `WindowEvent::DroppedFile` — the user released; arm the burst freeze.
    Dropped,
}

// Helper methods for WinitApp
impl WinitApp {
    /// Shared body of the `HoveredFile`/`DroppedFile` arms: feed the path
    /// into the drag-session state machine and dispatch any resulting event.
    ///
    /// Lock discipline (ADR-0038 §5): platform state is only locked to clone
    /// out the transfer source and the tracked cursor position; both that
    /// lock and the source's internal lock are released before
    /// `dispatch_input` runs, mirroring the CursorMoved arm.
    fn handle_drag_drop_path(
        &self,
        platform_id: WindowId,
        window: Option<&Arc<WinitWindow>>,
        path: PathBuf,
        kind: DragPath,
    ) {
        let (transfer, cursor) = self.platform.with_state(|state| {
            (
                Arc::clone(&state.data_transfer),
                state.cursor_positions.get(&platform_id).copied(),
            )
        });
        // Last tracked cursor position, if any — possibly stale on Wayland,
        // where an external drag grabs the cursor (documented limitation).
        let position = window
            .and_then(|win| cursor.map(|cursor| logical_cursor_point(cursor, win.scale_factor())));
        let event = match kind {
            DragPath::Hovered => transfer.note_hovered_file(platform_id, path, position),
            DragPath::Dropped => transfer.note_dropped_file(platform_id, path, position),
        };
        if let (Some(event), Some(win)) = (event, window) {
            win.callbacks()
                .dispatch_input(PlatformInput::DragDrop(event));
        }
    }

    /// Freeze completed drop bursts and dispatch their `Dropped` events.
    /// Cheap in the steady state: one uncontended lock to observe that no
    /// session is awaiting the freeze.
    fn freeze_completed_drops(&self) {
        let transfer = self
            .platform
            .with_state(|state| Arc::clone(&state.data_transfer));
        let windows = transfer.windows_awaiting_freeze();
        if windows.is_empty() {
            return;
        }

        // Snapshot window handles and cursor positions first; the platform
        // lock is released before scale-factor reads and dispatch.
        let mut snapshots: Vec<(WindowId, Arc<WinitWindow>, Option<_>)> = Vec::new();
        self.platform.with_state(|state| {
            for window_id in &windows {
                if let Some(win) = state.windows.get(window_id) {
                    snapshots.push((
                        *window_id,
                        Arc::clone(win),
                        state.cursor_positions.get(window_id).copied(),
                    ));
                }
            }
        });

        let mut positions = HashMap::new();
        let mut handles: HashMap<WindowId, Arc<WinitWindow>> = HashMap::new();
        for (window_id, win, cursor) in snapshots {
            if let Some(cursor) = cursor {
                positions.insert(window_id, logical_cursor_point(cursor, win.scale_factor()));
            }
            handles.insert(window_id, win);
        }

        for (window_id, event) in transfer.freeze_completed(&positions) {
            if let Some(win) = handles.get(&window_id) {
                win.callbacks()
                    .dispatch_input(PlatformInput::DragDrop(event));
            }
        }
    }

    fn process_control(&mut self, event_loop: &ActiveEventLoop) {
        if self.control.take_quit_requested() {
            self.complete_pending_closes(event_loop);
            self.request_exit(event_loop);
            return;
        }

        let drain_budget = self.control.begin_drain();
        for _ in 0..drain_budget {
            if self.control.take_quit_requested() {
                self.complete_pending_closes(event_loop);
                self.request_exit(event_loop);
                return;
            }

            let Some(command) = self.control.try_recv() else {
                break;
            };
            match command {
                ControlCommand::OpenWindow { options, reply } => {
                    // Claim-slot protocol (ADR-0039 §3): a request already
                    // abandoned before the owner ever looked at it skips
                    // creation entirely — never build a window nobody
                    // wants.
                    if reply.is_abandoned() {
                        tracing::debug!("window requester abandoned before creation; skipping");
                        continue;
                    }

                    let guard = OpenWindowReplyGuard::new(reply);
                    tracing::debug!("Processing window creation request");
                    match self.platform.create_window_now(event_loop, options) {
                        Ok(window_id) => {
                            let window = self
                                .platform
                                .window_by_id(window_id)
                                .expect("BUG: just-created window must be registered");
                            match guard.complete(Ok(window)) {
                                Some(reply) => self.in_flight_replies.push((window_id, reply)),
                                None => {
                                    // The requester abandoned in the race
                                    // between the `is_abandoned` check above
                                    // and this delivery — unwind instead of
                                    // leaking (ADR-0039 §3/§4).
                                    self.unwind_orphan_window(window_id);
                                }
                            }
                        }
                        Err(backend_error) => {
                            let _ = guard.complete(Err(backend_error));
                        }
                    }
                }
            }
        }

        self.complete_pending_closes(event_loop);

        // Sweep entries whose requester claimed delivery and then dropped
        // its handle without ever reading it (late abandonment) — reclaim
        // and unwind those, and drop fully-settled entries (claimed, or
        // abandoned-and-reclaimed) from the registry. Runs at this
        // `user_event` drain anchor every turn, per the claim-slot module
        // docs' sweep contract.
        self.sweep_settled_replies();

        if self.control.take_quit_requested() {
            self.request_exit(event_loop);
            return;
        }

        // Exit-policy re-evaluation (issue #558): a keep-alive application
        // service completed on a worker thread AFTER the last window's
        // close was vetoed — with no window left to produce events,
        // this coalesced request is the only path that ever re-asks the
        // hook, so a stale veto cannot hold the process open forever. Same
        // gate shape as the `CloseRequested` arm: only when this backend's
        // own window map is empty, hook leased and consulted OUTSIDE the
        // state lock (ADR-0039). Checked AFTER the drain above so a window
        // opened by a command in this same wake (a service asking for a
        // notification window while another service completes) is already
        // in the map and vetoes the re-check by count alone.
        if self.control.take_exit_reevaluation_requested() {
            let windows_empty = self.platform.with_state(|state| state.windows.is_empty());
            if windows_empty && self.platform.lease_exit_policy_hook().invoke() {
                self.request_exit(event_loop);
            }
        }
    }

    /// Reclaims and unwinds any `in_flight_replies` entry whose requester
    /// claimed delivery, then dropped its `PendingWindow` without reading
    /// it — and drops fully-settled entries (claimed, or already
    /// reclaimed) from the registry.
    fn sweep_settled_replies(&mut self) {
        // Take ownership of the whole registry first: `unwind_orphan_window`
        // needs `&self`, which would otherwise conflict with an active
        // `self.in_flight_replies.drain(..)` borrow for the loop's duration.
        let drained = std::mem::take(&mut self.in_flight_replies);
        let mut still_pending = Vec::with_capacity(drained.len());
        for (window_id, reply) in drained {
            if reply.take_abandoned().is_some() {
                self.unwind_orphan_window(window_id);
            }
            if !reply.is_settled() {
                still_pending.push((window_id, reply));
            }
        }
        self.in_flight_replies = still_pending;
    }

    /// Unwinds a window the requester never claimed: hides it (a visible
    /// flicker instead of a silent leak — the ADR's own framing), removes
    /// it from platform state, and retires any in-flight drag session
    /// addressed at it. No per-window user callbacks fire: the window was
    /// never delivered to any caller, so nothing outside this module ever
    /// registered one.
    fn unwind_orphan_window(&self, window_id: WindowId) {
        tracing::warn!(
            ?window_id,
            "orphaned window request (requester abandoned); unwinding"
        );
        let (window, data_transfer) = self.platform.with_state(|state| {
            let window = state.windows.remove(&window_id);
            state.window_id_map.retain(|_, v| *v != window_id);
            state.cursor_positions.remove(&window_id);
            (window, Arc::clone(&state.data_transfer))
        });
        if let Some(window) = window {
            // Directly, not through `PlatformWindow::close`: that route posts
            // a close request for the owner's next turn, and this window is
            // already out of the map it would look in.
            window.inner().set_visible(false);
        }
        data_transfer.forget_window(window_id);
    }

    /// Fires the harness self-close (see [`WinitApp::self_close_deadline`])
    /// when its deadline has passed and at least one window is tracked. On
    /// the default [`SelfCloseRoute::Compositor`] it synthesizes
    /// `WindowEvent::CloseRequested` through the ordinary
    /// [`ApplicationHandler::window_event`] entry so the whole close arm —
    /// should-close veto, per-window close callbacks, global handler, map
    /// removal, exit policy — runs exactly as a compositor-delivered close
    /// would; on [`SelfCloseRoute::Programmatic`] it calls
    /// [`PlatformWindow::close`] on that window instead, exactly as an
    /// application would. One-shot: the deadline is cleared on fire. If no window exists
    /// yet at the deadline, it stays armed until one does — re-paced a
    /// short interval forward on each check, never left in the past, so the
    /// loop keeps waiting instead of busy-waking on an expired
    /// `ControlFlow::WaitUntil`.
    fn fire_self_close_if_due(&mut self, event_loop: &ActiveEventLoop) {
        let due = self
            .self_close_deadline
            .is_some_and(|deadline| Instant::now() >= deadline);
        if !due {
            return;
        }
        let winit_id = self
            .platform
            .with_state(|state| state.window_id_map.keys().next().copied());
        let Some(winit_id) = winit_id else {
            // Due with no window tracked yet: re-pace instead of leaving the
            // past deadline armed. `about_to_wait` folds this deadline into
            // `ControlFlow::WaitUntil`, and an instant already behind
            // `Instant::now()` would re-fire winit's `ResumeTimeReached`
            // every iteration — a busy wake loop until a window appears.
            // FLUI's own bootstrap opens its window inside `on_ready`,
            // before the first `about_to_wait`, so this arm exists for an
            // embedder that opens its first window later (e.g. through the
            // deferred owner lane), which `Platform::run` permits.
            self.self_close_deadline = Some(Instant::now() + Duration::from_millis(50));
            return;
        };
        self.self_close_deadline = None;
        match self.self_close_route {
            SelfCloseRoute::Compositor => {
                tracing::info!(
                    route = "compositor",
                    "harness self-close deadline reached; synthesizing CloseRequested"
                );
                self.window_event(event_loop, winit_id, WinitWindowEvent::CloseRequested);
            }
            SelfCloseRoute::Programmatic => {
                // Through the public trait method, exactly as an application
                // would: it posts the close on the owner lane and wakes this
                // loop, whose next `user_event` drain runs the teardown.
                let window = self.platform.with_state(|state| {
                    state
                        .get_platform_window_id(winit_id)
                        .and_then(|id| state.windows.get(&id).cloned())
                });
                let Some(window) = window else {
                    return;
                };
                tracing::info!(
                    route = "programmatic",
                    "harness self-close deadline reached; calling PlatformWindow::close"
                );
                window.close();
            }
        }
    }

    /// The teardown every closing window takes once the decision to close
    /// is final — the ONE body behind both routes, because issue #919 was
    /// exactly the two routes being written separately: the compositor's
    /// `CloseRequested` arm (after its should-close veto) did all of this,
    /// while the programmatic [`PlatformWindow::close`] hid the window and
    /// fired its callback but never left the tracking map, so the exit
    /// policy — consulted only against that map — never saw the last
    /// window go, and the process lingered with nothing on screen.
    ///
    /// Always an owner-turn body, never run synchronously inside `close()`,
    /// for a reason stronger than "ending the loop needs the live
    /// `ActiveEventLoop`": a `close()` issued from inside one of this
    /// window's own callbacks runs while that callback is leased out of its
    /// slot, and `CallbackLease::drop` restores a leased callback into a
    /// slot it finds empty — so a synchronous `callbacks().clear()` would be
    /// undone the moment the callback returned, leaving the renderer-owning
    /// frame callback alive in a window that is out of the map (issue #713's
    /// class from the other direction). Deferring to the next owner turn is
    /// what makes step 4 stick.
    ///
    /// Order, load-bearing throughout:
    /// 1. hide the native window where the backend can (winit's
    ///    `set_visible` is a no-op on Wayland, Android and Web — there the
    ///    window disappears when its last handle drops) and report it not
    ///    visible; the visible effect should not wait for handle drops an
    ///    embedder may defer;
    /// 2. the per-window `on_close` callback (an embedder tears down what it
    ///    hung on this window — `flui-app` closes the window's presentation
    ///    here, BEFORE the exit policy below reads its realm registry);
    /// 3. removal from the window/cursor maps (and from `active_window`,
    ///    which otherwise keeps naming a window that no longer exists), and
    ///    retirement of any drag session addressed at this window (ADR-0038
    ///    offer lifecycle);
    /// 4. the callback clear — NOW, while the native window (`window` is
    ///    still live) and the event loop are both alive: the frame callback
    ///    owns the embedder's GPU renderer, whose `wgpu::Surface` must die
    ///    strictly before the winit window's Wayland objects (a swapchain
    ///    destroyed after its `wl_surface` marshals on a freed `wl_proxy`
    ///    and segfaults post-quit — issue #713). `WinitWindow::drop` repeats
    ///    the clear as a last resort for refs that die elsewhere;
    /// 5. the global `WindowEvent::Closed` — both routes: the window is gone
    ///    from this backend's ledger whichever way it left (the native
    ///    route's `CloseRequested` is emitted by its arm, before this body);
    /// 6. the exit-policy consult, only when this backend's own map is now
    ///    empty. That map is not the only ledger: an embedder hosting more
    ///    than one top-level window (issue #555's `WindowPolicy`) tracks
    ///    realms/presentations this layer cannot see, and a queued "open
    ///    another window" request (a splash's close handler asking for the
    ///    main window) must not be raced by an unconditional exit — so the
    ///    hook, when installed, gets the final word; unset, this is the
    ///    exact pre-#555 unconditional behavior.
    ///
    /// Lock discipline (ADR-0039): the global handler and the exit-policy
    /// hook are lease-taken and invoked OUTSIDE the platform state lock. The
    /// hook's own body drops removed realm state, whose destructors may call
    /// back into this platform (a dispose hook opening another window);
    /// consulting it under this non-reentrant lock would deadlock the
    /// instant such a callback re-entered `with_state`.
    fn complete_window_close(
        &mut self,
        event_loop: &ActiveEventLoop,
        platform_id: WindowId,
        window: Option<&Arc<WinitWindow>>,
    ) {
        if let Some(win) = window {
            win.inner().set_visible(false);
            win.set_visible(false);
            win.callbacks().dispatch_close();
        }

        let (windows_empty, transfer) = self.platform.with_state(|state| {
            state.window_id_map.retain(|_, v| *v != platform_id);
            state.windows.remove(&platform_id);
            state.cursor_positions.remove(&platform_id);
            if state.active_window == Some(platform_id) {
                state.active_window = None;
            }

            (state.windows.is_empty(), Arc::clone(&state.data_transfer))
        });
        transfer.forget_window(platform_id);

        if let Some(win) = window {
            win.callbacks().clear();
        }

        self.platform
            .lease_window_event_handler()
            .invoke(WindowEvent::Closed(platform_id));

        let should_exit = windows_empty && self.platform.lease_exit_policy_hook().invoke();
        if should_exit {
            self.request_exit(event_loop);
        }
    }

    /// Runs the close teardown for every window whose programmatic
    /// `close()` is waiting on the owner (issue #919). Called at the
    /// `user_event` drain anchor AFTER the command drain, so a window opened
    /// by a command in the same wake (a splash's close handler asking for
    /// the main window) is already in the map and vetoes exit by count
    /// alone; and called BEFORE any quit exits the loop — a close posted
    /// before the quit is a decision already made whose `on_close` the
    /// embedder is owed, and a quit that overtook it would strand that
    /// window hidden-but-tracked, the exact shape of the original defect. A
    /// window the map no longer holds (closed by the compositor in between,
    /// or an orphan already unwound) is skipped: its teardown already ran.
    fn complete_pending_closes(&mut self, event_loop: &ActiveEventLoop) {
        for window_id in self.control.take_close_requests() {
            let window = self
                .platform
                .with_state(|state| state.windows.get(&window_id).cloned());
            let Some(window) = window else {
                tracing::debug!(
                    ?window_id,
                    "programmatic close for a window no longer tracked"
                );
                continue;
            };
            // `route` is the live-smoke harnesses' oracle for this path
            // (`tools/live-smoke`): matched on the field, not the message.
            tracing::info!(
                ?window_id,
                route = "programmatic",
                "Window closed programmatically"
            );
            self.complete_window_close(event_loop, window_id, Some(&window));
        }
    }

    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
        tracing::info!("Quitting event loop");
        event_loop.exit();
        self.finish_shutdown();
    }

    fn finish_shutdown(&mut self) {
        self.close_owner_lane();
        self.notify_quit_once();
    }

    fn close_owner_lane(&mut self) {
        self.control.stop_accepting();
        self.reject_pending_commands();
        self.platform.mark_stopped();
    }

    fn reject_pending_commands(&self) {
        // Admission is closed and the lane is bounded to CONTROL_CAPACITY, so
        // draining to empty is finite and includes every accepted command.
        while let Some(command) = self.control.try_recv() {
            match command {
                ControlCommand::OpenWindow { reply, .. } => {
                    let _ = OpenWindowReplyGuard::new(reply)
                        .complete(Err(OpenWindowError::OwnerGone { rejected: None }));
                }
            }
        }
    }

    fn notify_quit_once(&mut self) {
        if self.quit_notified {
            return;
        }
        self.quit_notified = true;

        let callback = self.platform.with_state(|state| state.handlers.quit.take());
        if let Some(mut callback) = callback {
            callback();
        }
    }
}

impl Drop for WinitApp {
    fn drop(&mut self) {
        // Drop may run while user code is unwinding. Close only framework
        // resources here; user callbacks run exclusively from explicit finish.
        self.close_owner_lane();
    }
}

impl Platform for WinitPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        tracing::info!("Starting winit event loop via Platform::run()");
        let platform = Arc::new(*self);
        // Generic wording, not "Winit event loop error": the propagated
        // error may be a stashed `on_ready` bootstrap failure (window
        // creation, GPU init, root-widget attach), a genuine winit-level
        // error from `event_loop.run_app`, or both combined -- see
        // `run_event_loop`'s `combine_shutdown_result`. This is the sole
        // `error`-level log for whichever of those it turns out to be.
        platform.run_event_loop(on_ready).inspect_err(|error| {
            tracing::error!("winit Platform::run exited with an error: {:?}", error);
        })
    }

    fn quit(&self) {
        tracing::info!("Quit requested");

        let control = self.with_state(|state| match &mut state.run_state {
            WinitRunState::New { quit_requested } => {
                *quit_requested = true;
                None
            }
            WinitRunState::Starting { control, .. } | WinitRunState::Running { control, .. } => {
                Some(control.clone())
            }
            WinitRunState::Stopped => None,
        });
        if let Some(control) = control {
            control.request_quit();
        }
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        self.with_state(|state| {
            state.handlers.exit_policy = Some(hook);
        });
    }

    fn request_exit_policy_reevaluation(&self) {
        // Same run-state walk as `quit` above, minus the `New` arm: before
        // the loop starts there is no hook installed and no service
        // running, so a pre-run request has nothing to re-evaluate and is
        // deliberately dropped rather than parked.
        if let Some(control) = self.control_sender() {
            control.request_exit_reevaluation();
        }
    }

    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        self.with_state(|state| {
            // `Arc::from(Box<dyn Trait>)` is a real, sanctioned std
            // conversion, not a smuggled second allocation strategy: the
            // caller-facing API stays `Box`-based (matching
            // `set_exit_policy_hook`'s exact ergonomics), and this is the
            // one place that converts to the clone-able-out-of-the-lock
            // `Arc` storage `about_to_wait` needs (see `PlatformHandlers::
            // wake_deadline`'s own doc for why).
            state.handlers.wake_deadline = Some(Arc::from(hook));
        });
    }

    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
        tracing::info!(?options, "Requesting window creation");

        // Same-thread fast path: called synchronously from inside `on_ready`
        // (see `WinitApp::resumed`), where a live `ActiveEventLoop` is
        // published for exactly this nested call. Create the window
        // directly instead of enqueuing — the lane's `user_event` consumer
        // cannot run until this call returns, and would
        // deadlock forever otherwise (see the module-level doc).
        if let Some(event_loop_ptr) = ACTIVE_EVENT_LOOP.with(Cell::get) {
            tracing::debug!("Creating window on event-loop thread (same-thread fast path)");
            // SAFETY: `event_loop_ptr` is non-null only while
            // `with_active_event_loop` has published it on THIS thread, for
            // the exact nested `on_ready` call currently executing (see
            // `WinitApp::resumed`). The referent is `resumed`'s live
            // `&ActiveEventLoop` parameter, which outlives that whole call —
            // and this call is strictly nested inside it — so the reference
            // constructed here is valid for the borrow's entire duration.
            let event_loop = unsafe { event_loop_ptr.as_ref() };
            let window_id = self.create_window_now(event_loop, options)?;
            return self.window_by_id(window_id);
        }

        let control = match self.with_state(|state| {
            state
                .run_state
                .control_for_open_window(thread::current().id())
        }) {
            Ok(control) => control,
            // The loop has stopped: the owner is genuinely gone, and the
            // untouched options ride back so the caller can retry against a
            // fresh platform without rebuilding them.
            Err(OpenWindowStateError::Stopped) => {
                return Err(OpenWindowError::OwnerGone {
                    rejected: Some(options),
                });
            }
            // Lifecycle refusals: the loop is not (yet) in a state that can
            // service this call site — typed as `Unavailable`, distinct
            // from a dead owner.
            Err(state_error) => {
                return Err(OpenWindowError::Unavailable {
                    message: match state_error {
                        OpenWindowStateError::NotRunning => {
                            "open_window called before Platform::run — on the winit backend, \
                             create the first windows inside run()'s on_ready callback"
                        }
                        OpenWindowStateError::Starting => {
                            "open_window called from another thread before winit finished \
                             on_ready"
                        }
                        OpenWindowStateError::OwnerWouldBlock => {
                            "open_window cannot block the winit event-loop owner outside \
                             on_ready"
                        }
                        OpenWindowStateError::Stopped => unreachable!("BUG: handled above"),
                    }
                    .to_string(),
                });
            }
        };

        // The command crosses threads as data only. The owner creates the
        // window and completes this claim-slot request (ADR-0039 §3)
        // without exposing winit's thread-affine event-loop capability.
        let handle = control
            .request_open_window(options)
            .map_err(|error| match error {
                ControlSendError::Full { capacity, rejected } => {
                    OpenWindowError::LaneFull { capacity, rejected }
                }
                ControlSendError::OwnerGone { rejected } => OpenWindowError::OwnerGone {
                    rejected: Some(rejected),
                },
            })?;

        tracing::debug!("Waiting for window creation response");

        // This `handle` is freshly minted above and never offered to any
        // other caller, so `try_take` cannot have claimed it already —
        // `AlreadyClaimed` here would mean this fast path itself raced a
        // second claim attempt, which the code above never performs.
        // `OwnerGone` is real, though: the event-loop owner can disconnect
        // (quit, panic-unwind) while this request is in flight on the lane.
        let window = match handle.wait() {
            ClaimOutcome::Delivered(result) => result?,
            ClaimOutcome::AlreadyClaimed => {
                unreachable!("BUG: this handle is never polled by another caller before wait")
            }
            ClaimOutcome::OwnerGone => {
                return Err(OpenWindowError::OwnerGone { rejected: None });
            }
        };

        tracing::info!("Window created successfully");
        Ok(window)
    }

    fn active_window(&self) -> Option<WindowId> {
        self.with_state(|state| state.active_window)
    }

    // Empty until `WinitApp::resumed` runs `init_displays` on first resume —
    // callers that need real display info must call this from `on_ready` (or
    // later), never before `Platform::run` starts the event loop.
    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        self.with_state(|state| {
            state
                .displays
                .iter()
                .map(|d| d.clone() as Arc<dyn PlatformDisplay>)
                .collect()
        })
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        self.with_state(|state| {
            state
                .displays
                .iter()
                .find(|d| d.is_primary())
                .map(|d| d.clone() as Arc<dyn PlatformDisplay>)
        })
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|state| state.clipboard.clone())
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // Clones of the ONE source constructed at platform init (ADR-0038
        // §2): every id it mints stays redeemable at every clone.
        self.with_state(|state| Arc::clone(&state.data_transfer) as Arc<dyn DataTransferSource>)
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        return "Winit (Windows)";

        #[cfg(target_os = "macos")]
        return "Winit (macOS)";

        #[cfg(target_os = "linux")]
        return "Winit (Linux)";

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return "Winit";
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.quit = Some(callback);
        });
    }

    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|state| {
            state.handlers.reopen = Some(callback);
        });
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.with_state(|state| {
            state.handlers.window_event = Some(callback);
        });
    }

    fn reveal_path(&self, path: &Path) {
        // The extension, not the path. This is a document the user picked, so
        // its name is their data; what a trace needs is that the call happened
        // and roughly on what.
        tracing::info!(
            extension = ?path.extension(),
            "Revealing path"
        );

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg("/select,")
                .arg(path)
                .spawn();
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn();
        }

        #[cfg(target_os = "linux")]
        {
            // Try xdg-open with parent directory
            if let Some(parent) = path.parent() {
                let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
            }
        }
    }

    fn open_path(&self, path: &Path) {
        // See `reveal_path`: user-chosen document, extension only.
        tracing::info!(
            extension = ?path.extension(),
            "Opening path"
        );

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .arg("/C")
                .arg("start")
                .arg(path)
                .spawn();
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(path).spawn();
        }

        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        }
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        std::env::current_exe().map_err(|error| PlatformError::AppPath {
            message: error.to_string(),
        })
    }
}

/// [`OwnerHooks`] for the winit backend (ADR-0039 §1). Inside `on_ready`,
/// `ACTIVE_EVENT_LOOP` is published, so creation is direct and synchronous;
/// afterwards, this defers through the owner lane instead of blocking or
/// re-entering backend state — the load-bearing design choice that lets a
/// call from arbitrary owner-thread callback context (a handler invoked by
/// [`WinitApp::window_event`], now hoisted outside the state lock) neither
/// deadlock nor mutate the platform window map mid-frame.
struct WinitOwnerHooks {
    platform: Arc<WinitPlatform>,
    owner_thread: ThreadId,
}

impl OwnerHooks for WinitOwnerHooks {
    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        if let Some(event_loop_ptr) = ACTIVE_EVENT_LOOP.with(Cell::get) {
            // SAFETY: identical justification to `Platform::open_window`'s
            // same-thread fast path above — this call is reached only from
            // inside the same nested `on_ready` invocation that publishes
            // `ACTIVE_EVENT_LOOP`.
            let event_loop = unsafe { event_loop_ptr.as_ref() };
            let window_id = self.platform.create_window_now(event_loop, options)?;
            let window = self.platform.window_by_id(window_id)?;
            return Ok(WindowOpen::Ready(window));
        }

        // Post-bootstrap: defer through the owner lane instead of creating
        // directly (ADR-0039 §1) — a deferred `Pending` resolves at the
        // next `user_event` drain anchor rather than risking re-entrance
        // into backend state from arbitrary callback context.
        let control = self.platform.with_state(|state| match &state.run_state {
            WinitRunState::Running { control, .. } => Some(control.clone()),
            _ => None,
        });
        let Some(control) = control else {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(options),
            });
        };
        match control.request_open_window(options) {
            Ok(handle) => Ok(WindowOpen::Pending(PendingWindow::new(
                handle,
                self.owner_thread,
            ))),
            Err(ControlSendError::Full { capacity, rejected }) => {
                Err(OpenWindowError::LaneFull { capacity, rejected })
            }
            Err(ControlSendError::OwnerGone { rejected }) => Err(OpenWindowError::OwnerGone {
                rejected: Some(rejected),
            }),
        }
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        Arc::new(WinitProxyTransport {
            platform: Arc::clone(&self.platform),
            owner_thread: self.owner_thread,
        })
    }
}

/// [`ProxyTransport`] for the winit backend: worker threads reach the owner
/// lane exactly as the pre-ADR-0039 cross-thread `Platform::open_window`
/// path already did — enqueue-and-wake, claim-slot reply.
struct WinitProxyTransport {
    platform: Arc<WinitPlatform>,
    owner_thread: ThreadId,
}

impl WinitProxyTransport {
    fn control(&self) -> Option<ControlSender> {
        self.platform.with_state(|state| match &state.run_state {
            WinitRunState::Starting { control, .. } | WinitRunState::Running { control, .. } => {
                Some(control.clone())
            }
            WinitRunState::New { .. } | WinitRunState::Stopped => None,
        })
    }
}

impl ProxyTransport for WinitProxyTransport {
    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<PendingWindow, ProxySendError<WindowOptions>> {
        let Some(control) = self.control() else {
            return Err(ProxySendError::OwnerGone { rejected: options });
        };
        control
            .request_open_window(options)
            .map(|handle| PendingWindow::new(handle, self.owner_thread))
            .map_err(|error| match error {
                ControlSendError::Full { capacity, rejected } => {
                    ProxySendError::Full { capacity, rejected }
                }
                ControlSendError::OwnerGone { rejected } => ProxySendError::OwnerGone { rejected },
            })
    }

    fn request_quit(&self) -> Result<(), ProxySendError<()>> {
        match self.control() {
            Some(control) => {
                control.request_quit();
                Ok(())
            }
            // A lane existed (winit always has one) but the loop already
            // stopped -- `OwnerGone`, not `Unsupported`: this backend does
            // support cross-thread quit requests in general, this
            // particular loop instance is just gone.
            None => Err(ProxySendError::OwnerGone { rejected: () }),
        }
    }

    fn owner_thread(&self) -> ThreadId {
        self.owner_thread
    }
}

// The real-event-loop test family lives in its own file: every test in it
// runs `event_loop.run_app` on an ordinary test thread, and together with the
// four helpers nothing else uses it outgrew this file's budget (issue #923).
// A sibling module rather than a nested one, so its tests sit beside `tests`
// rather than two segments below it.
#[cfg(test)]
#[path = "platform/real_loop_tests.rs"]
mod real_loop_tests;

#[cfg(test)]
mod tests {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::{
        OpenWindowReplyGuard, OpenWindowStateError, SelfCloseRoute, WinitApp, WinitPlatform,
        WinitRunState, combine_shutdown_result,
    };
    use crate::{
        error::PlatformError,
        platforms::winit::control::{ControlCommand, control_lane},
        traits::{Platform, WindowOptions},
    };

    // ========================================================================
    // combine_shutdown_result
    // ========================================================================
    //
    // Fast, deterministic, no event loop needed -- these pin the exact
    // combining logic `run_event_loop` calls, independent of whether a real
    // winit loop can also be driven to produce both error shapes at once
    // (which it cannot, deterministically, in a test).

    #[test]
    fn combine_shutdown_result_is_ok_when_neither_side_errors() {
        assert!(combine_shutdown_result(None, Ok(())).is_ok());
    }

    #[test]
    fn combine_shutdown_result_surfaces_a_lone_bootstrap_error() {
        let error = combine_shutdown_result(Some("bootstrap failed".into()), Ok(()))
            .expect_err("a stashed bootstrap error must be Err even when the loop exits cleanly");
        match &error {
            PlatformError::Bootstrap { source, loop_error } => {
                assert_eq!(source.to_string(), "bootstrap failed");
                assert!(
                    loop_error.is_none(),
                    "no loop error occurred, so none may be attached: {loop_error:?}"
                );
            }
            other => panic!("expected PlatformError::Bootstrap, got: {other:?}"),
        }
    }

    #[test]
    fn combine_shutdown_result_surfaces_a_lone_loop_error() {
        let error = combine_shutdown_result(
            None,
            Err(PlatformError::EventLoop {
                message: "loop failed".to_string(),
            }),
        )
        .expect_err("a loop-level error with no bootstrap error must still be Err");
        assert_eq!(error.to_string(), "platform event loop failed: loop failed");
    }

    /// The regression this whole function exists to fix: checking `result`
    /// before `bootstrap_error` (the pre-fix ordering) would return only
    /// `loop failed` here and silently drop `bootstrap failed` -- in
    /// exactly the scenario the fallible-`on_ready` design exists to
    /// surface. The bootstrap error must still be the primary `Err`
    /// returned (its own error as the `source`), with the loop error
    /// preserved (not lost) alongside it.
    #[test]
    fn combine_shutdown_result_keeps_the_bootstrap_error_as_root_cause_when_both_occur() {
        let error = combine_shutdown_result(
            Some("bootstrap failed".into()),
            Err(PlatformError::EventLoop {
                message: "loop failed".to_string(),
            }),
        )
        .expect_err("both a bootstrap and a loop error must still be Err");
        match &error {
            PlatformError::Bootstrap { source, loop_error } => {
                assert_eq!(
                    source.to_string(),
                    "bootstrap failed",
                    "the bootstrap error (root cause) must survive as the source"
                );
                let loop_error = loop_error
                    .as_deref()
                    .expect("the loop error must be attached, not silently dropped");
                assert!(
                    loop_error.contains("loop failed"),
                    "the loop error's own message must survive, got: {loop_error:?}"
                );
            }
            other => panic!("expected PlatformError::Bootstrap, got: {other:?}"),
        }
    }

    #[test]
    fn winit_owner_thread_open_outside_active_event_loop_is_rejected() {
        let owner_thread = std::thread::current().id();
        let (control, _receiver) = control_lane(Arc::new(|| {}));
        let run_state = WinitRunState::Running {
            owner_thread,
            control,
        };

        assert_eq!(
            run_state
                .control_for_open_window(owner_thread)
                .expect_err("the owner must never block waiting on its own event loop"),
            OpenWindowStateError::OwnerWouldBlock
        );
    }

    /// `set_exit_policy_hook` must land in the same `PlatformHandlers` slot
    /// the `CloseRequested` path reads via `invoke_exit_policy` -- not a
    /// second, disconnected storage location. Storage-only by design; the
    /// full `CloseRequested` arm under a live `ActiveEventLoop` is driven
    /// end-to-end by
    /// `close_requested_drops_window_callbacks_and_self_close_exits_the_loop`
    /// below.
    #[test]
    fn set_exit_policy_hook_installs_into_the_shared_handler_slot() {
        let platform = WinitPlatform::new();
        assert!(
            platform.with_state(|state| state.handlers.exit_policy.is_none()),
            "no hook installed yet"
        );

        platform.set_exit_policy_hook(Box::new(|| false));

        let vetoes_exit = platform.with_state(|state| state.handlers.invoke_exit_policy());
        assert!(
            !vetoes_exit,
            "the installed hook's answer must be exactly what invoke_exit_policy returns"
        );
    }

    /// `set_wake_deadline_hook` must land in the same `PlatformHandlers` slot
    /// `about_to_wait` clones the hook out of before dropping the state
    /// guard — the storage-level counterpart of
    /// `set_exit_policy_hook_installs_into_the_shared_handler_slot` above.
    /// This test reaches that slot through `PlatformHandlers::
    /// invoke_wake_deadline` for convenience; `about_to_wait` itself does
    /// NOT call that method in production (it takes `&self` and would hold
    /// the platform state lock for the hook's whole re-entrant call —
    /// see `PlatformHandlers::wake_deadline`'s own doc), so
    /// `invoke_wake_deadline` is a public method with no production caller
    /// in this backend, not a `cfg(test)`-gated one. Storage-only: driving
    /// a real `about_to_wait` through a live `ActiveEventLoop` is not
    /// exercised anywhere in this test module (same stated gap as that
    /// test).
    #[test]
    fn set_wake_deadline_hook_installs_into_the_shared_handler_slot() {
        let platform = WinitPlatform::new();
        assert!(
            platform.with_state(|state| state.handlers.wake_deadline.is_none()),
            "no hook installed yet"
        );
        assert_eq!(
            platform.with_state(|state| state.handlers.invoke_wake_deadline()),
            None,
            "unset hook must answer None, not panic or fabricate a deadline"
        );

        let deadline = web_time::Instant::now() + std::time::Duration::from_millis(250);
        platform.set_wake_deadline_hook(Box::new(move || Some(deadline)));

        assert_eq!(
            platform.with_state(|state| state.handlers.invoke_wake_deadline()),
            Some(deadline),
            "the installed hook's answer must be exactly what invoke_wake_deadline returns"
        );
    }

    #[test]
    fn winit_quit_callback_runs_once_on_owner_outside_platform_state_lock() {
        let platform = Arc::new(WinitPlatform::new());
        let owner_thread = std::thread::current().id();
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_for_handler = Arc::clone(&callback_count);
        let state_for_handler = Arc::clone(&platform.state);
        platform.on_quit(Box::new(move || {
            assert_eq!(std::thread::current().id(), owner_thread);
            assert!(
                state_for_handler.try_lock().is_some(),
                "quit callback must run after releasing platform state"
            );
            callback_count_for_handler.fetch_add(1, Ordering::Relaxed);
        }));
        let (control, receiver) = control_lane(Arc::new(|| {}));
        platform.with_state(|state| {
            state.run_state = WinitRunState::Running {
                owner_thread,
                control,
            };
        });
        let platform_for_worker = Arc::clone(&platform);
        std::thread::spawn(move || platform_for_worker.quit())
            .join()
            .expect("quit requester does not panic");
        assert_eq!(
            callback_count.load(Ordering::Relaxed),
            0,
            "the requesting thread only signals the owner"
        );
        assert!(receiver.take_quit_requested());
        let mut app = WinitApp {
            platform,
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        };

        app.notify_quit_once();
        app.notify_quit_once();

        assert_eq!(callback_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn winit_shutdown_replies_to_admitted_requests_before_app_drop() {
        let platform = Arc::new(WinitPlatform::new());
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        let mut replies: Vec<_> = (0..3)
            .map(|index| {
                sender
                    .request_open_window(WindowOptions {
                        title: format!("pending-{index}"),
                        ..WindowOptions::default()
                    })
                    .expect("request is admitted before shutdown")
            })
            .collect();
        let mut app = WinitApp {
            platform,
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        };

        app.finish_shutdown();

        assert_eq!(app.control.pending_count(), 0);
        for reply in &mut replies {
            let result = reply
                .try_take()
                .expect("shutdown responds while WinitApp is still alive");
            assert!(result.is_err());
        }
    }

    #[test]
    fn winit_app_drop_during_unwind_closes_the_owner_lane() {
        let platform = Arc::new(WinitPlatform::new());
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        platform.with_state(|state| {
            state.run_state = WinitRunState::Running {
                owner_thread: std::thread::current().id(),
                control: sender.clone(),
            };
        });
        let mut replies: Vec<_> = (0..3)
            .map(|index| {
                sender
                    .request_open_window(WindowOptions {
                        title: format!("unwind-{index}"),
                        ..WindowOptions::default()
                    })
                    .expect("request is admitted before owner unwind")
            })
            .collect();

        let platform_for_unwind = Arc::clone(&platform);
        let unwind = catch_unwind(AssertUnwindSafe(move || {
            let _app = WinitApp {
                platform: platform_for_unwind,
                on_ready: None,
                control: receiver,
                quit_notified: false,
                in_flight_replies: Vec::new(),
                bootstrap_error: None,
                self_close_deadline: None,
                self_close_route: SelfCloseRoute::default(),
            };
            panic!("exercise WinitApp unwind cleanup");
        }));
        assert!(unwind.is_err());

        for reply in &mut replies {
            assert!(
                reply
                    .try_take()
                    .expect("owner unwind returns an explicit result, not disconnect")
                    .is_err()
            );
        }
        assert!(platform.with_state(|state| matches!(&state.run_state, WinitRunState::Stopped)));
        assert!(matches!(
            sender
                .request_open_window(WindowOptions::default())
                .expect_err("closed owner lane rejects new work"),
            crate::platforms::winit::control::ControlSendError::OwnerGone { .. }
        ));
    }

    #[test]
    fn winit_panicking_quit_callback_runs_after_idempotent_owner_close() {
        let platform = Arc::new(WinitPlatform::new());
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        platform.with_state(|state| {
            state.run_state = WinitRunState::Running {
                owner_thread: std::thread::current().id(),
                control: sender.clone(),
            };
        });
        let mut replies: Vec<_> = (0..3)
            .map(|index| {
                sender
                    .request_open_window(WindowOptions {
                        title: format!("quit-panic-{index}"),
                        ..WindowOptions::default()
                    })
                    .expect("request is admitted before quit")
            })
            .collect();
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_for_handler = Arc::clone(&callback_count);
        let platform_for_handler = Arc::clone(&platform);
        platform.on_quit(Box::new(move || {
            callback_count_for_handler.fetch_add(1, Ordering::Relaxed);
            assert!(
                platform_for_handler
                    .with_state(|state| matches!(&state.run_state, WinitRunState::Stopped))
            );
            panic!("exercise panicking quit callback cleanup");
        }));
        let mut app = WinitApp {
            platform: Arc::clone(&platform),
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        };

        let first_finish = catch_unwind(AssertUnwindSafe(|| app.finish_shutdown()));
        assert!(first_finish.is_err(), "user callback panic must propagate");
        for reply in &mut replies {
            assert!(
                reply
                    .try_take()
                    .expect("owner closes queued request before invoking user code")
                    .is_err()
            );
        }
        assert!(platform.with_state(|state| matches!(&state.run_state, WinitRunState::Stopped)));
        assert_eq!(callback_count.load(Ordering::Relaxed), 1);

        let second_finish = catch_unwind(AssertUnwindSafe(|| app.finish_shutdown()));
        assert!(second_finish.is_ok(), "owner close is idempotent");
        assert_eq!(callback_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn winit_in_flight_reply_guard_returns_explicit_error_during_unwind() {
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        let mut handle = sender
            .request_open_window(WindowOptions::default())
            .expect("request is admitted");
        assert_eq!(receiver.begin_drain(), 1);
        let ControlCommand::OpenWindow { reply, .. } =
            receiver.try_recv().expect("owner dequeues request");

        let unwind = catch_unwind(AssertUnwindSafe(move || {
            let _reply_guard = OpenWindowReplyGuard::new(reply);
            panic!("exercise in-flight reply unwind");
        }));
        assert!(unwind.is_err());
        assert!(
            handle
                .try_take()
                .expect("reply guard returns explicit error instead of disconnect")
                .is_err()
        );
    }

    /// Two touch devices routinely both report contact 0; identity must be
    /// the `(device, contact)` pair or the binding tears one sequence down
    /// with the other's events. Terminal phases release the entry, and ids
    /// are never reused within a session.
    #[test]
    fn touch_contact_identity_is_per_device_and_never_reused() {
        use winit::event::TouchPhase;

        let mut contacts = std::collections::HashMap::new();
        let mut next = 2u64;
        let device_a = winit::event::DeviceId::dummy();
        // winit exposes no second dummy device; distinct CONTACT ids on one
        // device exercise the same map key shape.
        let a0 = super::resolve_touch_pointer_id(
            &mut contacts,
            &mut next,
            (device_a, 0),
            TouchPhase::Started,
        );
        let a1 = super::resolve_touch_pointer_id(
            &mut contacts,
            &mut next,
            (device_a, 1),
            TouchPhase::Started,
        );
        assert_ne!(a0, a1, "simultaneous contacts get distinct pointer ids");
        assert_ne!(a0, 1, "never the mouse's PRIMARY");

        // A move resolves to the same id as its Down.
        let a0_move = super::resolve_touch_pointer_id(
            &mut contacts,
            &mut next,
            (device_a, 0),
            TouchPhase::Moved,
        );
        assert_eq!(a0, a0_move);

        // The terminal phase releases the entry; the NEXT contact 0 gets a
        // fresh id — a late event for the dead contact can never alias it.
        let a0_up = super::resolve_touch_pointer_id(
            &mut contacts,
            &mut next,
            (device_a, 0),
            TouchPhase::Ended,
        );
        assert_eq!(a0, a0_up, "the Up itself still addresses the old sequence");
        let a0_reborn = super::resolve_touch_pointer_id(
            &mut contacts,
            &mut next,
            (device_a, 0),
            TouchPhase::Started,
        );
        assert_ne!(a0, a0_reborn, "contact ids are not reused after release");
        assert!(
            contacts.len() == 2,
            "live entries: contact 1 and reborn contact 0"
        );
    }
}
