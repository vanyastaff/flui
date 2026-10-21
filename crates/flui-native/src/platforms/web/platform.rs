//! The web host's loop: the state every position of its table reads.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use flui_platform::SessionEndPhase;

use crate::host::{self, Loop as _};
use crate::{
    error::PlatformError, shared::PlatformHandlers, shared::WindowCallbacks,
    shared::owner_signal::OwnerSignal, traits::OwnerPlatform, traits::Platform,
    traits::PlatformExecutor, traits::PlatformReadyCallback, traits::owner::DirectOwnerHooks,
    traits::owner::OwnerHooks,
};
use flui_platform::{WindowEvent, WindowId};

use super::{
    clipboard::WebClipboard, display::WebDisplays, display_events::DisplayObservers,
    executor::WebExecutor, page_events::PageObservers, preferences::WebPreferences,
};

/// The self-referencing `requestAnimationFrame` callback slot: the closure
/// re-schedules itself through this handle, so it is created empty and filled
/// once the closure exists.
type RafClosureSlot = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

/// The web host's loop: the browser state every position of its table reads.
///
/// The page has one thread and one canvas window; the browser's own event loop
/// is the loop, and this holds what the positions reach through it.
/// [`WebPlatform`](crate::WebPlatform) is its platform.
pub struct WebLoop {
    pub(super) state: Arc<Mutex<WebState>>,
    pub(super) signal: Arc<OwnerSignal>,
    pub(super) preferences: RefCell<Option<WebPreferences>>,
    pub(super) displays: Arc<WebDisplays>,
    /// The browser listeners that keep `displays` current; present while the
    /// platform runs.
    display_observers: RefCell<Option<DisplayObservers>>,
    /// The listeners that report the page being hidden and shown again; present
    /// while the platform runs.
    page_observers: RefCell<Option<PageObservers>>,
}

pub(super) struct WebState {
    pub(super) handlers: PlatformHandlers,
    background_executor: Arc<WebExecutor>,
    pub(super) clipboard: Arc<WebClipboard>,
    /// The one HTML drag-and-drop transport every canvas shares.
    pub(super) transfer: Arc<super::drag::WebTransfer>,
    is_running: bool,
    /// Window callbacks for RAF loop frame dispatch.
    /// Set when `open_window` creates the single browser window.
    pub(super) window_callbacks: Option<Arc<WindowCallbacks>>,
    owner: Weak<WebLoop>,
}

// SAFETY: the page has one thread (the module refuses `+atomics`), so the value
// is never reached from another.
unsafe impl Send for WebLoop {}
unsafe impl Sync for WebLoop {}

impl std::fmt::Debug for WebLoop {
    // Hand-written: the state holds JS-backed handles (executors, clipboard,
    // callbacks) that carry no meaningful Debug representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebLoop").finish_non_exhaustive()
    }
}

impl WebLoop {
    pub(crate) fn new() -> Result<Self, PlatformError> {
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
        let preferences = WebPreferences::new(&signal)?;
        Ok(Self {
            state,
            signal,
            preferences: RefCell::new(Some(preferences)),
            displays: WebDisplays::new(),
            display_observers: RefCell::new(None),
            page_observers: RefCell::new(None),
        })
    }

    pub(super) fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        f(&mut self.state.lock())
    }

    /// Lease the callback outside state; a replacement registered during the
    /// call wins. Keep its owning envelope outside the caught invocation.
    pub(super) fn dispatch_window_event(state: &Mutex<WebState>, event: WindowEvent) {
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

impl host::Loop for WebLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        on_ready: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        tracing::info!("Starting web platform");

        self.with_state(|s| s.is_running = true);

        self.with_state(|state| state.owner = Arc::downgrade(&self));
        self.signal.start().map_err(|error| PlatformError::Init {
            message: error.to_string(),
        })?;

        // Display events wake the owner through the same signal; they are
        // observed from here on so the first snapshot already follows them.
        *self.display_observers.borrow_mut() =
            Some(DisplayObservers::start(&self.displays, &self.signal));

        // The page being hidden is the end of the session: the browser may
        // discard it without another event.
        let state = Arc::downgrade(&self.state);
        *self.page_observers.borrow_mut() = Some(PageObservers::start(move |phase| {
            if let Some(state) = state.upgrade() {
                Self::report_session_phase(&state, phase);
            }
        }));

        // Window creation is direct and always Ready. Wake and quit use the
        // shared owner signal, posted to the browser microtask queue.
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
            Arc::clone(&platform),
            Arc::clone(&self.signal),
        ));

        // Call on_ready synchronously — browser event loop is already
        // running. On `Err`, do NOT install the RAF loop over a half-built
        // page — return the failure instead.
        on_ready(OwnerPlatform::new(Arc::clone(&platform), hooks))
            .inspect_err(|_| self.quit())
            .map_err(PlatformError::bootstrap)?;

        // Start the RAF loop
        self.start_raf_loop();

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

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        let previous = self.with_state(|state| state.handlers.quit.replace(callback));
        drop(previous);
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        // The page ends when its tab does; closing the canvas never exits the
        // loop, so there is no exit to consult the hook about.
        drop(hook);
    }

    fn request_exit_policy_reevaluation(&self) {
        // With no exit policy hook to consult, there is nothing to re-evaluate.
    }

    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        // The browser's event loop idles the page and the animation-frame loop
        // drives its frames; there is no idle wait to bound.
        drop(hook);
    }
}

impl host::Services for WebLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.with_state(|s| s.background_executor.clone())
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

    fn open_url(&self, url: &str) {
        if let Some(w) = web_sys::window() {
            let _ = w.open_with_url_and_target(url, "_blank");
        }
    }

    fn compositor_name(&self) -> &'static str {
        "Browser"
    }
}

impl host::Input<WebLoop> for WebLoop {
    fn keyboard_layout(_: &WebLoop) -> String {
        "en-US".to_string()
    }

    fn on_keyboard_layout_change(_: &WebLoop, callback: Box<dyn FnMut() + Send>) {
        // The layout is the fixed "en-US" above; nothing changes it.
        drop(callback);
    }
}
