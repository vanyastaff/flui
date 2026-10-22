//! The macOS owner thread: the AppKit `NSApplication` loop and its delegate,
//! the owner lane every thread-affine AppKit call travels through, and the wake
//! pump that actuates the registered wake deadline.
//!
//! [`MacOSLoop`] is the loop of the macOS host's table; windows, the
//! clipboard and the other features keep their code under `platforms::macos`
//! and reach the owner thread through what this module re-exports. The loop
//! holds the displays through [`HostDisplays`], which the table makes
//! ([`MacosHost`]).

// `NSApplication.activateIgnoringOtherApps`, which the loop calls as the
// application starts, is deprecated in favour of `activate`, a macOS 14 method.
#![expect(deprecated)]

mod loop_control;
mod owner_lane;
mod wake_pump;

pub(crate) use loop_control::LoopControl;
#[cfg(test)]
pub(crate) use owner_lane::test_owner_queue;
pub(crate) use owner_lane::{
    exec_async_guarded, exec_on_owner, on_owner_thread, owner_queue, run_cleanup_guarded,
};
pub(crate) use wake_pump::WeakWakePump;

/// What the macOS host's table names for its owner thread (ADR-0151 §10): the
/// displays the loop holds. The owner reaches them through the hook and names
/// no feature.
pub(crate) trait MacosHost: 'static {
    /// The displays, made once per loop: the loop holds them for the table's
    /// displays position and hands them to every window it opens.
    fn displays() -> Arc<dyn HostDisplays>;
}

/// The host's screens as the loop and the windows it opens reach them, which
/// the table makes ([`MacosHost::displays`]). A wake marks them stale.
pub(crate) trait HostDisplays: StaleOnWake {
    /// The displays now: the last sample while nothing has invalidated it,
    /// otherwise a fresh one.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the snapshot revision space is used up.
    fn snapshot(&self, mtm: MainThreadMarker) -> Result<DisplaySnapshot, PlatformError>;

    /// Mark the snapshot stale; the next [`snapshot`](Self::snapshot) samples
    /// again. Any thread.
    fn invalidate(&self);

    /// The id of the display `screen` is on; `None` for a screen the source
    /// leaves out.
    fn id_of(&self, screen: &NSScreen) -> Option<DisplayId>;

    /// Mark the screens stale and wake the owner behind `signal` when AppKit
    /// reports a change of screen parameters, until the watch is dropped.
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;
}

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use flui_foundation::OwnerAffinity;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSScreen};
use parking_lot::Mutex;

use crate::host::{self, Host};
use crate::platform::HostPlatform;
use crate::platforms::macos::{
    MacOSWindow, PreferenceObservers, PreferenceSource, SystemNoticeSources,
};
use crate::{
    config::WindowConfiguration,
    error::PlatformError,
    executor::BackgroundExecutor,
    owner::DirectOwnerHooks,
    owner::OwnerHooks,
    owner::OwnerPlatform,
    owner::call_point::{Field, install},
    owner::handlers::PlatformHandlers,
    owner::signal::OwnerSignal,
    owner::watch::Watch,
    platform::Platform,
    platform::PlatformExecutor,
    platform::PlatformReadyCallback,
    shared::system_notice::{StaleOnWake, SystemNotices},
};
use flui_platform::{
    DisplayId, DisplaySnapshot, PlatformWindow, SessionEndAnswer, SessionEndPhase, SystemNotice,
    WindowOptions,
};

/// The macOS host's loop: the state every position of its table reads.
///
/// [`MacOSPlatform`](crate::MacOSPlatform) is its platform.
pub struct MacOSLoop {
    /// The process-wide `NSApplication` singleton, retained.
    pub(crate) app: Retained<NSApplication>,

    /// Open windows (keyed by NSWindow pointer as u64)
    pub(crate) windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Platform event handlers
    handlers: Arc<Mutex<PlatformHandlers>>,

