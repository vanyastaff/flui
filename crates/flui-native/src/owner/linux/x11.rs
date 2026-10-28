//! The X11 host's loop (ADR-0151 §10, §11): one connection to the X server,
//! read on the owner thread, and a doorbell beside it that wakes the loop.
//!
//! # The connection
//!
//! [`X11Connection`] is the libxcb connection the loop reads every event of
//! and the windows send their requests on. It is one per platform and is made
//! when the platform is: a window's surface names it as its display, so a
//! renderer's surface lives on the connection the window lives on. libxcb is
//! thread-safe; the owner thread alone reads events off it.
//!
//! # The loop
//!
//! [`X11Loop::run`] waits in `poll` on two descriptors, the connection and the
//! doorbell (an eventfd), with no timeout but the application's wake deadline:
//! an idle application costs no CPU. A turn of the loop:
//!
//! 1. reads the doorbell, before anything it rang for is read, so a ring that
//!    lands while the turn runs leaves the doorbell readable for the next one;
//! 2. delivers a wake deadline that came due as a frame request to every
//!    window ([`WakeDeadline`], which keeps a deadline no frame could service
//!    from spinning the loop);
//! 3. hands every event the server sent to the windows
//!    ([`LoopWindows::event`]);
//! 4. begins the owner turn, delivers what the system's sources queued (host
//!    notices, the portal's colour scheme, a change of the displays), and
//!    drives the turn (the application's wake callback, a window's work raised
//!    on another thread);
//! 5. delivers the frames the windows asked for, then asks the exit policy if a
//!    window closed or the application asked again;
//! 6. reads the events the turn's requests left queued, and waits only when
//!    none is queued and no window waits for a frame.
//!
//! A request that waits for its reply (a window reading its state) may read
//! events off the socket into libxcb's queue, where `poll` cannot see them;
//! step 6 takes them before the loop waits, so none waits for an unrelated
//! wake.
//!
//! # What the loop names
//!
//! It names no feature (ADR-0151 §10). Its windows are the window feature's,
//! reached through [`LoopWindows`]; the displays, the preference source and
//! the host notices are what the table makes through [`X11Host`]. A wake from
//! any thread (the owner signal, a window's redraw requester, an exit-policy
//! request) rings the [`Doorbell`].

use std::collections::VecDeque;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use flui_platform::{
    DisplayRevision, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, SystemNotice,
    SystemPreferences, WindowAppearance, WindowId,
};
use parking_lot::Mutex;
use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::io::Errno;
use x11rb::connection::Connection as _;
use x11rb::protocol::{Event, xproto};
use x11rb::xcb_ffi::XCBConnection;

use crate::error::PlatformError;
use crate::executor::BackgroundExecutor;
use crate::host;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::handlers::OpenWindows;
use crate::owner::notices::{LifecycleHooks, NoticeChannel, StaleOnWake};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::signal::OwnerSignal;
use crate::owner::sources::SystemPreferenceSource;
use crate::owner::wake_deadline::WakeDeadline;
use crate::owner::watch::Watch;
use crate::owner::{DirectOwnerHooks, OwnerHooks, OwnerPlatform};
use crate::platform::{AppHandler, PathPromptOptions, Platform, PlatformExecutor};
use crate::task::Task;

/// The connection to the X server the platform reads events from and its
/// windows send requests on.
pub(crate) struct X11Connection {
    xcb: XCBConnection,
    screen: usize,
}

impl std::fmt::Debug for X11Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Connection")
            .field("screen", &self.screen)
            .finish_non_exhaustive()
    }
}

