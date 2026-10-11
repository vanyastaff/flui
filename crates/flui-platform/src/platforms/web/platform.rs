//! Web platform core implementation

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::{Arc, Weak, atomic::Ordering};

use parking_lot::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
    error::PlatformError,
    shared::{PlatformHandlers, WindowCallbacks, owner_signal::OwnerSignal},
    traits::{
        Clipboard, HostWindow, OpenWindowError, OwnerPlatform, Platform, PlatformCapabilities,
        PlatformDisplay, PlatformExecutor, PlatformReadyCallback, WebCapabilities,
        WindowAppearance, WindowEvent, WindowId, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks},
    },
};

use super::{
    clipboard::WebClipboard, display::WebDisplay, executor::WebExecutor, window::WebWindow,
};

/// The self-referencing `requestAnimationFrame` callback slot: the closure
/// re-schedules itself through this handle, so it is created empty and filled
/// once the closure exists.
type RafClosureSlot = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

/// Web/WASM platform implementation
pub struct WebPlatform {
    state: Arc<Mutex<WebState>>,
    signal: Arc<OwnerSignal>,
    preferences: RefCell<Option<super::preferences::Preferences>>,
}

struct WebState {
    handlers: PlatformHandlers,
    background_executor: Arc<WebExecutor>,
    clipboard: Arc<WebClipboard>,
    is_running: bool,
    /// Window callbacks for RAF loop frame dispatch.
    /// Set when `open_window` creates the single browser window.
    window_callbacks: Option<Arc<WindowCallbacks>>,
    owner: Weak<WebPlatform>,
}

// SAFETY: WASM is single-threaded — no data races possible
unsafe impl Send for WebPlatform {}
unsafe impl Sync for WebPlatform {}

impl std::fmt::Debug for WebPlatform {
    // Hand-written: the state holds JS-backed handles (executors, clipboard,
    // callbacks) that carry no meaningful Debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebPlatform").finish_non_exhaustive()
    }
}

impl WebPlatform {
    /// Create a new Web platform instance
    pub fn new() -> Result<Self, PlatformError> {
        console_error_panic_hook::set_once();

        let state = WebState {
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(WebExecutor::new()),
            clipboard: Arc::new(WebClipboard::new()),
            is_running: false,
            window_callbacks: None,
            owner: Weak::new(),
        };
        let state = Arc::new(Mutex::new(state));
        let weak = Arc::downgrade(&state);
        let signal = OwnerSignal::new(Arc::new(move || {
            let weak = weak.clone();
            // spawn_local posts a microtask even when the future is ready.
            // Admission stays committed before any owner callback can run.
            js_sys::futures::spawn_local(async move {
                let owner = weak
                    .upgrade()
                    .and_then(|state| state.lock().owner.upgrade());
                if let Some(owner) = owner
                    && owner.signal.drive()
                {
                    owner.quit();
                }
            });
            Ok(())
        }));
        let preferences = super::preferences::Preferences::new(&signal)?;
        Ok(Self {
            state,
            signal,
            preferences: RefCell::new(preferences),
        })
    }

    fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        f(&mut self.state.lock())
    }

    /// Lease the callback outside state; a replacement registered during the
    /// call wins. Keep its owning envelope outside the caught invocation.
    fn dispatch_window_event(state: &Mutex<WebState>, event: WindowEvent) {
        let Some(callback) = state.lock().handlers.window_event.take() else {
            return;
        };
        let mut callback = Some(callback);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            callback
                .as_mut()
                .expect("BUG: leased window callback exists")(event);
        }));
        {
            let mut state = state.lock();
            if state.handlers.window_event.is_none() {
                state.handlers.window_event = callback.take();
            }
        }
        if let Err(payload) = outcome {
            // A replaced opaque callback may own panicking aggregate captures.
            // Do not retire it while preserving the callback's earlier failure.
            std::mem::forget(callback);
            resume_unwind(payload);
        }
        drop(callback);
    }

    /// Start the requestAnimationFrame render loop
    fn start_raf_loop(&self) {
        let state = Arc::clone(&self.state);

        // Recursive RAF pattern: closure references itself via Rc<RefCell>
        let f: RafClosureSlot = Rc::new(RefCell::new(None));
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
            Self::dispatch_window_event(
                &state,
                WindowEvent::RedrawRequested {
                    window_id: WindowId(0),
                },
            );

            // Request next frame after work (ensures smooth loop)
            if let Some(w) = web_sys::window() {
                let _ = w.request_animation_frame(
                    f.borrow()
                        .as_ref()
                        .expect("BUG: the RAF closure slot is filled before any frame runs")
                        .as_ref()
                        .unchecked_ref(),
                );
            }
        }));

        // Kick off the first frame
        let _ = window.request_animation_frame(
            g.borrow()
                .as_ref()
                .expect("BUG: the RAF closure slot was filled on the line above")
                .as_ref()
                .unchecked_ref(),
        );
    }
}

