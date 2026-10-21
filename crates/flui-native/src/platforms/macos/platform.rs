//! The macOS host's loop (AppKit/Cocoa) and its table's bindings to it.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use flui_foundation::OwnerAffinity;
use flui_foundation::geometry::Size;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSApplication;
use parking_lot::Mutex;

use super::{display, preferences::PreferenceSource, window::MacOSWindow};
use crate::host::{self, Host};
use crate::platform::HostPlatform;
use crate::{
    config::WindowConfiguration, error::PlatformError, executor::BackgroundExecutor,
    shared::PlatformHandlers, shared::system_notice::SystemNotices, traits::HostWindow,
    traits::OpenWindowError, traits::OwnerPlatform, traits::Platform, traits::PlatformExecutor,
    traits::PlatformReadyCallback, traits::owner::DirectOwnerHooks, traits::owner::OwnerHooks,
};
use flui_platform::{
    DisplaySnapshot, PlatformWindow, SessionEndAnswer, SessionEndPhase, SystemNotice,
    SystemPreferences, WindowCapabilities, WindowEvent, WindowId, WindowOptions,
};

/// The macOS host's loop: the state every position of its table reads.
///
/// [`MacOSPlatform`](crate::MacOSPlatform) is its platform.
pub struct MacOSLoop {
    /// The process-wide `NSApplication` singleton, retained.
    app: Retained<NSApplication>,

    /// Open windows (keyed by NSWindow pointer as u64)
    windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Platform event handlers
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// The landing place for the wake deadline `handlers.wake_deadline`
    /// publishes — AppKit has no `ControlFlow::WaitUntil`, so this module
    /// actuates it (see `super::wake_pump`). Each window holds a `Weak` back
    /// to it and arms it from `request_redraw`.
    wake_pump: Arc<super::wake_pump::WakePump>,

    loop_control: Arc<super::loop_control::LoopControl>,

    /// Wakes the owner when AppKit reports a preference change. Declared
    /// before the source so native observation ends first.
    _preference_observers: super::preferences::PreferenceObservers,

    preferences: Arc<PreferenceSource>,

    /// Background executor (GCD-based)
    background_executor: Arc<BackgroundExecutor>,

    /// Window configuration
    config: WindowConfiguration,

    /// The id registry and last snapshot of the displays; shared with every
    /// window so each names its screen by the same identity.
    displays: Arc<display::MacosDisplays>,

    /// Marks `displays` stale and wakes the owner when AppKit reports a
    /// change of screen parameters; removed when the platform is dropped.
    _screen_observer: display::ScreenObserver,

    /// Where sleep, wake, lock and memory notices go, and what a wake marks
    /// stale before the host hears of it.
    notices: Arc<SystemNotices>,

    /// Records the event-loop owner thread so the thread-affine AppKit
    /// operations below can `debug_assert` their caller (ADR-0039).
    affinity: OwnerAffinity,
}

/// Debug-only check that the caller is the OS main thread.
///
/// On AppKit the owner thread must *be* the process main thread —
/// `ThreadId` equality against the binding thread cannot express that on
/// its own, so this asks AppKit directly.
fn debug_assert_appkit_main_thread(op: &'static str) {
    #[cfg(debug_assertions)]
    {
        // `MainThreadMarker::new()` is objc2's typed form of the same
        // `+[NSThread isMainThread]` check; `None` means this is not the main
        // thread.
        let is_main = MainThreadMarker::new().is_some();
        debug_assert!(
            is_main,
            "BUG: `{op}` must run on the AppKit main thread — thread-affine \
             AppKit APIs are reached through the owner thread (ADR-0039)"
        );
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = op;
    }
}

// SAFETY: the NSApplication pointer is a process-wide singleton only messaged
// from the main thread (AppKit convention); all other fields are
// `Arc`/`Mutex`-protected. `host::Loop: Send + Sync` requires the wrapper to
// be shareable.
unsafe impl Send for MacOSLoop {}
// SAFETY: see `Send` above — interior mutability is Mutex-guarded and the raw
// pointer is main-thread-affine by AppKit convention.
unsafe impl Sync for MacOSLoop {}