    /// The landing place for the wake deadline `handlers.wake_deadline`
    /// publishes — AppKit has no `ControlFlow::WaitUntil`, so this module
    /// actuates it (see `wake_pump`). Each window holds a `Weak` back
    /// to it and arms it from `request_redraw`.
    pub(crate) wake_pump: Arc<wake_pump::WakePump>,

    pub(crate) loop_control: Arc<LoopControl>,

    /// Wakes the owner when AppKit reports a preference change. Declared
    /// before the source so native observation ends first.
    _preference_observers: PreferenceObservers,

    pub(crate) preferences: Arc<PreferenceSource>,

    /// Background executor (GCD-based)
    background_executor: Arc<BackgroundExecutor>,

    /// Window configuration
    pub(crate) config: WindowConfiguration,

    /// The id registry and last snapshot of the displays, as the table made
    /// them; shared with every window so each names its screen by the same
    /// identity.
    pub(crate) displays: Arc<dyn HostDisplays>,

    /// Marks `displays` stale and wakes the owner when AppKit reports a
    /// change of screen parameters; removed when the platform is dropped.
    _screen_observer: Watch,

    /// Where sleep, wake, lock and memory notices go, and what a wake marks
    /// stale before the host hears of it.
    pub(crate) notices: Arc<SystemNotices>,

    /// Records the event-loop owner thread so the thread-affine AppKit
    /// operations below can `debug_assert` their caller (ADR-0039).
    pub(crate) affinity: OwnerAffinity,
}

/// Debug-only check that the caller is the OS main thread.
///
/// On AppKit the owner thread must *be* the process main thread —
/// `ThreadId` equality against the binding thread cannot express that on
/// its own, so this asks AppKit directly.
pub(crate) fn debug_assert_appkit_main_thread(op: &'static str) {
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
    pub(crate) fn new<H: MacosHost>() -> Result<Self, PlatformError> {
        Self::with_config::<H>(WindowConfiguration::default())
    }

    /// One of the application's handlers, as a slot of the handler set.
    pub(crate) fn handler<H>(
        &self,
        project: fn(&mut PlatformHandlers) -> &mut Option<H>,
    ) -> Field<'_, Mutex<PlatformHandlers>, H> {
        Field::new(&*self.handlers, project)
    }

    pub(crate) fn with_config<H: MacosHost>(
        config: WindowConfiguration,
    ) -> Result<Self, PlatformError> {
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
            let wake_pump = wake_pump::WakePump::new(Arc::clone(&handlers), {
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

            let loop_control = LoopControl::new(Arc::clone(&windows), Arc::clone(&handlers));
            let (preferences, preference_observers) =
                PreferenceSource::new(&loop_control.owner_signal);
            let displays = H::displays();
            let screen_observer = Arc::clone(&displays).observe(&loop_control.owner_signal);
            let notices = Arc::new(SystemNotices::default());
            notices.mark_held_stale_on_wake(Arc::downgrade(&displays) as _);
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
    pub(crate) fn measure_placement_globally(&self, options: &mut WindowOptions) {
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
            loop_control::LoopOwnership::acquire(&self.app, Arc::clone(&self.loop_control))?;
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
            Arc::clone(&platform),
            Arc::clone(&self.loop_control.owner_signal),
        ));
        on_finish_launching(OwnerPlatform::new(platform, hooks))
            .map_err(PlatformError::bootstrap)?;
        // The sources start listening only now: the application has installed
        // its hooks while it bootstrapped, and a notice that arrived earlier
        // would find none and be lost.
        let notice_sources = SystemNoticeSources::new(&self.notices, &self.loop_control);
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
        install(&self.handler(|handlers| &mut handlers.quit), callback);
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        install(&self.handler(|handlers| &mut handlers.exit_policy), hook);
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
        install(
            &self.handler(|handlers| &mut handlers.wake_deadline),
            Arc::from(hook),
        );
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

impl host::Lifecycle<MacOSLoop> for MacOSLoop {
    fn on_session_end(
        owner: &MacOSLoop,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    ) {
        install(
            &owner.handler(|handlers| &mut handlers.session_end),
            callback,
        );
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
