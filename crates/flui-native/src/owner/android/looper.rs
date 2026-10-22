//! The Android host's loop.
//!
//! Native Android loop using the `android-activity` crate for NativeActivity
//! integration. It polls the Activity's events on the owner thread and drives
//! window management, event handling, and lifecycle support via
//! ANativeWindow + Vulkan.
//!
//! # Architecture
//!
//! ```text
//! android_main(AndroidApp)
//!   -> AndroidPlatform::new(app)
//!   -> Platform::run()  [poll_events loop]
//!     -> MainEvent::Start / Stop     -> visibility
//!     -> MainEvent::Resume / Pause   -> execution state (frame gate)
//!     -> MainEvent::Gained/LostFocus -> focus, nothing else
//!     -> MainEvent::InitWindow       -> on_ready(), create surface
//!     -> MainEvent::TerminateWindow  -> surface released
//!     -> MainEvent::LowMemory        -> memory pressure `Critical`
//!     -> Resume/Pause/Stop/LowMemory -> memory pressure from the trim level (JNI)
//!     -> MainEvent::Stop/SaveState   -> session end notice (once per start-to-stop cycle)
//!     -> MainEvent::Destroy          -> break loop
//!     -> each tick                   -> dispatch_request_frame()
//!   -> loop exit                     -> window released, callbacks cleared, quit hook
//! ```
//!
//! # Surface Lifecycle
//!
//! The native window (ANativeWindow) is valid between `InitWindow` and
//! `TerminateWindow`, and nowhere else: `AppCmd::InitWindow` is applied before
//! its callback runs, `AppCmd::TermWindow` is applied after its callback
//! returns, and `MainEvent::Pause` does not touch the window at all. A wgpu
//! surface built from a handle into that window therefore has to be gone
//! before the handle is, which is what the `false` edge of
//! [`PlatformWindow::on_surface_status_change`] asks the presentation to do.
//!
//! Both ends of the pair carry the signal: `Pause` and `TerminateWindow` emit
//! `false`, `Resume` and `InitWindow` emit `true`. `Pause` alone would be the
//! wrong place to stop, because the handle it would protect outlives it;
//! `TerminateWindow` is the event that actually ends the handle's life, so
//! dropping the surface inside that callback is the first moment the
//! surface's lifetime has a defined end at all. Emitting on `Pause` as well
//! moves the drop off it in the ordinary cycle, where the release is then an
//! idempotent no-op at `TerminateWindow`.
//!
//! The initial `on_ready` waits for the first `InitWindow`, not the first
//! `Resume`: `native_window()` is `None` until then, so a bootstrap that
//! needs a window has nothing to build a surface from before it.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use flui_platform::{MemoryPressureLevel, SessionEndPhase, SystemNotice, WindowExecutionState};

use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::owner::android::java_view;
use crate::owner::android::{ActivityPreferences, AndroidHost, HostDisplays};
use crate::owner::handlers::CallbackWindow;
use crate::platform::HostPlatform;
use crate::platforms::android::input;
use crate::{
    error::BootstrapError,
    error::PlatformError,
    owner::DirectOwnerHooks,
    owner::OwnerHooks,
    owner::OwnerPlatform,
    owner::android::activity_callbacks::ActivityCallbacks,
    owner::android::activity_callbacks::ActivityEvent,
    owner::android::activity_callbacks::ActivityTarget,
    owner::android::activity_callbacks::Reaction,
    owner::android::ndk::LoadedLibraries,
    owner::android::ndk::SymbolSource,
    owner::android::poll::LoopTurn,
    owner::android::poll::poll_timeout,
    owner::call_point::{Field, SharedField, call, install},
    owner::handlers::{PlatformHandlers, SessionEndHook},
    owner::notices::{LifecycleHooks, NoticeChannel},
    owner::redraw_poll::RedrawPoll,
    owner::signal::OwnerSignal,
    platform::Platform,
    platform::PlatformExecutor,
    platform::PlatformReadyCallback,
    shared::os_version::android::AndroidApi,
    shared::os_version::android::StylusHandwriting,
};

/// Whether the registered wake-deadline hook's answer means `run`'s loop
/// should WANT to force a frame dispatch this iteration, even though
/// nothing explicitly requested a redraw. Pulled out of `run`'s loop as its
/// own pure, free function — unlike the rest of that loop, which needs a
/// live `AndroidApp` (constructible only by the real Android runtime; no
/// test double exists for it, so `run` itself cannot be exercised in a unit
/// test on any host) — this one decision is entirely independent of that:
/// `None` (no hook installed, or the hook itself has nothing pending) is
/// never due; `Some(deadline)` is due once `now` has reached it.
///
/// This function is STATELESS and NOT self-limiting: called again with the
/// same `deadline` and a `now` that has not gone backwards, it answers
/// `true` again, and keeps answering `true` on every subsequent call until
/// either `deadline` itself advances/clears or `now` regresses (it never
/// does). It does not know, and cannot know, whether some earlier call's
/// `true` already caused a dispatch — it has no memory of ever being
/// called before. Something else has to be the actuator that stops asking:
/// either the deadline source's own consumer advances `deadline` in
/// response to acting on it (the desktop/Android device-recovery backoff
/// does this — see `DeviceRecoveryBackoff`'s two paired obligations), or
/// the CALLER stops treating "due" as "act now" once acting is impossible
/// (`run`'s loop does this by gating this answer's effect on `should_render`
/// on `resumed` — see that call site's own comment for the busy-spin this
/// prevents while backgrounded). Read this function's own `true` as "the
/// deadline has passed", never as "and therefore something happened".
fn is_deadline_due(deadline: Option<web_time::Instant>, now: web_time::Instant) -> bool {
    deadline.is_some_and(|deadline| now >= deadline)
}

