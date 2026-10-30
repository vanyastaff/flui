//! The web host's loop: the state every position of its table reads, and the
//! `requestAnimationFrame` loop that drives its frames.
//!
//! Its `Send` and `Sync` claims rest on the page having one thread, which this
//! module enforces by refusing a `+atomics` build.

// This module is one of the workspace's sanctioned `unsafe` islands: the page
// has one thread, so the loop and its executor claim `Send`/`Sync` by hand. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

// The host's `Send`/`Sync` claims rest on the page having one thread: the JS
// handles and `RefCell`s it holds are never reached from another. A
// shared-memory build (`+atomics`, wasm threads) breaks that, so it is refused
// here rather than left to a data race.
#[cfg(target_feature = "atomics")]
compile_error!(
    "the web backend assumes one thread; build wasm32 without the `atomics` target feature"
);

pub(crate) mod event;
mod executor;
pub(crate) mod listener;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use flui_platform::{SessionEndAnswer, SessionEndPhase};

use crate::host::{self, Loop as _};
use crate::owner::app_delivery::AppDelivery;
use crate::{
    error::PlatformError, owner::DirectOwnerHooks, owner::OwnerHooks, owner::OwnerPlatform,
    owner::signal::OwnerSignal, owner::window_delivery::WindowDelivery, platform::AppHandler,
    platform::Platform, platform::PlatformExecutor,
};
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::drag::PlatformDragSource;
use flui_platform::{DisplayId, DisplaySnapshot, SystemPreferences};

use crate::owner::sources::SystemPreferenceSource;
use crate::owner::watch::Watch;
use executor::WebExecutor;

/// What the web host's table names for its owner thread (ADR-0151 §10): the
/// displays, the preference source, the page's lifecycle, the clipboard, the
/// drag-and-drop transport and the input the loop holds. The owner reaches
/// them through the hooks and names no feature.
pub(crate) trait WebHost: 'static {
    /// The displays, made once per loop: the loop holds them for the table's
    /// displays position and hands them to the window it opens.
    fn displays() -> Arc<dyn HostDisplays>;

    /// The system preference source, for the table's preference position,
    /// kept until the loop quits; it wakes the owner behind `signal` when what
    /// it reads changes.
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the browser's preference queries
    /// cannot be observed.
    fn preferences(
        signal: &Arc<OwnerSignal>,
    ) -> Result<Rc<dyn SystemPreferenceSource>, PlatformError>;

    /// The page's lifecycle, which the loop observes while it runs.
    fn page() -> Box<dyn PageLifecycle>;

    /// The clipboard, made once per loop: the loop holds it and answers the
    /// table's clipboard position from it.
    fn clipboard() -> Arc<dyn crate::host::SharedClipboard>;

    /// The drag-and-drop transport, made once per loop: the loop holds it and
    /// answers the table's drag position from it, and the window it opens
    /// registers its canvas through it.
    fn drag() -> Arc<dyn HostDrag>;

    /// The page's input, made once per loop: the window it opens registers its
    /// canvas's input through it.
    fn input() -> Rc<dyn HostInput>;
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
        delivery: &Arc<WindowDelivery>,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<Box<dyn CanvasDrag>, crate::OpenWindowError>;
}

/// The page's input as the window the loop opens reaches it, which the table
/// makes ([`WebHost::input`]): the canvas's pointer, keyboard, wheel and
/// gesture events, and what the page's input path delivers.
pub(crate) trait HostInput {
    /// What the page's input path delivers (ADR-0213), with no device present
    /// yet: the window's first input capabilities.
    fn availability(&self) -> flui_platform::input_capabilities::InputCapabilities;

    /// Listen, in `registration`, for the input of `canvas`, dispatching to
    /// the window's `callbacks`; `latency` picks the event a pointer's
    /// movement arrives as. What the input keeps of the canvas lives in its
    /// listeners, and goes with them.
    ///
    /// # Errors
    /// What the registration of a listener reports.
    fn register(
        &self,
        canvas: &web_sys::HtmlCanvasElement,
        delivery: &Arc<WindowDelivery>,
        latency: flui_platform::InputLatency,
        registration: &mut dyn CanvasRegistration,
    ) -> Result<Rc<dyn CanvasKeys>, crate::OpenWindowError>;
}

