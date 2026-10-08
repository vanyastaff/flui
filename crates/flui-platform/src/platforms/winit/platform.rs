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
use parking_lot::Mutex;
use ui_events_winit::keyboard::from_winit_modifier_state;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent as WinitWindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::ModifiersState,
    window::{WindowAttributes, WindowId as WinitWindowId},
};

use flui_foundation::geometry::Point;

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
        Clipboard, DesktopCapabilities, HostWindow, OpenWindowError, OwnerPlatform, PendingWindow,
        Platform, PlatformCapabilities, PlatformDisplay, PlatformExecutor, PlatformInput,
        PlatformReadyCallback, PlatformWindow, ProxySendError, WindowEvent, WindowId, WindowOpen,
        WindowOptions,
        owner::{OwnerHooks, ProxyTransport},
    },
};

/// Convert a tracked physical cursor position to logical pixels.
fn logical_cursor_point(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
) -> Point<f64> {
    Point::new(position.x / scale_factor, position.y / scale_factor)
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
    owner_signal: Mutex<Option<Arc<crate::shared::owner_signal::OwnerSignal>>>,
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
    /// Native pointer identities and phase state are committed before dispatch.
    native_pointer: winit_events::NativePointerState,
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

    /// Current keyboard modifiers, held in winit's own raw form.
    ///
    /// Not pre-converted to `keyboard_types::Modifiers`: the keyboard path
    /// hands this straight to `keyboard_event` → `from_winit_keyboard_event`
    /// (which does its own `from_winit_modifier_state` conversion as part
    /// of assembling the whole event), and the pointer paths convert it at
    /// the point of use via `from_winit_modifier_state` — one native value,
    /// converted once per read site rather than cached in a second form
    /// that could drift from what `ModifiersChanged` actually reported.
    current_modifiers: ModifiersState,
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
            native_pointer: winit_events::NativePointerState::default(),
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
            current_modifiers: ModifiersState::empty(),
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
        let monitors = event_loop.available_monitors();
        let primary_monitor = event_loop.primary_monitor();
        self.displays = map_monitor_displays(monitors, primary_monitor, |monitor, id, primary| {
            Arc::new(WinitDisplay::new(monitor, id, primary))
        });

        tracing::info!(count = self.displays.len(), "Initialized displays");
    }
}