impl X11Connection {
    /// Connect to the X server `DISPLAY` names, loading libxcb.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when libxcb cannot be loaded, no server answers
    /// or the server lists no screen of the display's number.
    pub(crate) fn connect() -> Result<Arc<Self>, PlatformError> {
        let (xcb, screen) = XCBConnection::connect(None).map_err(|error| PlatformError::Init {
            message: format!("cannot connect to the X server: {error}"),
        })?;
        if xcb.setup().roots.get(screen).is_none() {
            return Err(PlatformError::Init {
                message: format!("the X server lists no screen {screen}"),
            });
        }
        Ok(Arc::new(Self { xcb, screen }))
    }

    /// The connection, for requests.
    pub(crate) fn xcb(&self) -> &XCBConnection {
        &self.xcb
    }

    /// The number of the screen the platform's windows open on.
    pub(crate) fn screen_number(&self) -> usize {
        self.screen
    }

    /// The screen the platform's windows open on.
    pub(crate) fn screen(&self) -> &xproto::Screen {
        self.xcb
            .setup()
            .roots
            .get(self.screen)
            .expect("BUG: the screen was checked when the connection was made")
    }

    /// The root window of the screen.
    pub(crate) fn root(&self) -> xproto::Window {
        self.screen().root
    }
}

/// What wakes the loop from any thread: an eventfd the loop waits on beside
/// the connection.
#[derive(Clone)]
pub(crate) struct Doorbell(Arc<OwnedFd>);

impl std::fmt::Debug for Doorbell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Doorbell").finish_non_exhaustive()
    }
}

impl Doorbell {
    fn new() -> Result<Self, PlatformError> {
        eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
            .map(|fd| Self(Arc::new(fd)))
            .map_err(|error| PlatformError::Init {
                message: format!("cannot create the X11 loop's wake descriptor: {error}"),
            })
    }

    /// Wake the loop. The descriptor stays readable until the loop reads it,
    /// so a ring is never lost, and rings before the read are one wake.
    pub(crate) fn ring(&self) -> Result<(), PlatformError> {
        match rustix::io::write(self.0.as_fd(), &1_u64.to_ne_bytes()) {
            // A full counter is a pending wake already.
            Ok(_) | Err(Errno::AGAIN) => Ok(()),
            Err(error) => Err(PlatformError::EventLoop {
                message: format!("cannot wake the X11 loop: {error}"),
            }),
        }
    }

    /// Take the rings so far; the loop reads it before it reads what they
    /// asked for.
    fn answer(&self) {
        let mut count = [0_u8; 8];
        match rustix::io::read(self.0.as_fd(), &mut count) {
            Ok(_) | Err(Errno::AGAIN) => {}
            Err(error) => tracing::error!(%error, "cannot read the X11 loop's wake descriptor"),
        }
    }
}

/// What the X11 host's table makes for its loop (ADR-0151 §10): the displays,
/// the channel system notices take, the Settings portal's source and the
/// sources of the host's notices. The loop holds each in a field typed by a
/// hook declared here, and names no feature.
pub(crate) trait X11Host: 'static {
    /// The displays: the X server's outputs, read on a connection of their own.
    fn displays() -> Arc<dyn HostDisplays>;

    /// The channel the host's notices take to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

    /// The Settings portal, which the loop starts as it runs.
    fn preferences() -> Box<dyn PreferenceStart>;

    /// The sources of the host's notices (logind, memory pressure), which the
    /// loop starts once the application can hear what they report.
    fn host_notices() -> Box<dyn HostNoticeStart>;
}

/// The displays as the X11 loop keeps them: the snapshot, and the watch that
/// wakes the owner when the outputs change. A wake from sleep marks them
/// stale.
pub(crate) trait HostDisplays: StaleOnWake {
    /// The display snapshot.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the server cannot be read.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    /// Watch the outputs until the returned watch drops, waking the owner
    /// behind `signal` when they change.
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch;
}

