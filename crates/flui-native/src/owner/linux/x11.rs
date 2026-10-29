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
//! `X11Loop::run` waits in `poll` on two descriptors, the connection and the
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
//! reached through [`LoopWindows`]; their input is the input feature's,
//! reached through [`LoopInput`], which hears every event before the windows
//! do; the displays, the preference source and the host notices are what the
//! table makes through [`X11Host`]. A wake from
//! any thread (the owner signal, a window's redraw requester, an exit-policy
//! request) rings the [`Doorbell`].

use std::collections::VecDeque;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cursor_icon::CursorIcon;
use flui_platform::CursorError;
use flui_platform::input_capabilities::InputCapabilities;
use std::sync::{Arc, Weak};

use flui_platform::{
    DisplayRevision, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, SystemNotice,
    SystemPreferences, WindowAppearance, WindowId,
};
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
use crate::owner::linux::{LinuxNotices, LinuxPreferences, PortalSlot, SessionLoop, xdg_open};
use crate::owner::notices::{LifecycleHooks, NoticeChannel, StaleOnWake};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::signal::OwnerSignal;
use crate::owner::wake_deadline::WakeDeadline;
use crate::owner::watch::Watch;
use crate::owner::window_delivery::WindowDelivery;
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

    /// The Settings portal's preferences, observed on `executor` and waking
    /// the owner behind `signal`; started as the loop runs.
    fn preferences(
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn LinuxPreferences>;

    /// The sources of the host's notices (logind, memory pressure), started
    /// once the application can hear what they report.
    fn host_notices(
        executor: &BackgroundExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn LinuxNotices>;

    /// The windows' input on `connection`: the pointers, the keyboard and
    /// the cursor, made on the thread that builds the loop.
    fn input(connection: &Arc<X11Connection>) -> Arc<dyn LoopInput>;
}

/// The input of the X11 loop's windows, the input feature's (ADR-0151 §10):
/// the loop hands it every event first, and a window tells it as it opens
/// and closes and sets its cursor through it. Every method but
/// [`capabilities`](Self::capabilities), [`keyboard_layout`](Self::keyboard_layout)
/// and [`on_keyboard_layout_change`](Self::on_keyboard_layout_change) runs
/// on the owner thread, and none runs user code with a lock or borrow of
/// its own held.
pub(crate) trait LoopInput: Send + Sync {
    /// The input capabilities a window opening now has.
    fn capabilities(&self) -> InputCapabilities;

    /// `target` opened: its input is selected and delivered to it from now
    /// on.
    fn window_opened(&self, target: InputTarget);

    /// `window` closed: nothing is delivered to it any more.
    fn window_closed(&self, window: WindowRef);

    /// An event the server sent; `true` when it was input's alone, `false`
    /// when the windows hear it too (a focus change the input also follows).
    fn event(&self, event: &Event) -> bool;

    /// Show `cursor` over `window`.
    ///
    /// # Errors
    /// What [`PlatformWindow::set_cursor`](flui_platform::PlatformWindow::set_cursor)
    /// reports.
    fn set_cursor(&self, window: WindowRef, cursor: CursorIcon) -> Result<(), CursorError>;

    /// [`Platform::keyboard_layout`].
    fn keyboard_layout(&self) -> String;

    /// [`Platform::on_keyboard_layout_change`].
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>);
}

/// What the input delivers to one window by: the window, its delivery and
/// its scale.
#[derive(Debug, Clone)]
pub(crate) struct InputTarget {
    pub(crate) window: WindowRef,
    pub(crate) delivery: Arc<WindowDelivery>,
    pub(crate) scale: WindowScale,
}

/// A window's scale (device pixels per logical pixel) as the window last
/// read it, which its input reads positions with.
#[derive(Debug, Clone)]
pub(crate) struct WindowScale(Arc<AtomicU64>);

impl WindowScale {
    /// A window at `scale`.
    pub(crate) fn new(scale: f64) -> Self {
        Self(Arc::new(AtomicU64::new(scale.to_bits())))
    }

    /// The scale now.
    pub(crate) fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Acquire))
    }

    /// The window is at `scale` now.
    pub(crate) fn set(&self, scale: f64) {
        self.0.store(scale.to_bits(), Ordering::Release);
    }
}

/// The input position is the loop's: the input the table made, which keeps
/// the keyboard's layout.
impl host::Input<X11Loop> for X11Loop {
    fn keyboard_layout(owner: &X11Loop) -> String {
        owner.input.keyboard_layout()
    }

