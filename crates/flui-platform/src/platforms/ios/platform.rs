//! iOS platform implementation (UIKit).
//!
//! `UIApplicationMain` owns the main thread for the process's life, so this
//! backend is built around it: [`IOSPlatform::run`] stashes the bootstrap
//! callback and enters `UIApplicationMain`, and the `AppDelegate` this module
//! declares runs it from `application:didFinishLaunchingWithOptions:` — the
//! process bootstrap. The scene delegate owns subsequent window attachments.
//!
//! # Lifecycle
//!
//! UIKit's scene transitions map onto the framework's active/surface
//! signals independently:
//!
//! ```text
//! didBecomeActive    → focus(true), eligible display-link resume
//! willResignActive   → focus(false), surface retained
//! didEnterBackground → suspended + hidden + surface(false)
//! willEnterForeground→ surface(true), running while unfocused
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
//! while the scene is suspended or detached. Process owner signals remain
//! independent; no OS background execution entitlement is implied.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::native_owner::NativeOwner;
use super::scene::{IOSSceneAttachmentId, IOSSceneEvent, IOSSceneSessionId, SceneHandler};
use crate::shared::owner_signal::OwnerSignal;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{ClassType, DefinedClass, MainThreadOnly, define_class};
use objc2::{MainThreadMarker, Message};
use objc2_foundation::{NSObjectProtocol, NSSet, NSString};
use objc2_ui_kit::{
    UIApplication, UIApplicationDelegate, UIScene, UISceneActivationState, UISceneConfiguration,
    UISceneDelegate, UISceneSession, UIWindowScene,
};

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
use super::window::{IOSWindow, LifecycleObservation};

#[derive(Clone)]
struct SceneOrigin {
    session: IOSSceneSessionId,
    attachment: u64,
}
struct SessionRecord {
    origin: SceneOrigin,
    session: Retained<UISceneSession>,
    window: Arc<IOSWindow>,
    installed: bool,
    attaching: bool,
    retiring: bool,
    closing: bool,
}
enum SceneAction {
    Connect(SceneOrigin, Retained<UIWindowScene>),
    Observe(SceneOrigin, LifecycleObservation),
    Disconnect(SceneOrigin),
    Discard(IOSSceneSessionId, bool),
    Rollback(SceneOrigin),
}
struct SceneState {
    terminal_sessions: HashSet<IOSSceneSessionId>,
    session: Option<SessionRecord>,
    handler: Option<SceneHandler>,
    registered: bool,
    reservation: Option<SceneOrigin>,
    pending: VecDeque<SceneAction>,
    draining: bool,
    next_window: u64,
    next_attachment: u64,
}

