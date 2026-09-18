//! iOS platform implementation (UIKit).
//!
//! `UIApplicationMain` owns the main thread for the process's life, so this
//! backend is built around it: [`IOSPlatform::run`] stashes the bootstrap
//! callback and enters `UIApplicationMain`, and the `AppDelegate` this module
//! declares runs it from `application:didFinishLaunchingWithOptions:` — the
//! first point at which UIKit permits a window to be created.
//!
//! # Lifecycle
//!
//! UIKit's application transitions map onto the framework's active/surface
//! signals exactly as Android's `MainEvent`s do:
//!
//! ```text
//! didBecomeActive    → active(true)  + surface(true)
//! willResignActive   → surface(false) + active(false)
//! didEnterBackground → surface(false)
//! willEnterForeground→ surface(true)
//! ```
//!
//! The surface edges are what let the presentation drop and rebuild its
//! `CAMetalLayer`-backed surface around a suspension, which is the same
//! contract Android's `Pause`/`InitWindow` pair carries (`platforms/android`
//! module doc, "Surface Lifecycle").
//!
//! # Frame source
//!
//! A `CADisplayLink` fires once per refresh; its callback requests a frame
//! through the window's callbacks. It is created with the window and paused
//! while the app is backgrounded, so a suspended app does no work.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use objc2::MainThreadMarker;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{ClassType, MainThreadOnly, define_class};
use objc2_foundation::{NSObjectProtocol, NSString};
use objc2_ui_kit::{UIApplication, UIApplicationDelegate};

use parking_lot::Mutex;

use crate::error::PlatformError;
use crate::shared::PlatformHandlers;
use crate::traits::{
    Clipboard, MobileCapabilities, OpenWindowError, OwnerPlatform, Platform, PlatformCapabilities,
    PlatformDisplay, PlatformExecutor, PlatformReadyCallback, PlatformWindow, WindowEvent,
    WindowId, WindowOptions,
    owner::{DirectOwnerHooks, OwnerHooks},
};

use super::clipboard::IOSClipboard;
use super::display::IOSDisplay;
use super::executor::IOSExecutor;
use super::window::IOSWindow;

/// The one window this backend hosts. iOS presents a single full-screen
/// window for the life of the app; a second `open_window` returns the same
/// one (the trait's `WindowId` is fixed, matching Android).
#[derive(Clone)]
struct WindowSlot {
    window: Option<Arc<IOSWindow>>,
}

impl std::fmt::Debug for WindowSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowSlot")
            .field("has_window", &self.window.is_some())
            .finish()
    }
}

/// iOS platform implementation.
pub struct IOSPlatform {
    handlers: Arc<Mutex<PlatformHandlers>>,
    running: Arc<AtomicBool>,
    window: Arc<Mutex<WindowSlot>>,
    background_executor: Arc<IOSExecutor>,
    clipboard: Arc<IOSClipboard>,
    capabilities: MobileCapabilities,
}

impl std::fmt::Debug for IOSPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSPlatform").finish_non_exhaustive()
    }
}

impl IOSPlatform {
    /// Create a new iOS platform.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Init`] if called off the main thread, which is
    /// where UIKit requires every window and view operation to run.
    pub fn new() -> Result<Self, PlatformError> {
        if MainThreadMarker::new().is_none() {
            return Err(PlatformError::Init {
                message: "IOSPlatform::new must run on the main thread (UIKit is \
                          main-thread-only)"
                    .to_string(),
            });
        }
        Ok(Self {
            handlers: Arc::new(Mutex::new(PlatformHandlers::new())),
            running: Arc::new(AtomicBool::new(true)),
            window: Arc::new(Mutex::new(WindowSlot { window: None })),
            background_executor: Arc::new(IOSExecutor),
            clipboard: Arc::new(IOSClipboard::new()),
            capabilities: MobileCapabilities::ios(),
        })
    }

    /// The one live window, if any.
    fn active(&self) -> Option<Arc<IOSWindow>> {
        self.window.lock().window.clone()
    }

    /// Fire the registered quit handler. Called from
    /// `applicationWillTerminate:` — iOS's one pre-exit notification — so the
    /// framework gets the loop-exit signal `UIApplicationMain`'s own
    /// never-returning loop would otherwise swallow.
    fn invoke_quit(&self) {
        self.handlers.lock().invoke_quit();
    }
}