/// The Activity's lifecycle as its callbacks reported it, shared by the
/// platform's event loop and the window it opened.
///
/// Three independent facts (see `lifecycle::android`): the Activity is on
/// screen (`onStart` to `onStop`), it may run (`onResume` to `onPause`), and its
/// window has input focus. None implies another: a dialog takes focus from a
/// running, visible Activity. All three are false until their first callback.
#[derive(Debug, Default)]
pub(crate) struct ActivityFlags {
    pub(crate) visible: AtomicBool,
    pub(crate) resumed: AtomicBool,
    pub(crate) focused: AtomicBool,
}

/// The Activity's window as the looper drives it: the window feature's, which
/// the loop holds without naming it (ADR-0151 §10). The looper hands it what
/// the Activity reports — a native window that comes or goes, a resize, a
/// configuration change, the Java view's events — and asks it for a frame.
pub(crate) trait ActivityWindow: CallbackWindow {
    /// Publishes the window's state if the Activity's changed it.
    fn publish_state(&self);

    /// Reads the input capabilities again (the Java view became ready).
    fn refresh_capabilities(&self);

    /// The outgoing drag the window started ended; `result` is whether a
    /// target took it.
    fn drag_ended(&self, result: bool);

    /// Asks the Java view for the system-bar and cutout insets.
    fn request_safe_area(&self);

    /// Delivers the insets the Java view answered with.
    fn settle_safe_area(&self);

    /// Delivers the night mode the configuration reports.
    fn settle_appearance(&self);

    /// The text sizing captured so far is stale.
    fn invalidate_text_sizing(&self);

    /// No text sizing is captured until the window has a surface again.
    fn fence_text_sizing(&self);

    /// The native window is gone: its geometry is no longer read.
    fn revoke_geometry(&self);

    /// The Java view is retired with the window it served.
    fn retire_java_view(&self);

    /// Sends the system the refresh-rate request the window last made.
    fn apply_frame_rate_hint(&self);

    /// Whether a frame was asked for.
    fn has_redraw_request(&self) -> bool;

    /// Takes the frame request when the window may run.
    fn take_deliverable_redraw_request(&self) -> bool;
}

/// The Android host's loop, over `android-activity`: the state every
/// position of its table reads.
///
/// Wraps the `AndroidApp` provided by `android_main()`.
/// [`AndroidPlatform`](crate::AndroidPlatform) is its platform.
pub struct AndroidLoop {
    pub(crate) app: AndroidApp,
    handlers: Arc<Mutex<PlatformHandlers>>,
    running: Arc<AtomicBool>,
    pub(crate) activity: Arc<ActivityFlags>,
    /// What each of the Activity's lifecycle callbacks means, as the table
    /// made it ([`AndroidHost::activity`]).
    activity_callbacks: Box<dyn ActivityCallbacks>,
    /// Where the trim level and the low-memory call hand system notices.
    notices: Arc<dyn NoticeChannel>,
    pub(crate) window: Arc<Mutex<Option<Arc<dyn ActivityWindow>>>>,
    background_executor: Arc<SimpleExecutor>,
    pub(crate) clipboard: Arc<dyn flui_platform::Clipboard>,
    pub(crate) input_state: Mutex<input::AndroidInputState>,
    pub(crate) owner_signal: Arc<OwnerSignal>,
    scroll_factors: Mutex<crate::shared::android_scroll::FactorCache>,
    /// The system preference source the table made
    /// ([`AndroidHost::preferences`]).
    preferences: Box<dyn ActivityPreferences>,
    /// `FluiView`, installed on the Java main thread at start (ADR-0247).
    pub(crate) java_view: Arc<java_view::JavaView>,
    /// The displays, as the table made them; shared with the window.
    pub(crate) displays: Arc<dyn HostDisplays>,
    pub(crate) api: AndroidApi,
}

// Opaque on purpose, matching `HeadlessLoop` and `WinitLoop`. A
// derived `Debug` here would format the clipboard, whose contents are the
// user's, into whatever sink formatted the platform — a log line, a panic
// message, a bug report. None of the remaining fields are useful to a reader
// either: handles, an executor, and a capability set.
impl std::fmt::Debug for AndroidLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidLoop").finish_non_exhaustive()
    }
}

