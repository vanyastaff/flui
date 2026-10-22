//! The web host's loop: the state every position of its table reads, and the
//! `requestAnimationFrame` loop that drives its frames.
//!
//! Its `Send` and `Sync` claims rest on the page having one thread, which
//! `platforms::web` enforces by refusing a `+atomics` build.

// This module is one of the workspace's sanctioned `unsafe` islands: the page
// has one thread, so the loop and its executor claim `Send`/`Sync` by hand. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

mod executor;
pub(crate) mod listener;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use flui_platform::{SessionEndAnswer, SessionEndPhase};

use crate::host::{self, Loop as _};
use crate::owner::call_point::{Field, Once, call, install};
use crate::{
    error::PlatformError, owner::DirectOwnerHooks, owner::OwnerHooks, owner::OwnerPlatform,
    owner::handlers::PlatformHandlers, owner::handlers::WindowCallbacks,
    owner::signal::OwnerSignal, platform::Platform, platform::PlatformExecutor,
    platform::PlatformReadyCallback,
};
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::drag::PlatformDragSource;
use flui_platform::{Clipboard, WindowEvent, WindowId};

use crate::platforms::web::registration::Registration;
use crate::platforms::web::{DisplayObservers, PageObservers, WebDisplays, WebPreferences};
use executor::WebExecutor;

/// What the web host's table names for its owner thread (ADR-0151 §10): the
/// clipboard and the drag-and-drop transport the loop holds. The owner reaches
/// them through hooks and names no feature.
pub(crate) trait WebHost: 'static {
    /// The clipboard, made once per loop: the loop holds it and answers the
    /// table's clipboard position from it.
    fn clipboard() -> Arc<dyn Clipboard>;

    /// The drag-and-drop transport, made once per loop: the loop holds it and
    /// answers the table's drag position from it, and the window it opens
    /// registers its canvas through it.
    fn drag() -> Arc<dyn HostDrag>;
}

/// The page's drag-and-drop transport as the loop and its window reach it,
/// which the table makes ([`WebHost::drag`]): the platform's one
/// data-transfer source.
pub(crate) trait HostDrag: DataTransferSource {
    /// Listen, in `registration`, for drags over `canvas` and for the drags
    /// its window starts, dispatching to the window's `callbacks`.
    ///
    /// # Errors
    /// What the registration of a listener reports.
    fn register(
        self: Arc<Self>,
        canvas: &web_sys::HtmlCanvasElement,
        callbacks: &Arc<WindowCallbacks>,
        registration: &mut Registration,
    ) -> Result<Box<dyn CanvasDrag>, crate::OpenWindowError>;
}

/// What a canvas's drag registration leaves its window.
pub(crate) trait CanvasDrag {
    /// The drag source the window hands out.
    fn source(&self) -> Arc<dyn PlatformDragSource>;

    /// Make the canvas draggable, once its whole registration succeeded.
    fn arm(&self);

    /// Put the canvas's draggable attribute back as the page had it.
    fn retire(&self);
}

/// The clipboard position: the clipboard the host's [`WebHost::clipboard`]
/// made, which the loop holds.
impl host::Clipboard<WebLoop> for WebLoop {
    fn clipboard(owner: &WebLoop) -> Arc<dyn Clipboard> {
        owner.with_state(|state| Arc::clone(&state.clipboard))
    }
}

/// The drag position: the transport the host's [`WebHost::drag`] made, which
/// the loop holds.
impl host::Drag<WebLoop> for WebLoop {
    fn data_transfer(owner: &WebLoop) -> Arc<dyn DataTransferSource> {
        Arc::clone(&owner.drag) as Arc<dyn DataTransferSource>
    }
}

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
    pub(crate) state: Arc<Mutex<WebState>>,
    /// The one HTML drag-and-drop transport every canvas shares, as the table
    /// made it.
    pub(crate) drag: Arc<dyn HostDrag>,
    pub(crate) signal: Arc<OwnerSignal>,
    pub(crate) preferences: RefCell<Option<WebPreferences>>,
    pub(crate) displays: Arc<WebDisplays>,
    /// The browser listeners that keep `displays` current; present while the
    /// platform runs.
    display_observers: RefCell<Option<DisplayObservers>>,
    /// The listeners that report the page being hidden and shown again; present
    /// while the platform runs.
    page_observers: RefCell<Option<PageObservers>>,
}