    fn on_keyboard_layout_change(owner: &X11Loop, callback: Box<dyn FnMut() + Send>) {
        owner.input.on_keyboard_layout_change(callback);
    }
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
    input: Arc<dyn LoopInput>,
    start_preferences: fn(&BackgroundExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxPreferences>,
    start_host_notices: fn(&BackgroundExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices>,
    background_executor: Arc<BackgroundExecutor>,
    /// The preference source while the loop runs.
    portal: PortalSlot,
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
        let input = H::input(&connection);
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
            input,
            start_preferences: H::preferences,
            start_host_notices: H::host_notices,
            background_executor: Arc::new(BackgroundExecutor::new()),
            portal: PortalSlot::default(),
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

    /// The windows' input, which a window opens and closes its input with
    /// and sets its cursor through.
    pub(crate) fn input(&self) -> &Arc<dyn LoopInput> {
        &self.input
    }

    /// The displays, which a window names its display from.
    pub(crate) fn displays(&self) -> &Arc<dyn HostDisplays> {
        &self.displays
    }

    /// The notice channel, which marks a window's state stale on a wake.
    pub(crate) fn notices(&self) -> &Arc<dyn NoticeChannel> {
        &self.notices
    }

    /// Ask the application whether the loop may end now that no window is
    /// open; with no policy of its own, the loop ends, as a desktop loop does.
    fn exit_allowed(&self) -> bool {
        self.app.exit_policy().or_resume(true)
    }

    /// The colour scheme the portal reported, to each window.
    fn deliver_appearance(&self) {
        if let Some(source) = self.portal.source() {
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
            // A handler's panic in what the input delivered still leaves the
            // windows the event, a focus change they follow too; the panic
            // is raised again after.
            let heard =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.input.event(&event)));
            match heard {
                Ok(true) => {}
                Ok(false) => self.windows.event(&event),
                Err(payload) => {
                    // The first failure is the one raised.
                    if let Err(second) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            self.windows.event(&event);
                        }))
                    {
                        flui_foundation::panic::retain_opaque_payload(second);
                    }
                    std::panic::resume_unwind(payload);
                }
            }
        }
    }

    /// Deliver what the system's sources queued for the owner: the host's
    /// notices, the portal's colour scheme, and a change of the displays,
    /// which every window reads itself again for.
    fn read_sources(&self, turns: &mut Turns, host_notices: Option<&dyn LinuxNotices>) {
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
        let before = before_wait(connection, || self.windows.wants_frame()).map_err(|error| {
            PlatformError::EventLoop {
                message: format!("the X server connection failed: {error}"),
            }
        })?;
        match before {
            BeforeWait::Event(event) => {
                turns.queued.push_back(event);
                return Ok(());
            }
            BeforeWait::Frame => return Ok(()),
            BeforeWait::Sleep => {}
        }
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
    fn run_loop(&self, host_notices: Option<&dyn LinuxNotices>) -> Result<(), PlatformError> {
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

/// The connection as the loop's last step before it sleeps sees it.
trait WaitSource {
    type Event;
    type Error: std::fmt::Display;

    /// Send what is buffered. libxcb reads the socket while it writes, so a
    /// flush may move events already on the wire into its own queue, where
    /// `poll` on the socket does not see them.
    fn send(&self) -> Result<(), Self::Error>;

    /// The next event already read, if any.
    fn take_event(&self) -> Result<Option<Self::Event>, Self::Error>;
}

impl WaitSource for XCBConnection {
    type Event = Event;
    type Error = x11rb::errors::ConnectionError;

    fn send(&self) -> Result<(), Self::Error> {
        x11rb::connection::Connection::flush(self)
    }

    fn take_event(&self) -> Result<Option<Event>, Self::Error> {
        self.poll_for_event()
    }
}

/// What the loop does instead of sleeping, if anything.
#[derive(Debug, PartialEq, Eq)]
enum BeforeWait<E> {
    /// An event is already read: deliver it.
    Event(E),
    /// A window waits for a frame.
    Frame,
    /// Nothing is owed: sleep in `poll`.
    Sleep,
}

/// The last step before the loop sleeps: flush first, and only then look for
/// an event already read, since the flush may have read some; a window that
/// waits for a frame keeps it awake too. The other order, an event looked for
/// and then a flush, leaves what the flush read in libxcb's queue while `poll`
/// sleeps on a socket that has nothing new.
fn before_wait<S: WaitSource>(
    source: &S,
    wants_frame: impl FnOnce() -> bool,
) -> Result<BeforeWait<S::Event>, S::Error> {
    source.send()?;
    if let Some(event) = source.take_event()? {
        return Ok(BeforeWait::Event(event));
    }
    if wants_frame() {
        return Ok(BeforeWait::Frame);
    }
    Ok(BeforeWait::Sleep)
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
    preferences: Option<Arc<dyn LinuxPreferences>>,
    host_notices: Option<Arc<dyn LinuxNotices>>,
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        let platform = self.platform;
        platform.signal.close();
        // Admission is closed, so a change a source still reports wakes
        // nobody; now the sources end.
        drop(self.display_watch.take());
        platform.portal.set(None);
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
        let preferences = (self.start_preferences)(&self.background_executor, &self.signal);
        self.portal.set(Some(&preferences));
        guard.preferences = Some(preferences);
        self.app
            .ready(OwnerPlatform::new(
                Arc::clone(&platform),
                Arc::clone(&hooks),
            ))
            .map_err(PlatformError::bootstrap)?;
        self.signal
            .start()
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            })?;
        // A sleep or a shutdown that arrived while the bootstrap ran would
        // have held logind's inhibitor with no turn to deliver it, so the
        // sources start only now.
        guard.host_notices = Some((self.start_host_notices)(
            &self.background_executor,
            &self.signal,
        ));
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

impl SessionLoop for X11Loop {
    fn notice(&self, notice: SystemNotice) {
        self.notices.dispatch(notice);
    }

    fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.app
            .session_end(phase)
            .or_resume(SessionEndAnswer::Proceed)
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
        owner
            .portal
            .source()
            .ok_or(PlatformError::PreferencesDeferred)?
            .sample()
    }
}