/// The Settings portal as the X11 loop starts it, on its owner thread, before
/// the application's bootstrap.
pub(crate) trait PreferenceStart: Send + Sync {
    /// Observe the portal on `executor`, waking the owner behind `signal` when
    /// what it accepted changes.
    fn start(
        &self,
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn PortalPreferences>;
}

/// The portal's preferences as the X11 loop keeps them: the snapshot, and the
/// colour scheme every window follows.
pub(crate) trait PortalPreferences: SystemPreferenceSource + Send + Sync {
    /// The colour scheme the portal reports.
    fn appearance(&self) -> WindowAppearance;
}

/// The sources of the host's notices as the X11 loop starts them.
pub(crate) trait HostNoticeStart: Send + Sync {
    /// Start the sources on `executor`; they wake the owner behind `signal`
    /// when something is queued.
    fn start(
        &self,
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Box<dyn HostNotices>;
}

/// The sources of the host's notices the X11 loop keeps.
pub(crate) trait HostNotices {
    /// Deliver what the sources queued since the loop's last turn, oldest
    /// first, to `platform`'s notice channel and session end.
    fn deliver(&self, platform: &X11Loop);
}

/// The windows the X11 loop holds through its hook: the window feature's,
/// which opens and keeps them (ADR-0151 §10). Every method runs on the owner
/// thread, with no lock of the loop held.
pub(crate) trait LoopWindows: OpenWindows {
    /// An event the server sent. The window it names handles it; an event of
    /// the root window may concern every window.
    fn event(&self, event: &Event);

    /// Deliver a frame request to every window that asked for one since the
    /// last call.
    fn deliver_frames(&self);

    /// Whether a window waits for a frame, so the loop must not wait.
    fn wants_frame(&self) -> bool;

    /// The application's wake deadline came due: every window is asked for a
    /// frame, the one path the deadline's work reaches.
    fn redraw_for_deadline(&self);

    /// Whether a window closed since the last call.
    fn take_closed(&self) -> bool;

    /// What every window reads beside its own reports changed (the displays):
    /// each reads itself again.
    fn refresh(&self);

    /// The colour scheme the system applies changed.
    fn observe_appearance(&self, appearance: WindowAppearance);

    /// How many frames the windows' handlers produced.
    fn frames(&self) -> u64;

    /// The loop ends: each window still open is let go of, its native window
    /// destroyed once its handler is.
    fn release(&self);
}

/// The X11 host's loop: the connection, the doorbell, and what the table made
/// for it.
pub struct X11Loop {
    connection: Arc<X11Connection>,
    doorbell: Doorbell,
    signal: Arc<OwnerSignal>,
    app: Arc<AppDelivery>,
    notices: Arc<dyn NoticeChannel>,
    displays: Arc<dyn HostDisplays>,
    windows: Arc<dyn LoopWindows>,
    preference_start: Box<dyn PreferenceStart>,
    host_notice_start: Box<dyn HostNoticeStart>,
    background_executor: Arc<BackgroundExecutor>,
    /// The preference source while the loop runs.
    preferences: Mutex<Option<Weak<dyn PortalPreferences>>>,
    /// The application asked for its exit policy again.
    exit_reevaluation: AtomicBool,
    /// The thread that built the platform, which its windows belong to and
    /// which alone runs the loop.
    affinity: flui_foundation::OwnerAffinity,
}

impl std::fmt::Debug for X11Loop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Loop")
            .field("connection", &self.connection)
            .finish_non_exhaustive()
    }
}

impl X11Loop {
    /// The loop of host `H` over `connection`, whose windows are `windows`,
    /// the window feature's. The calling thread owns it: the platform's
    /// windows belong to it and it alone runs the loop.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when the doorbell cannot be made.
    pub(crate) fn new<H: X11Host>(
        connection: Arc<X11Connection>,
        windows: Arc<dyn LoopWindows>,
    ) -> Result<Self, PlatformError> {
        let doorbell = Doorbell::new()?;
        let rung = doorbell.clone();
        let signal = OwnerSignal::new(Arc::new(move || rung.ring()));
        let app = Arc::new(AppDelivery::new());
        let notices = H::notices(&app);
        let displays = H::displays();
        notices.mark_held_stale_on_wake(Arc::downgrade(&displays) as Weak<dyn StaleOnWake>);
        let affinity = flui_foundation::OwnerAffinity::new();
        affinity.bind_current();
        Ok(Self {
            connection,
            doorbell,
            signal,
            app,
            notices,
            displays,
            windows,
            preference_start: H::preferences(),
            host_notice_start: H::host_notices(),
            background_executor: Arc::new(BackgroundExecutor::new()),
            preferences: Mutex::new(None),
            exit_reevaluation: AtomicBool::new(false),
            affinity,
        })
    }