impl std::fmt::Debug for MacOSLoop {
    // Hand-written: the remaining fields are raw platform handles and callback
    // payloads with no useful Debug form.
    //
    // `try_lock`, never `lock`: `parking_lot::Mutex` is not reentrant and
    // BLOCKS rather than panicking, so formatting this value while the same
    // thread already holds `windows` would deadlock silently — and a Debug
    // impl gets called from assertion messages and `tracing` fields, which is
    // exactly where a lock is likely to be held. Same pattern
    // `parking_lot::Mutex<T>: Debug` itself uses.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("MacOSLoop");
        match self.windows.try_lock() {
            Some(windows) => out.field("windows", &windows.len()),
            None => out.field("windows", &format_args!("<locked>")),
        };
        out.finish_non_exhaustive()
    }
}

impl MacOSLoop {
    pub(crate) fn new() -> Result<Self, PlatformError> {
        Self::with_config(WindowConfiguration::default())
    }

    pub(crate) fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
        // AppKit is touched below (`sharedApplication`,
        // `setActivationPolicy`), so the main-thread requirement starts HERE,
        // not at `run` — `MainThreadMarker::new()` both checks it and produces
        // the token objc2's AppKit API requires, which is why there is no
        // separate pre-check.
        let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Init {
            message: "MacOSPlatform::with_config must run on the AppKit main thread".to_string(),
        })?;
        debug_assert_appkit_main_thread("MacOSPlatform::with_config");

        {
            // Initialize NSApplication (the process-wide singleton).
            let app = NSApplication::sharedApplication(mtm);

            // Create executors
            let background_executor = Arc::new(BackgroundExecutor::new());

            // Bind the fields the wake pump needs as locals first: the pump is
            // passed a wake closure over the window map, and both must exist
            // before the struct literal that owns them.
            let windows = Arc::new(Mutex::new(HashMap::new()));
            let handlers = Arc::new(Mutex::new(PlatformHandlers::default()));
            let wake_pump = super::wake_pump::WakePump::new(Arc::clone(&handlers), {
                let windows = Arc::clone(&windows);
                Box::new(move || {
                    // Clone the window handles out and drop the guard
                    // before messaging any of them: `request_redraw`
                    // reaches the owner lane, and ADR-0038 §5's
                    // discipline for re-entrant calls applies to a lock
                    // held across one just as it does to the handler lock.
                    let open: Vec<Arc<MacOSWindow>> = windows.lock().values().cloned().collect();
                    for window in open {
                        // A frame for a window whose surface is already
                        // gone is the renderer's to decline — this only
                        // asks, exactly as `FrameWakeHandle::wake_frame`
                        // does through the same method.
                        PlatformWindow::request_redraw(window.as_ref());
                    }
                })
            });

            tracing::info!("macOS platform initialized with AppKit");

            let loop_control =
                super::loop_control::LoopControl::new(Arc::clone(&windows), Arc::clone(&handlers));
            let (preferences, preference_observers) =
                PreferenceSource::new(&loop_control.owner_signal);
            let displays = Arc::new(display::MacosDisplays::new());
            let screen_observer =
                display::ScreenObserver::new(&displays, &loop_control.owner_signal);
            let notices = Arc::new(SystemNotices::default());
            notices.mark_stale_on_wake(&displays);
            let platform = Self {
                loop_control,
                preferences,
                app,
                windows,
                handlers,
                wake_pump,
                background_executor,
                config,
                displays,
                _screen_observer: screen_observer,
                _preference_observers: preference_observers,
                notices,
                affinity: OwnerAffinity::new(),
            };
            // The constructor requires the main thread (checked above), so
            // the owner is known here — bind early so pre-`run` affine
            // calls are covered too.
            platform.affinity.bind_current();
            Ok(platform)
        }
    }

    pub(crate) fn app(&self) -> &Retained<NSApplication> {
        &self.app
    }

    /// Turn a position and a normal rectangle measured on a display into
    /// global logical pixels, which is all a window knows how to place. A
    /// display that is gone leaves them global, as the options document.
    fn measure_placement_globally(&self, options: &mut WindowOptions) {
        let Some(display) = options.display.take() else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(snapshot) = self.displays.snapshot(mtm) else {
            return;
        };
        let display = snapshot.get(display);
        options.position = options
            .position
            .map(|at| crate::shared::appkit_window_geometry::globalize(at, display));
        if let Some(normal) = options.normal_geometry.as_mut() {
            normal.position = normal
                .position
                .map(|at| crate::shared::appkit_window_geometry::globalize(at, display));
        }
    }
}