pub(crate) struct WebState {
    pub(crate) handlers: PlatformHandlers,
    background_executor: Arc<WebExecutor>,
    pub(crate) clipboard: Arc<dyn Clipboard>,
    is_running: bool,
    /// Window callbacks for RAF loop frame dispatch.
    /// Set when `open_window` creates the single browser window.
    pub(crate) window_callbacks: Option<Arc<WindowCallbacks>>,
    owner: Weak<WebLoop>,
}

// SAFETY: the page has one thread (`platforms::web` refuses `+atomics`), so the
// value is never reached from another.
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
    pub(crate) fn new<H: WebHost>() -> Result<Self, PlatformError> {
        console_error_panic_hook::set_once();

        let state = WebState {
            handlers: PlatformHandlers::new(),
            background_executor: Arc::new(WebExecutor::new()),
            clipboard: H::clipboard(),
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
            drag: H::drag(),
            signal,
            preferences: RefCell::new(Some(preferences)),
            displays: WebDisplays::new(),
            display_observers: RefCell::new(None),
            page_observers: RefCell::new(None),
        })
    }

    pub(crate) fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        f(&mut self.state.lock())
    }

    /// One of the application's handlers, as a slot of the page state.
    pub(crate) fn handler<H>(
        state: &Mutex<WebState>,
        project: fn(&mut WebState) -> &mut Option<H>,
    ) -> Field<'_, Mutex<WebState>, H> {
        Field::new(state, project)
    }

    /// Call the window-event handler through the call point, outside the
    /// page state's lock; a replacement registered during the call wins.
    pub(crate) fn dispatch_window_event(state: &Mutex<WebState>, event: WindowEvent) {
        call(
            &Self::handler(state, |state| &mut state.handlers.window_event),
            |handler| handler(event),
        )
        .or_resume(());
    }

    /// The page was hidden or shown again: tell the session-end hook, so the
    /// application saves what must survive.
    ///
    /// A page cannot be asked to wait, so the answer is not read. `pagehide`
    /// also precedes a move into the back/forward cache: a page that comes back
    /// reports `Cancelled`, and is hidden again later, which reports `Ending`
    /// again. The browser may stop the page before an asynchronous write
    /// started here finishes; only what the application hands over
    /// synchronously is certain to be accepted. A hook registered while it
    /// ran replaces this one.
    fn report_session_phase(state: &Mutex<WebState>, phase: SessionEndPhase) {
        call(
            &Self::handler(state, |state| &mut state.handlers.session_end),
            |hook| hook(phase),
        )
        .or_contain(SessionEndAnswer::Proceed, "the session-end hook");
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
        self.with_state(|state| state.is_running = false);
        // Shutdown is committed: the quit callback runs once, and its panic
        // reaches the Rust boundary with its captures retained.
        call(
            &Once(Self::handler(&self.state, |state| &mut state.handlers.quit)),
            |quit| quit(),
        )
        .or_resume(());
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        install(
            &Self::handler(&self.state, |state| &mut state.handlers.quit),
            callback,
        );
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

    // A page reveals or opens no path and shows no file prompt here: those
    // services move to the system capability crate.
    fn reveal_path(&self, path: &std::path::Path) {
        let _ = path;
    }

    fn open_path(&self, path: &std::path::Path) {
        let _ = path;
    }

    fn prompt_for_paths(
        &self,
        options: crate::PathPromptOptions,
    ) -> crate::Task<Result<Option<Vec<std::path::PathBuf>>, PlatformError>> {
        let _ = options;
        crate::Task::ready(Ok(None))
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::Task<Result<Option<std::path::PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        crate::Task::ready(Ok(None))
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
