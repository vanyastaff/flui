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
//! # Vsync pacing
//!
//! [`WinitApp::about_to_wait`] pins the event loop's control flow to
//! `ControlFlow::Wait` explicitly every iteration, even though `Wait` is
//! winit's documented default when nothing sets it. This is deliberate,
//! not decorative: `flui-app`'s frame loop is wake-driven (a redraw is
//! requested only from `AppBinding::wake_frame`/`request_redraw`, never
//! polled), and steady-state pacing for a frame that DOES present comes
//! entirely from the GPU-side blocking Fifo present in `flui-engine`'s
//! `Renderer::render_scene` (see the frame-pacing ADR). If a future winit
//! release changed its own default away from `Wait` (e.g. to `Poll`), that
//! pacing model would silently regress into a busy-spin with no compile or
//! CI signal — pinning the value here turns an upstream default change
//! into a one-line diff to review instead of a surprise.

use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
    thread::{self, ThreadId},
};

use anyhow::Result;
use flui_foundation::ClaimSlot;
use keyboard_types::Modifiers as KeyboardModifiers;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent as WinitWindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{WindowAttributes, WindowId as WinitWindowId},
};

use flui_types::geometry::{Pixels, Point, px};

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
    executor::BackgroundExecutor,
    shared::PlatformHandlers,
    traits::{
        Clipboard, DesktopCapabilities, OpenWindowError, OwnerPlatform, PendingWindow, Platform,
        PlatformCapabilities, PlatformDisplay, PlatformExecutor, PlatformInput,
        PlatformReadyCallback, PlatformWindow, ProxySendError, WindowEvent, WindowId, WindowOpen,
        WindowOptions, WinitWindow,
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
/// }));
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
}

impl WinitPlatformState {
    fn new() -> Self {
        // Initialize clipboard (may fail in headless environments)
        let clipboard = ArboardClipboard::new().map_or_else(
            |err| {
                tracing::warn!(?err, "Failed to initialize clipboard, using fallback");
                Arc::new(ArboardClipboard::default())
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
    pub fn run_event_loop(self: Arc<Self>, on_ready: PlatformReadyCallback) -> Result<()> {
        tracing::info!("Creating winit event loop");

        let event_loop = EventLoop::builder().build()?;
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
        };
        if quit_requested {
            control.request_quit();
        }

        let result = event_loop.run_app(&mut app);
        app.finish_shutdown();
        result?;

        Ok(())
    }

    fn install_control_lane(&self, owner_thread: ThreadId, control: ControlSender) -> Result<bool> {
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
                    Err(anyhow::anyhow!(
                        "the winit event loop can only be started once"
                    ))
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
    ) -> Result<WindowId> {
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

        let raw_window = Arc::new(event_loop.create_window(attributes)?);
        let winit_id = raw_window.id();
        let winit_window = Arc::new(WinitWindow::new(raw_window));

        let platform_id = self.with_state(|state| {
            let id = state.allocate_window_id();
            state.register_window(winit_id, id, winit_window.clone());
            id
        });

        tracing::info!(?platform_id, "Created window");

        Ok(platform_id)
    }

    /// Look up a previously-created window by [`WindowId`] and return the
    /// exact stored allocation as a [`PlatformWindow`]. Named `window_by_id`
    /// (not `window_handle`) to avoid colliding with the unrelated
    /// [`PlatformWindow::window_handle`] raw GPU-handle accessor.
    fn window_by_id(&self, window_id: WindowId) -> Result<Arc<dyn PlatformWindow>> {
        self.with_state(|state| {
            state
                .windows
                .get(&window_id)
                .ok_or_else(|| anyhow::anyhow!("Window not found in state"))
                .map(|window| Arc::clone(window) as Arc<dyn PlatformWindow>)
        })
    }

    /// Takes the global window-event handler out from under the state lock
    /// so it can be invoked outside it (U6, ADR-0039). Before this hoist,
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
            let _ = reply.deliver(Err(OpenWindowError::Backend {
                message: "winit event-loop owner stopped before completing the window request"
                    .to_string(),
            }));
        }
    }
}

