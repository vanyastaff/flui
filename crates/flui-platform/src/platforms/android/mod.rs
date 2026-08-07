//! Android platform implementation
//!
//! Native Android platform using `android-activity` crate for NativeActivity
//! integration. Provides window management, event handling, and lifecycle
//! support via ANativeWindow + Vulkan.
//!
//! # Architecture
//!
//! ```text
//! android_main(AndroidApp)
//!   -> AndroidPlatform::new(app)
//!   -> Platform::run()  [poll_events loop]
//!     -> MainEvent::Resumed  -> on_ready(), create surface
//!     -> MainEvent::Paused   -> surface becomes invalid
//!     -> MainEvent::Destroy  -> break loop
//!     -> each tick            -> dispatch_request_frame()
//! ```
//!
//! # Surface Lifecycle
//!
//! On Android, the native window (ANativeWindow) is only valid between
//! `Resumed` and `Paused` events. The wgpu surface must be created on Resume
//! and dropped on Pause.

pub mod input;
pub mod memory;
pub mod window;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use anyhow::Result;
pub use memory::{
    PageAlignedVec, PageAllocError, align_to_page_size, align_to_page_size_u64, get_page_size,
    is_16kb_page_size,
};
use parking_lot::Mutex;
pub use window::AndroidWindow;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
    shared::PlatformHandlers,
    traits::{
        Clipboard, DisplayId, MobileCapabilities, OwnerPlatform, Platform, PlatformCapabilities,
        PlatformDisplay, PlatformExecutor, PlatformReadyCallback, PlatformWindow, WindowEvent,
        WindowId, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks},
    },
};

/// Android platform implementation using `android-activity`
///
/// Wraps the `AndroidApp` provided by `android_main()` and implements the
/// `Platform` trait for integration with the FLUI framework.
///
/// # Usage
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     let platform = AndroidPlatform::new(app);
///     let _ = platform.run(Box::new(|owner| {
///         // Platform ready (first Resume) — create window and renderer
///         let _ = owner;
///         Ok(())
///     }));
/// }
/// ```
#[derive(Debug)]
pub struct AndroidPlatform {
    app: AndroidApp,
    handlers: Arc<Mutex<PlatformHandlers>>,
    running: Arc<AtomicBool>,
    window: Arc<Mutex<Option<Arc<AndroidWindow>>>>,
    background_executor: Arc<SimpleExecutor>,
    clipboard: Arc<MockClipboard>,
    capabilities: MobileCapabilities,
}

impl AndroidPlatform {
    /// Create a new Android platform from the `AndroidApp` provided by
    /// `android_main()`
    pub fn new(app: AndroidApp) -> Self {
        Self {
            app,
            handlers: Arc::new(Mutex::new(PlatformHandlers::new())),
            running: Arc::new(AtomicBool::new(true)),
            window: Arc::new(Mutex::new(None)),
            background_executor: Arc::new(SimpleExecutor),
            clipboard: Arc::new(MockClipboard::new()),
            capabilities: MobileCapabilities::android(),
        }
    }

    /// Get the underlying `AndroidApp`
    pub fn app(&self) -> &AndroidApp {
        &self.app
    }

