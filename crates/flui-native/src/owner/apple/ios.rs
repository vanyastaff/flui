//! The iOS owner thread: the UIKit loop, its application and scene delegates,
//! and the scene sessions they connect.
//!
//! `UIApplicationMain` owns the main thread for the process's life, so this
//! backend is built around it: [`IOSLoop`]'s `run` stashes the bootstrap
//! callback and enters `UIApplicationMain`, and the `AppDelegate` this module
//! declares runs it from `application:didFinishLaunchingWithOptions:` — the
//! process bootstrap. The scene delegate owns subsequent window attachments.
//! The loop makes and drives each scene's window as a [`SceneWindow`], the
//! window feature's, without naming it; the clipboard and the other features
//! keep their code in their own directories and reach the owner thread through
//! what this module re-exports. The loop holds the displays, the preference
//! source and the notice channel through the hooks the table makes
//! ([`IosHost`]), and is the table's displays, preference and lifecycle
//! position.
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
//! contract Android's `Pause`/`InitWindow` pair carries (`owner::android::looper`
//! module doc, "Surface Lifecycle").
//!
//! # Frame source
//!
//! A `CADisplayLink` fires once per refresh; its callback requests a frame
//! through the window's callbacks. It is created with the window and paused
//! while the scene is suspended or detached. Process owner signals remain
//! independent; no OS background execution entitlement is implied.

mod executor;
mod scene;
mod scene_error;

pub(crate) use super::NativeOwner;
pub use executor::IOSExecutor;
pub use scene::{IOSSceneAttachmentId, IOSSceneSessionId};
pub(crate) use scene::{SceneConsumer, SceneInstallation};

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::owner::signal::OwnerSignal;
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
use crate::host;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::call_point::{Called, Field, call, install, retire};
use crate::owner::notices::{LifecycleHooks, NoticeChannel};
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::sources::SystemPreferenceSource;
use crate::{
    owner::DirectOwnerHooks, owner::OwnerHooks, owner::OwnerPlatform, platform::AppHandler,
    platform::Platform, platform::PlatformExecutor,
};
use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemPreferences, WindowId};

use crate::host_window::HostWindow;
use crate::owner::watch::Watch;
use flui_platform::PlatformInput;
use flui_platform::{DisplayId, DisplaySnapshot, FrameRateHint};
use objc2_quartz_core::CADisplayLink;
use objc2_ui_kit::{UIHoverGestureRecognizer, UIScreen, UITouch, UIView};

/// What the iOS host's table names for its owner thread (ADR-0151 §10): the
/// displays, the preference source and the notice channel the loop holds. The
/// owner reaches them through the hooks and names no feature.
pub(crate) trait IosHost: 'static {
    /// The displays, made once per loop on the main thread: the loop holds
    /// them for the table's displays position and hands them to every window
    /// it opens.
    fn displays(marker: MainThreadMarker) -> Arc<dyn HostDisplays>;

    /// The system preference source, made on the main thread for the table's
    /// preference position; it wakes the owner behind `signal` when what it
    /// reads changes.
    fn preferences(
        signal: &Arc<OwnerSignal>,
        marker: MainThreadMarker,
    ) -> Box<dyn UIKitPreferences>;

    /// The channel the memory warning takes to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

    /// The touches of a window's view made now: the input's part of the
    /// view, which keeps the contacts it admitted.
    fn view_touches() -> Box<dyn ViewTouches>;
}

/// Which UIKit touch callback delivered a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    /// `touchesBegan:withEvent:`.
    Down,
    /// `touchesMoved:withEvent:`.
    Move,
    /// `touchesEnded:withEvent:`.
    Up,
    /// `touchesCancelled:withEvent:`.
    Cancel,
}

/// The input's part of one window's view: the touches and the Pencil hover
/// UIKit delivers to it, which the view hands over as they arrive and
/// dispatches what they carry. Made with the view ([`IosHost::view_touches`])
/// and dropped with it, so the contacts it admitted are the view's own.
pub(crate) trait ViewTouches {
    /// What `touch`, delivered in `phase`, carries.
    fn touch(&mut self, touch: &UITouch, phase: TouchPhase) -> Vec<PlatformInput>;

    /// What the Pencil's hover over `view`, as `recognizer` reports it,
    /// carries.
    fn hover(&mut self, recognizer: &UIHoverGestureRecognizer, view: &UIView)
    -> Vec<PlatformInput>;
}