impl ApplicationHandler for WinitApp {
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
            with_active_event_loop(event_loop, || on_ready(owner));
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

                // Notify per-window close callback
                if let Some(ref win) = window {
                    win.callbacks().dispatch_close();
                }

                // Lease-take the global handler and invoke it OUTSIDE the
                // state lock (U6, ADR-0039): preserves today's observable
                // ordering (handler invocation still precedes map
                // removal/`should_exit`) while letting the handler call
                // owner-thread platform methods (e.g. a deferred
                // `OwnerPlatform::open_window`) without deadlocking on this
                // non-reentrant lock.
                let mut handler = self.platform.lease_window_event_handler();
                handler.invoke(WindowEvent::CloseRequested {
                    window_id: platform_id,
                });
                drop(handler);

                let (should_exit, transfer) = self.platform.with_state(|state| {
                    // Remove window from tracking
                    state.window_id_map.retain(|_, v| *v != platform_id);
                    state.windows.remove(&platform_id);
                    state.cursor_positions.remove(&platform_id);

                    (state.windows.is_empty(), Arc::clone(&state.data_transfer))
                });

                // Retire any drag session addressed at this window; parked
                // deliveries resolve SourceGone (ADR-0038 offer lifecycle).
                transfer.forget_window(platform_id);

                if should_exit {
                    self.request_exit(event_loop);
                }
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

                // Notify platform handler (leased outside the state lock, U6).
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