/// iOS platform implementation.
pub struct IOSPlatform {
    handlers: Arc<Mutex<PlatformHandlers>>,
    running: Arc<AtomicBool>,
    scenes: NativeOwner<RefCell<SceneState>>,
    signal: Arc<OwnerSignal>,
    identity: Arc<()>,
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
        let marker = MainThreadMarker::new().expect("BUG: checked main thread above");
        let identity = Arc::new(());
        let signal_identity = Arc::clone(&identity);
        let signal = OwnerSignal::new(Arc::new(move || {
            let identity = Arc::clone(&signal_identity);
            dispatch2::DispatchQueue::main().exec_async(move || {
                FluiAppDelegate::with_platform(|platform| {
                    if Arc::ptr_eq(&platform.identity, &identity) && platform.signal.drive() {
                        platform.finish_quit();
                    }
                });
            });
            Ok(())
        }));
        Ok(Self {
            handlers: Arc::new(Mutex::new(PlatformHandlers::new())),
            running: Arc::new(AtomicBool::new(true)),
            scenes: NativeOwner::new(
                RefCell::new(SceneState {
                    terminal_sessions: HashSet::new(),
                    session: None,
                    handler: None,
                    registered: false,
                    reservation: None,
                    pending: VecDeque::new(),
                    draining: false,
                    next_window: 1,
                    next_attachment: 1,
                }),
                marker,
            ),
            signal,
            identity,
            background_executor: Arc::new(IOSExecutor),
            clipboard: Arc::new(IOSClipboard::new()),
            capabilities: MobileCapabilities::ios(),
        })
    }

    fn state(&self) -> &RefCell<SceneState> {
        self.scenes
            .get(MainThreadMarker::new().expect("BUG: UIKit scene owner is main-thread-only"))
    }

    /// Register the session consumer before entering UIKit. Native connection
    /// invokes it on the owner thread, outside storage borrows. An initial
    /// installation failure retires that session rather than publishing it.
    /// The accepted consumer is invoked and retired on main and may capture
    /// owner-local state such as `Rc`; no `Send` bound is required.
    pub fn on_scene_event(&self, callback: SceneHandler) -> Result<(), PlatformError> {
        if MainThreadMarker::new().is_none() || !self.signal.accepting() {
            return Err(PlatformError::Init {
                message: "scene registration requires a live main-thread owner".into(),
            });
        }
        let old = {
            let mut state = self.state().borrow_mut();
            state.registered = true;
            state.handler.replace(callback)
        };
        crate::shared::panic_boundary::contain_owner_callback(|| drop(old));
        Ok(())
    }

    fn emit_scene(&self, event: IOSSceneEvent) -> Result<(), crate::BootstrapError> {
        let mut handler = self.state().borrow_mut().handler.take().ok_or_else(|| {
            Box::new(PlatformError::Init {
                message: "no scene consumer registered".into(),
            }) as crate::BootstrapError
        })?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(event)));
        let old = {
            let mut state = self.state().borrow_mut();
            if state.handler.is_none() && self.signal.accepting() {
                state.handler.replace(handler)
            } else {
                Some(handler)
            }
        };
        crate::shared::panic_boundary::contain_owner_callback(|| drop(old));
        match result {
            Ok(result) => result,
            Err(payload) => {
                std::mem::forget(payload);
                Err(Box::new(PlatformError::Init {
                    message: "scene consumer panicked".into(),
                }))
            }
        }
    }

    fn active(&self) -> Option<Arc<IOSWindow>> {
        self.state()
            .borrow()
            .session
            .as_ref()
            .map(|record| Arc::clone(&record.window))
    }

    fn invoke_quit(&self) {
        let callback = self.handlers.lock().quit.take();
        if let Some(mut callback) = callback {
            crate::shared::panic_boundary::contain_owner_callback(&mut callback);
            crate::shared::panic_boundary::contain_owner_callback(|| drop(callback));
        }
    }

    fn finish_quit(&self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        self.signal.close();
        let session = self
            .state()
            .borrow()
            .session
            .as_ref()
            .map(|record| record.origin.session.clone());
        if let Some(session) = session {
            self.enqueue(SceneAction::Discard(session, true));
        }
        if !self.state().borrow().draining {
            self.invoke_quit();
            let callback = self.state().borrow_mut().handler.take();
            crate::shared::panic_boundary::contain_owner_callback(|| drop(callback));
        }
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
// adds no process-global.
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

        if DELEGATE_STATE.with(|state| state.borrow().platform.is_some()) {
            return Err(PlatformError::Init {
                message: "UIKit process owner is already installed".into(),
            });
        }
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
        let _ = self.signal.request_quit();
    }

    fn open_window(
        &self,
        _options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
        Err(OpenWindowError::Unavailable { message: "UIKit scene connection supplies the window through on_scene_event; arbitrary open_window is unsupported".into() })
    }

    fn active_window(&self) -> Option<WindowId> {
        MainThreadMarker::new().and_then(|_| self.active().map(|window| window.id()))
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
    let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
        Arc::clone(&owner_platform),
        Arc::clone(&platform.signal),
    ));
    if !platform.signal.accepting() {
        crate::shared::panic_boundary::contain_owner_callback(|| drop(on_ready));
        platform.finish_quit();
        return false;
    }
    // FnOnce consumes its own captures. Catch the entire invocation; a body
    // panic plus a capture panic during that unwind remains Rust's abort case.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        on_ready(OwnerPlatform::new(owner_platform, hooks))
    }));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            crate::shared::panic_boundary::contain_owner_callback(
                || tracing::error!(%error, "iOS bootstrap failed"),
            );
            platform.finish_quit();
            crate::shared::panic_boundary::contain_owner_callback(|| drop(error));
            return false;
        }
        Err(payload) => {
            std::mem::forget(payload);
            platform.finish_quit();
            return false;
        }
    }
    if let Err(error) = platform.signal.start() {
        crate::shared::panic_boundary::contain_owner_callback(
            || tracing::error!(%error, "owner signal start failed"),
        );
        platform.finish_quit();
        return false;
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
            let mut ready = false;
            crate::shared::panic_boundary::contain_owner_callback(|| { ready = run_pending_bootstrap(); });
            ready
        }

        #[unsafe(method_id(application:configurationForConnectingSceneSession:options:))]
        fn configuration(&self, _application: &UIApplication, session: &UISceneSession, _options: &AnyObject) -> Retained<UISceneConfiguration> {
            let marker = MainThreadMarker::new().expect("BUG: UIKit delegate runs on main");
            let configuration = UISceneConfiguration::initWithName_sessionRole(marker.alloc(), Some(&NSString::from_str("FLUI")), &session.role());
            // SAFETY: the registered class implements UISceneDelegate.
            unsafe { configuration.setDelegateClass(Some(FluiSceneDelegate::class())); }
            configuration
        }

        #[unsafe(method(application:didDiscardSceneSessions:))]
        fn discarded(&self, _application: &UIApplication, sessions: &NSSet<UISceneSession>) {
            Self::with_platform(|platform| {
                for session in sessions { platform.enqueue(SceneAction::Discard(IOSSceneSessionId(session.persistentIdentifier().to_string()), false)); }
            });
        }

        /// If UIKit sends termination, notify the loop. OS termination is not
        /// guaranteed to deliver this callback. This is currently where we run
        /// its loop-exit signal: `UIApplicationMain`'s loop never returns, so
        /// there is no "after `run`" for the runner to use.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _application: &UIApplication) {
            crate::shared::panic_boundary::contain_owner_callback(|| tracing::info!("UIApplication willTerminate — firing the quit handler"));
            Self::with_platform(IOSPlatform::finish_quit);
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
        let platform = DELEGATE_STATE.with(|state| state.borrow().platform.clone());
        if let Some(platform) = platform {
            crate::shared::panic_boundary::contain_owner_callback(|| body(&platform));
        }
    }
}