impl Platform for WebPlatform {
    fn preferences(&self) -> Result<flui_platform_api::SystemPreferences, PlatformError> {
        if !self.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner has stopped".into(),
            });
        }
        let observation = self.preferences.borrow().as_ref().map(|source| {
            (
                source.query.clone(),
                source.no_preference.clone(),
                Arc::clone(&source.active),
            )
        });
        let Some((query, no_preference, active)) = observation else {
            return Ok(flui_platform_api::SystemPreferences::default());
        };
        let motion = if query.matches() {
            Some(flui_platform_api::MotionPreference::Reduce)
        } else if no_preference.matches() {
            Some(flui_platform_api::MotionPreference::NoPreference)
        } else {
            None
        };
        if !active.load(Ordering::Acquire) || !self.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner stopped during sampling".into(),
            });
        }
        let preferences = flui_platform_api::SystemPreferences::default();
        Ok(match motion {
            Some(motion) => preferences.with_motion(motion),
            None => preferences,
        })
    }

    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|s| s.background_executor.clone())
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        tracing::info!("Starting web platform");

        self.with_state(|s| s.is_running = true);

        let platform = Arc::new(*self);
        platform.with_state(|state| state.owner = Arc::downgrade(&platform));
        platform
            .signal
            .start()
            .map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;

        // Window creation is direct and always Ready. Wake and quit use the
        // shared owner signal, posted to the browser microtask queue.
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
            Arc::clone(&platform) as Arc<dyn Platform>,
            Arc::clone(&platform.signal),
        ));

        // Call on_ready synchronously — browser event loop is already
        // running. On `Err`, do NOT install the RAF loop over a half-built
        // page — return the failure instead.
        on_ready(OwnerPlatform::new(
            Arc::clone(&platform) as Arc<dyn Platform>,
            hooks,
        ))
        .inspect_err(|_| platform.quit())
        .map_err(PlatformError::bootstrap)?;

        // Start the RAF loop
        platform.start_raf_loop();

        tracing::info!("Web platform ready");
        Ok(())
    }

    fn quit(&self) {
        tracing::info!("Web platform quit requested");
        self.signal.close();
        let preferences = self.preferences.borrow_mut().take();
        drop(preferences);
        let callback = self.with_state(|state| {
            state.is_running = false;
            state.handlers.quit.take()
        });
        if let Some(mut callback) = callback {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(&mut callback)) {
                // Shutdown is committed. Retain failed opaque captures before
                // propagating the original callback panic to the Rust boundary.
                std::mem::forget(callback);
                resume_unwind(payload);
            }
            drop(callback);
        }
    }

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        tracing::info!(title = %options.title, "Creating web window (canvas)");

        let window = WebWindow::new(
            WindowId(0), // Single window in browser
            &options.title,
            options.size.width,
            options.size.height,
        )?;

        // Register DOM event listeners on the canvas
        super::events::register_event_listeners(&window);

        // Store window callbacks in state so the RAF loop can dispatch frames
        let callbacks = window.callbacks().clone();
        self.with_state(|s| {
            s.window_callbacks = Some(callbacks);
        });

        // Notify window created
        Self::dispatch_window_event(&self.state, WindowEvent::Created(WindowId(0)));

        Ok(Arc::new(window))
    }

    fn active_window(&self) -> Option<WindowId> {
        Some(WindowId(0))
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

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // The web transport (navigator.clipboard, HTML5 DnD) needs its own
        // design (ADR-0038 §8) — until then this is inert and honest, not a
        // degraded fake over the write-cache clipboard.
        Arc::new(NullDataTransferSource)
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
        if let Some(w) = web_sys::window()
            && let Ok(Some(mql)) = w.match_media("(prefers-color-scheme: dark)")
            && mql.matches()
        {
            return WindowAppearance::Dark;
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
        let previous = self.with_state(|state| state.handlers.quit.replace(callback));
        drop(previous);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        let previous = self.with_state(|state| state.handlers.window_event.replace(callback));
        drop(previous);
    }

    fn app_path(&self) -> Result<std::path::PathBuf, PlatformError> {
        // web_sys::Window::location() returns Location, not Result
        // Location::origin() returns Result<String, JsValue>
        if let Some(w) = web_sys::window()
            && let Ok(origin) = w.location().href()
        {
            return Ok(std::path::PathBuf::from(origin));
        }
        Ok(std::path::PathBuf::from("/"))
    }
}