                // Notify platform handler (leased outside the state lock, U6).
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
                // handler invocation moves outside it (U6).
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
            }
            WinitWindowEvent::CursorMoved { position, .. } => {
                let modifiers = self.platform.with_state(|state| {
                    state.cursor_positions.insert(platform_id, position);
                    state.current_modifiers
                });

                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input = winit_events::cursor_moved_event(position, scale, modifiers);
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::MouseInput { state, button, .. } => {
                let (modifiers, cursor_pos) = self.platform.with_state(|s| {
                    (
                        s.current_modifiers,
                        s.cursor_positions
                            .get(&platform_id)
                            .copied()
                            .unwrap_or(winit::dpi::PhysicalPosition::new(0.0, 0.0)),
                    )
                });

                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input = winit_events::mouse_button_event(
                        button, state, cursor_pos, scale, modifiers,
                    );
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::MouseWheel { delta, .. } => {
                let (modifiers, cursor_pos) = self.platform.with_state(|s| {
                    (
                        s.current_modifiers,
                        s.cursor_positions
                            .get(&platform_id)
                            .copied()
                            .unwrap_or(winit::dpi::PhysicalPosition::new(0.0, 0.0)),
                    )
                });

                if let Some(ref win) = window {
                    let scale = win.scale_factor();
                    let input =
                        winit_events::mouse_wheel_event(delta, cursor_pos, scale, modifiers);
                    win.callbacks().dispatch_input(input);
                }
            }
            WinitWindowEvent::KeyboardInput { event, .. } => {
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

        // Explicit `Wait`, not a no-op: see the module doc's "Vsync pacing"
        // section. Re-asserted every iteration so an upstream winit default
        // change can't silently turn the wake-driven frame loop into a
        // busy poll.
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        self.process_control(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
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
            self.request_exit(event_loop);
            return;
        }

        let drain_budget = self.control.begin_drain();
        for _ in 0..drain_budget {
            if self.control.take_quit_requested() {
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
                            let _ = guard.complete(Err(OpenWindowError::Backend {
                                message: backend_error.to_string(),
                            }));
                        }
                    }
                }
            }
        }

        // Sweep entries whose requester claimed delivery and then dropped
        // its handle without ever reading it (late abandonment) — reclaim
        // and unwind those, and drop fully-settled entries (claimed, or
        // abandoned-and-reclaimed) from the registry. Runs at this
        // `user_event` drain anchor every turn, per the claim-slot module
        // docs' sweep contract.
        self.sweep_settled_replies();

        if self.control.take_quit_requested() {
            self.request_exit(event_loop);
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
            window.close();
        }
        data_transfer.forget_window(window_id);
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

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) {
        tracing::info!("Starting winit event loop via Platform::run()");
        let platform = Arc::new(*self);
        if let Err(e) = platform.run_event_loop(on_ready) {
            tracing::error!("Winit event loop error: {:?}", e);
        }
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

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn PlatformWindow>> {
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

        let control = self
            .with_state(|state| {
                state
                    .run_state
                    .control_for_open_window(thread::current().id())
            })
            .map_err(|error| match error {
                OpenWindowStateError::NotRunning => anyhow::anyhow!(
                    "open_window called before Platform::run — on the winit backend, \
                     create the first windows inside run()'s on_ready callback"
                ),
                OpenWindowStateError::Starting => anyhow::anyhow!(
                    "open_window called from another thread before winit finished on_ready"
                ),
                OpenWindowStateError::OwnerWouldBlock => anyhow::anyhow!(
                    "open_window cannot block the winit event-loop owner outside on_ready"
                ),
                OpenWindowStateError::Stopped => {
                    anyhow::anyhow!("open_window called after the winit event loop stopped")
                }
            })?;

        // The command crosses threads as data only. The owner creates the
        // window and completes this claim-slot request (ADR-0039 §3)
        // without exposing winit's thread-affine event-loop capability.
        let handle = control
            .request_open_window(options)
            .map_err(|error| match error {
                ControlSendError::Full { capacity, rejected } => {
                    drop(rejected);
                    anyhow::anyhow!("winit owner-control lane is full (capacity {capacity})")
                }
                ControlSendError::OwnerGone { rejected } => {
                    drop(rejected);
                    anyhow::anyhow!("winit event-loop owner is no longer available")
                }
            })?;

        tracing::debug!("Waiting for window creation response");

        let window = handle
            .wait()
            .map_err(|error| anyhow::anyhow!("winit window creation failed: {error}"))?;

        tracing::info!("Window created successfully");
        Ok(window)
    }

    fn active_window(&self) -> Option<WindowId> {
        self.with_state(|state| state.active_window)
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        None // Not easily supported by winit
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

    fn app_path(&self) -> Result<PathBuf> {
        std::env::current_exe().map_err(Into::into)
    }
}

/// [`OwnerHooks`] for the winit backend (ADR-0039 §1, U4/U5). Inside
/// `on_ready`, `ACTIVE_EVENT_LOOP` is published, so creation is direct and
/// synchronous; afterwards, this defers through the owner lane instead of
/// blocking or re-entering backend state — the load-bearing design choice
/// that lets a call from arbitrary owner-thread callback context (a handler
/// invoked by [`WinitApp::window_event`], now hoisted outside the state
/// lock by U6) neither deadlock nor mutate the platform window map
/// mid-frame.
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
            let window_id = self
                .platform
                .create_window_now(event_loop, options)
                .map_err(|error| OpenWindowError::Backend {
                    message: error.to_string(),
                })?;
            let window = self.platform.window_by_id(window_id).map_err(|error| {
                OpenWindowError::Backend {
                    message: error.to_string(),
                }
            })?;
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
/// path already did (U5) — enqueue-and-wake, claim-slot reply.
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

    fn request_quit(&self) {
        if let Some(control) = self.control() {
            control.request_quit();
        }
    }

    fn owner_thread(&self) -> ThreadId {
        self.owner_thread
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use flui_types::geometry::{Size, px};
    use parking_lot::Mutex;
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent as WinitWindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        window::WindowId as WinitWindowId,
    };

    use super::{
        OpenWindowReplyGuard, OpenWindowStateError, WinitApp, WinitPlatform, WinitRunState,
    };
    use crate::{
        platforms::winit::control::{ControlCommand, control_lane},
        traits::{Platform, WindowOptions},
    };

    fn options(title: impl Into<String>) -> WindowOptions {
        WindowOptions {
            title: title.into(),
            size: Size::new(px(320.0), px(240.0)),
            visible: false,
            ..WindowOptions::default()
        }
    }

    /// Polls (bounded, 2s) until the owner has reached `Running` — the point
    /// at which `resumed()` has returned and the owner lane accepts
    /// deferred, post-bootstrap requests. Real wall-clock, not simulated:
    /// these lane tests drive an actual winit event loop.
    fn wait_for_running(platform: &WinitPlatform) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !platform.with_state(|state| matches!(state.run_state, WinitRunState::Running { .. }))
        {
            assert!(
                Instant::now() < deadline,
                "owner never reached Running within 2s"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Polls (bounded, 2s) until `window_id_map.len() == expected`. Used
    /// both to detect that a deferred request actually landed (count goes
    /// up) and that an unwind completed (count goes back down) — a
    /// deterministic condition, not a blind sleep, even though the exact
    /// wake-up latency is real wall-clock time.
    fn wait_for_map_len(platform: &WinitPlatform, expected: usize, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let actual = platform.with_state(|state| state.window_id_map.len());
            if actual == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: window_id_map.len() = {actual}, expected {expected}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Builds a real `EventLoop` for a test running on an ordinary test
    /// thread, not the process's actual main thread. winit refuses this by
    /// default (a real cross-platform hazard for production code); Linux
    /// and Windows offer an explicit opt-out for exactly this situation
    /// (test harnesses, embedding). AppKit has no such opt-out — main-thread
    /// affinity there is enforced by the OS, not just winit's own guard —
    /// so these lane tests are `#[ignore]`d on macOS (see the two callers).
    ///
    /// **Only one `EventLoop` may ever exist per process** (a separate,
    /// permanent winit limitation, not the main-thread one above): a second
    /// `build()` call anywhere in the same process fails with
    /// `RecreationAttempt`, even sequentially, even after the first loop
    /// exited. Every test that calls this must run in its own process —
    /// `cargo nextest run` gives each test one by default; plain
    /// `cargo test` runs the whole binary in one process and WILL fail
    /// whichever of these tests happens to run second. Local-only per the
    /// ADR-0039 slice-2 plan either way (`flui-platform`'s suite is
    /// excluded from CI regardless).
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn build_test_event_loop() -> EventLoop<()> {
        #[cfg(target_os = "linux")]
        {
            // `EventLoopBuilderExtWayland` adds a same-named, same-effect
            // `with_any_thread` to this same shared `EventLoopBuilder` (the
            // backend, X11 or Wayland, is a runtime choice, not this
            // builder's) -- importing both traits makes the call
            // ambiguous, so this sets the flag through X11's alone; it
            // applies regardless of which backend is actually selected at
            // runtime.
            use winit::platform::x11::EventLoopBuilderExtX11;
            let mut builder = EventLoop::builder();
            EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
            builder
                .build()
                .expect("build a real winit event loop off the main thread")
        }
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            EventLoop::builder()
                .with_any_thread(true)
                .build()
                .expect("build a real winit event loop off the main thread")
        }
    }

    #[cfg(target_os = "macos")]
    fn build_test_event_loop() -> EventLoop<()> {
        EventLoop::builder()
            .build()
            .expect("build a real winit event loop (main-thread only on macOS)")
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

    /// Drives a REAL winit event loop (this sandbox has a live X11/Wayland
    /// display) to exercise the actual owner-side path: `create_window_now`
    /// needs a genuine `ActiveEventLoop`, so the drain/sweep/unwind sequence
    /// (`process_control` -> `sweep_settled_replies` -> `unwind_orphan_window`)
    /// cannot be exercised end-to-end with a hand-built `WinitApp` alone the
    /// way the lower-level claim-slot tests in `control.rs` do.
    ///
    /// `on_ready: None` — this test bypasses `OwnerPlatform` entirely and
    /// drives the lane directly through `ControlSender`, mirroring how a
    /// deferred post-bootstrap request actually reaches it (`WinitOwnerHooks`/
    /// `WinitProxyTransport` are thin wrappers over exactly this).
    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "winit requires AppKit's event loop on the real main thread; \
                  the test harness runs this on an ordinary test thread"
    )]
    fn winit_lane_dropped_after_delivery_unwinds_and_leaves_the_window_gone() {
        let platform = Arc::new(WinitPlatform::new());
        let event_loop = build_test_event_loop();
        let event_loop_proxy = event_loop.create_proxy();
        let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = event_loop_proxy.send_event(());
        });
        let (control, receiver) = control_lane(wake_owner);
        let owner_thread = thread::current().id();
        platform
            .install_control_lane(owner_thread, control.clone())
            .expect("first install succeeds");

        let platform_for_worker = Arc::clone(&platform);
        let control_for_worker = control.clone();
        let worker = thread::spawn(move || {
            // Catch a scenario panic (e.g. a bounded-poll timeout) so
            // `request_quit()` always runs below -- otherwise a failing
            // assertion leaves `run_app` parked forever waiting for a quit
            // that never comes, and the test hangs until nextest's
            // slow-test SIGKILL instead of failing fast.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                wait_for_running(&platform_for_worker);

                let before = platform_for_worker.with_state(|state| state.window_id_map.len());

                let handle = control_for_worker
                    .request_open_window(options("orphan-after-delivery"))
                    .expect("lane accepts the request");

                // Bounded poll, not a blind sleep: the request must
                // actually be created and delivered (the map grows) before
                // we abandon it -- otherwise we would only be testing the
                // already-covered dropped-*before*-delivery skip path.
                wait_for_map_len(&platform_for_worker, before + 1, "window creation");

                // Drop WITHOUT claiming: the claim-slot's
                // `Delivered -> Abandoned` transition (ADR-0039 §3) -- the
                // owner must unwind the window it already created for a
                // requester that vanished before reading it.
                drop(handle);

                wait_for_map_len(&platform_for_worker, before, "orphan-window unwind");

                let windows_len = platform_for_worker.with_state(|state| state.windows.len());
                assert_eq!(
                    windows_len, before,
                    "unwind_orphan_window must also remove the windows-map entry, \
                     not just window_id_map"
                );
            }));

            control_for_worker.request_quit();
            if let Err(payload) = outcome {
                resume_unwind(payload);
            }
        });

        let mut app = WinitApp {
            platform: Arc::clone(&platform),
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
        };
        event_loop
            .run_app(&mut app)
            .expect("event loop runs to completion");
        worker.join().expect("worker thread does not panic");
    }

    /// See the module doc on
    /// [`winit_lane_dropped_after_delivery_unwinds_and_leaves_the_window_gone`]
    /// for why this needs a real event loop. This test additionally proves
    /// the winit `window_event` entry's existing unknown-id tolerance
    /// (`tracing::warn!("Received event for unknown window"); return;`)
    /// covers a straggler that names an id this lane itself just unwound —
    /// not merely an id that was never registered at all.
    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "winit requires AppKit's event loop on the real main thread; \
                  the test harness runs this on an ordinary test thread"
    )]
    fn winit_lane_post_unwind_straggler_event_is_tolerated() {
        /// Wraps the real `WinitApp` to inject one synthetic `window_event`
        /// call carrying a stale (already-unwound) `WinitWindowId`, reusing
        /// the live `&ActiveEventLoop` the wrapping callback already has —
        /// there is no other way to obtain one outside a running loop.
        struct StragglerHarness {
            inner: WinitApp,
            stale_id: Arc<Mutex<Option<WinitWindowId>>>,
            inject_now: Arc<AtomicBool>,
            injected: AtomicBool,
        }

        impl ApplicationHandler for StragglerHarness {
            fn resumed(&mut self, event_loop: &ActiveEventLoop) {
                self.inner.resumed(event_loop);
            }

            fn window_event(
                &mut self,
                event_loop: &ActiveEventLoop,
                window_id: WinitWindowId,
                event: WinitWindowEvent,
            ) {
                self.inner.window_event(event_loop, window_id, event);
            }

            fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
                self.inner.user_event(event_loop, ());
                if self.inject_now.load(Ordering::Acquire)
                    && !self.injected.swap(true, Ordering::AcqRel)
                    && let Some(stale_id) = *self.stale_id.lock()
                {
                    // The straggler: an event for a window this app has
                    // already fully unregistered. Must not panic -- if it
                    // did, it would unwind through this real winit callback
                    // and the test would fail loudly rather than silently.
                    self.inner
                        .window_event(event_loop, stale_id, WinitWindowEvent::Focused(true));
                }
            }

            fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
                self.inner.about_to_wait(event_loop);
            }

            fn exiting(&mut self, event_loop: &ActiveEventLoop) {
                self.inner.exiting(event_loop);
            }
        }

        let platform = Arc::new(WinitPlatform::new());
        let event_loop = build_test_event_loop();
        let event_loop_proxy = event_loop.create_proxy();
        let event_loop_proxy_for_inject = event_loop.create_proxy();
        let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = event_loop_proxy.send_event(());
        });
        let (control, receiver) = control_lane(wake_owner);
        let owner_thread = thread::current().id();
        platform
            .install_control_lane(owner_thread, control.clone())
            .expect("first install succeeds");

        let stale_id: Arc<Mutex<Option<WinitWindowId>>> = Arc::new(Mutex::new(None));
        let inject_now = Arc::new(AtomicBool::new(false));

        let platform_for_worker = Arc::clone(&platform);
        let control_for_worker = control.clone();
        let stale_id_for_worker = Arc::clone(&stale_id);
        let inject_now_for_worker = Arc::clone(&inject_now);
        let worker = thread::spawn(move || {
            // See the sibling test's identical comment: `request_quit()`
            // must run even if a scenario assertion panics, or a failing
            // poll leaves `run_app` parked forever instead of failing fast.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                wait_for_running(&platform_for_worker);

                let keys_before: HashSet<_> = platform_for_worker
                    .with_state(|state| state.window_id_map.keys().copied().collect());

                let handle = control_for_worker
                    .request_open_window(options("straggler-source"))
                    .expect("lane accepts the request");

                wait_for_map_len(
                    &platform_for_worker,
                    keys_before.len() + 1,
                    "window creation",
                );

                let new_id = platform_for_worker
                    .with_state(|state| {
                        state
                            .window_id_map
                            .keys()
                            .copied()
                            .find(|id| !keys_before.contains(id))
                    })
                    .expect("a new winit id must have appeared");
                *stale_id_for_worker.lock() = Some(new_id);

                // Abandon without claiming -- same unwind path as the
                // sibling test, so by the time the map shrinks back,
                // `new_id` is a genuinely stale id: once valid, now
                // unregistered.
                drop(handle);
                wait_for_map_len(
                    &platform_for_worker,
                    keys_before.len(),
                    "orphan-window unwind",
                );

                // Arm the injection and wake the loop once more so the
                // harness's `user_event` fires the straggler `window_event`.
                inject_now_for_worker.store(true, Ordering::Release);
                let _ = event_loop_proxy_for_inject.send_event(());

                // If the injected call had panicked inside that callback,
                // the real winit loop would have unwound through it and
                // this request would never complete -- proving "no panic"
                // by the process still being alive to finish a normal
                // round trip, rather than asserting a negative directly.
                let sentinel = control_for_worker
                    .request_open_window(options("post-straggler-sentinel"))
                    .expect("lane still accepts requests after the straggler event");
                sentinel
                    .wait()
                    .expect("a normal request still completes after the straggler event");
            }));

            control_for_worker.request_quit();
            if let Err(payload) = outcome {
                resume_unwind(payload);
            }
        });

        let mut app = StragglerHarness {
            inner: WinitApp {
                platform: Arc::clone(&platform),
                on_ready: None,
                control: receiver,
                quit_notified: false,
                in_flight_replies: Vec::new(),
            },
            stale_id,
            inject_now,
            injected: AtomicBool::new(false),
        };
        event_loop
            .run_app(&mut app)
            .expect("event loop runs to completion");
        worker.join().expect("worker thread does not panic");
    }
}