/// The system preference source the UIKit loop keeps, on the main thread
/// (ADR-0172). The loop closes it as it quits: it then answers no more and
/// stops waking the owner.
pub(crate) trait UIKitPreferences: SystemPreferenceSource {
    /// Stop observing; later samples are refused.
    fn close(&self);
}

/// The number the iOS display source gives a `UIScreen` it retains: it keys
/// the screen's display and the link periods windows measure on it, and is
/// never given to another screen while the platform lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ScreenNumber(pub(crate) u64);

/// The screen a window is on, as the display source names it: the display's
/// id, and the number the source keys the screen by. The default names no
/// screen, which a window detached from its scene has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ScreenIdentity {
    /// The display, once the snapshot can hold it.
    pub(crate) id: Option<DisplayId>,
    /// The source's number for the screen; `None` once the numbers are used
    /// up.
    pub(crate) number: Option<ScreenNumber>,
}

/// The host's screens as the loop and the windows it opens reach them, which
/// the table makes ([`IosHost::displays`]). A window's display link reports
/// the cadence it measures here, since `UIScreen` reports none.
pub(crate) trait HostDisplays: Send + Sync {
    /// The displays now: the last sample while nothing has invalidated it,
    /// otherwise a fresh one.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the snapshot revision space is used up.
    fn snapshot(&self, marker: MainThreadMarker) -> Result<DisplaySnapshot, PlatformError>;

    /// The identity of `screen`, which the source retains.
    fn identify(&self, marker: MainThreadMarker, screen: &UIScreen) -> ScreenIdentity;

    /// `window`, whose screen `slot` holds, is now on the screen `next` names
    /// (the default names none, as for a window detached from its scene).
    /// When that differs, what `window`'s display link measured on the screen
    /// it left is forgotten and the snapshot is marked stale; answers whether
    /// it differed, for which the window owes a display-changed notification.
    fn move_window(
        &self,
        window: WindowId,
        slot: &Mutex<ScreenIdentity>,
        next: ScreenIdentity,
    ) -> bool;

    /// One tick of `window`'s display link on the screen numbered `number`
    /// showed `interval_seconds` to the frame it targets. Answers whether the
    /// period the link publishes for that screen changed, which marks the
    /// snapshot stale; the window then owes the owner a wake and a
    /// display-changed notification.
    fn observe_link(
        &self,
        window: WindowId,
        number: Option<ScreenNumber>,
        interval_seconds: f64,
    ) -> bool;

    /// `window`'s display link is gone: what it measured on the screen
    /// numbered `number` is forgotten, and the snapshot is marked stale when
    /// that screen had a period from it.
    fn forget_link(&self, window: WindowId, number: Option<ScreenNumber>);

    /// Ask `link` for the rate `hint` names: the best rate `screen` sustains on
    /// the version the platform runs on, or no preference.
    fn request_frame_rate(&self, link: &CADisplayLink, screen: &UIScreen, hint: FrameRateHint);

    /// Mark the screens stale and wake the owner behind `signal` on screen and
    /// scene notifications, until the watch is dropped.
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;
}

/// What a scene's delegate reports of its window's life.
#[derive(Clone, Copy)]
pub(crate) enum LifecycleObservation {
    Active,
    Inactive,
    Background,
    Foreground,
    Detached,
}

/// A scene's window as the iOS loop drives it: the window feature's, which
/// the loop holds without naming it (ADR-0151 §10). A scene connection
/// attaches it to a `UIWindowScene`, the loop publishes it once the
/// application installed it, and a disconnection or discard retires it.
pub(crate) trait SceneWindow: HostWindow {
    /// Attach the window to `scene` as `attachment`.
    fn attach(
        &self,
        scene: &UIWindowScene,
        attachment: IOSSceneAttachmentId,
        marker: MainThreadMarker,
    );

    /// Put `attachment` on screen.
    fn publish(&self, attachment: IOSSceneAttachmentId, marker: MainThreadMarker);

    /// The window's lifecycle generation, which moves on at every background,
    /// foreground and detach: a step that waited compares it to learn whether
    /// the window moved on meanwhile.
    fn resource_generation(&self) -> u64;

    /// Whether the window's current attachment is `attachment`.
    fn attachment_matches(
        &self,
        attachment: IOSSceneAttachmentId,
        marker: MainThreadMarker,
    ) -> bool;

    /// Pause or resume the window's frame source.
    fn set_frame_tick_paused(&self, paused: bool);

    /// Tell the window what its scene did.
    fn observe_lifecycle(&self, observation: LifecycleObservation);

    /// Stop the window's frames ahead of a disconnection or discard.
    fn begin_retirement(&self);

