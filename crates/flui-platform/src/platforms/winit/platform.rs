//! Winit-based platform implementation
//!
//! Cross-platform implementation using winit for window management.
//! This implementation covers Windows, macOS, and Linux desktop platforms.
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

use super::clipboard::ArboardClipboard;
use super::display::WinitDisplay;
use super::window_requests::{WindowRequest, WindowRequestQueue};
use crate::shared::PlatformHandlers;
use crate::traits::{
    Clipboard, DesktopCapabilities, Platform, PlatformCapabilities, PlatformDisplay,
    PlatformExecutor, PlatformTextSystem, PlatformWindow, WindowEvent, WindowId, WindowOptions,
    WinitWindow,
};

use anyhow::Result;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent as WinitWindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId as WinitWindowId};

/// Winit-based platform implementation
///
/// This is the primary cross-platform implementation using winit for window management.
/// It supports Windows, macOS, and Linux desktop environments.
///
/// # Usage
///
/// ```rust,ignore
/// let platform = WinitPlatform::new();
/// platform.run(Box::new(|| {
///     println!("Platform ready!");
/// }));
/// ```
pub struct WinitPlatform {
    state: Arc<Mutex<WinitPlatformState>>,
}

/// Internal state for WinitPlatform
struct WinitPlatformState {
    /// Platform capabilities
    capabilities: DesktopCapabilities,

    /// Callback handlers
    handlers: PlatformHandlers,

    /// Background executor
    background_executor: Arc<SimpleExecutor>,

    /// Foreground executor
    foreground_executor: Arc<SimpleExecutor>,

    /// Text system
    text_system: Arc<SimpleTextSystem>,

    /// Clipboard
    clipboard: Arc<ArboardClipboard>,

    /// Window request queue
    window_requests: Arc<WindowRequestQueue>,

    /// Map of winit window IDs to platform window IDs
    window_id_map: HashMap<WinitWindowId, WindowId>,

    /// Map of platform window IDs to Arc<Window> for window creation
    windows: HashMap<WindowId, Arc<winit::window::Window>>,

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
}

