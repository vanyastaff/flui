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
//!     -> MainEvent::Resume           -> resumed = true
//!     -> MainEvent::InitWindow       -> on_ready(), create surface
//!     -> MainEvent::TerminateWindow  -> surface released
//!     -> MainEvent::Pause            -> lifecycle gate only
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

pub mod input;
mod preferences;
pub(crate) mod text_sizing;
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

use parking_lot::Mutex;
pub use window::AndroidWindow;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
    error::{BootstrapError, PlatformError},
    redraw_poll::RedrawPoll,
    shared::{PlatformHandlers, owner_signal::OwnerSignal},
    traits::{
        Clipboard, DisplayId, MobileCapabilities, OpenWindowError, OwnerPlatform, Platform,
        PlatformCapabilities, PlatformDisplay, PlatformExecutor, PlatformReadyCallback,
        PlatformWindow, WindowEvent, WindowId, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks},
    },
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
pub struct AndroidPlatform {
    app: AndroidApp,
    handlers: Arc<Mutex<PlatformHandlers>>,
    running: Arc<AtomicBool>,
    execution_resumed: Arc<AtomicBool>,
    window: Arc<Mutex<Option<Arc<AndroidWindow>>>>,
    background_executor: Arc<SimpleExecutor>,
    clipboard: Arc<MockClipboard>,
    capabilities: MobileCapabilities,
    input_state: Mutex<input::AndroidInputState>,
    owner_signal: Arc<OwnerSignal>,
    scroll_factors: Mutex<crate::shared::android_scroll::FactorCache>,
    preference_admission: crate::shared::preference_read::ReadAdmission,
}

// Opaque on purpose, matching `HeadlessPlatform` and `WinitPlatform`. A
// derived `Debug` here traverses `Arc<MockClipboard>`'s
// `Mutex<Option<String>>` and prints the user's entire clipboard contents
// into whatever sink formatted the platform — a log line, a panic message,
// a bug report. None of the remaining fields are useful to a reader either:
// handles, an executor, and a capability set.
impl std::fmt::Debug for AndroidPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidPlatform").finish_non_exhaustive()
    }
}

impl AndroidPlatform {
    /// Create a new Android platform from the `AndroidApp` provided by
    /// `android_main()`
    pub fn new(app: AndroidApp) -> Self {
        let waker = app.create_waker();
        let owner_signal = OwnerSignal::new(Arc::new(move || {
            waker.wake();
            Ok(())
        }));
        Self {
            app,
            handlers: Arc::new(Mutex::new(PlatformHandlers::new())),
            running: Arc::new(AtomicBool::new(true)),
            execution_resumed: Arc::new(AtomicBool::new(false)),
            window: Arc::new(Mutex::new(None)),
            background_executor: Arc::new(SimpleExecutor),
            clipboard: Arc::new(MockClipboard::new()),
            capabilities: MobileCapabilities::android(),
            input_state: Mutex::new(input::AndroidInputState::default()),
            owner_signal,
            scroll_factors: Mutex::new(crate::shared::android_scroll::FactorCache::default()),
            preference_admission: crate::shared::preference_read::ReadAdmission::default(),
        }
    }

    /// Get the underlying `AndroidApp`
    pub fn app(&self) -> &AndroidApp {
        &self.app
    }

    fn refresh_preferences(&self) -> Result<crate::SystemPreferences, PlatformError> {
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
        let (current, needs_delivery) =
            self.preference_admission
                .read_observation(web_time::Instant::now(), || {
                    preferences::sample(&self.app).map_err(|error| PlatformError::Preferences {
                        message: error.to_string(),
                    })
                })?;
        if needs_delivery {
            let _ = self.owner_signal.wake();
        }
        Ok(current)
    }

    fn refresh_scroll_factors(&self) {
        // Query outside the owner-local cache guard. A failed refresh keeps
        // accepted logical factors; only the never-observed case uses authored
        // line normalization for compatibility with supported API21 devices.
        let reading = preferences::scroll_factors(&self.app).ok();
        self.scroll_factors.lock().observe(reading);
    }