/// What the macOS platform offers beyond [`Platform`].
impl<H: Host<Loop = MacOSLoop>> HostPlatform<H> {
    /// Get the NSApplication instance
    pub fn app(&self) -> &Retained<NSApplication> {
        self.owner().app()
    }
}

impl host::Loop for MacOSLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        on_finish_launching: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        // Idempotent when `with_config` already bound on this thread; trips
        // a debug assertion if `run` somehow migrated threads (ADR-0039).
        self.affinity.bind_current();
        debug_assert_appkit_main_thread("MacOSPlatform::run");

        let ownership =
            super::loop_control::LoopOwnership::acquire(&self.app, Arc::clone(&self.loop_control))?;
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
            Arc::clone(&platform),
            Arc::clone(&self.loop_control.owner_signal),
        ));
        on_finish_launching(OwnerPlatform::new(platform, hooks))
            .map_err(PlatformError::bootstrap)?;
        // The sources start listening only now: the application has installed
        // its hooks while it bootstrapped, and a notice that arrived earlier
        // would find none and be lost.
        let notice_sources =
            super::system_notices::SystemNoticeSources::new(&self.notices, &self.loop_control);
        ownership.run();
        drop(notice_sources);
        drop(ownership);

        Ok(())
    }

    fn quit(&self) {
        self.affinity.debug_assert_owner("MacOSPlatform::quit");
        debug_assert_appkit_main_thread("MacOSPlatform::quit");
        tracing::info!("Requesting application quit");
        self.loop_control.quit();
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.quit = Some(callback);
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        let previous = self.handlers.lock().exit_policy.replace(hook);
        crate::shared::panic_boundary::contain_owner_callback(|| drop(previous));
    }

    fn request_exit_policy_reevaluation(&self) {
        self.loop_control.request(false);
    }

    /// Unlike `winit`'s override (which hands the deadline to
    /// `ControlFlow::WaitUntil`) and Android's (whose own loop checks it
    /// once per iteration), AppKit's run loop is opaque and exposes neither —
    /// so this backend *actuates* the deadline instead: the private
    /// `wake_pump` module schedules a main-queue tick for it and asks the
    /// windows for a frame
    /// when it comes due. Storage is the identical
    /// `PlatformHandlers::wake_deadline` slot the other two backends use, for
    /// the same `Arc`-cloned-out-of-the-lock consultation discipline
    /// (ADR-0038 §5) — which is why the pump, not this method, owns the call.
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        // Store under the lock, then arm OUTSIDE it: arming consults the hook,
        // and the hook re-enters `flui-app` while taking gesture locks.
        self.handlers.lock().wake_deadline = Some(Arc::from(hook));
        self.wake_pump.arm();
    }
}

impl host::Services for MacOSLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        // `NSBundle.mainBundle` is the shared singleton; `bundlePath` is a
        // non-null `NSString` (Foundation returns an empty string, not nil,
        // for a bundle without a path), so there is no nil arm to guard.
        let bundle = objc2_foundation::NSBundle::mainBundle();
        let path = bundle.bundlePath();
        Ok(PathBuf::from(path.to_string()))
    }

    // The macOS host opens no URL, path or file prompt and reports no
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

