//! Web platform core implementation

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::{
    cursor::CursorStyle,
    shared::{PlatformHandlers, WindowCallbacks},
    traits::*,
};

use super::{
    clipboard::WebClipboard, display::WebDisplay, executor::WebExecutor, window::WebWindow,
};

/// Web/WASM platform implementation
pub struct WebPlatform {
    state: Arc<Mutex<WebState>>,
}

struct WebState {
    handlers: PlatformHandlers,
    foreground_executor: Arc<WebExecutor>,
    background_executor: Arc<WebExecutor>,
    clipboard: Arc<WebClipboard>,
    is_running: bool,
    /// Window callbacks for RAF loop frame dispatch.
    /// Set when `open_window` creates the single browser window.
    window_callbacks: Option<Arc<WindowCallbacks>>,
}

// SAFETY: WASM is single-threaded — no data races possible
unsafe impl Send for WebPlatform {}
unsafe impl Sync for WebPlatform {}

impl WebPlatform {
    /// Create a new Web platform instance
    pub fn new() -> Result<Self> {
        console_error_panic_hook::set_once();

        let state = WebState {
            handlers: PlatformHandlers::new(),
            foreground_executor: Arc::new(WebExecutor::new()),
            background_executor: Arc::new(WebExecutor::new()),
            clipboard: Arc::new(WebClipboard::new()),
            is_running: false,
            window_callbacks: None,
        };

        Ok(Self {
            state: Arc::new(Mutex::new(state)),
        })
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        f(&mut self.state.lock())
    }

    /// Start the requestAnimationFrame render loop
    fn start_raf_loop(&self) {
        let state = Arc::clone(&self.state);

        // Recursive RAF pattern: closure references itself via Rc<RefCell>
        let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let g = Rc::clone(&f);

        let window = web_sys::window().expect("no global window");

        *g.borrow_mut() = Some(Closure::new(move || {
            let (is_running, callbacks) = {
                let s = state.lock();
                (s.is_running, s.window_callbacks.clone())
            };
            if !is_running {
                return;
            }

            // Dispatch frame request to window callbacks
            if let Some(ref cbs) = callbacks {
                cbs.dispatch_request_frame();
            }

            // Also fire RedrawRequested through platform handlers
            {
                let mut s = state.lock();
                s.handlers
                    .invoke_window_event(WindowEvent::RedrawRequested {
                        window_id: WindowId(0),
                    });
            }

            // Request next frame after work (ensures smooth loop)
            if let Some(w) = web_sys::window() {
                let _ = w
                    .request_animation_frame(f.borrow().as_ref().unwrap().as_ref().unchecked_ref());
            }
        }));

        // Kick off the first frame
        let _ =
            window.request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }
}

impl Platform for WebPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|s| s.background_executor.clone())
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|s| s.foreground_executor.clone())
    }

    fn run(self: Box<Self>, on_ready: Box<dyn FnOnce(&dyn Platform)>) {
        tracing::info!("Starting web platform");

        self.with_state(|s| s.is_running = true);

        // Call on_ready synchronously — browser event loop is already running
        on_ready(&*self);

        // Start the RAF loop
        self.start_raf_loop();

        tracing::info!("Web platform ready");
    }

    fn quit(&self) {
        tracing::info!("Web platform quit requested");
        self.with_state(|s| {
            s.is_running = false;
            s.handlers.invoke_quit();
        });
    }

    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        tracing::info!(title = %options.title, "Creating web window (canvas)");

        let window = WebWindow::new(
            WindowId(0), // Single window in browser
            &options.title,
            options.size.width.0,
            options.size.height.0,
        )?;

        // Register DOM event listeners on the canvas
        super::events::register_event_listeners(&window);

        // Store window callbacks in state so the RAF loop can dispatch frames
        let callbacks = window.callbacks().clone();
        self.with_state(|s| {
            s.window_callbacks = Some(callbacks);
        });

        // Notify window created
        self.with_state(|s| {
            s.handlers
                .invoke_window_event(WindowEvent::Created(WindowId(0)));
        });

        Ok(Box::new(window))
    }

    fn active_window(&self) -> Option<WindowId> {
        Some(WindowId(0))
    }

    fn window_stack(&self) -> Option<Vec<WindowId>> {
        Some(vec![WindowId(0)])
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        vec![Arc::new(WebDisplay::from_browser())]
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        Some(Arc::new(WebDisplay::from_browser()))
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|s| s.clipboard.clone())
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &WebCapabilities
    }

    fn name(&self) -> &'static str {
        "Web (WASM)"
    }

    fn compositor_name(&self) -> &'static str {
        "Browser"
    }

    fn window_appearance(&self) -> WindowAppearance {
        if let Some(w) = web_sys::window() {
            if let Ok(Some(mql)) = w.match_media("(prefers-color-scheme: dark)") {
                if mql.matches() {
                    return WindowAppearance::Dark;
                }
            }
        }
        WindowAppearance::Light
    }

    fn open_url(&self, url: &str) {
        if let Some(w) = web_sys::window() {
            let _ = w.open_with_url_and_target(url, "_blank");
        }
    }

    fn keyboard_layout(&self) -> String {
        "en-US".to_string()
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.with_state(|s| s.handlers.quit = Some(callback));
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.with_state(|s| s.handlers.window_event = Some(callback));
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        let css_cursor = match style {
            CursorStyle::Arrow => "default",
            CursorStyle::IBeam => "text",
            CursorStyle::Crosshair => "crosshair",
            CursorStyle::ClosedHand => "grabbing",
            CursorStyle::OpenHand => "grab",
            CursorStyle::PointingHand => "pointer",
            CursorStyle::ResizeLeft => "w-resize",
            CursorStyle::ResizeRight => "e-resize",
            CursorStyle::ResizeLeftRight => "ew-resize",
            CursorStyle::ResizeUp => "n-resize",
            CursorStyle::ResizeDown => "s-resize",
            CursorStyle::ResizeUpDown => "ns-resize",
            CursorStyle::ResizeUpLeftDownRight => "nwse-resize",
            CursorStyle::ResizeUpRightDownLeft => "nesw-resize",
            CursorStyle::ResizeColumn => "col-resize",
            CursorStyle::ResizeRow => "row-resize",
            CursorStyle::OperationNotAllowed => "not-allowed",
            CursorStyle::DragLink => "alias",
            CursorStyle::DragCopy => "copy",
            CursorStyle::ContextualMenu => "context-menu",
            CursorStyle::None => "none",
        };

        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            if let Some(body) = document.body() {
                let _ = body.style().set_property("cursor", css_cursor);
            }
        }
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        // web_sys::Window::location() returns Location, not Result
        // Location::origin() returns Result<String, JsValue>
        if let Some(w) = web_sys::window() {
            if let Ok(origin) = w.location().href() {
                return Ok(std::path::PathBuf::from(origin));
            }
        }
        Ok(std::path::PathBuf::from("/"))
    }
}