    fn cancel_input_contacts(&self, reason: flui_platform_api::pointer::CancelReason) {
        let events = self.input_state.lock().cancel_contacts(reason);
        let window = self.window.lock().clone();
        if let Some(window) = window {
            for event in events {
                window.callbacks().dispatch_input(event);
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
                                        target: "flui_platform::android::input",
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
    fn preferences(&self) -> Result<crate::SystemPreferences, PlatformError> {
        self.refresh_preferences()
    }

    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.background_executor.clone()
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        tracing::info!("Starting Android platform event loop");

        // Converted once, up front: `on_ready` needs an `Arc<dyn Platform>`
        // to mint `OwnerPlatform` from (ADR-0039 §1), and the rest of
        // this loop reads through the same `Arc` for its whole lifetime
        // instead of the original `Box`.
        let platform = Arc::new(*self);
        platform.owner_signal.bind_owner();
        platform
            .owner_signal
            .start()
            .map_err(|error| PlatformError::Preferences {
                message: error.to_string(),
            })?;
        let mut next_preference_sample = web_time::Instant::now();

        let mut on_ready = Some(on_ready);
        platform.execution_resumed.store(false, Ordering::SeqCst);
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
        platform.running.store(true, Ordering::Relaxed);

        loop {
            if !platform.running.load(Ordering::Relaxed) {
                break;
            }

            // Public ViewConfiguration getters have no complete public change
            // notification. Bound refresh to 500ms, independently of a surface.
            let now = web_time::Instant::now();
            if now >= next_preference_sample {
                platform.refresh_scroll_factors();
                if let Err(error) = platform.refresh_preferences() {
                    tracing::warn!(%error, "Android preference query failed");
                }
                next_preference_sample = now + Duration::from_millis(500);
            }
            if platform.owner_signal.drive() {
                platform.running.store(false, Ordering::Relaxed);
                continue;
            }

            // Wall-clock wake (mirrors the winit backend's `about_to_wait`):
            // consult the registered wake-deadline hook fresh on EVERY
            // iteration -- a deadline that fires, a new one getting armed,
            // or every deadline clearing must all be reflected immediately,
            // not stale from whenever the hook was installed. Lock
            // discipline (ADR-0038 §5, the same rule winit's own
            // `about_to_wait` follows): the hook itself re-enters
            // `flui-app` -- clone the `Arc` out, drop the lock, THEN call
            // it, never while `platform.handlers` is still held.
            let wake_deadline_hook = platform.handlers.lock().wake_deadline.clone();
            let wake_deadline = wake_deadline_hook.and_then(|hook| hook());
            let deadline_due = is_deadline_due(wake_deadline, web_time::Instant::now());
            if deadline_due {
                // Owner services remain deliverable while execution gates frames.
                let _ = platform.owner_signal.wake();
                if platform.owner_signal.drive() {
                    platform.running.store(false, Ordering::Relaxed);
                    continue;
                }
            }

            // Check if we should render before polling. Pending sources are
            // gated on `resumed`; otherwise a due deadline would force a 0ms
            // busy loop while the backgrounded app cannot consume it. The redraw is observed
            // without consumption because `poll_events` may suspend execution
            // before the callback can be delivered.
            let redraw_requested = platform
                .window
                .lock()
                .as_ref()
                .is_some_and(|w| w.has_redraw_request());
            let resumed = platform.execution_resumed.load(Ordering::SeqCst);
            let mut should_call_ready = false;
            let ((), redraw_after_poll) =
                RedrawPoll::new(deadline_due, redraw_requested).poll(resumed, |should_render| {
                    let timeout = if should_render {
                        Duration::from_millis(0)
                    } else {
                        Duration::from_millis(16)
                    };
                    platform.app.poll_events(Some(timeout), |event| {
                if let PollEvent::Main(main_event) = event {
                    match main_event {
                        MainEvent::Resume { .. } => {
                            tracing::info!(
                                "Android: Resumed — lifecycle transition, native window untouched"
                            );
                            platform.execution_resumed.store(true, Ordering::SeqCst);

                            // Notify window of activation. The surface signal
                            // rides along: a `Resume` with no window yet (the
                            // startup order) or with the previous surface still
                            // released asks for one, and a `true` that finds a
                            // live surface simply replaces it — see
                            // `PlatformWindow::on_surface_status_change`.
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_active_status_change(true);
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
                            platform.execution_resumed.store(false, Ordering::SeqCst);
                            platform.cancel_input_contacts(flui_platform_api::pointer::CancelReason::FocusLost);

                            // Release BEFORE deactivating: the drop is the one
                            // step here with a validity window behind it, and
                            // `on_active_status_change` runs arbitrary embedder
                            // code.
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_surface_status_change(false);
                                w.callbacks().dispatch_active_status_change(false);
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

                            let window = platform.window.lock().clone();
                            if let Some(w) = window {
                                w.invalidate_text_sizing();
                                w.callbacks().dispatch_surface_status_change(true);
                            }
                        }
                        MainEvent::TerminateWindow { .. } => {
                            platform.cancel_input_contacts(flui_platform_api::pointer::CancelReason::CaptureLost);
                            tracing::debug!(
                                "Android: TerminateWindow — the native window is going away, \
                                 releasing the surface"
                            );

                            // The last moment the handle behind a surface is
                            // still valid: `AppCmd::TermWindow` is applied
                            // after this callback returns, so a surface that
                            // outlives it does too.
                            let window = platform.window.lock().clone();
                            if let Some(w) = window {
                                w.invalidate_text_sizing();
                                w.callbacks().dispatch_surface_status_change(false);
                            }
                        }
                        MainEvent::Destroy => {
                            platform.cancel_input_contacts(flui_platform_api::pointer::CancelReason::FocusLost);
                            tracing::info!("Android: Destroy — shutting down");
                            platform.execution_resumed.store(false, Ordering::SeqCst);

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
                            let window = platform.window.lock().clone();
                            if let Some(w) = window {
                                w.revoke_geometry();
                                w.callbacks().dispatch_close();
                            }

                            platform.running.store(false, Ordering::Relaxed);
                        }
                        MainEvent::WindowResized { .. } => {
                            tracing::info!("Android: Window resized");
                            if let Some(ref w) = *platform.window.lock() {
                                let size = w.logical_size();
                                let scale = w.scale_factor();
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
                            platform.cancel_input_contacts(flui_platform_api::pointer::CancelReason::FocusLost);
                            tracing::debug!("Android: Lost focus");
                            if let Some(ref w) = *platform.window.lock() {
                                w.callbacks().dispatch_active_status_change(false);
                            }
                        }
                        MainEvent::ConfigChanged { .. } => {
                            tracing::debug!("Android: Config changed");
                            let window = platform.window.lock().clone();
                            if let Some(window) = window {
                                window.invalidate_text_sizing();
                            }
                            platform.refresh_scroll_factors();
                            if let Err(error) = platform.refresh_preferences() {
                                tracing::warn!(%error, "Android configuration query failed");
                            }
                            next_preference_sample = web_time::Instant::now() + Duration::from_millis(500);
                            let window = platform.window.lock().clone();
                            if let Some(window) = window {
                                window.callbacks().dispatch_resize(window.logical_size(), window.scale_factor());
                            }
                        }
                        MainEvent::LowMemory => {
                            tracing::warn!("Android: Low memory warning");
                        }
                        _ => {}
                    }
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
                let owner_platform = Arc::clone(&platform) as Arc<dyn Platform>;
                let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
                    Arc::clone(&owner_platform),
                    Arc::clone(&platform.owner_signal),
                ));
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
            // Install accepted preference/configuration changes before the
            // first input packet translated in the new context.
            if platform.owner_signal.drive() {
                platform.running.store(false, Ordering::Relaxed);
                continue;
            }
            let resumed = platform.execution_resumed.load(Ordering::SeqCst);
            if resumed {
                platform.process_input_events();
            }

            // Consume the redraw only after polling confirms this turn can
            // deliver it. A queued Pause therefore preserves the request for
            // Resume rather than stranding an acknowledged continuation.
            if let Some(ref w) = *platform.window.lock()
                && redraw_after_poll
                    .take_if_deliverable(resumed, || w.take_deliverable_redraw_request())
            {
                w.callbacks().dispatch_request_frame();
            }
        }

        platform.execution_resumed.store(false, Ordering::SeqCst);
        platform.owner_signal.close();

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
        let window = platform.window.lock().take();
        if let Some(window) = window {
            window.revoke_geometry();
            window.callbacks().clear();
            tracing::debug!("Android: loop exited; window released and its callbacks cleared");
        }

        // Invoke quit handlers
        platform.handlers.lock().invoke_quit();
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
        self.execution_resumed.store(false, Ordering::SeqCst);
        self.running.store(false, Ordering::Relaxed);
        let _ = self.owner_signal.request_quit();
    }

    fn open_window(
        &self,
        _options: WindowOptions,
    ) -> Result<Arc<dyn crate::traits::HostWindow>, OpenWindowError> {
        let cancelled = self
            .input_state
            .lock()
            .cancel_contacts(flui_platform_api::pointer::CancelReason::CaptureLost);
        let window = Arc::new(AndroidWindow::new(
            self.app.clone(),
            Arc::clone(&self.execution_resumed),
            Arc::downgrade(&self.owner_signal),
        ));
        let previous = self.window.lock().replace(Arc::clone(&window));
        if let Some(previous) = previous {
            previous.revoke_geometry();
            for event in cancelled {
                previous.callbacks().dispatch_input(event);
            }
        }
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

    /// Unlike `winit`'s override (the trait's one other live implementor),
    /// this backend has no `ControlFlow::WaitUntil` to hand the deadline
    /// to — `run`'s own loop actuates it directly instead, by checking
    /// `now >= deadline` once per iteration and forcing a dispatch when due
    /// (see that loop's own comment). Storage is the identical
    /// `PlatformHandlers::wake_deadline` slot winit uses, for the same
    /// `Arc`-cloned-out-of-the-lock consultation discipline (ADR-0038 §5).
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        self.handlers.lock().wake_deadline = Some(Arc::from(hook));
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
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
struct MockClipboard {
    content: Mutex<Option<String>>,
}

// Redacted at the source, not just at `AndroidPlatform`'s boundary, so a
// future holder of this type cannot reintroduce the leak by deriving its
// own `Debug`. Whether the clipboard is set is safe to show; what it holds
// is the user's data.
impl std::fmt::Debug for MockClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockClipboard")
            .field("has_content", &self.content.lock().is_some())
            .finish()
    }
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

    fn bounds(&self) -> flui_foundation::geometry::Bounds<i32> {
        use flui_foundation::geometry::{Bounds, Point, Size};
        Bounds::new(Point::new(0, 0), Size::new(1080, 2340))
    }

    fn scale_factor(&self) -> f64 {
        2.75
    }

    fn is_primary(&self) -> bool {
        true
    }
}