/// How a DOM key event reaches a canvas's window as the canvas's own: from
/// the canvas, or from the editor its text input keeps beside it.
pub(crate) trait CanvasKeys {
    /// Deliver `event` as a key `state`, answering it with the result.
    fn deliver(&self, event: &web_sys::Event, state: flui_platform::keyboard::KeyState);

    /// Whether the window's text input composes now: a key delivered while it
    /// does is the input method's.
    fn composing(&self) -> bool;

    /// The window's text input started or ended a composition.
    fn set_composing(&self, composing: bool);
}

/// What a canvas's window keeps registered for the features listening beside
/// it: DOM listeners and deadline timers that run only while the window
/// accepts input and go when it retires or its opening rolls back, and the
/// custody of the pointers the canvas captures.
pub(crate) trait CanvasRegistration {
    /// Listen for `kind` events on `target` with `body`, handed over as
    /// `Rc::new(..)`: the listener keeps its only strong reference, and the
    /// browser's callback reaches it weakly, so a retired listener runs and
    /// keeps nothing of it.
    ///
    /// # Errors
    /// What the browser reports when the listener cannot be added.
    fn listen(
        &mut self,
        target: &web_sys::EventTarget,
        kind: &'static str,
        body: Rc<dyn Fn(web_sys::Event)>,
    ) -> Result<(), crate::OpenWindowError>;

    /// Whether the registration accepts: what a feature keeps beside its
    /// listeners reads it before it delivers.
    fn accepting(&self) -> Accepting;

    /// A deadline timer whose body runs only while the registration accepts;
    /// its retirement clears it.
    fn timer(&mut self, body: Box<dyn Fn()>) -> Rc<dyn CanvasTimer>;

    /// Run `retire` when the registration retires, once it no longer
    /// accepts and before its listeners go: what a feature holds of the
    /// canvas beyond its listeners (the pointers it captured) ends there. A
    /// registration whose opening rolls back retires too.
    fn on_retire(&mut self, retire: Box<dyn FnOnce()>);

    /// The custody of the canvas's pointer captures.
    fn custody(&self) -> Rc<dyn PointerCustody>;

    /// The canvas's focus area: the canvas and the editor its text input keeps
    /// beside it.
    fn focus(&self) -> Rc<dyn CanvasFocus>;
}

/// Whether a canvas's registration, or a source a window watches, still
/// accepts, as its readers see it: they read it, and only its owner, which
/// holds the flag itself, ends it.
#[derive(Clone)]
pub(crate) struct Accepting(Rc<Cell<bool>>);

impl Accepting {
    /// A reader of `flag`, which its owner keeps and ends.
    pub(crate) fn reading(flag: &Rc<Cell<bool>>) -> Self {
        Self(Rc::clone(flag))
    }

    /// Whether it still accepts.
    pub(crate) fn get(&self) -> bool {
        self.0.get()
    }
}

/// A canvas's focus area ([`CanvasRegistration::focus`]).
pub(crate) trait CanvasFocus {
    /// Whether the canvas, or the editor beside it, has the page's focus.
    fn has_focus(&self) -> bool;
}

/// Where a canvas's text input keeps its editor ([`CanvasRef::editor`]): the
/// canvas's focus area, as the window follows it.
pub(crate) trait CanvasEditor {
    /// `editor` is now part of the canvas's focus area; `None` when none is.
    fn set(&self, editor: Option<&web_sys::HtmlElement>);

    /// Whether the canvas, or the editor beside it, has the page's focus.
    fn has_focus(&self) -> bool;

    /// Whether `event` only moves focus within the canvas's focus area
    /// (between the canvas and its editor), which the window does not see as
    /// a focus change.
    fn is_handoff(&self, event: &web_sys::Event) -> bool;

    /// The focus area's focus may have changed: the window reads it again.
    fn sync(&self);
}

