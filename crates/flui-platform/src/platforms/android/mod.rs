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
pub use memory::{
    PageAlignedVec, PageAllocError, align_to_page_size, align_to_page_size_u64, get_page_size,
    is_16kb_page_size,
};
use parking_lot::Mutex;
pub use window::AndroidWindow;

use crate::{
    data_transfer::{DataTransferSource, NullDataTransferSource},
    error::{BootstrapError, PlatformError},
    shared::PlatformHandlers,
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

/// Whether `run`'s loop should force a zero-timeout `poll_events` call (and,
/// once actually dispatched a few lines later, a frame) THIS iteration.
///
/// `redraw_requested` passes through unconditionally: it comes from
/// [`AndroidWindow::take_redraw_request`](super::window::AndroidWindow::take_redraw_request), which
/// consumes on read (an atomic swap to `false`), so it is already
/// self-limiting the way [`is_deadline_due`] is NOT — one call answering
/// `true` clears it for every subsequent call until something re-arms it.
///
/// `deadline_due` is gated on `resumed` because it carries no such
/// self-limiting property, and round 6 found what happens when it is
/// counted in unconditionally: with the app paused and a deadline armed —
/// which device loss and `MainEvent::Pause` are frequently the same
/// real-world event for — `is_deadline_due` answers `true` on every
/// iteration (see its own doc: it is stateless), forcing a 0ms
/// `poll_events` timeout every iteration, while the actual dispatch this
/// would be for stays gated on `resumed` too (`run`'s loop, a few lines
/// below this decision) and so never runs — nothing ever consumes the
/// deadline, and the loop spins at 100% CPU on a backgrounded phone for as
/// long as it stays paused. Gating here means a due-but-unconsumable
/// deadline instead falls through to the ordinary ~16ms idle timeout; it is
/// not lost, only deferred — the moment a real `MainEvent::Resume` arrives,
/// the wake-deadline hook is re-consulted on the very next iteration (that
/// read is unconditional and never cached) and a still-due deadline is
/// caught then.
fn should_force_render_poll(resumed: bool, deadline_due: bool, redraw_requested: bool) -> bool {
    (resumed && deadline_due) || redraw_requested
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
    window: Arc<Mutex<Option<Arc<AndroidWindow>>>>,
    background_executor: Arc<SimpleExecutor>,
    clipboard: Arc<MockClipboard>,
    capabilities: MobileCapabilities,
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

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
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

            // Check if we should render before polling — see
            // `should_force_render_poll`'s own doc for why `deadline_due`'s
            // contribution is gated on `resumed` and `redraw_requested`'s is
            // not.
            let redraw_requested = platform
                .window
                .lock()
                .as_ref()
                .is_some_and(|w| w.take_redraw_request());
            let should_render = should_force_render_poll(resumed, deadline_due, redraw_requested);

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

        if let Some(source) = bootstrap_error {
            return Err(PlatformError::bootstrap(source));
        }
        Ok(())
    }

    fn quit(&self) {
        tracing::info!("Android: quit requested");
        self.running.store(false, Ordering::Relaxed);
    }

    fn open_window(
        &self,
        _options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
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

#[cfg(test)]
mod wake_deadline_tests {
    use std::time::Duration;

    use super::{is_deadline_due, should_force_render_poll};

    #[test]
    fn no_hook_or_an_empty_hook_is_never_due() {
        assert!(!is_deadline_due(None, web_time::Instant::now()));
    }

    #[test]
    fn a_future_deadline_is_not_due_yet() {
        let now = web_time::Instant::now();
        let deadline = now + Duration::from_millis(16);
        assert!(!is_deadline_due(Some(deadline), now));
    }

    #[test]
    fn a_due_or_past_deadline_is_due() {
        let now = web_time::Instant::now();
        let deadline = now
            .checked_sub(Duration::from_millis(1))
            .expect("now is far past the epoch");
        assert!(is_deadline_due(Some(deadline), now));
        assert!(
            is_deadline_due(Some(now), now),
            "exactly-now must count as due, not just strictly-past"
        );
    }

    /// The property this function's own doc leads with: it is stateless, so
    /// a due deadline stays due across repeated calls with the same
    /// arguments — it does not remember having already answered `true`. Any
    /// self-limiting behavior (stopping the loop from re-asking, or
    /// stopping "due" from re-forcing an action once nothing can act on it)
    /// has to live in a caller, never here.
    #[test]
    fn a_due_deadline_stays_due_across_repeated_calls_with_unchanged_arguments() {
        let now = web_time::Instant::now();
        let deadline = now
            .checked_sub(Duration::from_secs(1))
            .expect("now is far past the epoch");
        for _ in 0..5 {
            assert!(
                is_deadline_due(Some(deadline), now),
                "is_deadline_due must not self-extinguish -- it has no state to extinguish"
            );
        }
    }

    /// Round 6's finding, pinned directly: a due deadline must NOT force a
    /// render poll while paused, because nothing downstream would consume
    /// it (the actual dispatch is gated on `resumed` too) — reported
    /// unconditionally, this is a 100% CPU spin on a backgrounded phone for
    /// as long as it stays paused, since `is_deadline_due` has no memory of
    /// already having said "yes" once (see its own doc).
    #[test]
    fn a_due_deadline_does_not_force_a_render_poll_while_paused() {
        assert!(
            !should_force_render_poll(false, true, false),
            "paused + due + no explicit redraw request must fall through to the ordinary idle \
             timeout, not force a 0ms poll nobody can act on"
        );
    }

    #[test]
    fn a_due_deadline_forces_a_render_poll_while_resumed() {
        assert!(should_force_render_poll(true, true, false));
    }

    #[test]
    fn an_explicit_redraw_request_forces_a_render_poll_regardless_of_resumed_or_deadline_state() {
        // `redraw_requested` is already self-limiting (consume-on-read), so
        // unlike `deadline_due` it is never gated on `resumed` here.
        assert!(should_force_render_poll(false, false, true));
        assert!(should_force_render_poll(true, false, true));
    }

    #[test]
    fn nothing_pending_never_forces_a_render_poll() {
        assert!(!should_force_render_poll(false, false, false));
        assert!(!should_force_render_poll(true, false, false));
    }
}
