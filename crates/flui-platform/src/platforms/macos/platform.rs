//! macOS platform implementation using AppKit/Cocoa

use std::{collections::HashMap, sync::Arc};

use flui_foundation::OwnerAffinity;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use parking_lot::Mutex;

use super::{display, window::MacOSWindow};
use crate::{
    config::WindowConfiguration,
    data_transfer::{DataTransferSource, NullDataTransferSource},
    error::PlatformError,
    executor::BackgroundExecutor,
    shared::PlatformHandlers,
    traits::{
        Clipboard, DesktopCapabilities, OpenWindowError, OwnerPlatform, Platform,
        PlatformCapabilities, PlatformDisplay, PlatformExecutor, PlatformReadyCallback,
        PlatformWindow, WindowEvent, WindowId, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks},
    },
};

/// Capabilities descriptor shared by all macOS platform instances.
static MACOS_CAPABILITIES: DesktopCapabilities = DesktopCapabilities;

/// macOS platform state
pub struct MacOSPlatform {
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

    /// Background executor (GCD-based)
    background_executor: Arc<BackgroundExecutor>,

    /// Window configuration
    config: WindowConfiguration,

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
// `Arc`/`Mutex`-protected. `Platform: Send + Sync` requires the wrapper to be
// shareable.
unsafe impl Send for MacOSPlatform {}
// SAFETY: see `Send` above — interior mutability is Mutex-guarded and the raw
// pointer is main-thread-affine by AppKit convention.
unsafe impl Sync for MacOSPlatform {}

impl std::fmt::Debug for MacOSPlatform {
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
        let mut out = f.debug_struct("MacOSPlatform");
        match self.windows.try_lock() {
            Some(windows) => out.field("windows", &windows.len()),
            None => out.field("windows", &format_args!("<locked>")),
        };
        out.finish_non_exhaustive()
    }
}

impl MacOSPlatform {
    /// Create a new macOS platform with default configuration
    pub fn new() -> Result<Self, PlatformError> {
        Self::with_config(WindowConfiguration::default())
    }

    /// Create a new macOS platform with custom configuration
    pub fn with_config(config: WindowConfiguration) -> Result<Self, PlatformError> {
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

            // Set activation policy to regular app (shows in Dock)
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

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

            let platform = Self {
                app,
                windows,
                handlers,
                wake_pump,
                background_executor,
                config,
                affinity: OwnerAffinity::new(),
            };
            // The constructor requires the main thread (checked above), so
            // the owner is known here — bind early so pre-`run` affine
            // calls are covered too.
            platform.affinity.bind_current();
            Ok(platform)
        }
    }

    /// Get the NSApplication instance
    pub fn app(&self) -> &Retained<NSApplication> {
        &self.app
    }
}

impl Platform for MacOSPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    fn run(
        self: Box<Self>,
        on_finish_launching: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        // Idempotent when `with_config` already bound on this thread; trips
        // a debug assertion if `run` somehow migrated threads (ADR-0039).
        self.affinity.bind_current();
        debug_assert_appkit_main_thread("MacOSPlatform::run");

        // Clone the retained singleton before `self` moves into the
        // `Arc<dyn Platform>` the capability needs.
        let app = Retained::clone(&self.app);

        // No owner lane on this backend: every `OwnerPlatform::open_window`
        // call creates directly and is always `Ready` (ADR-0039 slice 2).
        // `run` never returns on macOS (`terminate:` exits the process), so
        // there is no loop-scoped TLS host to clear here either.
        //
        // Call the launch callback. This is an ordinary (safe) call — keep
        // it outside the `unsafe` block below rather than widening that
        // block's scope to cover code that needs no unsafe justification.
        // `on_finish_launching` runs before `app.run()` starts: on `Err`,
        // return without ever starting the NSApplication event loop rather
        // than launching over a half-built app.
        let platform: Arc<dyn Platform> = Arc::new(*self);
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::new(Arc::clone(&platform)));
        on_finish_launching(OwnerPlatform::new(platform, hooks))
            .map_err(PlatformError::bootstrap)?;

        // Runs on the main thread; `app` is the live NSApplication singleton.
        // Activate the app (bring to foreground).
        app.activateIgnoringOtherApps(true);

        // Run the NSApplication event loop. Window lifecycle events are
        // delivered via NSWindowDelegate; input events via the content view's
        // NSResponder chain.
        tracing::info!("Starting NSApplication event loop");
        app.run();

        // Unreachable in practice: `app.run()` never returns on macOS
        // (`terminate:` exits the process) — this satisfies the trait's
        // `Result` return type for the compiler, not a real code path.
        Ok(())
    }

    fn quit(&self) {
        self.affinity.debug_assert_owner("MacOSPlatform::quit");
        debug_assert_appkit_main_thread("MacOSPlatform::quit");
        tracing::info!("Requesting application quit");
        self.app.terminate(None);
    }

    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
        self.affinity
            .debug_assert_owner("MacOSPlatform::open_window");
        debug_assert_appkit_main_thread("MacOSPlatform::open_window");
        let window = MacOSWindow::new(options, Arc::clone(&self.windows), self.config.clone())?;
        // Installed immediately after construction, before the window can be
        // handed to anyone who might redraw it — the slot is a `OnceLock`, so
        // a later install would be silently ignored rather than racing.
        window.install_wake_pump(Arc::downgrade(&self.wake_pump));

        Ok(window)
    }

    fn active_window(&self) -> Option<WindowId> {
        self.affinity
            .debug_assert_owner("MacOSPlatform::active_window");
        debug_assert_appkit_main_thread("MacOSPlatform::active_window");
        // `keyWindow` answers `None` when no window is key. The window id is
        // the native `NSWindow` pointer, the same identity `MacOSWindow`
        // reports, so a caller can compare them.
        let key_window = self.app.keyWindow()?;
        let ptr = Retained::as_ptr(&key_window) as u64;
        Some(WindowId(ptr))
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        self.affinity.debug_assert_owner("MacOSPlatform::displays");
        debug_assert_appkit_main_thread("MacOSPlatform::displays");
        display::enumerate_displays()
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        self.affinity
            .debug_assert_owner("MacOSPlatform::primary_display");
        debug_assert_appkit_main_thread("MacOSPlatform::primary_display");
        display::enumerate_displays()
            .into_iter()
            .find(|d| d.is_primary())
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        Arc::new(super::MacOSClipboard::new())
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // No AppKit transport yet (NSDraggingDestination/NSPasteboard land
        // with the native slices of ADR-0038): inert and honest.
        Arc::new(NullDataTransferSource)
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &MACOS_CAPABILITIES
    }

    fn name(&self) -> &'static str {
        "macOS (AppKit)"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        let mut handlers = self.handlers.lock();
        handlers.window_event = Some(callback);
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

    fn app_path(&self) -> Result<std::path::PathBuf, PlatformError> {
        // `NSBundle.mainBundle` is the shared singleton; `bundlePath` is a
        // non-null `NSString` (Foundation returns an empty string, not nil,
        // for a bundle without a path), so there is no nil arm to guard.
        let bundle = objc2_foundation::NSBundle::mainBundle();
        let path = bundle.bundlePath();
        Ok(std::path::PathBuf::from(path.to_string()))
    }
}

impl Drop for MacOSPlatform {
    fn drop(&mut self) {
        tracing::debug!("Dropping MacOSPlatform");
        // NSApplication is a singleton, no need to release
    }
}