    /// Whether the caller is the thread that built the platform, which owns
    /// its windows.
    pub(crate) fn is_owner_thread(&self) -> bool {
        self.affinity.is_owner()
    }

    /// What wakes the loop from another thread.
    pub(crate) fn doorbell(&self) -> &Doorbell {
        &self.doorbell
    }

    /// The owner signal, whose turns deliver what another thread raised.
    pub(crate) fn signal(&self) -> &Arc<OwnerSignal> {
        &self.signal
    }

    /// The displays, which a window names its display from.
    pub(crate) fn displays(&self) -> &Arc<dyn HostDisplays> {
        &self.displays
    }

    /// The notice channel, which marks a window's state stale on a wake.
    pub(crate) fn notices(&self) -> &Arc<dyn NoticeChannel> {
        &self.notices
    }

    /// Report `notice` to the application.
    pub(crate) fn notice(&self, notice: SystemNotice) {
        self.notices.dispatch(notice);
    }

    /// Ask the application about a phase of the session ending; with no
    /// handler bound the session goes on.
    pub(crate) fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.app
            .session_end(phase)
            .or_resume(SessionEndAnswer::Proceed)
    }

    /// Ask the application whether the loop may end now that no window is
    /// open; with no policy of its own, the loop ends, as a desktop loop does.
    fn exit_allowed(&self) -> bool {
        self.app.exit_policy().or_resume(true)
    }

    /// The colour scheme the portal reported, to each window.
    fn deliver_appearance(&self) {
        let source = self.preferences.lock().as_ref().and_then(Weak::upgrade);
        if let Some(source) = source {
            self.windows.observe_appearance(source.appearance());
        }
    }

    /// Pump the loop by hand for `span`, as [`run`](host::Loop::run) does on
    /// each turn, for a test that owns the platform's thread and opens its
    /// windows without running the platform (a window fixture's `settle`).
    /// No application handler is bound: no wake deadline, no exit policy.
    ///
    /// # Errors
    /// [`PlatformError::EventLoop`] when the connection fails.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn pump_for(&self, span: std::time::Duration) -> Result<(), PlatformError> {
        let deadline = web_time::Instant::now() + span;
        let mut turns = Turns::default();
        loop {
            self.doorbell.answer();
            self.dispatch_events(&mut turns)?;
            let turn = self.signal.begin_turn();
            self.read_sources(&mut turns, None);
            let _ = turn.drive();
            self.windows.deliver_frames();
            let _ = self.windows.take_closed();
            if web_time::Instant::now() >= deadline {
                return Ok(());
            }
            self.wait(&mut turns, Some(deadline))?;
        }
    }

    /// Hand every event the server sent, and those a turn's requests left
    /// queued, to the windows, until none is queued.
    fn dispatch_events(&self, turns: &mut Turns) -> Result<(), PlatformError> {
        loop {
            let event = match turns.queued.pop_front() {
                Some(event) => event,
                None => match self.connection.xcb().poll_for_event() {
                    Ok(Some(event)) => event,
                    Ok(None) => return Ok(()),
                    Err(error) => {
                        return Err(PlatformError::EventLoop {
                            message: format!("the X server connection failed: {error}"),
                        });
                    }
                },
            };
            if let Event::Error(error) = &event {
                // A request on a window the server no longer has (a property
                // read racing a close): the request's own reply says so too.
                tracing::debug!(?error, "the X server refused a request");
                continue;
            }
            self.windows.event(&event);
        }
    }

    /// Deliver what the system's sources queued for the owner: the host's
    /// notices, the portal's colour scheme, and a change of the displays,
    /// which every window reads itself again for.
    fn read_sources(&self, turns: &mut Turns, host_notices: Option<&dyn HostNotices>) {
        if let Some(host_notices) = host_notices {
            host_notices.deliver(self);
        }
        self.deliver_appearance();
        match self.displays.snapshot() {
            Ok(snapshot) if turns.displays != Some(snapshot.revision()) => {
                turns.displays = Some(snapshot.revision());
                self.windows.refresh();
            }
            Ok(_) => {}
            Err(error) => tracing::debug!(%error, "the displays cannot be read"),
        }
    }

    /// Wait until the server or the doorbell has something, or until
    /// `deadline`. Events a turn's requests left queued, and a window waiting
    /// for a frame, mean no wait.
    fn wait(
        &self,
        turns: &mut Turns,
        deadline: Option<web_time::Instant>,
    ) -> Result<(), PlatformError> {
        let connection = self.connection.xcb();
        match connection.poll_for_event() {
            Ok(Some(event)) => {
                turns.queued.push_back(event);
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => {
                return Err(PlatformError::EventLoop {
                    message: format!("the X server connection failed: {error}"),
                });
            }
        }
        if self.windows.wants_frame() {
            return Ok(());
        }
        connection
            .flush()
            .map_err(|error| PlatformError::EventLoop {
                message: format!("the X server connection failed: {error}"),
            })?;
        let timeout =
            deadline.and_then(|deadline| poll_timeout(deadline, web_time::Instant::now()));
        let mut fds = [
            PollFd::new(connection, PollFlags::IN),
            PollFd::new(&*self.doorbell.0, PollFlags::IN),
        ];
        match poll(&mut fds, timeout.as_ref()) {
            Ok(_) | Err(Errno::INTR) => Ok(()),
            Err(error) => Err(PlatformError::EventLoop {
                message: format!("waiting on the X server failed: {error}"),
            }),
        }
    }

    /// The loop proper, after the application's bootstrap: until a quit, an
    /// exit the policy allows, or a failed connection.
    fn run_loop(&self, host_notices: Option<&dyn HostNotices>) -> Result<(), PlatformError> {
        let mut turns = Turns::default();
        let mut deadline = WakeDeadline::default();
        loop {
            self.doorbell.answer();
            if deadline.take_due(web_time::Instant::now(), self.windows.frames()) {
                self.windows.redraw_for_deadline();
            }
            self.dispatch_events(&mut turns)?;
            let turn = self.signal.begin_turn();
            self.read_sources(&mut turns, host_notices);
            if turn.drive() {
                return Ok(());
            }
            self.windows.deliver_frames();
            let reevaluate = self.exit_reevaluation.swap(false, Ordering::AcqRel);
            if (self.windows.take_closed() || reevaluate)
                && self.windows.is_empty()
                && self.signal.accepting()
                && self.exit_allowed()
                // Asked with no lock held, the policy may have opened a
                // window: one opened meanwhile keeps the loop.
                && self.windows.is_empty()
            {
                return Ok(());
            }
            let wake_at = self.wake_deadline(&mut deadline);
            self.wait(&mut turns, wake_at)?;
        }
    }

    /// The instant the application asks the loop to wake at, armed by the
    /// rule of [`WakeDeadline`].
    fn wake_deadline(&self, deadline: &mut WakeDeadline) -> Option<web_time::Instant> {
        let answer = if self.signal.accepting() {
            self.app.wake_deadline().or_resume(None)
        } else {
            None
        };
        deadline.arm(answer, self.windows.frames())
    }
}