impl AndroidLoop {
    /// One of the application's handlers, as a slot of the handler set.
    pub(crate) fn handler<H>(
        &self,
        project: fn(&mut PlatformHandlers) -> &mut Option<H>,
    ) -> Field<'_, Mutex<PlatformHandlers>, H> {
        Field::new(&*self.handlers, project)
    }

    /// Run the application's quit handler through the call point, with no
    /// lock of the handler set held: it may install another handler or call
    /// back into the platform.
    fn invoke_quit(&self) {
        call(&self.handler(|handlers| &mut handlers.quit), |quit| quit()).or_resume(());
    }

    /// The loop of host `H`, whose table makes the displays, the preference
    /// source, the notice channel, the meaning of the Activity's callbacks, the
    /// clipboard and the drag-and-drop transport, and names what `FluiView`
    /// reports to.
    pub(crate) fn new<H: AndroidHost>(app: AndroidApp) -> Self {
        let waker = app.create_waker();
        let owner_signal = OwnerSignal::new(Arc::new(move || {
            waker.wake();
            Ok(())
        }));
        // Opened once and kept for the platform's life: every function pointer
        // looked up in it is valid until the last handle to it is gone.
        let library: Arc<dyn SymbolSource> = Arc::new(LoadedLibraries::open());
        let api = AndroidApi::detect(library.as_ref()).unwrap_or_else(|| {
            tracing::warn!(
                "Android: the API level could not be read; nothing above the floor is attempted"
            );
            AndroidApi::UNKNOWN
        });
        tracing::info!(api = %api, "Android API level");
        let java_view = java_view::JavaView::start(
            app.clone(),
            Arc::downgrade(&owner_signal),
            api,
            H::input_method(),
            H::transfer,
        );
        let owner = Arc::downgrade(&owner_signal);
        let preferences = H::preferences(&app, api, &owner_signal);
        let activity_callbacks = H::activity(&app);
        Self {
            java_view,
            app,
            handlers: Arc::new(Mutex::new(PlatformHandlers::new())),
            running: Arc::new(AtomicBool::new(true)),
            activity: Arc::new(ActivityFlags::default()),
            activity_callbacks,
            notices: H::notices(),
            window: Arc::new(Mutex::new(None)),
            background_executor: Arc::new(SimpleExecutor),
            clipboard: H::clipboard(),
            input_state: Mutex::new(input::AndroidInputState::default()),
            owner_signal,
            scroll_factors: Mutex::new(crate::shared::android_scroll::FactorCache::default()),
            preferences,
            displays: H::displays(api, library, owner),
            api,
        }
    }

    /// Record one fact of the Activity's lifecycle and tell the window: its
    /// state is published first, so an observer reads the new value.
    fn publish_activity_fact(
        &self,
        record: impl FnOnce(&ActivityFlags),
        notify: impl FnOnce(&dyn ActivityWindow),
    ) {
        record(&self.activity);
        let window = self.window.lock().clone();
        if let Some(window) = window {
            window.publish_state();
            notify(window.as_ref());
        }
    }

    /// Read the system preferences on the live owner thread, through the source
    /// the table made.
    fn refresh_preferences(&self) -> Result<flui_platform::SystemPreferences, PlatformError> {
        if !self.owner_signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "the Android preference owner has stopped".into(),
            });
        }
        if self.owner_signal.owner() != std::thread::current().id() {
            return Err(PlatformError::Preferences {
                message: "Android preferences require the activity owner thread".into(),
            });
        }
        self.preferences.sample()
    }

    /// When the loop reads preferences, scroll factors and insets next: the
    /// cadence while `FluiView` does not observe, or the retry a failed read
    /// owes, whichever comes first. Nothing else would come back for that
    /// retry: a change reported during its backoff is deferred, not queued.
    fn next_sample(&self, cadence: Option<web_time::Instant>) -> Option<web_time::Instant> {
        [cadence, self.preferences.retry_deadline()]
            .into_iter()
            .flatten()
            .min()
    }

    fn sample_due(&self, cadence: Option<web_time::Instant>, now: web_time::Instant) -> bool {
        self.next_sample(cadence).is_some_and(|at| now >= at)
    }

    fn refresh_scroll_factors(&self) {
        // Query outside the owner-local cache guard. A failed refresh keeps
        // accepted logical factors; only the never-observed case uses authored
        // line normalization for compatibility with supported API21 devices.
        let reading = input::scroll_factors(&self.app, self.api).ok();
        self.scroll_factors.lock().observe(reading);
    }

    fn cancel_input_contacts(&self, reason: crate::shared::pointer_assembly::NativeCancel) {
        let events = self.input_state.lock().cancel_contacts(reason);
        let window = self.window.lock().clone();
        if let Some(window) = window {
            for event in events {
                window.callbacks().dispatch_input(event);
            }
        }
    }

    /// The loop ended: the window's field and drag are detached, and the view
    /// stops calling native code and leaves the Activity.
    fn retire_java_view(&self) {
        let retiring = self.window.lock().clone();
        if let Some(retiring) = retiring {
            retiring.retire_java_view();
        }
        self.java_view.detach();
    }

    /// Deliver what `FluiView` reported since the last turn, in order, on the
    /// owner thread (ADR-0247).
    fn deliver_view_events(&self) {
        let events = self.java_view.take_events();
        if events.is_empty() {
            return;
        }
        let window = self.window.lock().clone();
        for event in events {
            match event {
                java_view::ViewEvent::Installed => {
                    if let Some(window) = &window {
                        window.refresh_capabilities();
                    }
                }
                java_view::ViewEvent::RunOnWindow(work) => {
                    if let Some(window) = &window {
                        work(&**window);
                    }
                }
                java_view::ViewEvent::Drag(event) => match &window {
                    Some(window) => {
                        window
                            .callbacks()
                            .dispatch_input(flui_platform::PlatformInput::DragDrop(event));
                        window.request_redraw();
                    }
                    None => {
                        if let flui_platform::DragDropEvent::Dropped { id, .. } = event {
                            self.java_view.transfer().conclude_drop(id);
                        }
                    }
                },
                java_view::ViewEvent::DragEnded { result } => {
                    if let Some(window) = &window {
                        window.drag_ended(result);
                    }
                }
                java_view::ViewEvent::SettingsChanged => {
                    self.refresh_scroll_factors();
                    if let Err(error) = self.refresh_preferences() {
                        tracing::warn!(%error, "Android preference query failed");
                    }
                }
                java_view::ViewEvent::InsetsChanged => {
                    if let Some(window) = &window {
                        window.request_safe_area();
                    }
                }
            }
        }
    }

    /// A pen tip that touches down is offered to the input method, whose
    /// handwriting takes a stroke inside the focused field (API 33+):
    /// Android's own initiation sees only the stylus events native code
    /// leaves unhandled.
    fn offer_handwriting(
        &self,
        window: &dyn ActivityWindow,
        events: &[flui_platform::PlatformInput],
    ) {
        use flui_platform::pointer::{PenTool, PointerEvent, PointerKind};
        if self.api.since::<StylusHandwriting>().is_none() {
            return;
        }
        for event in events {
            let flui_platform::PlatformInput::Pointer(PointerEvent::Down(press)) = event else {
                continue;
            };
            if press.pointer.kind == (PointerKind::Pen { tool: PenTool::Tip }) {
                self.java_view
                    .input_method()
                    .pen_down(window, press.sample.position.get());
            }
        }
    }

    /// Process pending input events from the Android event queue.
    ///
    /// Drains all buffered input events via `input_events_iter()` and
    /// dispatches them through the window's callbacks as `PlatformInput`.
    /// The first `MotionEvent` of the process is logged at `info`
    /// (`FIRST_MOTION_SEEN`), every later one at `debug`.
    fn process_input_events(&self) {
        /// Whether any `MotionEvent` has reached this function in this
        /// process.
        static FIRST_MOTION_SEEN: AtomicBool = AtomicBool::new(false);

        let window_guard = self.window.lock();
        let Some(window) = window_guard.as_ref().cloned() else {
            // No window yet — still drain events to prevent ANR
            drop(window_guard);
            if let Ok(mut iter) = self.app.input_events_iter() {
                while iter.next(|_event| InputStatus::Unhandled) {}
            }
            return;
        };

        drop(window_guard);

        let scale_factor = window.scale_factor();
        let scroll_policy = self.scroll_factors.lock().policy();
        let callbacks = window.callbacks();

        match self.app.input_events_iter() {
            Ok(mut iter) => loop {
                let read = iter.next(|event| {
                    use android_activity::input::InputEvent;

                    let handled = match event {
                        InputEvent::MotionEvent(motion) => {
                            let cached = self.input_state.lock().capabilities(motion.device_id());
                            let device = input::motion_device(&self.app, motion, cached);
                            let events = self.input_state.lock().convert_motion_event(
                                motion,
                                scale_factor,
                                scroll_policy,
                                device,
                            );
                            // The one place a touch is visible between the
                            // OS and the framework: a tap that changes
                            // nothing on screen is diagnosed from these
                            // lines (present → the framework's hit test or
                            // arena; absent → the queue never delivered
                            // it). The FIRST motion event of the process is
                            // an `info` line — a lifecycle fact like "first
                            // frame rendered", visible under the default
                            // `info` filter — and every one after it is
                            // `debug`, so a drag does not flood logcat.
                            let first = !FIRST_MOTION_SEEN.swap(true, Ordering::Relaxed);
                            let pointer = motion.pointer_at_index(0);
                            // One field list, two levels: `tracing`'s
                            // callsite level must be a constant, so the
                            // level is pasted by a local macro rather than
                            // chosen at runtime.
                            macro_rules! motion_event {
                                ($level:expr) => {
                                    tracing::event!(
                                        target: "flui_native::android::input",
                                        $level,
                                        action = ?motion.action(),
                                        pointers = motion.pointer_count(),
                                        converted = events.len(),
                                        x = pointer.x(),
                                        y = pointer.y(),
                                        scale_factor,
                                        first,
                                        "motion event reached the input queue"
                                    )
                                };
                            }
                            if first {
                                motion_event!(tracing::Level::INFO);
                            } else {
                                motion_event!(tracing::Level::DEBUG);
                            }
                            let mut any_handled = false;
                            self.offer_handwriting(window.as_ref(), &events);
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
                            if let Some(platform_input) =
                                input::convert_key_event(&self.app, key, self.java_view.composing())
                            {
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
        // A hover exit no press continued in this drain was a leave.
        let held = self.input_state.lock().finish_drain();
        for event in held {
            callbacks.dispatch_input(event);
        }
    }
}

impl<H: Host<Loop = AndroidLoop>> HostPlatform<H> {
    /// Get the underlying `AndroidApp`
    pub fn app(&self) -> &AndroidApp {
        &self.owner().app
    }
}

impl ActivityTarget for AndroidLoop {
    fn visibility_changed(&self, visible: bool) {
        self.publish_activity_fact(
            |flags| flags.visible.store(visible, Ordering::SeqCst),
            |window| {
                window
                    .callbacks()
                    .dispatch_visibility_status_change(visible);
            },
        );
    }

    fn execution_changed(&self, state: WindowExecutionState) {
        self.publish_activity_fact(
            |flags| {
                flags
                    .resumed
                    .store(state == WindowExecutionState::Running, Ordering::SeqCst);
            },
            |window| {
                window.callbacks().dispatch_execution_state_change(state);
            },
        );
    }

    fn focus_changed(&self, focused: bool) {
        self.publish_activity_fact(
            |flags| flags.focused.store(focused, Ordering::SeqCst),
            |window| {
                window.callbacks().dispatch_active_status_change(focused);
            },
        );
    }

    /// The system does not wait for an answer, so this is
    /// [`SessionEndPhase::Ending`] only: no query, no block.
    fn session_ended(&self) {
        // The system does not wait for an answer: there is nothing to veto.
        let _answer = call(
            &self.handler(|handlers| &mut handlers.session_end),
            |hook| hook(SessionEndPhase::Ending),
        )
        .or_resume(flui_platform::SessionEndAnswer::Proceed);
    }

    fn memory_pressure_raised(&self, level: MemoryPressureLevel) {
        self.notices.dispatch(SystemNotice::MemoryPressure(level));
    }
}

impl host::Loop for AndroidLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        on_ready: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        tracing::info!("Starting Android platform event loop");

        self.owner_signal.bind_owner();
        self.owner_signal
            .start()
            .map_err(|error| PlatformError::Preferences {
                message: error.to_string(),
            })?;
        // When preferences, scroll factors and insets are read next; `None`
        // waits for a change to be reported.
        let mut next_preference_sample = Some(web_time::Instant::now());
        // The choreographer is the loop thread's own: register here, take down
        // at the loop's exit.
        let refresh_rate = self.displays.observe();

        let mut on_ready = Some(on_ready);
        self.activity.resumed.store(false, Ordering::SeqCst);
        // Set when `on_ready` returns `Err`: a fallible bootstrap
        // failure (window creation, GPU init, root-widget attach) has no
        // other return path back to `run`'s caller. Stopping `running` and
        // `continue`-ing to the loop's top-of-iteration check exits
        // promptly instead of continuing to pump input/frame dispatch for
        // an app that never finished bootstrapping.
        let mut bootstrap_error: Option<BootstrapError> = None;
        // Relaxed everywhere on `running`: it is a bare stop-signal with no
        // data published through it. The loop re-reads it every iteration,
        // the Destroy store happens on the loop thread itself, and a
        // cross-thread `quit()` only needs eventual visibility.
        self.running.store(true, Ordering::Relaxed);

        loop {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            // `FluiView` reports setting and inset changes, and configuration
            // changes arrive as `ConfigChanged`; each reads them again. A
            // resumed Activity reads them once more on its first turn and on
            // `Resume`. Until the view is in the window, or if it never gets
            // there, a 500ms cadence while resumed stands in, so no change is
            // lost; a paused Activity reads them again on `Resume`.
            let now = web_time::Instant::now();
            let window = self.window.lock().clone();
            if let Some(window) = window.as_ref() {
                window.settle_safe_area();
            }
            if self.activity.resumed.load(Ordering::SeqCst)
                && self.sample_due(next_preference_sample, now)
            {
                // Taken before the read: a view that becomes ready during it
                // reports once it observes, and only a view that was ready
                // already ends the cadence.
                let observed = self.java_view.is_ready();
                if let Some(window) = window.as_ref() {
                    window.request_safe_area();
                }
                self.refresh_scroll_factors();
                if let Err(error) = self.refresh_preferences() {
                    tracing::warn!(%error, "Android preference query failed");
                }
                next_preference_sample = (!observed).then(|| now + Duration::from_millis(500));
            }
            if self.owner_signal.drive() {
                self.running.store(false, Ordering::Relaxed);
                continue;
            }

            // Wall-clock wake (mirrors the winit backend's `about_to_wait`):
            // consult the registered wake-deadline hook fresh on EVERY
            // iteration -- a deadline that fires, a new one getting armed,
            // or every deadline clearing must all be reflected immediately,
            // not stale from whenever the hook was installed. Lock
            // discipline (ADR-0038 §5, the same rule winit's own
            // `about_to_wait` follows): the hook itself re-enters
            // `flui-app`, so it is called through the call point, never
            // while `self.handlers` is held.
            let wake_deadline = call(
                &SharedField::new(&*self.handlers, |handlers| &mut handlers.wake_deadline),
                |hook| hook(),
            )
            .or_resume(None);
            let deadline_due = is_deadline_due(wake_deadline, web_time::Instant::now());
            if deadline_due {
                // Owner services remain deliverable while execution gates frames.
                let _ = self.owner_signal.wake();
                if self.owner_signal.drive() {
                    self.running.store(false, Ordering::Relaxed);
                    continue;
                }
            }

            // Check if we should render before polling. Pending sources are
            // gated on `resumed`, and so is a deadline that is already behind
            // (`poll_timeout`): the backgrounded app cannot consume it, and
            // either would turn the poll into a 0ms busy loop. The redraw is
            // observed without consumption because `poll_events` may suspend
            // execution before the callback can be delivered.
            let redraw_requested = self
                .window
                .lock()
                .as_ref()
                .is_some_and(|w| w.has_redraw_request());
            let resumed = self.activity.resumed.load(Ordering::SeqCst);
            let mut should_call_ready = false;
            let ((), redraw_after_poll) =
                RedrawPoll::new(deadline_due, redraw_requested).poll(resumed, |should_render| {
                    let timeout = poll_timeout(
                        LoopTurn {
                            should_render,
                            resumed,
                            wake_deadline,
                            next_preference_sample: self.next_sample(next_preference_sample),
                        },
                        web_time::Instant::now(),
                    );
                    self.app.poll_events(timeout, |event| {
                if let PollEvent::Main(main_event) = event {
                    // What the Activity callback means, as the table's hook
                    // answers. An arm that must act around its signal takes and
                    // delivers it itself; what is left of the reaction (the signal
                    // of every other callback, then the memory pressure) is
                    // delivered after the match.
                    let mut reaction = ActivityEvent::from_main_event(&main_event)
                        .map_or_else(Reaction::default, |event| {
                            self.activity_callbacks.react(event)
                        });
                    match main_event {
                        // Visibility, focus gained and the end of the session
                        // have nothing to do besides delivering their signal.
                        MainEvent::Start
                        | MainEvent::Stop
                        | MainEvent::GainedFocus
                        | MainEvent::SaveState { .. } => {
                            tracing::debug!(event = ?main_event, "Android: lifecycle callback");
                        }
                        MainEvent::Resume { .. } => {
                            tracing::info!(
                                "Android: Resumed — lifecycle transition, native window untouched"
                            );
                            // The Activity may have come back on another display.
                            self.displays.invalidate();
                            // Nothing was sampled while paused: read the
                            // preferences and insets again on the next turn.
                            next_preference_sample = Some(web_time::Instant::now());

                            // The execution state first, then the surface: a
                            // presentation rebuilt on `true` below already runs.
                            // The surface signal rides along: a `Resume` with
                            // no window yet (the startup order) or with the
                            // previous surface still released asks for one, and
                            // a `true` that finds a live surface simply
                            // replaces it — see
                            // `PlatformWindow::on_surface_status_change`.
                            if let Some(signal) = reaction.signal.take() {
                                signal.deliver(&*self);
                            }
                            // The window is cloned out of its lock: the surface
                            // observer runs embedder code that may call back
                            // into the platform.
                            let window = self.window.lock().clone();
                            if let Some(w) = window {
                                w.callbacks().dispatch_surface_status_change(true);
                                w.request_redraw();
                            }
                        }
                        MainEvent::Pause => {
                            tracing::info!(
                                "Android: Paused — releasing the surface; the native window \
                                 outlives this"
                            );
                            // Publish suspension before invoking embedder callbacks: a nested
                            // continuation request must not acknowledge a redraw opportunity
                            // that this loop will refuse to dispatch.
                            self.activity.resumed.store(false, Ordering::SeqCst);
                            self.cancel_input_contacts(crate::shared::pointer_assembly::NativeCancel::FocusLost);

                            // Release BEFORE suspending: the drop is the one
                            // step here with a validity window behind it, and
                            // the execution observer runs arbitrary embedder
                            // code.
                            let window = self.window.lock().clone();
                            if let Some(w) = window {
                                w.callbacks().dispatch_surface_status_change(false);
                            }
                            if let Some(signal) = reaction.signal.take() {
                                signal.deliver(&*self);
                            }
                        }
                        MainEvent::InitWindow { .. } => {
                            tracing::debug!(
                                "Android: InitWindow — a new native window is ready, \
                                 requesting a surface"
                            );

                            // The bootstrap waits for the WINDOW, not for the
                            // resume: `native_window()` is `None` until this
                            // event, so an `on_ready` keyed on `Resume` would
                            // build its surface from nothing. On the first
                            // `InitWindow` no window exists yet, so the
                            // dispatch below finds no registrant — the
                            // bootstrap's own acquire is that acquire.
                            if on_ready.is_some() {
                                should_call_ready = true;
                            }

                            let window = self.window.lock().clone();
                            if let Some(w) = window {
                                w.invalidate_text_sizing();
                                // The new native window may have a size the
                                // old one never had (rotated while in the
                                // background), and no `WindowResized` has to
                                // follow it. Published before the surface is
                                // asked for, so the recreated surface is
                                // configured at this size, not the old one.
                                w.publish_state();
                                w.callbacks().dispatch_logical_resize(w.logical_size(), w.scale_factor());
                                w.request_safe_area();
                                // The request rode on the window that is gone. The
                                // host sends its demand again on
                                // `PrimarySurfaceRestored` as well: this one covers
                                // a window that is replaced without the host
                                // noticing, that one covers hosts whose windows
                                // do not remember. Sending twice is idempotent.
                                w.apply_frame_rate_hint();
                                w.callbacks().dispatch_surface_status_change(true);
                            }
                        }
                        MainEvent::TerminateWindow { .. } => {
                            self.cancel_input_contacts(crate::shared::pointer_assembly::NativeCancel::CaptureLost);
                            tracing::debug!(
                                "Android: TerminateWindow — the native window is going away, \
                                 releasing the surface"
                            );

                            // The last moment the handle behind a surface is
                            // still valid: `AppCmd::TermWindow` is applied
                            // after this callback returns, so a surface that
                            // outlives it does too.
                            let window = self.window.lock().clone();
                            if let Some(w) = window {
                                w.invalidate_text_sizing();
                                w.callbacks().dispatch_surface_status_change(false);
                            }
                        }
                        MainEvent::Destroy => {
                            self.cancel_input_contacts(crate::shared::pointer_assembly::NativeCancel::FocusLost);
                            tracing::info!("Android: Destroy — shutting down");
                            self.activity.resumed.store(false, Ordering::SeqCst);

                            // A window that never arrived leaves `on_ready`
                            // untaken, and exiting clean here would report a
                            // presentation that never started as success.
                            if on_ready.is_some() {
                                bootstrap_error = Some(
                                    "Android: the event loop shut down before any \
                                     MainEvent::InitWindow, so the presentation was never \
                                     bootstrapped"
                                        .into(),
                                );
                            }

                            // Dispatch close before stopping
                            let window = self.window.lock().clone();
                            if let Some(w) = window {
                                w.revoke_geometry();
                                w.callbacks().dispatch_close();
                            }

                            self.running.store(false, Ordering::Relaxed);
                        }
                        MainEvent::WindowResized { .. } => {
                            tracing::info!("Android: Window resized");
                            // Cloned out of its lock: the resize observer runs
                            // embedder code that may call back into the platform.
                            let window = self.window.lock().clone();
                            if let Some(w) = window {
                                w.publish_state();
                                w.callbacks().dispatch_logical_resize(w.logical_size(), w.scale_factor());
                                // The insets of the old size no longer apply.
                                w.request_safe_area();
                                w.request_redraw();
                            }
                        }
                        MainEvent::LostFocus => {
                            self.cancel_input_contacts(crate::shared::pointer_assembly::NativeCancel::FocusLost);
                            tracing::debug!("Android: Lost focus");
                        }
                        MainEvent::ConfigChanged { .. } => {
                            tracing::debug!("Android: Config changed");
                            self.displays.invalidate();
                            let window = self.window.lock().clone();
                            if let Some(window) = window {
                                window.invalidate_text_sizing();
                                window.settle_appearance();
                            }
                            self.refresh_scroll_factors();
                            if let Err(error) = self.refresh_preferences() {
                                tracing::warn!(%error, "Android configuration query failed");
                            }
                            if !self.java_view.is_ready() {
                                next_preference_sample = Some(
                                    web_time::Instant::now() + Duration::from_millis(500),
                                );
                            }
                            let window = self.window.lock().clone();
                            if let Some(window) = window {
                                window.publish_state();
                                window.callbacks().dispatch_logical_resize(window.logical_size(), window.scale_factor());
                                window.request_safe_area();
                            }
                        }
                        MainEvent::LowMemory => {
                            tracing::warn!("Android: Low memory warning");
                        }
                        _ => {}
                    }
                    reaction.deliver(&*self);
                }
                    });
                });

            // Call on_ready outside of poll_events (FnOnce can't be called in
            // closure). Fires once, at the first `MainEvent::InitWindow` — the
            // module doc's `InitWindow -> on_ready() -> create surface`
            // sequence (ADR-0039 §1: the pre-run bootstrap that used to
            // run before this loop started now runs from here, in `flui-app`'s
            // `on_ready` migration). The `InitWindow` arm is what makes that
            // acquire legal: it is the first event at which
            // `AndroidApp::native_window()` returns a window, and `Resume`
            // precedes it at startup. No owner lane on this backend: every
            // `OwnerPlatform::open_window` call creates directly and is
            // always `Ready`.
            if should_call_ready && let Some(ready) = on_ready.take() {
                let owner_platform = Arc::clone(&platform);
                let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
                    Arc::clone(&owner_platform),
                    Arc::clone(&self.owner_signal),
                ));
                if let Err(error) = ready(OwnerPlatform::new(owner_platform, hooks)) {
                    tracing::error!(%error, "on_ready bootstrap failed; stopping the event loop");
                    bootstrap_error = Some(error);
                    self.running.store(false, Ordering::Relaxed);
                    // Skip input/frame dispatch below for this iteration --
                    // the top-of-loop check exits on the next pass.
                    continue;
                }
            }

            // Process input events (touch, key) and dispatch through callbacks
            // Install accepted preference/configuration changes before the
            // first input packet translated in the new context.
            if self.owner_signal.drive() {
                self.running.store(false, Ordering::Relaxed);
                continue;
            }
            let window = self.window.lock().clone();
            if let Some(window) = window {
                self.java_view.set_scale(window.scale_factor());
            }
            self.deliver_view_events();
            let resumed = self.activity.resumed.load(Ordering::SeqCst);
            if resumed {
                self.process_input_events();
            }

            // Consume the redraw only after polling confirms this turn can
            // deliver it. A queued Pause therefore preserves the request for
            // Resume rather than stranding an acknowledged continuation. The
            // window is cloned out of its lock: the frame runs embedder code
            // that may call back into the platform.
            let window = self.window.lock().clone();
            if let Some(ref w) = window
                && redraw_after_poll
                    .take_if_deliverable(resumed, || w.take_deliverable_redraw_request())
            {
                w.callbacks().dispatch_request_frame();
            }
        }

        self.activity.resumed.store(false, Ordering::SeqCst);
        // Unregister the choreographer callback on the thread it was registered
        // on, before its data can be freed with the loop.
        drop(refresh_rate);
        self.owner_signal.close();

        // The loop's one exit, reached by its three returning routes: a
        // `MainEvent::Destroy`, a `quit()` from any thread, a bootstrap
        // that returned `Err`. A panic that unwinds out of this function
        // skips it, by decision (ADR-0063 decision 5): a clear during
        // unwind would drop a configured surface on a device that may be
        // the panic's cause, and a second panic there aborts the process
        // instead of letting `android-activity`'s `catch_unwind` finish the
        // activity. This is where the window this backend tracked is
        // released, in the order the winit and headless close bodies use:
        // the platform's own reference first, then the callback slots.
        //
        // The slots are the platform's only owning path into the embedder's
        // presentation. `flui-app` registers a frame callback and a
        // surface-status callback that own the raster lane, which owns the
        // renderer, whose surface lease holds an `Arc` of this window
        // (ADR-0063 decision 5): window -> slot -> closure -> lane ->
        // renderer -> `Arc<AndroidWindow>`. Only a clear breaks that cycle;
        // without it every activity recreation strands one window, lane and
        // renderer for the process's life.
        //
        // After the loop rather than in the `Destroy` arm, because `Destroy`
        // is not the only way out: a `quit()` returns from `android_main`,
        // and `android-activity` then finishes the activity without ever
        // delivering `Destroy` here. Before `invoke_quit`, so a quit hook
        // that panics still leaves the cycle broken. Outside the `window`
        // lock, because it drops embedder closures (ADR-0038 §5); the
        // `take()` is its own statement so its guard is gone before the
        // clear runs. At the top level of the window's FIFO, because no
        // `poll_events` callback is on the stack here, so no lease can
        // restore what it takes (issue #919).
        //
        // The surface is safe on every returning route. On `Destroy` it is
        // already gone: `NativeActivity.onDestroy` destroys the surface
        // before it unloads the native code, and `android-activity`'s
        // `set_window(None)` parks the JVM thread until `TermWindow` has
        // been applied, so the `TerminateWindow` arm above released it
        // before `Destroy` was even written. On a `quit()` the native
        // window is still live, so the surface dies here, before it, which
        // is the order issue #713 requires.
        //
        // No registration can land on the cleared set afterwards: `on_ready`
        // is `FnOnce`, so the bootstrap runs once per platform, and a
        // recreated activity gets a new `android_main`, a new `AndroidApp`,
        // a new platform and a new window (`ANativeActivity_onCreate`
        // spawns one thread per activity).
        self.retire_java_view();
        let window = self.window.lock().take();
        if let Some(window) = window {
            window.revoke_geometry();
            window.callbacks().clear();
            tracing::debug!("Android: loop exited; window released and its callbacks cleared");
        }

        // The quit handler runs with no lock held: it may install another
        // handler or call back into the platform.
        self.invoke_quit();
        tracing::info!("Android platform event loop finished");

        if let Some(source) = bootstrap_error {
            return Err(PlatformError::bootstrap(source));
        }
        Ok(())
    }

    fn quit(&self) {
        tracing::info!("Android: quit requested");
        let window = self.window.lock().clone();
        if let Some(window) = window {
            // A foreign quit fences leases immediately; native retirement stays
            // on the app owner when its loop exits.
            window.fence_text_sizing();
        }
        self.activity.resumed.store(false, Ordering::SeqCst);
        self.running.store(false, Ordering::Relaxed);
        let _ = self.owner_signal.request_quit();
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        install(&self.handler(|handlers| &mut handlers.quit), callback);
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        // The Activity lifecycle decides when the loop ends; there is no exit
        // policy to consult.
        drop(hook);
    }

    fn request_exit_policy_reevaluation(&self) {
        // No exit policy is consulted, so there is nothing to re-evaluate.
    }

    /// Unlike `winit`'s override (the trait's one other live implementor),
    /// this backend has no `ControlFlow::WaitUntil` to hand the deadline
    /// to — `run`'s own loop actuates it directly instead, by checking
    /// `now >= deadline` once per iteration and forcing a dispatch when due
    /// (see that loop's own comment). Storage is the identical
    /// `PlatformHandlers::wake_deadline` slot winit uses, for the same
    /// call-a-clone-outside-the-lock consultation discipline (ADR-0038 §5).
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        install(
            &self.handler(|handlers| &mut handlers.wake_deadline),
            Arc::from(hook),
        );
    }
}

impl host::Services for AndroidLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.background_executor.clone()
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        Ok(PathBuf::from("/data/local/tmp"))
    }

    // The Android host opens no URL, path or file prompt and reports no
    // compositor: those services move to the system capability crate.
    fn open_url(&self, url: &str) {
        let _ = url;
    }

    fn reveal_path(&self, path: &std::path::Path) {
        let _ = path;
    }

    fn open_path(&self, path: &std::path::Path) {
        let _ = path;
    }

    fn prompt_for_paths(
        &self,
        options: crate::PathPromptOptions,
    ) -> crate::Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        let _ = options;
        crate::Task::ready(Ok(None))
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::Task<Result<Option<PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        crate::Task::ready(Ok(None))
    }

    fn compositor_name(&self) -> &'static str {
        ""
    }
}