/// What a file prompt answers on this host: the file chooser portal is not
/// spoken here yet.
fn no_file_prompt() -> PlatformError {
    flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotYetOnThisHost).into()
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

    fn open_url(&self, url: &str) {
        xdg_open(url.as_ref());
    }

    fn reveal_path(&self, path: &Path) {
        // The extension, not the path: a document the user picked is their
        // data.
        tracing::info!(extension = ?path.extension(), "revealing a path");
        if let Some(parent) = path.parent() {
            xdg_open(parent.as_os_str());
        }
    }

    fn open_path(&self, path: &Path) {
        tracing::info!(extension = ?path.extension(), "opening a path");
        xdg_open(path.as_os_str());
    }

    /// The file chooser portal is the system capability crate's (ADR-0151
    /// §3): this host has none, and says so.
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        let _ = options;
        Task::ready(Err(no_file_prompt()))
    }

    /// See [`Self::prompt_for_paths`].
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        Task::ready(Err(no_file_prompt()))
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
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::{BeforeWait, WaitSource, before_wait, wait_for};

    /// A connection with events on the wire, which a flush reads into the
    /// queue, as libxcb's does.
    #[derive(Default)]
    struct Wire {
        unread: RefCell<Vec<u32>>,
        queued: RefCell<VecDeque<u32>>,
        flushes: Cell<usize>,
    }

    impl WaitSource for Wire {
        type Event = u32;
        type Error = std::convert::Infallible;

        fn send(&self) -> Result<(), Self::Error> {
            self.flushes.set(self.flushes.get() + 1);
            self.queued
                .borrow_mut()
                .extend(self.unread.borrow_mut().drain(..));
            Ok(())
        }

        fn take_event(&self) -> Result<Option<u32>, Self::Error> {
            Ok(self.queued.borrow_mut().pop_front())
        }
    }

    /// An event the last flush read is delivered, not slept on: `poll` sees
    /// only the socket, which the flush emptied.
    #[test]
    fn an_event_the_flush_reads_keeps_the_loop_awake() {
        /// A case, the events on the wire, those queued, whether a frame is
        /// wanted, and the outcome.
        type Row = (
            &'static str,
            &'static [u32],
            &'static [u32],
            bool,
            BeforeWait<u32>,
        );

        let table: [Row; 5] = [
            ("read by the flush", &[7], &[], false, BeforeWait::Event(7)),
            ("already queued", &[], &[5], false, BeforeWait::Event(5)),
            (
                "queued before the wire",
                &[7],
                &[5],
                false,
                BeforeWait::Event(5),
            ),
            ("a frame is owed", &[], &[], true, BeforeWait::Frame),
            ("nothing", &[], &[], false, BeforeWait::Sleep),
        ];
        for (case, wire, queued, frame, expected) in table {
            let source = Wire::default();
            source.unread.borrow_mut().extend_from_slice(wire);
            source.queued.borrow_mut().extend(queued.iter().copied());
            let Ok(outcome) = before_wait(&source, || frame);
            assert_eq!(outcome, expected, "{case}");
            assert_eq!(source.flushes.get(), 1, "{case}: one flush");
        }
    }

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