/// What a run of the loop keeps from one turn to the next.
#[derive(Default)]
struct Turns {
    /// Events a turn's requests left queued, where `poll` cannot see them.
    queued: VecDeque<Event>,
    /// The display snapshot the windows last read themselves against.
    displays: Option<DisplayRevision>,
}

/// How long the loop waits for `deadline` at `now`: rounded up to the
/// millisecond, so a deadline a fraction of one away is not a busy wait, and
/// `None`, no limit, for one too far away to be told.
fn poll_timeout(deadline: web_time::Instant, now: web_time::Instant) -> Option<Timespec> {
    wait_for(deadline.saturating_duration_since(now))
}

/// [`poll_timeout`] for what remains until the deadline.
fn wait_for(remaining: std::time::Duration) -> Option<Timespec> {
    let millis = remaining.as_nanos().div_ceil(1_000_000);
    let rounded = std::time::Duration::from_millis(u64::try_from(millis).ok()?);
    Timespec::try_from(rounded).ok()
}

/// The loop's end, run as `run` returns or unwinds: the owner stops admitting
/// work, the sources stop, every window still open is let go of, and the
/// application hears the quit, then is released.
struct RunGuard<'a> {
    platform: &'a X11Loop,
    display_watch: Option<Watch>,
    preferences: Option<Arc<dyn PortalPreferences>>,
    host_notices: Option<Box<dyn HostNotices>>,
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        let platform = self.platform;
        platform.signal.close();
        // Admission is closed, so a change a source still reports wakes
        // nobody; now the sources end.
        drop(self.display_watch.take());
        *platform.preferences.lock() = None;
        drop(self.preferences.take());
        drop(self.host_notices.take());
        contain_owner_callback(|| platform.windows.release());
        // The connection carries the destruction of the windows released
        // above to the server before the handler hears the quit.
        if let Err(error) = platform.connection.xcb().flush() {
            tracing::debug!(%error, "the X server connection failed at the loop's end");
        }
        platform
            .app
            .quit_requested()
            .or_contain((), "the application's quit");
        platform.app.release();
    }
}