/// The displays position is the loop's: it answers from the displays the table
/// made ([`AndroidHost::displays`]).
impl host::Displays<AndroidLoop> for AndroidLoop {
    fn snapshot(owner: &AndroidLoop) -> Result<flui_platform::DisplaySnapshot, PlatformError> {
        let snapshot = owner.displays.snapshot(&owner.app)?;
        if owner.displays.take_max_rate_change() {
            // A `High` hint skipped while the display's highest rate was
            // unknown (or sent with an older one) is asked again.
            let window = owner.window.lock().clone();
            if let Some(window) = window {
                window.apply_frame_rate_hint();
            }
        }
        Ok(snapshot)
    }
}

impl host::Preferences<AndroidLoop> for AndroidLoop {
    fn sample(owner: &AndroidLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
        owner.refresh_preferences()
    }
}

/// The clipboard position: the clipboard the host's
/// [`AndroidHost::clipboard`] made, which the loop holds.
impl host::Clipboard<AndroidLoop> for AndroidLoop {
    fn clipboard(owner: &AndroidLoop) -> Arc<dyn flui_platform::Clipboard> {
        Arc::clone(&owner.clipboard)
    }
}

/// The drag position: `FluiView`'s drag and drop (ADR-0211, ADR-0247), the
/// transport the host's [`AndroidHost::transfer`] made for the view.
impl host::Drag<AndroidLoop> for AndroidLoop {
    fn data_transfer(
        owner: &AndroidLoop,
    ) -> Arc<dyn flui_platform::data_transfer::DataTransferSource> {
        Arc::clone(owner.java_view.transfer())
            as Arc<dyn flui_platform::data_transfer::DataTransferSource>
    }
}