    /// Process pending input events from the Android event queue.
    ///
    /// Drains all buffered input events via `input_events_iter()` and
    /// dispatches them through the window's callbacks as `PlatformInput`.
    fn process_input_events(&self) {
        let window_guard = self.window.lock();
        let Some(window) = window_guard.as_ref() else {
            // No window yet — still drain events to prevent ANR
            drop(window_guard);
            if let Ok(mut iter) = self.app.input_events_iter() {
                while iter.next(|_event| InputStatus::Unhandled) {}
            }
            return;
        };

        let scale_factor = window.scale_factor();
        let callbacks = window.callbacks();

        match self.app.input_events_iter() {
            Ok(mut iter) => loop {
                let read = iter.next(|event| {
                    use android_activity::input::InputEvent;

                    let handled = match event {
                        InputEvent::MotionEvent(motion) => {
                            let events = input::convert_motion_event(motion, scale_factor);
                            let mut any_handled = false;
                            for platform_input in events {
                                let result = callbacks.dispatch_input(platform_input);
                                if result.default_prevented {
                                    any_handled = true;
                                }
                            }
                            // Request redraw on touch input
                            if any_handled {
                                window.request_redraw();
                            }
                            any_handled
                        }
                        InputEvent::KeyEvent(key) => {
                            if let Some(platform_input) = input::convert_key_event(key) {
                                let result = callbacks.dispatch_input(platform_input);
                                result.default_prevented
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };

                    if handled {
                        InputStatus::Handled
                    } else {
                        InputStatus::Unhandled
                    }
                });

                if !read {
                    break;
                }
            },
            Err(e) => {
                tracing::error!("Failed to get input events: {:?}", e);
            }
        }
    }
}

impl Platform for AndroidPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.background_executor.clone()
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> anyhow::Result<()> {
        tracing::info!("Starting Android platform event loop");

        // Converted once, up front: `on_ready` needs an `Arc<dyn Platform>`
        // to mint `OwnerPlatform` from (ADR-0039 slice 2), and the rest of
        // this loop reads through the same `Arc` for its whole lifetime
        // instead of the original `Box`.
        let platform = Arc::new(*self);

        let mut on_ready = Some(on_ready);
        let mut resumed = false;
        // Set when `on_ready` returns `Err`: a fallible bootstrap
        // failure (window creation, GPU init, root-widget attach) has no
        // other return path back to `run`'s caller. Stopping `running` and
        // `continue`-ing to the loop's top-of-iteration check exits
        // promptly instead of continuing to pump input/frame dispatch for
        // an app that never finished bootstrapping.
        let mut bootstrap_error: Option<anyhow::Error> = None;
        // Relaxed everywhere on `running`: it is a bare stop-signal with no
        // data published through it. The loop re-reads it every iteration,
        // the Destroy store happens on the loop thread itself, and a
        // cross-thread `quit()` only needs eventual visibility.
        platform.running.store(true, Ordering::Relaxed);

        loop {
            if !platform.running.load(Ordering::Relaxed) {
                break;
            }

            // Check if we should render before polling
            let should_render = platform
                .window
                .lock()
                .as_ref()
                .is_some_and(|w| w.take_redraw_request());

            let timeout = if should_render {
                Duration::from_millis(0)
            } else {
                Duration::from_millis(16)
            };

            let mut should_call_ready = false;

            platform.app.poll_events(Some(timeout), |event| {
                if let PollEvent::Main(main_event) = event {
                    match main_event {
                        MainEvent::Resume { .. } => {
                            tracing::info!("Android: Resumed — native window available");
                            resumed = true;

                            if on_ready.is_some() {
                                should_call_ready = true;
                            }

                            // Notify window of activation
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_active_status_change(true);
                                w.request_redraw();
                            }
                        }
                        MainEvent::Pause => {
                            tracing::info!("Android: Paused — native window may become invalid");
                            resumed = false;

                            // Notify window of deactivation
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_active_status_change(false);
                            }
                        }
                        MainEvent::Destroy => {
                            tracing::info!("Android: Destroy — shutting down");

                            // Dispatch close before stopping
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_close();
                            }

                            platform.running.store(false, Ordering::Relaxed);
                        }
                        MainEvent::WindowResized { .. } => {
                            tracing::info!("Android: Window resized");
                            if let Some(ref w) = *platform.window.lock() {
                                let size = w.logical_size();
                                let scale = w.scale_factor() as f32;
                                w.callbacks().dispatch_resize(size, scale);
                                w.request_redraw();
                            }
                        }
                        MainEvent::GainedFocus => {
                            tracing::debug!("Android: Gained focus");
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_active_status_change(true);
                            }
                        }
                        MainEvent::LostFocus => {
                            tracing::debug!("Android: Lost focus");
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_active_status_change(false);
                            }
                        }
                        MainEvent::ConfigChanged { .. } => {
                            tracing::debug!("Android: Config changed");
                        }
                        MainEvent::LowMemory => {
                            tracing::warn!("Android: Low memory warning");
                        }
                        _ => {}
                    }
                }
            });

            // Call on_ready outside of poll_events (FnOnce can't be called in
            // closure). Fires once, at the first `Resume` — the module doc's
            // `Resumed -> on_ready() -> create surface` sequence (ADR-0039
            // slice 2: the pre-run bootstrap that used to run before this
            // loop started now runs from here, in `flui-app`'s `on_ready`
            // migration). No owner lane on this backend: every
            // `OwnerPlatform::open_window` call creates directly and is
            // always `Ready`.
            if should_call_ready && let Some(ready) = on_ready.take() {
                let owner_platform = Arc::clone(&platform) as Arc<dyn Platform>;
                let hooks: Arc<dyn OwnerHooks> =
                    Arc::new(DirectOwnerHooks::new(Arc::clone(&owner_platform)));
                if let Err(error) = ready(OwnerPlatform::new(owner_platform, hooks)) {
                    tracing::error!(%error, "on_ready bootstrap failed; stopping the event loop");
                    bootstrap_error = Some(error);
                    platform.running.store(false, Ordering::Relaxed);
                    // Skip input/frame dispatch below for this iteration --
                    // the top-of-loop check exits on the next pass.
                    continue;
                }
            }

            // Process input events (touch, key) and dispatch through callbacks
            if resumed {
                platform.process_input_events();
            }

            // Dispatch frame rendering if resumed and redraw was requested
            if resumed
                && should_render
                && let Some(ref w) = *platform.window.lock()
            {
                w.callbacks().dispatch_request_frame();
            }
        }

        // Invoke quit handlers
        platform.handlers.lock().invoke_quit();
        tracing::info!("Android platform event loop finished");

        if let Some(error) = bootstrap_error {
            return Err(error);
        }
        Ok(())
    }

    fn quit(&self) {
        tracing::info!("Android: quit requested");
        self.running.store(false, Ordering::Relaxed);
    }

    fn open_window(&self, _options: WindowOptions) -> Result<Arc<dyn PlatformWindow>> {
        let window = Arc::new(AndroidWindow::new(self.app.clone()));
        *self.window.lock() = Some(Arc::clone(&window));
        tracing::info!("Android window created (wrapping ANativeWindow)");
        Ok(window)
    }

    fn active_window(&self) -> Option<WindowId> {
        self.window.lock().as_ref().map(|_| WindowId(0))
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        vec![Arc::new(AndroidDisplay)]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(AndroidDisplay))
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.clipboard.clone()
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // No Android transport yet (ADR-0038): inert and honest.
        Arc::new(NullDataTransferSource)
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        "Android"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.handlers.lock().quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.handlers.lock().window_event = Some(callback);
    }

    fn app_path(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/data/local/tmp"))
    }
}