    /// Detach the window from its scene; the window stays to reattach.
    fn disconnect(&self, marker: MainThreadMarker);

    /// Close the window for good: it reports its close and drops its native
    /// attachment, and answers nothing more.
    fn close_for_good(&self);
}

/// Makes the windows the scenes connect: the window feature's, handed to the
/// loop by the host.
pub(crate) trait SceneWindows: Send + Sync {
    /// A new window `id`, which wakes the owner through `signal`, names its
    /// screen through `displays`, hands its view's touches to `touches` and
    /// delivers to `handler`.
    fn create(
        &self,
        marker: MainThreadMarker,
        id: WindowId,
        signal: std::sync::Weak<OwnerSignal>,
        displays: Arc<dyn HostDisplays>,
        touches: Box<dyn ViewTouches>,
        handler: std::rc::Rc<dyn flui_platform::WindowHandler>,
    ) -> Arc<dyn SceneWindow>;
}

/// An iOS window as the features beside it reach it (ADR-0151 §3), on the
/// main thread: the view that outlives its scene attachments, the owner it
/// wakes, and the system chrome its root controllers answer UIKit from, which
/// the window keeps because UIKit asks the controller, not the application,
/// for the status bar. The table's text, haptics and chrome positions make
/// the window's part of them from this, as the window is made.
///
/// Public where it is defined, so the host table can name it as the window's
/// reference; the module is private, so it is nameable only inside the crate.
pub struct ViewRef {
    /// The window's view.
    pub(crate) view: Retained<UIView>,
    /// The thread the reference is read on.
    pub(crate) marker: MainThreadMarker,
    /// The owner the window wakes.
    pub(crate) signal: std::sync::Weak<OwnerSignal>,
    /// The window's system chrome.
    pub(crate) chrome: Arc<dyn flui_platform::PlatformSystemChrome>,
}

impl std::fmt::Debug for ViewRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewRef").finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct SceneOrigin {
    session: IOSSceneSessionId,
    attachment: IOSSceneAttachmentId,
}
struct SessionRecord {
    origin: SceneOrigin,
    session: Retained<UISceneSession>,
    window: Arc<dyn SceneWindow>,
    installed: bool,
    attaching: bool,
    retiring: bool,
    closing: bool,
    installation: Option<PendingSceneInstallation>,
}

struct PendingSceneInstallation {
    origin: SceneOrigin,
    scene: Retained<UIWindowScene>,
    resource_generation: u64,
    completion: Box<dyn SceneInstallation>,
}

struct SceneInstallationGuard<'a> {
    platform: &'a IOSLoop,
    origin: Option<SceneOrigin>,
}
impl Drop for SceneInstallationGuard<'_> {
    fn drop(&mut self) {
        if let Some(origin) = self.origin.take()
            && self.platform.current(&origin).is_some()
        {
            self.platform.enqueue(SceneAction::Rollback(origin));
        }
    }
}
enum SceneAction {
    Connect(SceneOrigin, Retained<UIWindowScene>),
    Publish(PendingSceneInstallation),
    Observe(SceneOrigin, LifecycleObservation),
    Disconnect(SceneOrigin),
    Discard(IOSSceneSessionId, bool),
    Rollback(SceneOrigin),
}
struct SceneState {
    terminal_sessions: HashSet<IOSSceneSessionId>,
    session: Option<SessionRecord>,
    handler: Option<Box<dyn SceneConsumer>>,
    registered: bool,
    reservation: Option<SceneOrigin>,
    pending: VecDeque<SceneAction>,
    draining: bool,
    next_window: u64,
    next_attachment: u64,
}

/// The iOS host's loop: the state every position of its table reads.
///
/// [`IOSPlatform`](crate::IOSPlatform) is its platform.
pub struct IOSLoop {
    /// What the loop delivers to the application's handler, which the
    /// bootstrap binds.
    app: Arc<AppDelivery>,
    /// Makes the window a scene session connects.
    windows: Arc<dyn SceneWindows>,
    running: Arc<AtomicBool>,
    scenes: NativeOwner<RefCell<SceneState>>,
    /// The system preference source the table made.
    preferences: NativeOwner<Box<dyn UIKitPreferences>>,
    signal: Arc<OwnerSignal>,
    identity: Arc<()>,
    deadline: Mutex<Option<(Arc<()>, web_time::Instant)>>,
    background_executor: Arc<IOSExecutor>,
    /// The screens, as the table made them; shared with every window.
    pub(crate) displays: Arc<dyn HostDisplays>,
    /// Marks `displays` stale and wakes the owner on screen and scene
    /// notifications; main-thread owned like `preferences`.
    _screen_observer: NativeOwner<Watch>,
    /// Where the memory warning goes on its way to the host.
    notices: Arc<dyn NoticeChannel>,
    /// Makes the touches of each window's view, as the table names them.
    view_touches: fn() -> Box<dyn ViewTouches>,
}

