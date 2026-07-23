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
    sync::{Arc, mpsc::SyncSender},
    thread::{self, ThreadId},
};

use anyhow::Result;
use keyboard_types::Modifiers as KeyboardModifiers;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent as WinitWindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{WindowAttributes, WindowId as WinitWindowId},
};

use super::{
    clipboard::ArboardClipboard,
    control::{ControlCommand, ControlReceiver, ControlSendError, ControlSender, control_lane},
    display::WinitDisplay,
    events as winit_events,
};
use crate::{
    executor::BackgroundExecutor,
    shared::PlatformHandlers,
    traits::{
        Clipboard, DesktopCapabilities, Platform, PlatformCapabilities, PlatformDisplay,
        PlatformExecutor, PlatformReadyCallback, PlatformWindow, WindowEvent, WindowId,
        WindowOptions, WinitWindow,
    },
};

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
/// platform.run(Box::new(|platform| {
///     // `open_window` is safe to call here — see the module-level doc on
///     // same-thread window creation.
///     let _window = platform.open_window(Default::default());
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
}

/// Completes a dequeued window request even if owner-side processing unwinds.
struct OpenWindowReplyGuard {
    response: Option<SyncSender<anyhow::Result<WindowId>>>,
}

impl OpenWindowReplyGuard {
    fn new(response: SyncSender<anyhow::Result<WindowId>>) -> Self {
        Self {
            response: Some(response),
        }
    }

    fn complete(mut self, result: anyhow::Result<WindowId>) -> bool {
        self.response
            .take()
            .is_some_and(|response| response.send(result).is_ok())
    }
}

impl Drop for OpenWindowReplyGuard {
    fn drop(&mut self) {
        if let Some(response) = self.response.take() {
            let _ = response.send(Err(anyhow::anyhow!(
                "winit event-loop owner stopped before completing the window request"
            )));
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
        // module-level doc).
        if let Some(on_ready) = self.on_ready.take() {
            tracing::info!("Calling on_ready callback");
            let platform = Arc::clone(&self.platform);
            with_active_event_loop(event_loop, || on_ready(&*platform));
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

                // Notify platform handler
                let should_exit = self.platform.with_state(|state| {
                    state
                        .handlers
                        .invoke_window_event(WindowEvent::CloseRequested {
                            window_id: platform_id,
                        });

                    // Remove window from tracking
                    state.window_id_map.retain(|_, v| *v != platform_id);
                    state.windows.remove(&platform_id);
                    state.cursor_positions.remove(&platform_id);

                    state.windows.is_empty()
                });

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

                // Notify platform handler
                self.platform.with_state(|state| {
                    state.handlers.invoke_window_event(WindowEvent::Resized {
                        window_id: platform_id,
                        size,
                    });
                });
            }
            WinitWindowEvent::RedrawRequested => {
                tracing::trace!(?platform_id, "Redraw requested");

                // Dispatch per-window frame request callback
                if let Some(ref win) = window {
                    win.callbacks().dispatch_request_frame();
                }

                // Notify platform handler
                self.platform.with_state(|state| {
                    state
                        .handlers
                        .invoke_window_event(WindowEvent::RedrawRequested {
                            window_id: platform_id,
                        });
                });
            }
            WinitWindowEvent::Focused(focused) => {
                tracing::debug!(?platform_id, ?focused, "Window focus changed");

                // Update WinitWindow focus state
                if let Some(ref win) = window {
                    win.set_focused(focused);
                    win.callbacks().dispatch_active_status_change(focused);
                }

                // Update active window in platform state
                self.platform.with_state(|state| {
                    if focused {
                        state.active_window = Some(platform_id);
                    } else if state.active_window == Some(platform_id) {
                        state.active_window = None;
                    }

                    state
                        .handlers
                        .invoke_window_event(WindowEvent::FocusChanged {
                            window_id: platform_id,
                            focused,
                        });
                });
            }
            WinitWindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::debug!(?platform_id, ?scale_factor, "Scale factor changed");

                self.platform.with_state(|state| {
                    state
                        .handlers
                        .invoke_window_event(WindowEvent::ScaleFactorChanged {
                            window_id: platform_id,
                            scale_factor,
                        });
                });
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
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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

// Helper methods for WinitApp
impl WinitApp {
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
                ControlCommand::OpenWindow { options, response } => {
                    let reply = OpenWindowReplyGuard::new(response);
                    tracing::debug!("Processing window creation request");
                    let result = self.platform.create_window_now(event_loop, options);
                    if !reply.complete(result) {
                        tracing::debug!("window requester was dropped before the response");
                    }
                }
            }
        }

        if self.control.take_quit_requested() {
            self.request_exit(event_loop);
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
                ControlCommand::OpenWindow { response, .. } => {
                    let _ = OpenWindowReplyGuard::new(response).complete(Err(anyhow::anyhow!(
                        "winit event-loop owner is shutting down"
                    )));
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
        // window and completes this one-shot without exposing winit's
        // thread-affine event-loop capability.
        let response_rx = control
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

        let window_id = response_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Failed to receive window creation response"))??;

        tracing::info!(?window_id, "Window created successfully");
        self.window_by_id(window_id)
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
        tracing::info!(?path, "Revealing path");

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
        tracing::info!(?path, "Opening path");

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
        OpenWindowReplyGuard, OpenWindowStateError, WinitApp, WinitPlatform, WinitRunState,
    };
    use crate::{
        platforms::winit::control::{ControlCommand, control_lane},
        traits::{Platform, WindowOptions},
    };

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
        };

        app.notify_quit_once();
        app.notify_quit_once();

        assert_eq!(callback_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn winit_shutdown_replies_to_admitted_requests_before_app_drop() {
        let platform = Arc::new(WinitPlatform::new());
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        let replies: Vec<_> = (0..3)
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
        };

        app.finish_shutdown();

        assert_eq!(app.control.pending_count(), 0);
        for reply in replies {
            let result = reply
                .try_recv()
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
        let replies: Vec<_> = (0..3)
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
            };
            panic!("exercise WinitApp unwind cleanup");
        }));
        assert!(unwind.is_err());

        for reply in replies {
            assert!(
                reply
                    .try_recv()
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
        let replies: Vec<_> = (0..3)
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
        };

        let first_finish = catch_unwind(AssertUnwindSafe(|| app.finish_shutdown()));
        assert!(first_finish.is_err(), "user callback panic must propagate");
        for reply in replies {
            assert!(
                reply
                    .try_recv()
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
        let reply = sender
            .request_open_window(WindowOptions::default())
            .expect("request is admitted");
        assert_eq!(receiver.begin_drain(), 1);
        let ControlCommand::OpenWindow { response, .. } =
            receiver.try_recv().expect("owner dequeues request");

        let unwind = catch_unwind(AssertUnwindSafe(move || {
            let _reply_guard = OpenWindowReplyGuard::new(response);
            panic!("exercise in-flight reply unwind");
        }));
        assert!(unwind.is_err());
        assert!(
            reply
                .try_recv()
                .expect("reply guard returns explicit error instead of disconnect")
                .is_err()
        );
    }
}