impl IOSPlatform {
    fn connect(&self, scene: &UIWindowScene) -> Option<SceneOrigin> {
        if !self.signal.accepting() {
            return None;
        }
        let session = scene.session();
        let id = IOSSceneSessionId(session.persistentIdentifier().to_string());
        let marker = MainThreadMarker::new().expect("BUG: scene connection runs on main");
        let reserved = {
            let mut state = self.state().borrow_mut();
            if state.terminal_sessions.contains(&id)
                || !state.registered
                || state.reservation.is_some()
                || state
                    .session
                    .as_ref()
                    .is_some_and(|record| record.origin.session != id || record.closing)
            {
                None
            } else {
                let attachment = state.next_attachment;
                state.next_attachment = attachment
                    .checked_add(1)
                    .expect("BUG: scene attachment identity exhausted");
                let origin = SceneOrigin {
                    session: id.clone(),
                    attachment,
                };
                let fresh = if state.session.is_some() {
                    None
                } else {
                    let id = WindowId(state.next_window);
                    state.next_window = state
                        .next_window
                        .checked_add(1)
                        .expect("BUG: window identity exhausted");
                    state.reservation = Some(origin.clone());
                    Some(id)
                };
                Some((origin, fresh))
            }
        };
        let Some((origin, fresh)) = reserved else {
            tracing::warn!(
                session = id.as_str(),
                "rejecting unsupported additional scene before window creation"
            );
            return None;
        };
        if let Some(id) = fresh {
            let window = Arc::new(IOSWindow::new(marker, id));
            let admitted = {
                let mut state = self.state().borrow_mut();
                let admitted = self.signal.accepting()
                    && state
                        .reservation
                        .as_ref()
                        .is_some_and(|reserved| reserved.attachment == origin.attachment);
                state.reservation = None;
                if admitted {
                    state.session = Some(SessionRecord {
                        origin: origin.clone(),
                        session,
                        window: Arc::clone(&window),
                        installed: false,
                        attaching: false,
                        retiring: false,
                        closing: false,
                    });
                }
                admitted
            };
            if !admitted {
                window.close_inner();
                return None;
            }
        }
        Some(origin)
    }

    fn current(&self, origin: &SceneOrigin) -> Option<(Arc<IOSWindow>, bool)> {
        if !self.signal.accepting() {
            return None;
        }
        self.state()
            .borrow()
            .session
            .as_ref()
            .filter(|record| {
                !record.closing
                    && record.origin.session == origin.session
                    && record.origin.attachment == origin.attachment
            })
            .map(|record| (Arc::clone(&record.window), record.installed))
    }