impl std::fmt::Debug for IOSLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSLoop").finish_non_exhaustive()
    }
}

impl IOSLoop {
    /// A loop over the displays `H` makes, whose scenes' windows `windows`
    /// makes.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] off the main thread.
    pub(crate) fn new<H: IosHost>(windows: Arc<dyn SceneWindows>) -> Result<Self, PlatformError> {
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
                os_entry(
                    "the UIKit owner turn",
                    (),
                    AfterPanic::Answer(|()| ()),
                    move || {
                        FluiAppDelegate::with_platform(|platform| {
                            if Arc::ptr_eq(&platform.identity, &identity) {
                                let quitting = platform.signal.drive();
                                platform.poll_scene_installation();
                                if quitting {
                                    platform.finish_quit();
                                } else {
                                    crate::owner::panic_boundary::contain_owner_callback(|| {
                                        platform.arm_deadline();
                                    });
                                }
                            }
                        });
                    },
                );
            });
            Ok(())
        }));
        let displays = H::displays(marker);
        let screen_observer = Arc::clone(&displays).observe(&signal);
        let app = Arc::new(AppDelivery::new());
        Ok(Self {
            app: Arc::clone(&app),
            windows,
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
            preferences: NativeOwner::new(H::preferences(&signal, marker), marker),
            signal,
            identity,
            deadline: Mutex::new(None),
            background_executor: Arc::new(IOSExecutor),
            displays,
            _screen_observer: NativeOwner::new(screen_observer, marker),
            notices: H::notices(&app),
            view_touches: H::view_touches,
        })
    }

    fn state(&self) -> &RefCell<SceneState> {
        self.scenes
            .get(MainThreadMarker::new().expect("BUG: UIKit scene owner is main-thread-only"))
    }

    /// Register the application's consumer of scene sessions, before UIKit
    /// connects one; a later registration replaces it.
    pub(crate) fn install_scene_consumer(
        &self,
        consumer: Box<dyn SceneConsumer>,
    ) -> Result<(), PlatformError> {
        if MainThreadMarker::new().is_none() || !self.signal.accepting() {
            return Err(PlatformError::Init {
                message: "scene registration requires a live main-thread owner".into(),
            });
        }
        self.state().borrow_mut().registered = true;
        install(
            &Field::new(self.state(), |state| &mut state.handler),
            consumer,
        );
        Ok(())
    }

    /// Tell the scene consumer what `tell` says, through the call point, with
    /// no borrow of the scene state held. It goes back afterwards while the
    /// owner still accepts work; a consumer registered meanwhile wins.
    fn tell_scene_consumer<T>(
        &self,
        tell: impl FnOnce(&mut dyn SceneConsumer) -> Result<T, crate::BootstrapError>,
    ) -> Result<T, crate::BootstrapError> {
        let consumer = Field::new(self.state(), |state| &mut state.handler)
            .restoring_while(|| self.signal.accepting());
        match call(&consumer, |consumer| tell(consumer.as_mut())) {
            Called::Vacant => Err(Box::new(PlatformError::Init {
                message: "no scene consumer registered".into(),
            })),
            Called::Returned(result) => result,
            Called::Panicked(failure) => {
                failure.contain("the scene consumer");
                Err(Box::new(PlatformError::Init {
                    message: "scene consumer panicked".into(),
                }))
            }
        }
    }

    /// Retire the scene consumer outside the scene state's borrow.
    fn retire_scene_consumer(&self) {
        let consumer = self.state().borrow_mut().handler.take();
        retire(consumer);
    }

    pub(crate) fn active(&self) -> Option<Arc<dyn SceneWindow>> {
        self.state()
            .borrow()
            .session
            .as_ref()
            .map(|record| Arc::clone(&record.window))
    }

    /// UIKit is about to terminate the process: let the application write what
    /// must survive. There is no way to ask it to wait, so the answer is not
    /// read.
    fn announce_session_end(&self) {
        let _answer = self
            .app
            .session_end(SessionEndPhase::Ending)
            .or_contain(SessionEndAnswer::Proceed, "the session-end handler");
    }

    /// UIKit's memory warning means the system is close to terminating the
    /// application, which is the critical level.
    fn warn_of_memory_pressure(&self) {
        self.notices
            .dispatch(flui_platform::SystemNotice::MemoryPressure(
                flui_foundation::memory::MemoryPressureLevel::Critical,
            ));
    }

    /// Tell the application's handler the platform quits, then let it go.
    fn invoke_quit(&self) {
        self.app
            .quit_requested()
            .or_contain((), "the application's quit");
        self.app.release();
    }

    fn finish_quit(&self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        self.signal.close();
        self.deadline.lock().take();
        self.preferences
            .get(MainThreadMarker::new().expect("BUG: UIKit quit runs on main"))
            .close();
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
            self.retire_scene_consumer();
        }
    }

    fn arm_deadline(&self) {
        if !self.signal.accepting() {
            return;
        }
        let next = self.app.wake_deadline().or_resume(None);
        let ticket = {
            let mut current = self.deadline.lock();
            if current.as_ref().map(|(_, deadline)| *deadline) == next {
                return;
            }
            let Some(deadline) = next else {
                current.take();
                return;
            };
            let ticket = Arc::new(());
            *current = Some((Arc::clone(&ticket), deadline));
            ticket
        };
        let deadline = next.expect("BUG: an armed ticket retains a deadline");
        let delay = deadline.saturating_duration_since(web_time::Instant::now());
        let Ok(when) = dispatch2::DispatchTime::try_from(delay) else {
            self.deadline.lock().take();
            tracing::warn!("UIKit owner deadline exceeds dispatch time range");
            return;
        };
        let identity = Arc::clone(&self.identity);
        let _ = dispatch2::DispatchQueue::main().after(when, move || {
            os_entry(
                "the UIKit owner deadline",
                (),
                AfterPanic::Answer(|()| ()),
                move || {
                    FluiAppDelegate::with_platform(|platform| {
                        if !Arc::ptr_eq(&platform.identity, &identity) {
                            return;
                        }
                        let admitted = {
                            let mut current = platform.deadline.lock();
                            if current
                                .as_ref()
                                .is_some_and(|(active, _)| Arc::ptr_eq(active, &ticket))
                            {
                                current.take();
                                true
                            } else {
                                false
                            }
                        };
                        if admitted {
                            let _ = platform.signal.wake();
                        }
                    });
                },
            );
        });
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
        handler: None,
        hooks: None,
        platform: None,
        owner_platform: None,
    }) };
}