/// One canvas window as its window-level positions are made for it
/// (`host::Window::Ref`): the canvas, whether its registration accepts, the
/// key route its input keeps and the focus area its text input's editor
/// joins. Public where it is defined, as the host's table names it, and
/// nameable only inside the crate.
pub struct CanvasRef {
    /// The canvas.
    pub(crate) canvas: web_sys::HtmlCanvasElement,
    /// Whether the canvas's registration accepts.
    pub(crate) accepting: Accepting,
    /// The key route the canvas's input keeps, which a key the editor takes
    /// reaches the window by.
    pub(crate) keys: Rc<dyn CanvasKeys>,
    /// The canvas's focus area, which the text input's editor joins.
    pub(crate) editor: Rc<dyn CanvasEditor>,
}

impl std::fmt::Debug for CanvasRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasRef").finish_non_exhaustive()
    }
}

/// A deadline timer of a canvas's registration
/// ([`CanvasRegistration::timer`]): one pending run at a time, cleared when
/// the registration retires.
pub(crate) trait CanvasTimer {
    /// Run the body once `delay` has passed, replacing a pending run.
    fn arm(&self, delay: std::time::Duration);
}

/// The custody of a canvas's pointer captures
/// ([`CanvasRegistration::custody`]): a capture belongs to the registration
/// that admitted it, so a retired one cannot release a pointer a newer owner
/// of the same canvas admitted.
pub(crate) trait PointerCustody {
    /// Take custody of pointer `id`; `false` when another registration holds
    /// it.
    fn admit(&self, id: i32) -> bool;

    /// Give up custody of `id` without releasing its capture.
    fn forget(&self, id: i32);

    /// Give up custody of `id` and release its capture.
    fn release(&self, id: i32);
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
    fn clipboard(owner: &WebLoop) -> Rc<dyn crate::NativeClipboard> {
        Rc::new(owner.with_state(|state| Arc::clone(&state.clipboard)))
    }
}

/// The drag position: the transport the host's [`WebHost::drag`] made, which
/// the loop holds.
impl host::Drag<WebLoop> for WebLoop {
    fn data_transfer(owner: &WebLoop) -> Arc<dyn DataTransferSource> {
        Arc::clone(&owner.drag) as Arc<dyn DataTransferSource>
    }
}

/// The page's lifecycle as the web loop observes it while it runs.
pub(crate) trait PageLifecycle {
    /// Observe the page until the returned watch is dropped; `report` hears
    /// each phase of the session end the page goes through.
    fn listen(&self, report: Box<dyn Fn(SessionEndPhase)>) -> Watch;
}

/// The page's screens as the loop and its window reach them, which the table
/// makes ([`WebHost::displays`]). The page has one thread, so they need not be
/// `Send`.
pub(crate) trait HostDisplays {
    /// The cached snapshot, sampled from the browser when an event
    /// invalidated it.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the snapshot revision space is used up.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    /// The display the window is on; `None` when the platform has no id for it.
    fn current_display(&self) -> Option<DisplayId>;

    /// Feed one animation-frame timestamp (milliseconds): `true` when the
    /// published period changed, so the owner is woken to sample again.
    fn frame(&self, timestamp_ms: f64) -> bool;

    /// Whether the refresh estimate still needs frames: the animation-frame
    /// loop runs for it while no frame is asked for.
    fn measuring(&self) -> bool;

    /// The animation-frame loop runs again after standing still: the time
    /// since its last frame is no interval of the display's cadence.
    fn frames_resumed(&self);

    /// Keep the snapshot current from the browser's events, each of which
    /// wakes the owner behind `signal`, until the watch is dropped.
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;

    /// Watch the device pixel ratio for a window's layout: `on_change` runs
    /// each time it changes while `active` holds, until the watch is dropped.
    /// `None` in a browser without media queries.
    fn watch_pixel_ratio(
        &self,
        active: &Accepting,
        on_change: Box<dyn Fn()>,
    ) -> Option<Box<dyn PixelRatioWatch>>;
}