// ==================== Mock implementations (MVP) ====================

/// Simple executor that runs tasks on the current thread
#[derive(Debug)]
struct SimpleExecutor;

impl PlatformExecutor for SimpleExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        task();
    }
}

/// Mock clipboard for Android MVP
#[derive(Debug)]
struct MockClipboard {
    content: Mutex<Option<String>>,
}

impl MockClipboard {
    fn new() -> Self {
        Self {
            content: Mutex::new(None),
        }
    }
}

impl Clipboard for MockClipboard {
    fn read_text(&self) -> Option<String> {
        self.content.lock().clone()
    }

    fn write_text(&self, text: String) {
        *self.content.lock() = Some(text);
    }
}

/// Android display info (MVP — returns reasonable defaults)
struct AndroidDisplay;

impl PlatformDisplay for AndroidDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(0)
    }

    fn name(&self) -> String {
        "Android Display".to_string()
    }

    fn bounds(&self) -> flui_types::geometry::Bounds<flui_types::geometry::DevicePixels> {
        use flui_types::geometry::{Bounds, Point, Size, device_px};
        Bounds::new(
            Point::new(device_px(0), device_px(0)),
            Size::new(device_px(1080), device_px(2340)),
        )
    }

    fn scale_factor(&self) -> f64 {
        2.75
    }

    fn is_primary(&self) -> bool {
        true
    }
}