impl host::Loop for X11Loop {
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        if !self.affinity.is_owner() {
            return Err(PlatformError::EventLoop {
                message: "the X11 platform runs on the thread that built it, which its windows \
                          belong to"
                    .into(),
            });
        }
        tracing::info!("Running the X11 platform");
        self.signal.bind_owner();
        // The delivery holds the platform and the hooks weakly: they live in
        // this frame until the loop ends, declared before the guard so they
        // outlive it and the handler still hears the quit.
        let hooks: Arc<dyn OwnerHooks> = Arc::new(DirectOwnerHooks::with_signal(
            Arc::clone(&platform),
            Arc::clone(&self.signal),
        ));
        let mut guard = RunGuard {
            platform: &self,
            display_watch: None,
            preferences: None,
            host_notices: None,
        };
        self.app.bind(handler, &platform, &hooks);
        self.app.resume_through_signal(&self.signal);
        // The displays and the portal are read before the bootstrap, which
        // opens windows on them and reads the preferences.
        guard.display_watch = Some(Arc::clone(&self.displays).observe(&self.signal));
        let preferences = self
            .preference_start
            .start(&self.background_executor, &self.signal);
        *self.preferences.lock() = Some(Arc::downgrade(&preferences));
        guard.preferences = Some(preferences);
        self.app
            .ready(OwnerPlatform::new(Arc::clone(&platform), Arc::clone(&hooks)))
            .map_err(PlatformError::bootstrap)?;
        self.signal
            .start()
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            })?;
        // A sleep or a shutdown that arrived while the bootstrap ran would
        // have held logind's inhibitor with no turn to deliver it, so the
        // sources start only now.
        guard.host_notices = Some(
            self.host_notice_start
                .start(&self.background_executor, &self.signal),
        );
        let result = self.run_loop(guard.host_notices.as_deref());
        drop(guard);
        result
    }

    fn quit(&self) {
        tracing::info!("Quitting the X11 platform");
        if let Err(error) = self.signal.request_quit() {
            tracing::debug!(%error, "the X11 platform was asked to quit after it stopped");
        }
    }

    fn request_exit_policy_reevaluation(&self) {
        self.exit_reevaluation.store(true, Ordering::Release);
        if let Err(error) = self.doorbell.ring() {
            tracing::error!(%error, "an exit-policy request could not wake the X11 loop");
        }
    }
}