    fn enqueue(&self, action: SceneAction) {
        // The ownership drain encloses the outer lifecycle observation. Nested
        // observations still reach the window pump immediately so its resource
        // supersession rules hold, but retirement waits until that pump returns.
        if let SceneAction::Observe(origin, observation) = &action {
            let (admitted, nested) = {
                let state = self.state().borrow();
                let admitted = state.session.as_ref().is_some_and(|record| {
                    !record.closing
                        && !record.retiring
                        && record.origin.session == origin.session
                        && record.origin.attachment == origin.attachment
                });
                (admitted, state.draining)
            };
            if !admitted {
                return;
            }
            if nested {
                self.observe_scene(origin, *observation);
                return;
            }
        }
        // Admission closes synchronously, even if installation currently leases
        // the event consumer and terminal disposal must wait for that lease.
        let disconnecting = matches!(
            &action,
            SceneAction::Disconnect(_) | SceneAction::Discard(_, _) | SceneAction::Rollback(_)
        );
        let (pause, drive) = {
            let mut state = self.state().borrow_mut();
            if let SceneAction::Discard(session, _) = &action {
                state.terminal_sessions.insert(session.clone());
                if state
                    .reservation
                    .as_ref()
                    .is_some_and(|origin| origin.session == *session)
                {
                    state.reservation = None;
                }
                if let Some(record) = state
                    .session
                    .as_mut()
                    .filter(|record| record.origin.session == *session)
                {
                    record.closing = true;
                }
            }
            if let SceneAction::Disconnect(origin) = &action
                && let Some(record) = state.session.as_mut().filter(|record| {
                    record.origin.session == origin.session
                        && record.origin.attachment == origin.attachment
                })
            {
                record.retiring = true;
            }
            let pause = match &action {
                SceneAction::Discard(session, _) => state
                    .session
                    .as_ref()
                    .filter(|record| record.origin.session == *session)
                    .map(|record| Arc::clone(&record.window)),
                SceneAction::Rollback(origin) => state
                    .session
                    .as_ref()
                    .filter(|record| {
                        record.origin.session == origin.session
                            && record.origin.attachment == origin.attachment
                    })
                    .map(|record| Arc::clone(&record.window)),
                SceneAction::Observe(origin, LifecycleObservation::Background)
                | SceneAction::Disconnect(origin) => state
                    .session
                    .as_ref()
                    .filter(|record| {
                        record.origin.session == origin.session
                            && record.origin.attachment <= origin.attachment
                    })
                    .map(|record| Arc::clone(&record.window)),
                _ => None,
            };
            state.pending.push_back(action);
            let drive = !state.draining;
            state.draining = true;
            (pause, drive)
        };
        if let Some(window) = pause {
            if disconnecting {
                window.begin_retirement();
            } else {
                window.set_frame_tick_paused(true);
            }
        }
        if !drive {
            return;
        }
        struct Drain<'a>(&'a RefCell<SceneState>);
        impl Drop for Drain<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().draining = false;
            }
        }
        let guard = Drain(self.state());
        loop {
            let action = self.state().borrow_mut().pending.pop_front();
            let Some(action) = action else { break };
            crate::shared::panic_boundary::contain_owner_callback(|| self.apply_scene(action));
        }
        drop(guard);
        if !self.running.load(Ordering::SeqCst) {
            self.invoke_quit();
            let callback = self.state().borrow_mut().handler.take();
            crate::shared::panic_boundary::contain_owner_callback(|| drop(callback));
        }
    }

    fn observe_scene(&self, origin: &SceneOrigin, observation: LifecycleObservation) {
        let admitted = self
            .state()
            .borrow()
            .session
            .as_ref()
            .is_some_and(|record| {
                !record.closing
                    && !record.retiring
                    && record.origin.session == origin.session
                    && record.origin.attachment == origin.attachment
            });
        if !admitted {
            return;
        }
        let marker = MainThreadMarker::new().expect("BUG: scene observations run on main");
        if let Some((window, _)) = self.current(origin)
            && window.attachment_matches(origin.attachment, marker)
        {
            window.observe_lifecycle(observation);
            let can_publish = self
                .state()
                .borrow()
                .session
                .as_ref()
                .is_some_and(|record| {
                    record.origin.session == origin.session
                        && record.origin.attachment == origin.attachment
                        && record.installed
                        && !record.attaching
                        && !record.closing
                        && !record.retiring
                });
            if can_publish
                && self.signal.accepting()
                && window.is_visible()
                && window.execution_state() == crate::WindowExecutionState::Running
            {
                window.publish(origin.attachment, marker);
            }
        }
    }

    fn apply_scene(&self, action: SceneAction) {
        let marker = MainThreadMarker::new().expect("BUG: scene actions run on main");
        match action {
            SceneAction::Connect(origin, scene) => {
                if !self.signal.accepting() {
                    return;
                }
                let current = {
                    let mut state = self.state().borrow_mut();
                    state
                        .session
                        .as_mut()
                        .filter(|record| {
                            !record.closing
                                && record.origin.session == origin.session
                                && record.origin.attachment <= origin.attachment
                        })
                        .map(|record| {
                            record.origin = origin.clone();
                            record.attaching = true;
                            record.retiring = false;
                            (Arc::clone(&record.window), record.installed)
                        })
                };
                let Some((window, reconnect)) = current else {
                    return;
                };
                struct Installation<'a> {
                    platform: &'a IOSPlatform,
                    origin: Option<SceneOrigin>,
                }
                impl Drop for Installation<'_> {
                    fn drop(&mut self) {
                        if let Some(origin) = self.origin.take()
                            && self.platform.current(&origin).is_some()
                        {
                            // The scene pump is still leased: queue cleanup,
                            // never invoke a consumer during stack unwinding.
                            self.platform.enqueue(SceneAction::Rollback(origin));
                        }
                    }
                }
                let mut installation = Installation {
                    platform: self,
                    origin: Some(origin.clone()),
                };
                window.attach(&scene, origin.attachment, marker);
                if self.current(&origin).is_none() {
                    return;
                }
                let resource_generation = window.resource_generation();
                let result = self.emit_scene(IOSSceneEvent::Connected {
                    session: origin.session.clone(),
                    attachment: IOSSceneAttachmentId(origin.attachment),
                    window: Arc::clone(&window) as Arc<dyn PlatformWindow>,
                    reconnect,
                });
                if let Err(error) = result {
                    tracing::error!(%error, "scene installation failed");
                    self.enqueue(SceneAction::Rollback(origin));
                    return;
                }
                if self.current(&origin).is_none() {
                    return;
                }
                if let Some(record) = self.state().borrow_mut().session.as_mut() {
                    record.installed = true;
                    record.attaching = false;
                }
                let retiring = self.state().borrow().pending.iter().any(|action| {
                    let (SceneAction::Disconnect(pending)
                    | SceneAction::Observe(pending, LifecycleObservation::Background)) = action
                    else {
                        return false;
                    };
                    pending.session == origin.session && pending.attachment == origin.attachment
                });
                if retiring
                    || (window.resource_generation() != resource_generation && !window.is_visible())
                {
                    // Keep the acknowledged realm, but do not publish a native
                    // attachment already superseded by a queued detach intent.
                    installation.origin = None;
                    return;
                }
                window.publish(origin.attachment, marker);
                if self.current(&origin).is_none() {
                    return;
                }
                if window.resource_generation() != resource_generation {
                    installation.origin = None;
                    return;
                }
                match scene.activationState() {
                    UISceneActivationState::Background => {
                        window.observe_lifecycle(LifecycleObservation::Background);
                    }
                    UISceneActivationState::Unattached => {}
                    state => {
                        window.observe_lifecycle(LifecycleObservation::Foreground);
                        if state == UISceneActivationState::ForegroundActive {
                            window.observe_lifecycle(LifecycleObservation::Active);
                        }
                    }
                }
                installation.origin = None;
            }
            SceneAction::Observe(origin, observation) => {
                self.observe_scene(&origin, observation);
            }
            SceneAction::Disconnect(origin) => {
                if let Some((window, _)) = self.current(&origin) {
                    if !window.attachment_matches(origin.attachment, marker) {
                        return;
                    }
                    window.disconnect(marker);
                    if let Err(error) = self.emit_scene(IOSSceneEvent::Disconnected {
                        session: origin.session,
                        attachment: IOSSceneAttachmentId(origin.attachment),
                    }) {
                        tracing::error!(%error, "scene disconnect observer failed");
                    }
                }
            }
            SceneAction::Rollback(origin) => {
                let record = {
                    let mut state = self.state().borrow_mut();
                    if !state.terminal_sessions.contains(&origin.session)
                        && state.session.as_ref().is_some_and(|record| {
                            record.origin.session == origin.session
                                && record.origin.attachment == origin.attachment
                        })
                    {
                        state.session.take()
                    } else {
                        None
                    }
                };
                if let Some(record) = record {
                    record.window.close_inner();
                    if let Err(error) = self.emit_scene(IOSSceneEvent::InstallationAborted {
                        session: origin.session,
                        attachment: IOSSceneAttachmentId(origin.attachment),
                    }) {
                        tracing::error!(%error, "scene installation rollback observer failed");
                    }
                }
            }
            SceneAction::Discard(session, request_native) => {
                let record = {
                    let mut state = self.state().borrow_mut();
                    if state
                        .session
                        .as_ref()
                        .is_some_and(|record| record.origin.session == session)
                    {
                        state.session.take()
                    } else {
                        None
                    }
                };
                let Some(record) = record else { return };
                record.window.close_inner();
                if let Err(error) = self.emit_scene(IOSSceneEvent::Discarded { session }) {
                    tracing::error!(%error, "scene discard observer failed");
                }
                if request_native {
                    let handler = crate::shared::apple_scene_error::destruction_error_handler();
                    UIApplication::sharedApplication(marker)
                        .requestSceneSessionDestruction_options_errorHandler(
                            &record.session,
                            None,
                            Some(&handler),
                        );
                }
            }
        }
    }
}