// The bootstrap the `AppDelegate` runs once `didFinishLaunching` arrives, and
// the platform value the delegate's lifecycle callbacks reach for the rest of
// the session.
//
// Thread-local rather than a process static, on purpose: `UIApplicationMain`
// owns the main thread for the process's life, and both the delegate and
// these values are reachable only from there. This is the same owner-affine
// scope the winit backend uses for its `ActiveEventLoop` publication, and it
// keeps the ambient-reach ratchet (`docs/runtime-contract.toml`) honest —
// there is no new process-global.
thread_local! {
    static DELEGATE_STATE: RefCell<DelegateState> = const { RefCell::new(DelegateState {
        on_ready: None,
        platform: None,
    }) };
}

struct DelegateState {
    /// Taken exactly once, by `didFinishLaunching`.
    on_ready: Option<PlatformReadyCallback>,
    /// Live for the whole session; read by every lifecycle callback.
    platform: Option<Arc<IOSPlatform>>,
}

impl Platform for IOSPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.background_executor.clone()
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Init {
            message: "IOSPlatform::run must be called on the main thread".to_string(),
        })?;

        tracing::info!("Starting iOS platform event loop (UIApplicationMain)");

        let platform = Arc::new(*self);
        platform.running.store(true, Ordering::Relaxed);

        DELEGATE_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.platform = Some(Arc::clone(&platform));
            state.on_ready = Some(on_ready);
        });

        // FORCE the delegate class to register before `UIApplicationMain`
        // resolves its name: `define_class!` registers lazily, on the first
        // `ClassType::class()` call, and `UIApplicationMain`'s lookup is by
        // runtime name — so without this the delegate would not be found and
        // the app would launch with no delegate at all.
        let delegate_class = FluiAppDelegate::class();
        let delegate_name = NSString::from_class(delegate_class);

        // `UIApplicationMain` never returns: it runs the run loop until the
        // process terminates. This backend therefore has no post-loop exit
        // path (there is no `teardown` after it, exactly as macOS's
        // `NSApplication::run` documents).
        UIApplication::main(None, Some(&delegate_name), mtm);
    }

    fn quit(&self) {
        // Apple discourages programmatic termination; the honest iOS behavior
        // is to mark the loop stopped so callbacks stop being serviced, and
        // let the OS decide when the process ends.
        tracing::info!("iOS quit requested (process exit is the OS's call)");
        self.running.store(false, Ordering::Relaxed);
    }

    fn open_window(
        &self,
        _options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| OpenWindowError::Unavailable {
            message: "iOS windows are main-thread-only; open_window ran off the main thread"
                .to_string(),
        })?;
        let mut slot = self.window.lock();
        if let Some(existing) = &slot.window {
            return Ok(Arc::clone(existing) as Arc<dyn PlatformWindow>);
        }
        let window = Arc::new(IOSWindow::new(mtm));
        slot.window = Some(Arc::clone(&window));
        // Report the initial size so a bootstrap that never sees a layout
        // pass still has one.
        let size = window.logical_size();
        window
            .callbacks()
            .dispatch_resize(size, window.scale_factor() as f32);
        tracing::info!("iOS window created (UIWindow + FluiView)");
        Ok(window as Arc<dyn PlatformWindow>)
    }

    fn active_window(&self) -> Option<WindowId> {
        self.window.lock().window.as_ref().map(|_| WindowId(1))
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        MainThreadMarker::new()
            .map(|mtm| vec![IOSDisplay::main_display(mtm)])
            .unwrap_or_default()
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        MainThreadMarker::new().map(IOSDisplay::main_display)
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.clipboard.clone()
    }

    fn data_transfer(&self) -> Arc<dyn crate::data_transfer::DataTransferSource> {
        Arc::new(crate::data_transfer::NullDataTransferSource)
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        &self.capabilities
    }

    fn name(&self) -> &'static str {
        "iOS (UIKit)"
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.handlers.lock().quit = Some(callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.handlers.lock().window_event = Some(callback);
    }

    /// iOS has no `ControlFlow::WaitUntil` — like Android, its loop actuates
    /// deadlines itself. A `CADisplayLink` is the frame source; this stores
    /// the hook so a future deadline actuation can consult it.
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        self.handlers.lock().wake_deadline = Some(Arc::from(hook));
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        // The app bundle's path, from `+[NSBundle mainBundle]`. Read through
        // Foundation rather than hard-coded.
        let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Init {
            message: "app_path must run on the main thread".to_string(),
        })?;
        let _ = mtm;
        let bundle = objc2_foundation::NSBundle::mainBundle();
        let path = bundle.bundlePath();
        Ok(PathBuf::from(path.to_string()))
    }
}