/// The X11 loop hears its host's lifecycle on its turns; logind cannot be
/// asked to wait for the user, so no session end is held open.
impl LifecycleHooks for X11Loop {
    fn release_session_end(&self) {}
}

/// The displays position is the loop's: the source the table made.
impl host::Displays<X11Loop> for X11Loop {
    fn snapshot(owner: &X11Loop) -> Result<DisplaySnapshot, PlatformError> {
        owner.displays.snapshot()
    }
}

/// The preference position is the loop's: the portal's source, started as the
/// loop runs and read on its owner thread.
impl host::Preferences<X11Loop> for X11Loop {
    fn sample(owner: &X11Loop) -> Result<SystemPreferences, PlatformError> {
        if !owner.affinity.is_owner() {
            return Err(PlatformError::Preferences {
                message: "the system's preferences are read on the X11 platform's owner thread"
                    .into(),
            });
        }
        let source = owner.preferences.lock().as_ref().and_then(Weak::upgrade);
        source.ok_or(PlatformError::PreferencesDeferred)?.sample()
    }
}

impl host::Services for X11Loop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        std::env::current_exe().map_err(|error| PlatformError::AppPath {
            message: error.to_string(),
        })
    }

    fn reveal_path(&self, path: &Path) {
        // The extension, not the path: a document the user picked is their
        // data.
        tracing::info!(extension = ?path.extension(), "Revealing path");
        if let Some(parent) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
        }
    }

    fn open_path(&self, path: &Path) {
        tracing::info!(extension = ?path.extension(), "Opening path");
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }

    // The X11 host opens no URL and shows no file prompt: those services move
    // to the system capability crate (ADR-0151 §9).
    fn open_url(&self, url: &str) {
        let _ = url;
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        let _ = options;
        Task::ready(Ok(None))
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        Task::ready(Ok(None))
    }

    fn compositor_name(&self) -> &'static str {
        ""
    }
}

/// What a feature reaches one X11 window by: the window's X id, the server's
/// name for it, which the server may give another window once this one is
/// gone, and the identity the platform gave it, which no other window of the
/// platform has. A feature that keeps state of a window keys it by the
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRef {
    pub(crate) xid: xproto::Window,
    pub(crate) id: WindowId,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::wait_for;

    /// The wait for a deadline is rounded up to the millisecond, never down to
    /// a busy wait, and a deadline beyond what `poll` can be told is no limit.
    #[test]
    fn the_wait_for_a_deadline_rounds_up_to_the_millisecond() {
        let millis = |wait: Option<rustix::event::Timespec>| {
            wait.map(|wait| wait.tv_sec * 1_000 + wait.tv_nsec / 1_000_000)
        };
        // (case, remaining, milliseconds waited)
        let table = [
            ("due", Duration::ZERO, Some(0)),
            ("a nanosecond away", Duration::from_nanos(1), Some(1)),
            ("a millisecond away", Duration::from_millis(1), Some(1)),
            (
                "a nanosecond past a millisecond",
                Duration::from_nanos(1_000_001),
                Some(2),
            ),
            ("seconds away", Duration::from_secs(3), Some(3_000)),
            ("too far to tell", Duration::MAX, None),
        ];
        for (case, remaining, expected) in table {
            assert_eq!(millis(wait_for(remaining)), expected, "{case}");
        }
    }
}