/// A window's watch of the device pixel ratio
/// ([`HostDisplays::watch_pixel_ratio`]).
pub(crate) trait PixelRatioWatch {
    /// Arm now if the browser refused the last query, and report a change that
    /// went unseen meanwhile.
    fn arm_if_unarmed(&self);
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
    /// What the page delivers to the application's handler, which `run` binds.
    pub(crate) app: Arc<AppDelivery>,
    /// The one HTML drag-and-drop transport every canvas shares, as the table
    /// made it.
    pub(crate) drag: Arc<dyn HostDrag>,
    /// The page's input, as the table made it: the window registers its
    /// canvas's input through it.
    pub(crate) input: Rc<dyn HostInput>,
    pub(crate) signal: Arc<OwnerSignal>,
    /// The system preference source the table made; present until the platform
    /// quits.
    preferences: RefCell<Option<Rc<dyn SystemPreferenceSource>>>,
    /// The screens, as the table made them; shared with the window.
    pub(crate) displays: Arc<dyn HostDisplays>,
    /// The browser listeners that keep `displays` current; present while the
    /// platform runs.
    display_observers: RefCell<Option<Watch>>,
    /// The page's lifecycle, as the table made it.
    page: Box<dyn PageLifecycle>,
    /// The listeners that report the page being hidden and shown again; present
    /// while the platform runs.
    page_observers: RefCell<Option<Watch>>,
    /// The frames the window asks for: the animation-frame loop runs while
    /// one is asked for.
    frames: Arc<crate::owner::frame_demand::FrameDemand>,
    /// The running animation-frame loop: its closure, and whether it stopped
    /// for want of a request.
    raf: RefCell<Option<(RafClosureSlot, Rc<Cell<bool>>)>>,
}

pub(crate) struct WebState {
    background_executor: Arc<WebExecutor>,
    pub(crate) clipboard: Arc<dyn crate::host::SharedClipboard>,
    is_running: bool,
    /// Window callbacks for RAF loop frame dispatch.
    /// Set when `open_window` creates the single browser window.
    pub(crate) window_delivery: Option<Arc<WindowDelivery>>,
    owner: Weak<WebLoop>,
}