/// The Activity's callbacks arrive at the looper: a stop or a saved state ends
/// the session, the system does not wait for an answer, and it starts or
/// recreates an Activity itself rather than reopening one. The lifecycle
/// feature's position reaches the hooks through this.
impl LifecycleHooks for AndroidLoop {
    fn notices(&self) -> &dyn NoticeChannel {
        &*self.notices
    }

    fn set_session_end_hook(&self, hook: SessionEndHook) {
        install(&self.handler(|handlers| &mut handlers.session_end), hook);
    }

    fn release_session_end(&self) {
        // The system does not wait for an answer to a session end, so no end
        // is held open.
    }

    fn set_reopen_hook(&self, hook: Box<dyn FnMut() + Send>) {
        // Android has no reopen signal.
        drop(hook);
    }
}

impl host::Storage<AndroidLoop> for AndroidLoop {
    /// The app's private `files` directory, which Auto Backup copies, for
    /// data that follows the user; its sibling `no_backup`, which Android
    /// never backs up, for state that belongs to this device.
    fn data_dirs(
        owner: &AndroidLoop,
        app: &flui_platform::StorageName,
    ) -> Option<crate::storage::DataDirs> {
        let files = owner.app.internal_data_path()?;
        let no_backup = files.parent()?.join("no_backup");
        Some(crate::storage::DataDirs {
            roaming: files.join(app.as_str()),
            local: no_backup.join(app.as_str()),
        })
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
