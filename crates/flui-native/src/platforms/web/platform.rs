//! Web platform core implementation

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::{Arc, Weak, atomic::Ordering};

use parking_lot::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use flui_platform::{SessionEndAnswer, SessionEndPhase};

use crate::{
    data_transfer::DataTransferSource,
    error::PlatformError,
    shared::{PlatformHandlers, WindowCallbacks, owner_signal::OwnerSignal},
    traits::{
        Clipboard, DisplaySnapshot, HostWindow, OpenWindowError, OwnerPlatform, Platform,
        PlatformExecutor, PlatformReadyCallback, WindowEvent, WindowId, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks},
    },
};

use super::{
    clipboard::WebClipboard, display::WebDisplays, display_events::DisplayObservers,
    executor::WebExecutor, page_events::PageObservers, window::WebWindow,
};

/// The self-referencing `requestAnimationFrame` callback slot: the closure
/// re-schedules itself through this handle, so it is created empty and filled
/// once the closure exists.
type RafClosureSlot = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

/// Web/WASM platform implementation
pub struct WebPlatform {
    state: Arc<Mutex<WebState>>,
    signal: Arc<OwnerSignal>,
    preferences: RefCell<Option<super::preferences::Preferences>>,
    displays: Arc<WebDisplays>,
    /// The browser listeners that keep `displays` current; present while the
    /// platform runs.
    display_observers: RefCell<Option<DisplayObservers>>,
    /// The listeners that report the page being hidden and shown again; present
    /// while the platform runs.
    page_observers: RefCell<Option<PageObservers>>,
}

struct WebState {
    handlers: PlatformHandlers,
    background_executor: Arc<WebExecutor>,
    clipboard: Arc<WebClipboard>,
    /// The one HTML drag-and-drop transport every canvas shares.
    transfer: Arc<super::drag::WebTransfer>,
    is_running: bool,
    /// Window callbacks for RAF loop frame dispatch.
    /// Set when `open_window` creates the single browser window.
    window_callbacks: Option<Arc<WindowCallbacks>>,
    owner: Weak<WebPlatform>,
}