// SAFETY: the page has one thread (`owner::web` refuses `+atomics`), so the
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
    pub(crate) fn try_new<H: WebHost>() -> Result<Self, PlatformError> {
        console_error_panic_hook::set_once();

        let state = WebState {
            background_executor: Arc::new(WebExecutor::new()),
            clipboard: H::clipboard(),
            is_running: false,
            window_delivery: None,
            owner: Weak::new(),
        };
        let state = Arc::new(Mutex::new(state));
        let weak = Arc::downgrade(&state);
        let signal = OwnerSignal::new(Arc::new(move || {
            let weak = weak.clone();
            // spawn_local posts a microtask even when the future is ready.
            // Admission stays committed before any owner callback can run.
            js_sys::futures::spawn_local(async move {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let (owner, window) = {
                    let state = state.lock();
                    (state.owner.upgrade(), state.window_delivery.clone())
                };
                // A wake is how a screen change reaches the page: the window
                // reads which screen it is on again before the owner's turn, so
                // the turn sees the display in the window's snapshot.
                // Contained, as the session-end hook is: a state callback
                // that panics must not cost the owner its turn.
                if let Some(window) = window
                    && let Err(payload) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            window.publish_state();
                        }))
                {
                    tracing::error!(
                        panic = ?flui_foundation::panic::payload_text(payload.as_ref()),
                        "the window's state callback panicked on a wake"
                    );
                    // An arbitrary payload's drop may panic again.
                    std::mem::forget(payload);
                }
                if let Some(owner) = owner {
                    // A frame request wakes the owner: the loop runs again.
                    owner.resume_frames();
                    if owner.signal.drive() {
                        owner.quit();
                    }
                }
            });
            Ok(())
        }));
        let preferences = H::preferences(&signal)?;
        Ok(Self {
            state,
            app: Arc::new(AppDelivery::new()),
            drag: H::drag(),
            input: H::input(),
            signal,
            preferences: RefCell::new(Some(preferences)),
            displays: H::displays(),
            display_observers: RefCell::new(None),
            page: H::page(),
            page_observers: RefCell::new(None),
            frames: Arc::default(),
            raf: RefCell::new(None),
        })
    }

    pub(crate) fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut WebState) -> R,
    {
        f(&mut self.state.lock())
    }

    /// The page was hidden or shown again: tell the application's handler, so
    /// it saves what must survive.
    ///
    /// A page cannot be asked to wait, so the answer is not read. `pagehide`
    /// also precedes a move into the back/forward cache: a page that comes back
    /// reports `Cancelled`, and is hidden again later, which reports `Ending`
    /// again. The browser may stop the page before an asynchronous write
    /// started here finishes; only what the application hands over
    /// synchronously is certain to be accepted.
    fn report_session_phase(app: &AppDelivery, phase: SessionEndPhase) {
        let _answer = app
            .session_end(phase)
            .or_contain(SessionEndAnswer::Proceed, "the session-end handler");
    }

    /// Start the `requestAnimationFrame` loop. It runs while the window asks
    /// for frames (`frames`) or the refresh estimate is measuring, and stands
    /// still after a frame neither wanted; a request wakes the owner, whose
    /// turn resumes it ([`resume_frames`](Self::resume_frames)).
    fn start_raf_loop(&self) {
        let state = Arc::clone(&self.state);
        let displays = Arc::downgrade(&self.displays);
        let signal = Arc::downgrade(&self.signal);
        let frames = Arc::clone(&self.frames);

        // Recursive RAF pattern: closure references itself via Rc<RefCell>
        let f: RafClosureSlot = Rc::new(RefCell::new(None));
        let g = Rc::clone(&f);
        let idle = Rc::new(Cell::new(false));
        let stopped = Rc::clone(&idle);

        *g.borrow_mut() = Some(Closure::new(move |timestamp: f64| {
            let (is_running, delivery) = {
                let s = state.lock();
                (s.is_running, s.window_delivery.clone())
            };
            if !is_running {
                return;
            }

            crate::owner::animation_frame::turn(
                &frames,
                || {
                    displays
                        .upgrade()
                        .is_some_and(|displays| displays.measuring())
                },
                || request_frame(&f),
                || stopped.set(true),
                |requested| {
                    // The callback's timestamp is the display's cadence: the
                    // refresh estimate follows it. A new published period
                    // wakes the owner, whose next turn reads the changed
                    // snapshot.
                    if let Some(displays) = displays.upgrade()
                        && displays.frame(timestamp)
                        && let Some(signal) = signal.upgrade()
                    {
                        // A failed wake leaves the signal's retry debt.
                        let _ = signal.wake();
                    }

                    if requested && let Some(ref cbs) = delivery {
                        cbs.dispatch_frame(frame_request_for_animation_frame(timestamp));
                    }
                },
            );
        }));

        // Kick off the first frame
        request_frame(&g);
        *self.raf.borrow_mut() = Some((g, idle));
    }

    /// Resume the animation-frame loop if it stood still and a frame is asked
    /// for now, or the refresh estimate restarted. The time the loop stood
    /// still is no interval of the display's cadence.
    fn resume_frames(&self) {
        let raf = self.raf.borrow();
        let Some((slot, idle)) = raf.as_ref() else {
            return;
        };
        let wanted = self.frames.pending() || self.displays.measuring();
        if idle.get() && wanted && self.state.lock().is_running {
            idle.set(false);
            self.displays.frames_resumed();
            request_frame(slot);
        }
    }

    /// How the window asks for a frame from anywhere.
    pub(crate) fn frame_requests(&self) -> FrameRequests {
        FrameRequests {
            demand: Arc::clone(&self.frames),
            signal: Arc::downgrade(&self.signal),
        }
    }
}

/// Ask the browser for the next animation frame of the loop in `slot`.
fn request_frame(slot: &RafClosureSlot) {
    if let Some(window) = web_sys::window() {
        let _ = window.request_animation_frame(
            slot.borrow()
                .as_ref()
                .expect("BUG: the RAF closure slot is filled before a frame is asked for")
                .as_ref()
                .unchecked_ref(),
        );
    }
}

/// The page window's redraw requester: it marks the window's frame demand,
/// and the first request since the loop stopped wakes the owner, whose turn
/// resumes the loop.
pub(crate) struct FrameRequests {
    demand: Arc<crate::owner::frame_demand::FrameDemand>,
    signal: Weak<OwnerSignal>,
}