struct DelegateState {
    /// Taken exactly once, by `didFinishLaunching`.
    handler: Option<Rc<dyn AppHandler>>,
    /// The owner hooks of the run, kept for the session: the delivery holds
    /// them weakly.
    hooks: Option<Arc<dyn OwnerHooks>>,
    /// Live for the whole session; read by every lifecycle callback.
    platform: Option<Arc<IOSLoop>>,
    /// The platform over `platform` that the owner capability is built on,
    /// handed in by `run`.
    owner_platform: Option<Arc<dyn Platform>>,
}

impl host::Loop for IOSLoop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Init {
            message: "IOSPlatform::run must be called on the main thread".to_string(),
        })?;

        if DELEGATE_STATE.with(|state| state.borrow().platform.is_some()) {
            return Err(PlatformError::Init {
                message: "UIKit process owner is already installed".into(),
            });
        }
        tracing::info!("Starting iOS platform event loop (UIApplicationMain)");

        self.running.store(true, Ordering::Relaxed);

        DELEGATE_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.platform = Some(self);
            state.owner_platform = Some(platform);
            state.handler = Some(handler);
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

    fn request_exit_policy_reevaluation(&self) {
        // With no exit policy there is nothing to consult again.
    }
}

impl host::Services for IOSLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.background_executor.clone()
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

    // The iOS host opens no URL, path or file prompt and reports no
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

/// The displays position is the loop's: it answers from the screens the table
/// made ([`IosHost::displays`]), on the main thread.
impl host::Displays<IOSLoop> for IOSLoop {
    fn snapshot(owner: &IOSLoop) -> Result<DisplaySnapshot, PlatformError> {
        let marker = MainThreadMarker::new().ok_or_else(|| PlatformError::Displays {
            message: "UIKit display sampling must run on the main thread".into(),
        })?;
        owner.displays.snapshot(marker)
    }
}

/// The preference position is the loop's: it reads the source the table made
/// ([`IosHost::preferences`]), on the main thread.
impl host::Preferences<IOSLoop> for IOSLoop {
    fn sample(owner: &IOSLoop) -> Result<SystemPreferences, PlatformError> {
        let marker = MainThreadMarker::new().ok_or_else(|| PlatformError::Preferences {
            message: "UIKit preference sampling must run on the main thread".into(),
        })?;
        owner.preferences.get(marker).sample()
    }
}