// SAFETY: the page has one thread (the module refuses `+atomics`), so the value
// is never reached from another.
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
            transfer: Arc::default(),
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
            displays: WebDisplays::new(),
            display_observers: RefCell::new(None),
            page_observers: RefCell::new(None),
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

    /// The page was hidden or shown again: tell the session-end hook, so the
    /// application saves what must survive.
    ///
    /// A page cannot be asked to wait, so the answer is not read. `pagehide`
    /// also precedes a move into the back/forward cache: a page that comes back
    /// reports `Cancelled`, and is hidden again later, which reports `Ending`
    /// again. The browser may stop the page before an asynchronous write
    /// started here finishes; only what the application hands over
    /// synchronously is certain to be accepted.
    fn report_session_phase(state: &Mutex<WebState>, phase: SessionEndPhase) {
        let Some(mut hook) = state.lock().handlers.session_end.take() else {
            return;
        };
        crate::shared::panic_boundary::contain_owner_callback(|| {
            hook(phase);
        });
        // A hook registered while it ran replaces this one.
        let replaced = {
            let mut state = state.lock();
            if state.handlers.session_end.is_none() {
                state.handlers.session_end = Some(hook);
                None
            } else {
                Some(hook)
            }
        };
        crate::shared::panic_boundary::contain_owner_callback(|| drop(replaced));
    }

    /// Start the requestAnimationFrame render loop
    fn start_raf_loop(&self) {
        let state = Arc::clone(&self.state);
        let displays = Arc::downgrade(&self.displays);
        let signal = Arc::downgrade(&self.signal);

        // Recursive RAF pattern: closure references itself via Rc<RefCell>
        let f: RafClosureSlot = Rc::new(RefCell::new(None));
        let g = Rc::clone(&f);

        let window = web_sys::window().expect("no global window");

        *g.borrow_mut() = Some(Closure::new(move |timestamp: f64| {
            let (is_running, callbacks) = {
                let s = state.lock();
                (s.is_running, s.window_callbacks.clone())
            };
            if !is_running {
                return;
            }

            // The callback's timestamp is the display's cadence: the refresh
            // estimate follows it. A new published period wakes the owner,
            // whose next turn reads the changed snapshot.
            if let Some(displays) = displays.upgrade()
                && displays.frame(timestamp)
                && let Some(signal) = signal.upgrade()
            {
                // A failed wake leaves the signal's retry debt.
                let _ = signal.wake();
            }

            // Dispatch frame request to window callbacks
            if let Some(ref cbs) = callbacks {
                cbs.dispatch_request_frame();
                // The frame callback can close and reopen the canvas. An old
                // frame receipt cannot produce a redraw for the newer slot.
                let current = state
                    .lock()
                    .window_callbacks
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, cbs));
                if current {
                    Self::dispatch_window_event(
                        &state,
                        WindowEvent::RedrawRequested {
                            window_id: WindowId(0),
                        },
                    );
                }
            }

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
    fn window_capabilities(&self) -> flui_platform::WindowCapabilities {
        super::window::window_capabilities()
    }

    fn preferences(&self) -> Result<flui_platform::SystemPreferences, PlatformError> {
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
            return Ok(flui_platform::SystemPreferences::default());
        };
        let motion = if query.matches() {
            Some(flui_platform::MotionPreference::Reduce)
        } else if no_preference.matches() {
            Some(flui_platform::MotionPreference::NoPreference)
        } else {
            None
        };
        if !active.load(Ordering::Acquire) || !self.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner stopped during sampling".into(),
            });
        }
        let preferences = flui_platform::SystemPreferences::default();
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

        // Display events wake the owner through the same signal; they are
        // observed from here on so the first snapshot already follows them.
        *platform.display_observers.borrow_mut() = Some(DisplayObservers::start(
            &platform.displays,
            &platform.signal,
        ));

        // The page being hidden is the end of the session: the browser may
        // discard it without another event.
        let state = Arc::downgrade(&platform.state);
        *platform.page_observers.borrow_mut() = Some(PageObservers::start(move |phase| {
            if let Some(state) = state.upgrade() {
                Self::report_session_phase(&state, phase);
            }
        }));

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
        let display_observers = self.display_observers.borrow_mut().take();
        drop(display_observers);
        let page_observers = self.page_observers.borrow_mut().take();
        drop(page_observers);
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
            Arc::clone(&self.displays),
        )?;

        // Store window callbacks in state so the RAF loop can dispatch frames
        let callbacks = window.callbacks().clone();
        callbacks.set_input_capabilities(crate::shared::input_availability::web(
            super::events::has_gesture_events(),
        ));
        callbacks.set_input_wake(OwnerSignal::callbacks_wake(&self.signal, &callbacks));
        let retiring = Arc::clone(&callbacks);
        let state = Arc::downgrade(&self.state);
        let transfer = self.with_state(|s| Arc::clone(&s.transfer));
        window.install(options.input_latency, &transfer, move || {
            let removed = state.upgrade().and_then(|state| {
                let mut state = state.lock();
                if state
                    .window_callbacks
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &retiring))
                {
                    state.window_callbacks.take()
                } else {
                    None
                }
            });
            drop(removed);
        })?;
        self.with_state(|s| {
            s.window_callbacks = Some(callbacks);
        });

        // Notify window created
        Self::dispatch_window_event(&self.state, WindowEvent::Created(WindowId(0)));

        Ok(Arc::new(window))
    }

    fn active_window(&self) -> Option<WindowId> {
        self.state
            .lock()
            .window_callbacks
            .as_ref()
            .map(|_| WindowId(0))
    }

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.displays.snapshot()
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.with_state(|s| s.clipboard.clone())
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // HTML drag and drop; `clipboard_offer` stays `None`, since the
        // clipboard reads only its own writes.
        self.with_state(|s| Arc::clone(&s.transfer)) as Arc<dyn DataTransferSource>
    }

    fn name(&self) -> &'static str {
        "Web (WASM)"
    }

    fn compositor_name(&self) -> &'static str {
        "Browser"
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

    fn on_session_end(&self, callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>) {
        let previous = self.with_state(|state| state.handlers.session_end.replace(callback));
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
