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

use crate::{shared::PlatformHandlers, traits::*};

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
///     platform.run(Box::new(|| {
///         // Platform ready — create window and renderer
///     }));
/// }
/// ```
pub struct AndroidPlatform {
    app: AndroidApp,
    handlers: Arc<Mutex<PlatformHandlers>>,
    running: Arc<AtomicBool>,
    window: Arc<Mutex<Option<Arc<AndroidWindow>>>>,
    background_executor: Arc<SimpleExecutor>,
    text_system: Arc<MockTextSystem>,
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
            text_system: Arc::new(MockTextSystem),
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
        let window = match window_guard.as_ref() {
            Some(w) => w,
            None => {
                // No window yet — still drain events to prevent ANR
                drop(window_guard);
                if let Ok(mut iter) = self.app.input_events_iter() {
                    while iter.next(|_event| InputStatus::Unhandled) {}
                }
                return;
            }
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

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.background_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    fn run(&self, on_ready: Box<dyn FnOnce()>) {
        tracing::info!("Starting Android platform event loop");

        let mut on_ready = Some(on_ready);
        let mut resumed = false;
        self.running.store(true, Ordering::SeqCst);

        loop {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }

            // Check if we should render before polling
            let should_render = self
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

            self.app.poll_events(Some(timeout), |event| {
                match event {
                    PollEvent::Main(main_event) => match main_event {
                        MainEvent::Resume { .. } => {
                            tracing::info!("Android: Resumed — native window available");
                            resumed = true;

                            if on_ready.is_some() {
                                should_call_ready = true;
                            }

                            // Notify window of activation
                            if let Some(ref w) = *self.window.lock() {
                                w.callbacks().dispatch_active_status_change(true);
                                w.request_redraw();
                            }
                        }
                        MainEvent::Pause => {
                            tracing::info!("Android: Paused — native window may become invalid");
                            resumed = false;

                            // Notify window of deactivation
                            if let Some(ref w) = *self.window.lock() {
                                w.callbacks().dispatch_active_status_change(false);
                            }
                        }
                        MainEvent::Destroy => {
                            tracing::info!("Android: Destroy — shutting down");

                            // Dispatch close before stopping
                            if let Some(ref w) = *self.window.lock() {
                                w.callbacks().dispatch_close();
                            }

                            self.running.store(false, Ordering::SeqCst);
                        }
                        MainEvent::WindowResized { .. } => {
                            tracing::info!("Android: Window resized");
                            if let Some(ref w) = *self.window.lock() {
                                let size = w.logical_size();
                                let scale = w.scale_factor() as f32;
                                w.callbacks().dispatch_resize(size, scale);
                                w.request_redraw();
                            }
                        }
                        MainEvent::GainedFocus => {
                            tracing::debug!("Android: Gained focus");
                            if let Some(ref w) = *self.window.lock() {
                                w.callbacks().dispatch_active_status_change(true);
                            }
                        }
                        MainEvent::LostFocus => {
                            tracing::debug!("Android: Lost focus");
                            if let Some(ref w) = *self.window.lock() {
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
                    },
                    _ => {}
                }
            });

            // Call on_ready outside of poll_events (FnOnce can't be called in closure)
            if should_call_ready {
                if let Some(ready) = on_ready.take() {
                    ready();
                }
            }

            // Process input events (touch, key) and dispatch through callbacks
            if resumed {
                self.process_input_events();
            }

            // Dispatch frame rendering if resumed and redraw was requested
            if resumed && should_render {
                if let Some(ref w) = *self.window.lock() {
                    w.callbacks().dispatch_request_frame();
                }
            }
        }

        // Invoke quit handlers
        self.handlers.lock().invoke_quit();
        tracing::info!("Android platform event loop finished");
    }

    fn quit(&self) {
        tracing::info!("Android: quit requested");
        self.running.store(false, Ordering::SeqCst);
    }

    fn request_frame(&self) {
        if let Some(ref w) = *self.window.lock() {
            w.request_redraw();
        }
    }

    fn open_window(&self, _options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        let window = Arc::new(AndroidWindow::new(self.app.clone()));
        *self.window.lock() = Some(Arc::clone(&window));
        tracing::info!("Android window created (wrapping ANativeWindow)");
        Ok(Box::new(window.as_ref().clone()))
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
struct SimpleExecutor;

impl PlatformExecutor for SimpleExecutor {
    fn spawn(&self, task: Box<dyn FnOnce() + Send>) {
        task();
    }
}

/// Mock text system for Android MVP
struct MockTextSystem;

impl PlatformTextSystem for MockTextSystem {
    fn add_fonts(&self, _fonts: Vec<std::borrow::Cow<'static, [u8]>>) -> Result<()> {
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        vec!["Roboto".to_string()]
    }

    fn font_id(&self, _descriptor: &Font) -> Result<FontId> {
        Ok(FontId(0))
    }

    fn font_metrics(&self, _font_id: FontId) -> FontMetrics {
        FontMetrics {
            units_per_em: 1000,
            ascent: 800.0,
            descent: 200.0,
            line_gap: 0.0,
            underline_position: -100.0,
            underline_thickness: 50.0,
            cap_height: 700.0,
            x_height: 500.0,
        }
    }

    fn glyph_for_char(&self, _font_id: FontId, ch: char) -> Option<GlyphId> {
        Some(GlyphId(ch as u32))
    }

    fn layout_line(&self, text: &str, font_size: f32, _runs: &[FontRun]) -> LineLayout {
        let char_count = text.chars().count() as f32;
        LineLayout {
            font_size,
            width: char_count * font_size * 0.6,
            ascent: font_size * 0.8,
            descent: font_size * 0.2,
            runs: Vec::new(),
            len: text.len(),
        }
    }
}

/// Mock clipboard for Android MVP
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
