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
//! cross-thread [`WindowRequestQueue`] path: that queue's only consumer,
//! `about_to_wait`, is a *later* dispatch on the same thread and cannot run
//! until `resumed` (and therefore `on_ready`) returns, which would deadlock
//! forever. [`ACTIVE_EVENT_LOOP`] publishes the live `ActiveEventLoop` for
//! the exact duration of the `on_ready` call so `open_window` can create the
//! window directly instead.

use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::Arc,
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
    display::WinitDisplay,
    events as winit_events,
    window_requests::{WindowRequest, WindowRequestQueue},
};
use crate::{
    shared::PlatformHandlers,
    traits::{
        Clipboard, DesktopCapabilities, Platform, PlatformCapabilities, PlatformDisplay,
        PlatformExecutor, PlatformReadyCallback, PlatformWindow, WindowEvent, WindowId,
        WindowOptions, WinitWindow,
    },
};

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
    background_executor: Arc<SimpleExecutor>,

    /// Foreground executor
    foreground_executor: Arc<SimpleExecutor>,

    /// Clipboard
    clipboard: Arc<ArboardClipboard>,

    /// Window request queue
    window_requests: Arc<WindowRequestQueue>,

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

    /// Whether platform is running
    is_running: bool,

    /// Whether quit was requested
    should_quit: bool,

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
            background_executor: Arc::new(SimpleExecutor::new("background")),
            foreground_executor: Arc::new(SimpleExecutor::new("foreground")),
            clipboard,
            window_requests: Arc::new(WindowRequestQueue::new()),
            window_id_map: HashMap::new(),
            windows: HashMap::new(),
            displays: Vec::new(),
            active_window: None,
            next_window_id: 1,
            is_running: false,
            should_quit: false,
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

        let mut app = WinitApp {
            platform: self,
            on_ready: Some(on_ready),
        };

        event_loop.run_app(&mut app)?;

        Ok(())
    }

    /// Create a winit window immediately using a live `ActiveEventLoop` and
    /// register it in platform state. Shared by the cross-thread queued path
    /// (`WinitApp::process_window_requests`) and `open_window`'s same-thread
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

    /// Look up a previously-created window by [`WindowId`] and wrap it as a
    /// `PlatformWindow` handle for the caller. Named `window_by_id` (not
    /// `window_handle`) to avoid colliding with
    /// `PlatformWindow::window_handle` — the unrelated `raw_window_handle`
    /// accessor `WinitWindowHandle` implements below for GPU surface
    /// creation.
    fn window_by_id(&self, window_id: WindowId) -> Result<Box<dyn PlatformWindow>> {
        self.with_state(|state| {
            state
                .windows
                .get(&window_id)
                .ok_or_else(|| anyhow::anyhow!("Window not found in state"))
                .map(|win| {
                    Box::new(WinitWindowHandle { inner: win.clone() }) as Box<dyn PlatformWindow>
                })
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
        // directly instead of deadlocking on the cross-thread queue (see the
        // module-level doc).
        if let Some(on_ready) = self.on_ready.take() {
            tracing::info!("Calling on_ready callback");
            let platform = Arc::clone(&self.platform);
            with_active_event_loop(event_loop, || on_ready(&*platform));

            self.platform.with_state(|state| {
                state.is_running = true;
            });
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
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
                self.platform.with_state(|state| {
                    state
                        .handlers
                        .invoke_window_event(WindowEvent::CloseRequested {
                            window_id: platform_id,
                        });

                    // Remove window from tracking
                    state.window_id_map.retain(|_, v| *v != platform_id);
                    state.windows.remove(&platform_id);
                    state.cursor_positions.remove(&platform_id);

                    // Quit if no windows remain
                    if state.windows.is_empty() {
                        state.should_quit = true;
                    }
                });
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
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Process pending window creation requests
        self.process_window_requests(event_loop);

        // Check if we should quit
        let should_quit = self.platform.with_state(|state| state.should_quit);
        if should_quit {
            tracing::info!("Quitting event loop");
            event_loop.exit();
        }
    }
}

// Helper methods for WinitApp
impl WinitApp {
    /// Process pending window creation requests
    fn process_window_requests(&mut self, event_loop: &ActiveEventLoop) {
        let requests = self
            .platform
            .with_state(|state| state.window_requests.drain_pending());

        for request in requests {
            tracing::debug!("Processing window creation request");

            let result = self.platform.create_window_now(event_loop, request.options);

            // Send response back
            if let Err(err) = request.response.send(result) {
                tracing::error!(?err, "Failed to send window creation response");
            }
        }
    }
}

impl Platform for WinitPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.background_executor.clone())
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|state| state.foreground_executor.clone())
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) {
        tracing::info!("Starting winit event loop via Platform::run()");

        let event_loop = match EventLoop::builder().build() {
            Ok(el) => el,
            Err(e) => {
                tracing::error!("Failed to create winit event loop: {:?}", e);
                return;
            }
        };

        let platform = Arc::new(*self);
        let mut app = WinitApp {
            platform,
            on_ready: Some(on_ready),
        };

        if let Err(e) = event_loop.run_app(&mut app) {
            tracing::error!("Winit event loop error: {:?}", e);
        }
    }

    fn quit(&self) {
        tracing::info!("Quit requested");

        self.with_state(|state| {
            state.should_quit = true;
            state.handlers.invoke_quit();
        });
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!(?options, "Requesting window creation");

        // Same-thread fast path: called synchronously from inside `on_ready`
        // (see `WinitApp::resumed`), where a live `ActiveEventLoop` is
        // published for exactly this nested call. Create the window
        // directly instead of enqueuing — the queue's only consumer,
        // `about_to_wait`, cannot run until this call returns, and would
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

        // Fail fast instead of deadlocking: the cross-thread path below
        // blocks on a response that only `about_to_wait` can send, and
        // `about_to_wait` cannot run until the event loop has started
        // pumping — i.e. until `on_ready` has returned and set
        // `is_running`. Calling `open_window` before `Platform::run` (or
        // synchronously from something other than `on_ready`, which hits
        // this same window) used to hang forever; reject it instead.
        let is_running = self.with_state(|state| state.is_running);
        if !is_running {
            anyhow::bail!(
                "open_window called before Platform::run — on the winit backend, \
                 create windows inside run()'s on_ready callback"
            );
        }

        // Cross-thread path: enqueue a request and block until the event
        // loop's `about_to_wait` dispatch drains it and creates the window.
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);
        let request_sender = self.with_state(|state| state.window_requests.sender());
        let request = WindowRequest {
            options,
            response: response_tx,
        };

        request_sender
            .send(request)
            .map_err(|_| anyhow::anyhow!("Failed to send window creation request"))?;

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