impl flui_platform::RedrawRequester for FrameRequests {
    fn request_redraw(&self) {
        if self.demand.request()
            && let Some(signal) = self.signal.upgrade()
        {
            // A failed wake leaves the signal's retry debt.
            let _ = signal.wake();
        }
    }
}

impl host::Loop for WebLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
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
            Some(Arc::clone(&self.displays).observe(&self.signal));

        // The page being hidden is the end of the session: the browser may
        // discard it without another event.
        let app = Arc::downgrade(&self.app);
        *self.page_observers.borrow_mut() = Some(self.page.listen(Box::new(move |phase| {
            if let Some(app) = app.upgrade() {
                Self::report_session_phase(&app, phase);
            }
        })));

        // Window creation is direct and always Ready. Wake and quit use the
        // shared owner signal, posted to the browser microtask queue.
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
            Arc::clone(&platform),
            Arc::clone(&self.signal),
        ));

        // Run the bootstrap synchronously — the browser's event loop is
        // already running. On `Err`, do NOT install the RAF loop over a
        // half-built page — return the failure instead. The page outlives
        // `run`: the handler stays bound, and the owner capability the
        // bootstrap keeps holds the platform and hooks the delivery holds
        // weakly.
        self.app.bind(handler, &platform, &hooks);
        self.app.resume_through_signal(&self.signal);
        self.app
            .ready(OwnerPlatform::new(Arc::clone(&platform), hooks))
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
        // Shutdown is committed: the handler hears the quit once, and its
        // panic reaches the Rust boundary. The handler is let go after.
        self.app.quit_requested().or_resume(());
        self.app.release();
    }

    fn request_exit_policy_reevaluation(&self) {
        // The page ends when its tab does: with no exit policy to ask, there
        // is nothing to re-evaluate.
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

/// The displays position is the loop's: it answers from the screens the table
/// made ([`WebHost::displays`]).
impl host::Displays<WebLoop> for WebLoop {
    fn snapshot(owner: &WebLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner.displays.snapshot()
    }
}

/// The preference position is the loop's: it reads the source the table made
/// ([`WebHost::preferences`]) while the platform runs.
impl host::Preferences<WebLoop> for WebLoop {
    fn sample(owner: &WebLoop) -> Result<SystemPreferences, PlatformError> {
        if !owner.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner has stopped".into(),
            });
        }
        // Sample outside the borrow: the browser's answers must not observe
        // the host's own state held.
        let Some(source) = owner.preferences.borrow().clone() else {
            return Err(PlatformError::Preferences {
                message: "browser preference source is closed".into(),
            });
        };
        let preferences = source.sample()?;
        if !owner.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "browser preference owner stopped during sampling".into(),
            });
        }
        Ok(preferences)
    }
}

/// The lifecycle position is the loop's: the page reports the end of its
/// session (`WebHost::page`) and nothing else.
impl host::Lifecycle<WebLoop> for WebLoop {
    fn release_session_end(_: &WebLoop) {
        // A page cannot be asked to wait, so the handler's answer is never
        // read and no session end is held open.
    }

    fn session_end_support(_: &WebLoop) -> flui_platform::SessionEndSupport {
        flui_platform::SessionEndSupport::NotifyOnly
    }

    fn activate(_: &WebLoop, _: bool) {
        // A page cannot bring its tab to the front.
    }
}

/// The frame request an animation-frame callback with `timestamp` (milliseconds on the page's
/// high-resolution timeline) stands for: the vsync is the browser's own frame time, rebased by its
/// age against `performance.now()`. A timestamp the page's clock cannot place (no `performance`,
/// or one ahead of it) is the callback's own arrival.
fn frame_request_for_animation_frame(timestamp: f64) -> flui_platform::FrameRequest {
    let age_ms = web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now() - timestamp)
        .filter(|age| age.is_finite() && *age >= 0.0)
        .unwrap_or(0.0);
    crate::frame_request_aged(std::time::Duration::from_secs_f64(age_ms / 1000.0))
}