fn map_monitor_displays<M: PartialEq, D>(
    monitors: impl Iterator<Item = M>,
    primary: Option<M>,
    mut make: impl FnMut(M, u64, bool) -> D,
) -> Vec<D> {
    monitors
        .enumerate()
        .map(|(index, monitor)| {
            let is_primary = primary
                .as_ref()
                .map_or(index == 0, |primary| primary == &monitor);
            make(monitor, index as u64, is_primary)
        })
        .collect()
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
            owner_signal: Mutex::new(None),
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
        let owner_proxy = event_loop_proxy.clone();
        let signal = crate::shared::owner_signal::OwnerSignal::new(Arc::new(move || {
            owner_proxy
                .send_event(())
                .map_err(|error| PlatformError::EventLoop {
                    message: error.to_string(),
                })
        }));
        let previous_signal = self.owner_signal.lock().replace(signal);
        drop(previous_signal);
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
                options.size.width,
                options.size.height,
            ))
            .with_resizable(options.resizable)
            .with_decorations(options.decorated)
            .with_visible(options.visible);

        if let Some(min) = options.min_size {
            attributes =
                attributes.with_min_inner_size(winit::dpi::LogicalSize::new(min.width, min.height));
        }
        if let Some(max) = options.max_size {
            attributes =
                attributes.with_max_inner_size(winit::dpi::LogicalSize::new(max.width, max.height));
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
    /// exact stored allocation as a [`HostWindow`]. Named `window_by_id`
    /// (not `window_handle`) to avoid colliding with the unrelated
    /// [`PlatformWindow::window_handle`] raw GPU-handle accessor.
    fn window_by_id(&self, window_id: WindowId) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        self.with_state(|state| {
            state
                .windows
                .get(&window_id)
                .ok_or_else(|| OpenWindowError::Backend {
                    message: "Window not found in state".to_string(),
                })
                .map(|window| Arc::clone(window) as Arc<dyn HostWindow>)
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

enum NativeDispatch {
    Input(PlatformInput),
    Hover(bool),
}

fn dispatch_native_inputs(inputs: Vec<(Arc<WinitWindow>, NativeDispatch)>) {
    let mut first = None;
    for (window, input) in inputs {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match input {
            NativeDispatch::Input(input) => {
                window.callbacks().dispatch_input(input);
            }
            NativeDispatch::Hover(hovered) => {
                window.callbacks().dispatch_hover_status_change(hovered);
            }
        }));
        if let Err(payload) = result {
            if first.is_some() {
                flui_foundation::panic::retain_opaque_payload(payload);
            } else {
                first = Some(payload);
            }
        }
        // Callback reentry can close the window and release its final other
        // owner. After a failure, its opaque captures must not retire here.
        if first.is_some() {
            std::mem::forget(window);
        } else if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(window)))
        {
            first = Some(payload);
        }
    }
    if let Some(payload) = first {
        std::panic::resume_unwind(payload);
    }
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
        let signal = self.platform.owner_signal.lock().clone();
        if let Some(signal) = signal
            && let Err(error) = signal.start()
        {
            tracing::error!(%error, "owner wake start failed");
            self.request_exit(event_loop);
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        let inputs = self.platform.with_state(|state| {
            let mut windows: Vec<_> = state.windows.keys().copied().collect();
            windows.sort_by_key(|window| window.0);
            let inputs = state
                .native_pointer
                .device_event(device_id, &event, &windows);
            inputs
                .into_iter()
                .filter_map(|(window, input)| {
                    state
                        .windows
                        .get(&window)
                        .cloned()
                        .map(|window| (window, NativeDispatch::Input(input)))
                })
                .collect()
        });
        dispatch_native_inputs(inputs);
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

        let native_pointer = matches!(
            event,
            WinitWindowEvent::CursorMoved { .. }
                | WinitWindowEvent::MouseInput { .. }
                | WinitWindowEvent::Touch(_)
                | WinitWindowEvent::MouseWheel { .. }
                | WinitWindowEvent::PinchGesture { .. }
                | WinitWindowEvent::RotationGesture { .. }
                | WinitWindowEvent::PanGesture { .. }
                | WinitWindowEvent::TouchpadPressure { .. }
                | WinitWindowEvent::CursorEntered { .. }
                | WinitWindowEvent::CursorLeft { .. }
        );
        if native_pointer {
            let Some(window) = window else {
                return;
            };
            if let WinitWindowEvent::MouseWheel { delta, .. } = &event {
                tracing::debug!(?delta, "MouseWheel");
            }
            let scale = window.scale_factor();
            let inputs = self.platform.with_state(|state| {
                if let WinitWindowEvent::CursorMoved { position, .. } = &event {
                    state.cursor_positions.insert(platform_id, *position);
                }
                let modifiers = from_winit_modifier_state(state.current_modifiers);
                state
                    .native_pointer
                    .window_event(platform_id, &event, scale, modifiers)
                    .unwrap_or_default()
            });
            let mut batch: Vec<_> = inputs
                .into_iter()
                .map(|input| (Arc::clone(&window), NativeDispatch::Input(input)))
                .collect();
            match event {
                WinitWindowEvent::CursorEntered { .. } => {
                    batch.push((Arc::clone(&window), NativeDispatch::Hover(true)));
                }
                WinitWindowEvent::CursorLeft { .. } => {
                    batch.push((Arc::clone(&window), NativeDispatch::Hover(false)));
                }
                _ => {}
            }
            dispatch_native_inputs(batch);
            return;
        }
        if matches!(event, WinitWindowEvent::Focused(false)) {
            let inputs = self.platform.with_state(|state| {
                state.native_pointer.cancel_window(
                    platform_id,
                    flui_platform_api::pointer::CancelReason::FocusLost,
                )
            });
            if let Some(window) = window.as_ref() {
                dispatch_native_inputs(
                    inputs
                        .into_iter()
                        .map(|input| (Arc::clone(window), NativeDispatch::Input(input)))
                        .collect(),
                );
            }
        }

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
                use flui_foundation::geometry::Size;

                let size = Size::new(physical_size.width as i32, physical_size.height as i32);

                tracing::debug!(?platform_id, ?size, "Window resized");

                // Dispatch per-window resize callback
                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let logical = Size::new(
                        physical_size.width as f64 / scale,
                        physical_size.height as f64 / scale,
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
                    } else if state.active_window == Some(platform_id) {
                        state.active_window = None;
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
                // A state-sync event ought to update key state without being
                // treated as the user's direct action. FLUI's
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
                // Raw winit modifiers state, unconverted: `keyboard_event`
                // hands it straight to `from_winit_keyboard_event`, which
                // does its own `from_winit_modifier_state` conversion as
                // part of assembling the whole `KeyboardEvent`.
                let modifiers = self.platform.with_state(|s| s.current_modifiers);

                if let Some(ref win) = window {
                    let input = winit_events::keyboard_event(event, modifiers);
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
                    state.current_modifiers = new_modifiers.state();
                });
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

        // An exit requested anywhere in this callback (the self-close above
        // is one) must not be followed by a blocking wait: winit's Windows
        // runner blocks on the control flow set here right after
        // `about_to_wait` returns and checks the exit flag only once the
        // wait ends. With the owner lane already shut down nothing wakes it,
        // so the loop stayed parked until an unrelated message arrived (six
        // minutes in the test harness). `Poll` makes that wait return
        // immediately, and the runner exits on the check that follows.
        if event_loop.exiting() {
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
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
        let signal = self.platform.owner_signal.lock().clone();
        if signal.as_ref().is_some_and(|signal| signal.quitting()) {
            self.request_exit(event_loop);
            return;
        }
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

        if let Some(signal) = signal
            && signal.drive()
        {
            self.request_exit(event_loop);
            return;
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
            state.native_pointer.cancel_window(
                window_id,
                flui_platform_api::pointer::CancelReason::FocusLost,
            );
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
            win.mark_closed();
            win.inner().set_visible(false);
            win.set_visible(false);
            win.callbacks().dispatch_close();
        }

        let (windows_empty, transfer) = self.platform.with_state(|state| {
            state.window_id_map.retain(|_, v| *v != platform_id);
            state.windows.remove(&platform_id);
            state.cursor_positions.remove(&platform_id);
            state.native_pointer.cancel_window(
                platform_id,
                flui_platform_api::pointer::CancelReason::FocusLost,
            );
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
        let signal = self.platform.owner_signal.lock().clone();
        if let Some(signal) = signal {
            signal.close();
        }
        self.release_open_window_callbacks();
        self.close_owner_lane();
        self.notify_quit_once();
    }

    /// Clears every still-tracked window's callback slots on the way out.
    ///
    /// `complete_pending_closes` only tears down windows whose `close()` was
    /// actually requested — a window left open when the loop is asked to
    /// quit (`owner.quit()`, or `on_ready` failing after step 6) never runs
    /// [`Self::complete_window_close`] at all, so its `on_request_frame`
    /// callback — where production code
    /// (`install_pre_present_hook`, `crates/flui-app/src/app/runner/frame_pacing.rs`)
    /// stores its own `Arc::clone` of the window inside the renderer — was
    /// never released: window -> callback slot -> frame closure ->
    /// `Arc<window>` is a strong cycle, and only a callback-clearing call
    /// breaks it (see [`crate::shared::WindowCallbacks::clear`]'s own doc).
    /// This does not run the rest of `complete_window_close`'s teardown
    /// (visibility, `on_close`, the global `Closed` event, the exit-policy
    /// re-consult) — that body takes a live `&ActiveEventLoop`, and
    /// [`Self::finish_shutdown`] (this method's caller) also runs once more
    /// from `run_event_loop`, after `event_loop.run_app` has returned and
    /// consumed the `EventLoop` itself, by which point no `ActiveEventLoop`
    /// can exist to pass it. Clearing callbacks is the minimum that breaks
    /// the cycle and is safe to run unconditionally: `clear()` is
    /// idempotent-terminal, so a window already closed through the normal
    /// path (its slots already empty) is a no-op here.
    fn release_open_window_callbacks(&mut self) {
        let windows = self
            .platform
            .with_state(|state| state.windows.values().cloned().collect::<Vec<_>>());
        for window in windows {
            window.mark_closed();
            window.callbacks().clear();
        }
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
        let signal = self.owner_signal.lock().clone();
        if let Some(signal) = signal {
            signal.fence();
        }

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

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
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
    fn on_wake(
        &self,
        callback: Box<dyn FnMut() + Send>,
    ) -> Result<(), crate::WakeRegistrationError> {
        let signal = self
            .platform
            .owner_signal
            .lock()
            .clone()
            .ok_or(crate::WakeRegistrationError::OwnerGone)?;
        signal.register(callback)
    }

    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        if self
            .platform
            .owner_signal
            .lock()
            .as_ref()
            .is_some_and(|signal| !signal.accepting())
        {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(options),
            });
        }

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
            signal: self
                .platform
                .owner_signal
                .lock()
                .as_ref()
                .map(Arc::downgrade)
                .unwrap_or_default(),
            platform: Arc::clone(&self.platform),
            owner_thread: self.owner_thread,
        })
    }
}