/// UIKit tells the application delegate, which is the loop's, that the process
/// is about to terminate and that memory is low; it cannot be asked to wait
/// and has no reopen. The lifecycle feature's position reaches the hooks
/// through this.
impl LifecycleHooks for IOSLoop {
    fn release_session_end(&self) {
        // The answer to a session end is not read, so no session end is held
        // open.
    }
}

/// Run the bootstrap stashed by [`IOSLoop`]'s `run`, on the main thread,
/// from `didFinishLaunching`. Returns `true` if a bootstrap ran.
fn run_pending_bootstrap() -> bool {
    let (platform, owner_platform, handler) = DELEGATE_STATE.with(|state| {
        let mut state = state.borrow_mut();
        (
            state.platform.clone(),
            state.owner_platform.clone(),
            state.handler.take(),
        )
    });
    let (Some(platform), Some(owner_platform), Some(handler)) = (platform, owner_platform, handler)
    else {
        return false;
    };

    let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
        Arc::clone(&owner_platform),
        Arc::clone(&platform.signal),
    ));
    if !platform.signal.accepting() {
        crate::owner::panic_boundary::contain_owner_callback(|| drop(handler));
        platform.finish_quit();
        return false;
    }
    // The delivery holds the platform and the hooks weakly: the delegate's
    // state keeps them for the session, which `UIApplicationMain` never ends.
    platform.app.bind(handler, &owner_platform, &hooks);
    platform.app.resume_through_signal(&platform.signal);
    DELEGATE_STATE.with(|state| state.borrow_mut().hooks = Some(Arc::clone(&hooks)));
    // The bootstrap's panic is caught here: UIKit's callback cannot unwind.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        platform
            .app
            .ready(OwnerPlatform::new(owner_platform, hooks))
    }));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            crate::owner::panic_boundary::contain_owner_callback(
                || tracing::error!(%error, "iOS bootstrap failed"),
            );
            platform.finish_quit();
            crate::owner::panic_boundary::contain_owner_callback(|| drop(error));
            return false;
        }
        Err(payload) => {
            std::mem::forget(payload);
            platform.finish_quit();
            return false;
        }
    }
    if let Err(error) = platform.signal.start() {
        crate::owner::panic_boundary::contain_owner_callback(
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
            // A bootstrap that panicked did not launch.
            os_entry(
                "application:didFinishLaunchingWithOptions:",
                (), AfterPanic::Answer(|()| false),
                run_pending_bootstrap,
            )
        }

        /// UIKit needs a configuration to connect the scene; a panic leaves
        /// none to give, so the process ends.
        #[unsafe(method_id(application:configurationForConnectingSceneSession:options:))]
        fn configuration(&self, _application: &UIApplication, session: &UISceneSession, _options: &AnyObject) -> Retained<UISceneConfiguration> {
            os_entry("application:configurationForConnectingSceneSession:options:", (), AfterPanic::Abort, || {
                let marker = MainThreadMarker::new().expect("BUG: UIKit delegate runs on main");
                let configuration = UISceneConfiguration::initWithName_sessionRole(marker.alloc(), Some(&NSString::from_str("FLUI")), &session.role());
                // SAFETY: the registered class implements UISceneDelegate.
                unsafe { configuration.setDelegateClass(Some(FluiSceneDelegate::class())); }
                configuration
            })
        }

        #[unsafe(method(application:didDiscardSceneSessions:))]
        fn discarded(&self, _application: &UIApplication, sessions: &NSSet<UISceneSession>) {
            os_entry("application:didDiscardSceneSessions:", (), AfterPanic::Answer(|()| ()), || {
                Self::with_platform(|platform| {
                    for session in sessions { platform.enqueue(SceneAction::Discard(IOSSceneSessionId(session.persistentIdentifier().to_string()), false)); }
                });
            });
        }

        /// If UIKit sends termination, tell the application its session is
        /// ending, then notify the loop. OS termination is not guaranteed to
        /// deliver this callback. This is currently where we run its loop-exit
        /// signal: `UIApplicationMain`'s loop never returns, so there is no
        /// "after `run`" for the runner to use. Each step is contained apart.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _application: &UIApplication) {
            os_entry("applicationWillTerminate:", (), AfterPanic::Answer(|()| ()), || {
                crate::owner::panic_boundary::contain_owner_callback(|| tracing::info!("UIApplication willTerminate — ending the session, then firing the quit handler"));
                Self::with_platform(IOSLoop::announce_session_end);
                Self::with_platform(IOSLoop::finish_quit);
            });
        }

        /// UIKit asks the application to free memory it can rebuild: the
        /// system is close to terminating it.
        #[unsafe(method(applicationDidReceiveMemoryWarning:))]
        fn did_receive_memory_warning(&self, _application: &UIApplication) {
            os_entry("applicationDidReceiveMemoryWarning:", (), AfterPanic::Answer(|()| ()), || {
                Self::with_platform(IOSLoop::warn_of_memory_pressure);
            });
        }
    }

    unsafe impl NSObjectProtocol for FluiAppDelegate {}
    unsafe impl UIApplicationDelegate for FluiAppDelegate {}
);