/// Run the bootstrap stashed by [`IOSPlatform::run`], on the main thread,
/// from `didFinishLaunching`. Returns `true` if a bootstrap ran.
fn run_pending_bootstrap() -> bool {
    let (platform, on_ready) = DELEGATE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        (state.platform.clone(), state.on_ready.take())
    });
    let (Some(platform), Some(on_ready)) = (platform, on_ready) else {
        return false;
    };

    let owner_platform = Arc::clone(&platform) as Arc<dyn Platform>;
    let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::new(Arc::clone(&owner_platform)));
    if let Err(error) = on_ready(OwnerPlatform::new(owner_platform, hooks)) {
        tracing::error!(%error, "iOS bootstrap failed");
        // A failed bootstrap leaves nothing to run; stop accepting frames.
        platform.running.store(false, Ordering::Relaxed);
    }
    true
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements beyond what the runtime
    //   already provides.
    // - This class does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiAppDelegate"]
    #[ivars = FluiDelegateIvars]
    pub struct FluiAppDelegate;

    impl FluiAppDelegate {
        #[unsafe(method(application:didFinishLaunchingWithOptions:))]
        fn did_finish_launching(
            &self,
            _application: &UIApplication,
            _options: Option<&AnyObject>,
        ) -> bool {
            tracing::info!("UIApplication didFinishLaunching — running iOS bootstrap");
            run_pending_bootstrap();
            true
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _application: &UIApplication) {
            Self::with_platform(|p| {
                if let Some(w) = p.active() {
                    w.callbacks().dispatch_active_status_change(true);
                    w.callbacks().dispatch_surface_status_change(true);
                    w.request_redraw();
                }
            });
        }

        #[unsafe(method(applicationWillResignActive:))]
        fn will_resign_active(&self, _application: &UIApplication) {
            Self::with_platform(|p| {
                if let Some(w) = p.active() {
                    // Pause the tick first: a link that fires while the
                    // surface is being torn down would request a frame the
                    // presentation can no longer present.
                    w.set_frame_tick_paused(true);
                    // Release the surface BEFORE deactivating — the same
                    // ordering Android's `Pause` arm uses, because
                    // `on_active_status_change` runs embedder code.
                    w.callbacks().dispatch_surface_status_change(false);
                    w.callbacks().dispatch_active_status_change(false);
                }
            });
        }

        #[unsafe(method(applicationDidEnterBackground:))]
        fn did_enter_background(&self, _application: &UIApplication) {
            Self::with_platform(|p| {
                if let Some(w) = p.active() {
                    w.set_frame_tick_paused(true);
                    w.callbacks().dispatch_surface_status_change(false);
                }
            });
        }

        #[unsafe(method(applicationWillEnterForeground:))]
        fn will_enter_foreground(&self, _application: &UIApplication) {
            Self::with_platform(|p| {
                if let Some(w) = p.active() {
                    w.callbacks().dispatch_surface_status_change(true);
                    w.set_frame_tick_paused(false);
                }
            });
        }

        /// The process is about to exit. This is the one pre-exit
        /// notification iOS sends, and the only place the framework can run
        /// its loop-exit signal: `UIApplicationMain`'s loop never returns, so
        /// there is no "after `run`" for the runner to use.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _application: &UIApplication) {
            tracing::info!("UIApplication willTerminate — firing the quit handler");
            Self::with_platform(IOSPlatform::invoke_quit);
        }
    }

    unsafe impl NSObjectProtocol for FluiAppDelegate {}
    unsafe impl UIApplicationDelegate for FluiAppDelegate {}
);

/// The delegate holds no state of its own; the platform is reached through
/// the thread-local session slot (see [`DELEGATE_STATE`]).
pub struct FluiDelegateIvars;

impl FluiAppDelegate {
    fn with_platform(body: impl FnOnce(&IOSPlatform)) {
        // Reached through the session slot, which `run` installs before
        // `UIApplicationMain` and which outlives the one-shot `on_ready`
        // take: the lifecycle callbacks keep arriving for the whole session.
        DELEGATE_STATE.with(|state| {
            if let Some(platform) = state.borrow().platform.as_ref() {
                body(platform);
            }
        });
    }
}