impl WinitPlatformState {
    fn new() -> Self {
        // Initialize clipboard (may fail in headless environments)
        let clipboard = ArboardClipboard::new().map(Arc::new).unwrap_or_else(|err| {
            tracing::warn!(?err, "Failed to initialize clipboard, using fallback");
            Arc::new(ArboardClipboard::default())
        });

        Self {
            capabilities: DesktopCapabilities,
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(SimpleExecutor::new("background")),
            foreground_executor: Arc::new(SimpleExecutor::new("foreground")),
            text_system: Arc::new(SimpleTextSystem::new()),
            clipboard,
            window_requests: Arc::new(WindowRequestQueue::new()),
            window_id_map: HashMap::new(),
            windows: HashMap::new(),
            displays: Vec::new(),
            active_window: None,
            next_window_id: 1,
            is_running: false,
            should_quit: false,
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
        window: Arc<winit::window::Window>,
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
                    .map(|pm| pm.name() == monitor.name())
                    .unwrap_or(idx == 0);

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
    /// This is the main entry point for the platform. It creates the event loop,
    /// initializes the platform, and runs until quit is requested.
    pub fn run_event_loop(self: Arc<Self>, on_ready: Box<dyn FnOnce()>) -> Result<()> {
        tracing::info!("Creating winit event loop");

        let event_loop = EventLoop::builder().build()?;

        let mut app = WinitApp {
            platform: self,
            on_ready: Some(on_ready),
        };

        event_loop.run_app(&mut app)?;

        Ok(())
    }
}

/// Application handler for winit event loop
///
/// Implements `ApplicationHandler` to receive events from winit without consuming the event loop.
struct WinitApp {
    platform: Arc<WinitPlatform>,
    on_ready: Option<Box<dyn FnOnce()>>,
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

        // Call on_ready callback once
        if let Some(on_ready) = self.on_ready.take() {
            tracing::info!("Calling on_ready callback");
            on_ready();

            self.platform.with_state(|state| {
                state.is_running = true;
            });
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WinitWindowId,
        event: WinitWindowEvent,
    ) {
        let platform_id = self
            .platform
            .with_state(|state| state.get_platform_window_id(window_id));

        let Some(platform_id) = platform_id else {
            tracing::warn!("Received event for unknown window");
            return;
        };

        match event {
            WinitWindowEvent::CloseRequested => {
                tracing::info!(?platform_id, "Window close requested");

                // Notify handler
                self.platform.with_state(|state| {
                    state
                        .handlers
                        .invoke_window_event(WindowEvent::CloseRequested {
                            window_id: platform_id,
                        });
                    state.should_quit = true;
                });

                event_loop.exit();
            }
            WinitWindowEvent::Resized(physical_size) => {
                use flui_types::geometry::{device_px, Size};

                let size = Size::new(
                    device_px(physical_size.width as i32),
                    device_px(physical_size.height as i32),
                );

                tracing::debug!(?platform_id, ?size, "Window resized");

                // Notify handler
                self.platform.with_state(|state| {
                    state.handlers.invoke_window_event(WindowEvent::Resized {
                        window_id: platform_id,
                        size,
                    });
                });
            }
            WinitWindowEvent::RedrawRequested => {
                tracing::trace!(?platform_id, "Redraw requested");

                // Notify handler
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

                // Update active window
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

            let result = self.create_window(event_loop, request.options);

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

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.with_state(|state| state.text_system.clone())
    }

    fn run(&self, _on_ready: Box<dyn FnOnce()>) {
        // This method can't actually run the event loop because it takes &self
        // Users should call run_event_loop() instead
        panic!("Use WinitPlatform::run_event_loop() instead of Platform::run()");
    }

    fn quit(&self) {
        tracing::info!("Quit requested");

        self.with_state(|state| {
            state.should_quit = true;
            state.handlers.invoke_quit();
        });
    }

    fn request_frame(&self) {
        // Request redraw on all windows
        self.with_state(|state| {
            for window in state.windows.values() {
                window.request_redraw();
            }
        });
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!(?options, "Requesting window creation");

        // Create a oneshot channel for the response
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);

        // Get the window request sender
        let request_sender = self.with_state(|state| state.window_requests.sender());

        // Send the window creation request
        let request = WindowRequest {
            options,
            response: response_tx,
        };

        request_sender
            .send(request)
            .map_err(|_| anyhow::anyhow!("Failed to send window creation request"))?;

        tracing::debug!("Waiting for window creation response");

        // Wait for the response (this will block until the event loop processes the request)
        let window_id = response_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Failed to receive window creation response"))??;

        tracing::info!(?window_id, "Window created successfully");

        // Get the window from state and create WinitWindow wrapper
        self.with_state(|state| {
            state
                .windows
                .get(&window_id)
                .ok_or_else(|| anyhow::anyhow!("Window not found in state"))
                .map(|arc_window| {
                    Box::new(WinitWindow::new(arc_window.clone())) as Box<dyn PlatformWindow>
                })
        })
    }

    fn active_window(&self) -> Option<WindowId> {
        self.with_state(|state| state.active_window)
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        None // Not easily supported by winit
    }

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
        // SAFETY: capabilities field is immutable after initialization
        // and lives as long as the platform
        unsafe { &*(&self.with_state(|state| state.capabilities) as *const _) }
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

// ==================== Helper implementations ====================

impl WinitApp {
    /// Create a window within the event loop
    #[allow(dead_code)]
    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        options: WindowOptions,
    ) -> Result<WindowId> {
        let attributes = WindowAttributes::default()
            .with_title(options.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                options.size.width.0,
                options.size.height.0,
            ));

        let window = Arc::new(event_loop.create_window(attributes)?);
        let winit_id = window.id();

        let platform_id = self.platform.with_state(|state| {
            let id = state.allocate_window_id();
            state.register_window(winit_id, id, window.clone());
            id
        });

        tracing::info!(?platform_id, "Created window");

        Ok(platform_id)
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

/// Simple text system implementation
struct SimpleTextSystem;

impl SimpleTextSystem {
    fn new() -> Self {
        Self
    }
}

impl PlatformTextSystem for SimpleTextSystem {
    fn default_font_family(&self) -> String {
        #[cfg(target_os = "windows")]
        return "Segoe UI".to_string();

        #[cfg(target_os = "macos")]
        return "SF Pro Text".to_string();

        #[cfg(target_os = "linux")]
        return "Ubuntu".to_string();

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return "sans-serif".to_string();
    }
}