/// The delegate holds no state of its own; the platform is reached through
/// the thread-local session slot (see [`DELEGATE_STATE`]).
pub struct FluiDelegateIvars;

impl FluiAppDelegate {
    fn with_platform(body: impl FnOnce(&IOSLoop)) {
        // Reached through the session slot, which `run` installs before
        // `UIApplicationMain` and which outlives the one-shot `on_ready`
        // take: the lifecycle callbacks keep arriving for the whole session.
        let platform = DELEGATE_STATE.with(|state| state.borrow().platform.clone());
        if let Some(platform) = platform {
            crate::owner::panic_boundary::contain_owner_callback(|| body(&platform));
        }
    }
}

impl IOSLoop {
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
                    attachment: IOSSceneAttachmentId(attachment),
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
            // The window delivers to its handler from its first event on, so
            // the application makes it before the window exists. No handler,
            // no window: the reservation is released and the scene refused.
            let handler = match self
                .tell_scene_consumer(|consumer| Ok(consumer.window_handler(&origin.session)))
            {
                Ok(handler) => handler,
                Err(error) => {
                    self.state().borrow_mut().reservation = None;
                    tracing::warn!(
                        session = origin.session.as_str(),
                        %error,
                        "rejecting a scene whose window has no handler"
                    );
                    return None;
                }
            };
            let window = self.windows.create(
                marker,
                id,
                Arc::downgrade(&self.signal),
                Arc::clone(&self.displays),
                (self.view_touches)(),
                handler,
            );
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
                        installation: None,
                    });
                }
                admitted
            };
            if !admitted {
                window.close_for_good();
                return None;
            }
        }
        Some(origin)
    }

    fn current(&self, origin: &SceneOrigin) -> Option<(Arc<dyn SceneWindow>, bool)> {
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
            crate::owner::panic_boundary::contain_owner_callback(|| self.apply_scene(action));
        }
        drop(guard);
        self.poll_scene_installation();
        if !self.running.load(Ordering::SeqCst) {
            self.invoke_quit();
            self.retire_scene_consumer();
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
                && window.state().visible
                && window.state().execution == flui_platform::WindowExecutionState::Running
            {
                window.publish(origin.attachment, marker);
            }
        }
    }

    fn poll_scene_installation(&self) {
        let completed = {
            let mut state = self.state().borrow_mut();
            state.session.as_mut().and_then(|record| {
                let installed = record.installation.as_ref()?.completion.installed()?;
                Some((
                    record
                        .installation
                        .take()
                        .expect("BUG: observed installation is retained"),
                    installed,
                ))
            })
        };
        if let Some((pending, installed)) = completed {
            if installed {
                self.enqueue(SceneAction::Publish(pending));
            } else {
                self.enqueue(SceneAction::Rollback(pending.origin));
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
                            (
                                Arc::clone(&record.window),
                                record.installed,
                                record.installation.take(),
                            )
                        })
                };
                let Some((window, reconnect, replaced)) = current else {
                    return;
                };
                drop(replaced);
                let mut installation = SceneInstallationGuard {
                    platform: self,
                    origin: Some(origin.clone()),
                };
                window.attach(&scene, origin.attachment, marker);
                if self.current(&origin).is_none() {
                    return;
                }
                let resource_generation = window.resource_generation();
                let proxy = crate::PlatformProxy::new(Arc::new(
                    crate::owner::signal::SignalTransport::new(&self.signal),
                ));
                let result = self.tell_scene_consumer(|consumer| {
                    consumer.connected(
                        origin.session.clone(),
                        origin.attachment,
                        Arc::clone(&window) as Arc<dyn HostWindow>,
                        reconnect,
                        proxy,
                    )
                });
                let completion = match result {
                    Ok(completion) => completion,
                    Err(error) => {
                        tracing::error!(%error, "scene installation failed");
                        self.enqueue(SceneAction::Rollback(origin));
                        return;
                    }
                };
                // The record keeps what the loop watches of the installation;
                // the poll below reads an outcome the application already
                // reached.
                if let Some(record) = self.state().borrow_mut().session.as_mut() {
                    record.installation = Some(PendingSceneInstallation {
                        origin: origin.clone(),
                        scene,
                        resource_generation,
                        completion,
                    });
                }
                installation.origin = None;
                self.poll_scene_installation();
            }
            SceneAction::Publish(pending) => {
                let PendingSceneInstallation {
                    origin,
                    scene,
                    resource_generation,
                    ..
                } = pending;
                let mut installation = SceneInstallationGuard {
                    platform: self,
                    origin: Some(origin.clone()),
                };
                let Some((window, _)) = self.current(&origin) else {
                    return;
                };
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
                    || (window.resource_generation() != resource_generation
                        && !window.state().visible)
                {
                    // Keep the acknowledged ui_runtime, but do not publish a native
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
                    if let Err(error) = self.tell_scene_consumer(|consumer| {
                        consumer.disconnected(origin.session, origin.attachment)
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
                    record.window.close_for_good();
                    if let Err(error) = self.tell_scene_consumer(|consumer| {
                        consumer.installation_aborted(origin.session, origin.attachment)
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
                record.window.close_for_good();
                if let Err(error) = self.tell_scene_consumer(|consumer| consumer.discarded(session))
                {
                    tracing::error!(%error, "scene discard observer failed");
                }
                if request_native {
                    let handler = scene_error::destruction_error_handler();
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

pub(crate) fn request_close(id: WindowId) {
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
        dispatch2::DispatchQueue::main().exec_async(move || {
            os_entry("a scene close", (), AfterPanic::Answer(|()| ()), close);
        });
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
        /// UIKit needs the initialised delegate; a panic leaves none to give,
        /// so the process ends.
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            os_entry("FluiSceneDelegate's init", (), AfterPanic::Abort, || {
                let this = this.set_ivars(SceneDelegateIvars { origin: RefCell::new(None) });
                // SAFETY: `NSObject`'s `init` on the allocated receiver whose
                // Rust ivars were set just above, as `define_class!` requires.
                unsafe { objc2::msg_send![super(this), init] }
            })
        }
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn connect(&self, scene: &UIScene, _session: &UISceneSession, _options: &AnyObject) {
            os_entry("scene:willConnectToSession:options:", (), AfterPanic::Answer(|()| ()), || {
                FluiAppDelegate::with_platform(|platform| {
                    if let Some(scene) = scene.downcast_ref::<UIWindowScene>() {
                        let origin = platform.connect(scene);
                        if let Some(origin) = origin {
                            *self.ivars().origin.borrow_mut() = Some((origin.clone(), objc2::rc::Weak::new(scene.as_super())));
                            platform.enqueue(SceneAction::Connect(origin, scene.retain()));
                        }
                    }
                });
            });
        }
        #[unsafe(method(sceneDidDisconnect:))]
        fn disconnect(&self, scene: &UIScene) { Self::scene_event(self, scene, SceneAction::Disconnect); }
        #[unsafe(method(sceneDidBecomeActive:))]
        fn active(&self, scene: &UIScene) { Self::scene_event(self, scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Active)); }
        #[unsafe(method(sceneWillResignActive:))]
        fn inactive(&self, scene: &UIScene) { Self::scene_event(self, scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Inactive)); }
        #[unsafe(method(sceneDidEnterBackground:))]
        fn background(&self, scene: &UIScene) { Self::scene_event(self, scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Background)); }
        #[unsafe(method(sceneWillEnterForeground:))]
        fn foreground(&self, scene: &UIScene) { Self::scene_event(self, scene, |origin| SceneAction::Observe(origin, LifecycleObservation::Foreground)); }
    }
    unsafe impl NSObjectProtocol for FluiSceneDelegate {}
    unsafe impl UISceneDelegate for FluiSceneDelegate {}
);
impl FluiSceneDelegate {
    /// The boundary of the scene's lifecycle methods: `action` of the scene's
    /// origin is queued, while the scene is the one this delegate connected.
    fn scene_event(&self, scene: &UIScene, action: impl FnOnce(SceneOrigin) -> SceneAction) {
        os_entry(
            "a UIKit scene event",
            (),
            AfterPanic::Answer(|()| ()),
            || {
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
            },
        );
    }
}