/// [`ProxyTransport`] for the winit backend: worker threads reach the owner
/// lane exactly as the pre-ADR-0039 cross-thread `Platform::open_window`
/// path already did — enqueue-and-wake, claim-slot reply.
struct WinitProxyTransport {
    signal: std::sync::Weak<crate::shared::owner_signal::OwnerSignal>,
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
    fn wake(&self) -> Result<(), ProxySendError<()>> {
        let signal = self
            .signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?;
        signal.wake()
    }

    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<PendingWindow, ProxySendError<WindowOptions>> {
        if self
            .signal
            .upgrade()
            .is_none_or(|signal| !signal.accepting())
        {
            return Err(ProxySendError::OwnerGone { rejected: options });
        }
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
        let signal = self
            .signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?;
        signal.request_quit()
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

    use super::{SelfCloseRoute, WinitApp, WinitPlatform, WinitRunState};
    use crate::{
        platforms::winit::control::control_lane,
        traits::{Platform, WindowOptions},
    };

    #[derive(Clone, Debug)]
    struct Monitor {
        uuid: u64,
        label: &'static str,
    }
    impl PartialEq for Monitor {
        fn eq(&self, other: &Self) -> bool {
            self.uuid == other.uuid
        }
    }
    fn monitor(uuid: u64, label: &'static str) -> Monitor {
        Monitor { uuid, label }
    }
    fn mapped(
        monitors: Vec<Monitor>,
        primary: Option<Monitor>,
    ) -> Vec<(u64, u64, &'static str, bool)> {
        super::map_monitor_displays(monitors.into_iter(), primary, |monitor, id, primary| {
            (id, monitor.uuid, monitor.label, primary)
        })
    }
    fn duplicate_model_labels_select_only_the_primary_uuid() {
        assert_eq!(
            mapped(
                vec![monitor(10, "Monitor #7"), monitor(20, "Monitor #7")],
                Some(monitor(20, "Monitor #7"))
            ),
            [(0, 10, "Monitor #7", false), (1, 20, "Monitor #7", true)]
        );
    }
    fn changed_label_preserves_the_primary_uuid() {
        assert_eq!(
            mapped(
                vec![monitor(10, "new label")],
                Some(monitor(10, "old label"))
            ),
            [(0, 10, "new label", true)]
        );
    }
    fn absent_primary_uses_only_the_first_display() {
        assert_eq!(
            mapped(vec![monitor(10, "first"), monitor(20, "second")], None),
            [(0, 10, "first", true), (1, 20, "second", false)]
        );
    }
    fn missing_primary_identity_does_not_select_a_substitute() {
        assert_eq!(
            mapped(vec![monitor(10, "same")], Some(monitor(30, "same"))),
            [(0, 10, "same", false)]
        );
    }
    fn empty_monitor_lists_have_no_primary_display() {
        assert_eq!(mapped(Vec::new(), None), []);
        assert_eq!(mapped(Vec::new(), Some(monitor(30, "missing"))), []);
    }
    #[test]
    fn monitor_display_mapping_matrix() {
        let mut failures = Vec::new();
        for (name, row) in [
            (
                "duplicate_model_labels_select_only_the_primary_uuid",
                duplicate_model_labels_select_only_the_primary_uuid as fn(),
            ),
            (
                "changed_label_preserves_the_primary_uuid",
                changed_label_preserves_the_primary_uuid,
            ),
            (
                "absent_primary_uses_only_the_first_display",
                absent_primary_uses_only_the_first_display,
            ),
            (
                "missing_primary_identity_does_not_select_a_substitute",
                missing_primary_identity_does_not_select_a_substitute,
            ),
            (
                "empty_monitor_lists_have_no_primary_display",
                empty_monitor_lists_have_no_primary_display,
            ),
        ] {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(row)) {
                failures.push(name);
                flui_foundation::panic::retain_opaque_payload(payload);
            }
        }
        assert!(
            failures.is_empty(),
            "failed monitor mapping cases: {failures:?}"
        );
    }

    // ========================================================================
    // combine_shutdown_result
    // ========================================================================
    //
    // Fast, deterministic, no event loop needed -- these pin the exact
    // combining logic `run_event_loop` calls, independent of whether a real
    // winit loop can also be driven to produce both error shapes at once
    // (which it cannot, deterministically, in a test).

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
}