impl host::Window<MacOSLoop> for MacOSWindow {
    fn open(
        owner: &MacOSLoop,
        mut options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::open_window");
        debug_assert_appkit_main_thread("MacOSPlatform::open_window");
        if !owner.loop_control.accepts_windows() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        owner.measure_placement_globally(&mut options);
        let window = MacOSWindow::new(options, Arc::clone(&owner.windows), owner.config.clone())?;
        owner.notices.mark_stale_on_wake(window.state_cell());
        // Installed immediately after construction, before the window can be
        // handed to anyone who might redraw it — the slot is a `OnceLock`, so
        // a later install would be silently ignored rather than racing.
        window.install_loop_control(Arc::downgrade(&owner.loop_control));
        window.install_wake_pump(Arc::downgrade(&owner.wake_pump));
        window.bind_displays(Arc::clone(&owner.displays));
        let callbacks = window.callbacks();
        callbacks.set_input_capabilities(crate::shared::input_availability::macos());
        callbacks.set_input_wake(crate::shared::owner_signal::OwnerSignal::callbacks_wake(
            &owner.loop_control.owner_signal,
            callbacks,
        ));

        Ok(window)
    }

    fn active(owner: &MacOSLoop) -> Option<WindowId> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::active_window");
        debug_assert_appkit_main_thread("MacOSPlatform::active_window");
        // `keyWindow` answers `None` when no window is key. The window id is
        // the native `NSWindow` pointer, the same identity `MacOSWindow`
        // reports, so a caller can compare them.
        let key_window = owner.app.keyWindow()?;
        let ptr = Retained::as_ptr(&key_window) as u64;
        Some(WindowId(ptr))
    }

    fn capabilities(_: &MacOSLoop) -> WindowCapabilities {
        super::window::window_capabilities()
    }

    fn frame_inset(owner: &MacOSLoop, _scale_factor: f64) -> Size<f64> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::window_frame_inset");
        debug_assert_appkit_main_thread("MacOSPlatform::window_frame_inset");
        // AppKit measures a frame in points, so the scale does not enter.
        let main = MainThreadMarker::new().expect(
            "BUG: the owner thread is the AppKit main thread, which the assertions above check",
        );
        super::window::decorated_frame_inset(main)
    }

    fn on_event(owner: &MacOSLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        let mut handlers = owner.handlers.lock();
        handlers.window_event = Some(callback);
    }
}

impl host::Displays<MacOSLoop> for display::MacosDisplays {
    fn snapshot(owner: &MacOSLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::display_snapshot");
        debug_assert_appkit_main_thread("MacOSPlatform::display_snapshot");
        let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Displays {
            message: "NSScreen sampling must run on the AppKit main thread".into(),
        })?;
        owner.displays.snapshot(mtm)
    }
}

impl host::Input<MacOSLoop> for MacOSLoop {
    fn keyboard_layout(_: &MacOSLoop) -> String {
        // The host does not read AppKit's input source: the layout is unknown.
        String::new()
    }

    fn on_keyboard_layout_change(_: &MacOSLoop, callback: Box<dyn FnMut() + Send>) {
        // No layout is read, so no change is ever reported.
        drop(callback);
    }
}

impl host::Preferences<MacOSLoop> for PreferenceSource {
    fn sample(owner: &MacOSLoop) -> Result<SystemPreferences, PlatformError> {
        owner.preferences.read()
    }
}

impl host::Lifecycle<MacOSLoop> for MacOSLoop {
    fn on_session_end(
        owner: &MacOSLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        let previous = owner.handlers.lock().session_end.replace(callback);
        crate::shared::panic_boundary::contain_owner_callback(|| drop(previous));
    }

    fn on_host_notice(owner: &MacOSLoop, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        owner.notices.set_hook(callback);
    }

    fn release_session_end(owner: &MacOSLoop) {
        owner.loop_control.session_end_released();
    }

    fn on_reopen(owner: &MacOSLoop, callback: Box<dyn FnMut() + Send>) {
        owner.loop_control.set_reopen(callback);
    }

    fn activate(_: &MacOSLoop, _: bool) {
        // The application is activated once, as its loop starts; a later
        // request is not acted on.
    }
}

impl Drop for MacOSLoop {
    fn drop(&mut self) {
        tracing::debug!("Dropping MacOSLoop");
        // NSApplication is a singleton, no need to release
    }
}