pub(super) fn request_close(id: WindowId) {
    let close = move || {
        FluiAppDelegate::with_platform(|platform| {
            let session = platform
                .state()
                .borrow()
                .session
                .as_ref()
                .filter(|record| record.window.id() == id)
                .map(|record| record.origin.session.clone());
            if let Some(session) = session {
                platform.enqueue(SceneAction::Discard(session, true));
            }
        });
    };
    if MainThreadMarker::new().is_some() {
        close();
    } else {
        dispatch2::DispatchQueue::main().exec_async(close);
    }
}

pub struct SceneDelegateIvars {
    origin: RefCell<Option<(SceneOrigin, objc2::rc::Weak<UIScene>)>>,
}
define_class!(
    // SAFETY: NSObject has no extra subclass invariants. UIKit creates this
    // main-thread class through init, which initializes all Rust ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FluiSceneDelegate"]
    #[ivars = SceneDelegateIvars]
    pub struct FluiSceneDelegate;
    impl FluiSceneDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(SceneDelegateIvars { origin: RefCell::new(None) });
            unsafe { objc2::msg_send![super(this), init] }
        }
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn connect(&self, scene: &UIScene, _session: &UISceneSession, _options: &AnyObject) {
            FluiAppDelegate::with_platform(|platform| {
                if let Some(scene) = scene.downcast_ref::<UIWindowScene>() {
                    let origin = platform.connect(scene);
                    if let Some(origin) = origin {
                        *self.ivars().origin.borrow_mut() = Some((origin.clone(), objc2::rc::Weak::new(scene.as_super())));
                        platform.enqueue(SceneAction::Connect(origin, scene.retain()));
                    }
                }
            });
        }
        #[unsafe(method(sceneDidDisconnect:))]
        fn disconnect(&self, scene: &UIScene) { self.send(scene, SceneAction::Disconnect); }
        #[unsafe(method(sceneDidBecomeActive:))]
        fn active(&self, scene: &UIScene) { self.send(scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Active)); }
        #[unsafe(method(sceneWillResignActive:))]
        fn inactive(&self, scene: &UIScene) { self.send(scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Inactive)); }
        #[unsafe(method(sceneDidEnterBackground:))]
        fn background(&self, scene: &UIScene) { self.send(scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Background)); }
        #[unsafe(method(sceneWillEnterForeground:))]
        fn foreground(&self, scene: &UIScene) { self.send(scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Foreground)); }
    }
    unsafe impl NSObjectProtocol for FluiSceneDelegate {}
    unsafe impl UISceneDelegate for FluiSceneDelegate {}
);
impl FluiSceneDelegate {
    fn send(&self, scene: &UIScene, action: impl FnOnce(SceneOrigin) -> SceneAction) {
        crate::shared::panic_boundary::contain_owner_callback(|| {
            use objc2::DefinedClass;
            let origin = self
                .ivars()
                .origin
                .borrow()
                .as_ref()
                .and_then(|(origin, weak)| {
                    weak.load()
                        .filter(|current| std::ptr::eq::<UIScene>(&raw const **current, scene))
                        .map(|_| origin.clone())
                });
            if let Some(origin) = origin {
                FluiAppDelegate::with_platform(|platform| platform.enqueue(action(origin)));
            }
        });
    }
}