// ==================== Simple Implementations ====================

/// Simple executor implementation
struct SimpleExecutor {
    name: String,
}

impl SimpleExecutor {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl PlatformExecutor for SimpleExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        tracing::debug!(executor = %self.name, "Spawning task");
        // TODO: Use actual thread pool or async runtime
        std::thread::spawn(task);
    }
}

// ==================== Window Handle (delegates to Arc<WinitWindow>) ====================

/// A handle that delegates `PlatformWindow` through `Arc<WinitWindow>`.
///
/// This exists because `open_window()` returns `Box<dyn PlatformWindow>`
/// while the platform internally stores `Arc<WinitWindow>`. The handle
/// delegates all trait methods to the shared `WinitWindow`.
struct WinitWindowHandle {
    inner: Arc<WinitWindow>,
}

impl PlatformWindow for WinitWindowHandle {
    fn physical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
        self.inner.physical_size()
    }

    fn logical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::Pixels> {
        self.inner.logical_size()
    }

    fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    fn is_focused(&self) -> bool {
        self.inner.is_focused()
    }

    fn is_visible(&self) -> bool {
        self.inner.is_visible()
    }

    fn set_title(&self, title: &str) {
        self.inner.set_title(title);
    }

    fn minimize(&self) {
        self.inner.minimize();
    }

    fn maximize(&self) {
        self.inner.maximize();
    }

    fn restore(&self) {
        self.inner.restore();
    }

    fn toggle_fullscreen(&self) {
        self.inner.toggle_fullscreen();
    }

    fn close(&self) {
        self.inner.close();
    }

    fn on_input(
        &self,
        callback: Box<
            dyn FnMut(crate::traits::PlatformInput) -> crate::traits::DispatchEventResult + Send,
        >,
    ) {
        self.inner.on_input(callback);
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        self.inner.on_request_frame(callback);
    }

    fn on_resize(
        &self,
        callback: Box<
            dyn FnMut(flui_types::geometry::Size<flui_types::geometry::Pixels>, f32) + Send,
        >,
    ) {
        self.inner.on_resize(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
        self.inner.on_moved(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        self.inner.on_close(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        self.inner.on_should_close(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.inner.on_active_status_change(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        self.inner.on_hover_status_change(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        self.inner.on_appearance_changed(callback);
    }

    // Delegate to `WinitWindow`'s own overrides — without these, GPU surface
    // creation (`Renderer::new`) falls through to the `PlatformWindow` trait
    // defaults (`Err(HandleError::Unavailable)`) even though the underlying
    // `winit::window::Window` supports both handles directly.
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.inner.window_handle()
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.inner.display_handle()
    }

    #[cfg(feature = "winit-backend")]
    fn as_winit(&self) -> Option<&Arc<winit::window::Window>> {
        self.inner.as_winit()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
