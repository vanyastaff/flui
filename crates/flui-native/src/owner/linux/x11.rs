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
//! 3. takes the events the server sent, at most [`MAX_TURN_EVENTS`] of them,
//!    and hands each to the features and the windows
//!    ([`LoopWindows::event`]); the rest wait for the next turn;
//! 4. begins the owner turn, delivers what the system's sources queued (host
//!    notices, the portal's colour scheme, a change of the displays), has
//!    each window the turn's events may have changed read itself once
//!    ([`LoopWindows::read_stale`]), and drives the turn (the application's
//!    wake callback, a window's work raised on another thread);
//! 5. delivers the frames the windows asked for, then asks the exit policy if a
//!    window closed or the application asked again;
//! 6. looks for an event already read, and waits only when none is and no
//!    window waits for a frame.
//!
//! `X11Loop::turn` is steps 1 to 5, and the one turn there is: `run` and a
//! test's `pump_for` both take it.
//!
//! A request that waits for its reply (a window reading its state) may read
//! events off the socket into libxcb's queue, where `poll` cannot see them;
//! step 6 takes one before the loop waits, so none waits for an unrelated
//! wake.
//!
//! # What bounds a turn
//!
//! The server is a peer: a stream of its events must not keep the frames, the
//! owner turn or a deadline from their turn. So a turn's work does not grow
//! with what the server sends:
//!
//! - a turn takes at most [`MAX_TURN_EVENTS`] events off the connection;
//! - an event that may change a window's snapshot only marks the window: the
//!   window asks the server for its state once in the turn, after the events,
//!   however many of them named it (`LoopWindows::read_stale`); a change of
//!   what the window manager supports is read once a turn the same way;
//! - the motions of one pointer over one window that came with nothing
//!   between them are one event to every feature, the last, and the input
//!   delivers the earlier ones with it as the positions one move passed
//!   through;
//! - a change of the keymap only marks it: it is read once, before the next
//!   key or as the events end (`LoopInput::settle`);
//! - a handler's panic leaves the rest of the turn's events heard, and the
//!   first goes on after the last of them.
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

use std::cell::Cell;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use cursor_icon::CursorIcon;
use flui_platform::CursorError;
use flui_platform::input_capabilities::InputCapabilities;
use std::sync::{Arc, Weak};

use flui_foundation::panic::FirstPanic;
use flui_platform::data_transfer::DragTransport;
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
use crate::executor::NativeExecutor;
use crate::host;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::handlers::OpenWindows;
use crate::owner::linux::single_instance::FirstInstance;
use crate::owner::linux::{LinuxNotices, LinuxPreferences, PortalSlot, SessionLoop, xdg_open};
use crate::owner::notices::{LifecycleHooks, NoticeChannel, StaleOnWake};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::signal::{OwnerSignal, OwnerTurnSlot};
use crate::owner::wake_deadline::WakeDeadline;
use crate::owner::watch::Watch;
use crate::owner::window_delivery::WindowDelivery;
use crate::owner::{DirectOwnerHooks, OwnerHooks, OwnerPlatform};
use crate::platform::{AppHandler, PathPromptOptions, Platform};
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
    /// or the server lists no screen of the display's number;
    /// [`PlatformError::MissingRequirement`] when it has no RandR 1.5.
    pub(crate) fn connect() -> Result<Arc<Self>, PlatformError> {
        let (xcb, screen) = XCBConnection::connect(None).map_err(|error| PlatformError::Init {
            message: format!("cannot connect to the X server: {error}"),
        })?;
        if xcb.setup().roots.get(screen).is_none() {
            return Err(PlatformError::Init {
                message: format!("the X server lists no screen {screen}"),
            });
        }
        // RandR 1.5 is part of Linux's floor: the monitor list is read with it
        // (ADR-0244 §3).
        crate::version::requirement::randr_version(
            x11rb::protocol::randr::query_version(&xcb, 1, 5)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|version| (version.major_version, version.minor_version)),
        )?;
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

    /// Whether `window` is a peer's: another client's window, not the root.
    /// A window a peer names (a window manager's check window or selection
    /// owner, a transfer's requestor) is any id that peer chose to write, and
    /// the platform selects its events, which only a peer's window may have
    /// selected for it.
    ///
    /// The events a client selects on a window are one mask per client, and
    /// selecting replaces it: a window this connection made hears what its
    /// own feature selected, and the root what the windows selected
    /// (`X11Windows::new`). The server gives each client a range of ids, and
    /// every resource the client makes is numbered in it.
    pub(crate) fn is_peers(&self, window: xproto::Window) -> bool {
        let setup = self.xcb.setup();
        let made_here = window & !setup.resource_id_mask == setup.resource_id_base;
        window != self.root() && !made_here
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
    fn try_new() -> Result<Self, PlatformError> {
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
    /// The arguments the application was launched with, without its own
    /// name: what a registered scheme or file type hands it
    /// ([`launch_arguments`](crate::owner::launch_arguments)).
    fn launch_arguments() -> Vec<std::ffi::OsString>;

    /// The displays: the X server's outputs, read on a connection of their own.
    fn displays() -> Arc<dyn HostDisplays>;

    /// The channel the host's notices take to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

    /// The Settings portal's preferences, observed on `executor` and waking
    /// the owner behind `signal`; started as the loop runs.
    fn preferences(
        executor: &NativeExecutor,
        signal: &Arc<OwnerSignal>,
    ) -> Arc<dyn LinuxPreferences>;

    /// The sources of the host's notices (logind, memory pressure), started
    /// once the application can hear what they report.
    fn host_notices(executor: &NativeExecutor, signal: &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices>;

    /// The windows' input on `connection`: the pointers, the keyboard and
    /// the cursor, made on the thread that builds the loop. Every key goes
    /// through `text`.
    fn input(connection: &Arc<X11Connection>, text: Rc<dyn LoopText>) -> Rc<dyn LoopInput>;

    /// The windows' text input: the input method's D-Bus input context,
    /// reached on `executor`, which rings `doorbell` when the input method
    /// sent something. Made on the thread that builds the loop.
    fn text(
        connection: &Arc<X11Connection>,
        executor: &NativeExecutor,
        doorbell: &Doorbell,
    ) -> Rc<dyn LoopText>;

    /// The clipboard and the primary selection: the `CLIPBOARD` and
    /// `PRIMARY` selections, owned and read on a connection and a thread of
    /// their own, which ring `doorbell` when the application's primary
    /// selection is asked for. Made on the thread that builds the loop.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when its connection, window or thread cannot
    /// be made.
    fn clipboard(doorbell: &Doorbell) -> Result<Rc<dyn LoopClipboard>, PlatformError>;

    /// The windows' drag and drop on `connection`: XDND drops on them, made
    /// on the thread that builds the loop, ringing `doorbell` when work started
    /// off the loop has a deadline for it.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when its atoms cannot be interned.
    fn drag(
        connection: &Arc<X11Connection>,
        doorbell: &Doorbell,
    ) -> Result<Arc<dyn LoopDrag>, PlatformError>;
}

/// The drag and drop of the X11 loop's windows, the drag feature's (ADR-0151
/// §10): the platform's one data-transfer source, which the loop hands
/// every event before the input, and which a window tells as it opens and
/// closes. A fetch it waits on has a deadline the loop wakes for. Every
/// method but the [`DragTransport`]'s runs on the owner thread, and
/// none runs user code with a lock of its own held.
pub(crate) trait LoopDrag: DragTransport {
    /// The drag source of `window`, whose sessions run on this transfer.
    fn drag_source(
        self: Arc<Self>,
        window: WindowRef,
    ) -> Arc<dyn flui_platform::drag::PlatformDragSource>;

    /// `target` opened: drags over it are offered to it from now on.
    fn window_opened(&self, target: InputTarget);

    /// `window` closed: a drag over it ends with nothing told.
    fn window_closed(&self, window: WindowRef);

    /// An event the server sent; `true` when it was the drag's alone.
    fn event(&self, event: &Event) -> bool;

    /// The windows whose press a session took since the last call, which
    /// the loop has the input end: no release reaches them.
    fn pointers_taken(&self) -> Vec<xproto::Window>;

    /// When the first fetch under way, or the session's wait for its
    /// target, times out.
    fn deadline(&self) -> Option<web_time::Instant>;

    /// End the fetches whose time is out at `now`, or whose request was
    /// dropped.
    fn expire(&self, now: web_time::Instant);
}

/// What the loop asks of the clipboard, the clipboard feature's: what
/// [`Platform::clipboard`] and
/// [`Platform::primary_selection`] hand
/// out, and the turn's answer for the application's primary selection,
/// which the application offers as a closure only the owner thread may
/// call.
pub(crate) trait LoopClipboard:
    host::NativeClipboard + host::NativePrimarySelection
{
    /// Answer the requests for the application's primary selection that
    /// rang the doorbell, asking its text with no lock of the loop or of
    /// the clipboard held.
    fn deliver(&self);
}

/// The input of the X11 loop's windows, the input feature's (ADR-0151 §10):
/// the loop hands it every event first, and a window tells it as it opens
/// and closes and sets its cursor through it. Every method runs on the owner
/// thread, and none runs user code with a borrow of its own held.
pub(crate) trait LoopInput {
    /// The input capabilities a window opening now has.
    fn capabilities(&self) -> InputCapabilities;

    /// `target` opened: its input is selected and delivered to it from now
    /// on.
    fn window_opened(&self, target: InputTarget);

    /// `window` closed: nothing is delivered to it any more.
    fn window_closed(&self, window: WindowRef);

    /// An event the server sent, other than a pointer's motion; `true` when
    /// it was input's alone, `false` when the windows hear it too (a focus
    /// change the input also follows).
    fn event(&self, event: &Event) -> bool;

    /// A run of one pointer's motions over one window, delivered as one move
    /// through them; `true` when it was input's alone.
    fn motions(&self, run: &PointerRun) -> bool;

    /// The turn's events are heard: what they marked changed and left to be
    /// read once (the keymap) is read now.
    fn settle(&self);

    /// An OS drag session took the pointer from `window` mid-press: its
    /// contacts end with their capture lost, since no release will reach them.
    fn pointer_taken(&self, window: xproto::Window);

    /// Show `cursor` over `window`.
    ///
    /// # Errors
    /// What [`PlatformWindow::set_cursor`](flui_platform::PlatformWindow::set_cursor)
    /// reports.
    fn set_cursor(&self, window: WindowRef, cursor: CursorIcon) -> Result<(), CursorError>;

    /// [`Platform::keyboard_layout`].
    fn keyboard_layout(&self) -> String;

    /// [`Platform::on_keyboard_layout_change`].
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>);
}

/// One key as the input read it, which the text input hands to the window's
/// input method before the window hears it (ADR-0151 §10: the input names no
/// text, and the text no input).
pub(crate) struct X11Key {
    /// The window the key was typed into.
    pub(crate) window: WindowRef,
    /// Its keysym in the keyboard's state as it was typed.
    pub(crate) keysym: u32,
    /// Its X key code (the kernel's plus 8).
    pub(crate) keycode: u8,
    /// The X modifier mask as it was typed.
    pub(crate) state: u16,
    /// A release, not a press.
    pub(crate) release: bool,
    /// The server's time on it.
    pub(crate) time: xproto::Timestamp,
    /// Hand the key to the window's handler: application code.
    pub(crate) deliver: Box<dyn FnOnce()>,
}

impl std::fmt::Debug for X11Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Key")
            .field("window", &self.window)
            .field("keysym", &self.keysym)
            .field("release", &self.release)
            .finish_non_exhaustive()
    }
}

/// The windows' text input, the text feature's (ADR-0151 §10): the input
/// method each window's focused field composes with. The input hands it
/// every key, which it delivers now or once the input method has answered
/// for it, in the order the keys came. Every method runs on the owner
/// thread, and none runs user code with a borrow of its own held.
pub(crate) trait LoopText {
    /// `window` opened at `scale`, its text `text`, the window's text
    /// position's: its field reaches the input method from now on.
    fn window_opened(
        &self,
        window: WindowRef,
        scale: WindowScale,
        text: Option<Arc<dyn X11WindowText>>,
    );

    /// `window` closed: nothing more reaches it, and its keys still held
    /// are delivered.
    fn window_closed(&self, window: WindowRef);

    /// An event the server sent, which the windows and the input hear too:
    /// a window's focus is the input method's.
    fn event(&self, event: &Event);

    /// A key the input read, delivered now or once the input method answered
    /// for it.
    fn key(&self, key: X11Key);

    /// A pointer or touch press reaches the window of X id `xid`, which may
    /// move its caret: the keys it holds are typed first, in order, so the
    /// user's edits keep the order they were made in.
    fn pointer_pressed(&self, xid: xproto::Window);

    /// Apply what the input method sent since the last turn, and deliver
    /// the keys whose answer came or whose time is out at `now`.
    fn deliver(&self, now: web_time::Instant);

    /// When the first key held runs out of time.
    fn deadline(&self) -> Option<web_time::Instant>;

    /// The loop ends: the input method hears no more and the keys still held
    /// go undelivered.
    fn release(&self);
}

/// A window's text as the loop's text input reaches it: the text position's
/// state of the window, which the text input knows by its own type.
pub trait X11WindowText: std::any::Any + Send + Sync {}

/// A text input with no input method: each key goes to its window as it
/// comes, for a test of the input alone.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct KeysAsTyped;

#[cfg(test)]
impl LoopText for KeysAsTyped {
    fn window_opened(&self, _: WindowRef, _: WindowScale, _: Option<Arc<dyn X11WindowText>>) {}

    fn window_closed(&self, _: WindowRef) {}

    fn event(&self, _: &Event) {}

    fn key(&self, key: X11Key) {
        (key.deliver)();
    }

    fn pointer_pressed(&self, _: xproto::Window) {}

    fn deliver(&self, _: web_time::Instant) {}

    fn deadline(&self) -> Option<web_time::Instant> {
        None
    }

    fn release(&self) {}
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

/// The clipboard position is the loop's: the clipboard the table made, on
/// its own connection.
impl host::Clipboard<X11Loop> for X11Loop {
    fn clipboard(owner: &X11Loop) -> Rc<dyn host::NativeClipboard> {
        Rc::clone(&owner.clipboard) as Rc<dyn host::NativeClipboard>
    }
}

/// The drag position is the loop's: the drag the table made, the platform's
/// one data-transfer source.
impl host::Drag<X11Loop> for X11Loop {
    fn data_transfer(owner: &X11Loop) -> Arc<dyn DragTransport> {
        Arc::clone(&owner.drag) as Arc<dyn DragTransport>
    }
}

/// The primary selection position is the loop's too: X11 always has
/// `PRIMARY`, on the clipboard's connection.
impl host::PrimarySelection<X11Loop> for X11Loop {
    fn primary_selection(owner: &X11Loop) -> Option<Rc<dyn host::NativePrimarySelection>> {
        Some(Rc::clone(&owner.clipboard) as Rc<dyn host::NativePrimarySelection>)
    }
}

/// The input position is the loop's: the input the table made, which keeps
/// the keyboard's layout.
impl host::Input<X11Loop> for X11Loop {
    fn keyboard_layout(owner: &X11Loop) -> String {
        owner.input.keyboard_layout()
    }

    fn on_keyboard_layout_change(owner: &X11Loop, callback: Box<dyn FnMut()>) {
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
    /// the root window may concern every window. One that may change a
    /// window's snapshot marks the window and reads nothing:
    /// [`read_stale`](Self::read_stale) does, once a turn.
    fn event(&self, event: &Event);

    /// Each window an event of this turn, or [`refresh`](Self::refresh),
    /// marked reads itself again, once, and its handler hears
    /// `state_changed` for a new revision; what the window manager supports
    /// is read first, once, if an event said it may have changed.
    fn read_stale(&self);

    /// A pointer event the server sent, asked before the input hears it:
    /// `true` when a window took it for its chrome (a press on a title bar or
    /// a resize border the application draws, and that press's release), so
    /// the input never hears it (ADR-0286 §10).
    fn chrome_event(&self, event: &Event) -> bool;

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
    /// each is marked, and reads itself again at
    /// [`read_stale`](Self::read_stale).
    fn refresh(&self);

    /// The colour scheme the system applies changed.
    fn observe_appearance(&self, appearance: WindowAppearance);

    /// How many frames the windows' handlers produced.
    fn frames(&self) -> u64;

    /// The loop ends: each window still open is let go of, its native window
    /// destroyed once its handler is.
    fn release(&self);

    /// Whether a window is open and shown. A later launch made while none is
    /// asks the application to reopen; a window the application hid stays
    /// hidden until it shows the window itself.
    fn shows_any(&self) -> bool;

    /// Another instance of the application was launched and handed its
    /// launch over: bring a shown window forward, at the time `token`, the
    /// launcher's startup id, names, else now.
    fn activate_forwarded(&self, token: Option<&str>);
}

/// The X11 host's loop: the connection, the doorbell, and what the table made
/// for it.
pub struct X11Loop {
    connection: Arc<X11Connection>,
    doorbell: Doorbell,
    signal: Arc<OwnerSignal>,
    /// The application's wake callback between owner turns.
    turn: Rc<OwnerTurnSlot>,
    app: Arc<AppDelivery>,
    notices: Arc<dyn NoticeChannel>,
    displays: Arc<dyn HostDisplays>,
    windows: Arc<dyn LoopWindows>,
    input: Rc<dyn LoopInput>,
    text: Rc<dyn LoopText>,
    clipboard: Rc<dyn LoopClipboard>,
    drag: Arc<dyn LoopDrag>,
    start_preferences: fn(&NativeExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxPreferences>,
    start_host_notices: fn(&NativeExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices>,
    background_executor: NativeExecutor,
    /// The preference source while the loop runs.
    portal: PortalSlot,
    /// The application asked for its exit policy again.
    exit_reevaluation: Cell<bool>,
    /// The thread that built the platform, which its windows belong to and
    /// which alone runs the loop.
    affinity: flui_foundation::OwnerAffinity,
    /// The application's name on the session bus, while this process is its
    /// one instance: the launches later processes hand over.
    instance: Option<FirstInstance>,
}
// Owner-local (ADR-0327 §5): the loop, its input, input method and clipboard
// never leave the thread that built them.
static_assertions::assert_not_impl_any!(X11Loop: Send, Sync);

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
    /// windows belong to it and it alone runs the loop. `instance`, when this
    /// process is its application's one instance, hands the loop the
    /// launches later processes forward.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when the doorbell cannot be made.
    pub(crate) fn try_new<H: X11Host>(
        connection: Arc<X11Connection>,
        windows: Arc<dyn LoopWindows>,
        executor: NativeExecutor,
        instance: Option<FirstInstance>,
    ) -> Result<Self, PlatformError> {
        let doorbell = Doorbell::try_new()?;
        let rung = doorbell.clone();
        let signal = OwnerSignal::new(Arc::new(move || rung.ring()));
        if let Some(instance) = &instance {
            instance.attach(&signal);
        }
        let app = Arc::new(AppDelivery::new());
        // What launched the application waits for its `ready`.
        app.opened(crate::owner::launch_arguments::launched_by(
            H::launch_arguments(),
        ))
        .or_contain((), "the launch's arguments");
        let notices = H::notices(&app);
        let displays = H::displays();
        let background_executor = executor;
        let text = H::text(&connection, &background_executor, &doorbell);
        let input = H::input(&connection, Rc::clone(&text));
        let clipboard = H::clipboard(&doorbell)?;
        let drag = H::drag(&connection, &doorbell)?;
        notices.mark_held_stale_on_wake(Arc::downgrade(&displays) as Weak<dyn StaleOnWake>);
        let affinity = flui_foundation::OwnerAffinity::new();
        affinity.bind_current();
        Ok(Self {
            connection,
            doorbell,
            signal,
            turn: Rc::default(),
            app,
            notices,
            displays,
            windows,
            input,
            text,
            clipboard,
            drag,
            start_preferences: H::preferences,
            start_host_notices: H::host_notices,
            background_executor,
            portal: PortalSlot::default(),
            exit_reevaluation: Cell::new(false),
            affinity,
            instance,
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
    pub(crate) fn input(&self) -> &Rc<dyn LoopInput> {
        &self.input
    }

    /// The windows' drag and drop, which a window opens and closes as a
    /// drop target with.
    pub(crate) fn drag(&self) -> &Arc<dyn LoopDrag> {
        &self.drag
    }

    /// The windows' text input, which a window opens and closes its field's
    /// input method with.
    pub(crate) fn text(&self) -> &Rc<dyn LoopText> {
        &self.text
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

    /// Turn the loop by hand for `span`, each turn the one
    /// [`run`](host::Loop::run) takes, for a test that owns the platform's
    /// thread and opens its windows without running the platform (a window
    /// fixture's `settle`). No application handler is bound, so its
    /// questions go unanswered: no wake deadline arms, and a quit or an exit
    /// the loop would end on ends nothing here.
    ///
    /// # Errors
    /// [`PlatformError::EventLoop`] when the connection fails.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn pump_for(&self, span: std::time::Duration) -> Result<(), PlatformError> {
        let until = web_time::Instant::now() + span;
        let mut turns = Turns::default();
        loop {
            let _: TurnEnd = self.turn(&mut turns, None)?;
            if web_time::Instant::now() >= until {
                return Ok(());
            }
            let wake_at = earliest(Some(until), self.wake_at(&mut turns));
            self.wait(&mut turns, wake_at)?;
        }
    }

    /// One turn of the loop (the module's steps 1 to 5): what `run` and
    /// `pump_for` both take.
    fn turn(
        &self,
        turns: &mut Turns,
        host_notices: Option<&dyn LinuxNotices>,
    ) -> Result<TurnEnd, PlatformError> {
        self.doorbell.answer();
        if turns
            .deadline
            .take_due(web_time::Instant::now(), self.windows.frames())
        {
            self.windows.redraw_for_deadline();
        }
        self.dispatch_events(turns)?;
        self.text.deliver(web_time::Instant::now());
        // The application's primary selection is asked for its text here,
        // at the turn, with nothing of the loop held.
        self.clipboard.deliver();
        let turn = self.signal.begin_turn();
        self.read_sources(turns, host_notices);
        self.windows.read_stale();
        if turn.drive_in(&self.turn) {
            return Ok(TurnEnd::Quit);
        }
        self.windows.deliver_frames();
        let reevaluate = self.exit_reevaluation.replace(false);
        if (self.windows.take_closed() || reevaluate)
            && self.windows.is_empty()
            && self.signal.accepting()
            && self.exit_allowed()
            // Asked with no lock held, the policy may have opened a
            // window: one opened meanwhile keeps the loop.
            && self.windows.is_empty()
        {
            return Ok(TurnEnd::Quit);
        }
        self.drag.expire(web_time::Instant::now());
        Ok(TurnEnd::Wait)
    }

    /// When the loop wakes with nothing to read: the application's wake
    /// deadline, a drag's fetch or a held key running out of time.
    fn wake_at(&self, turns: &mut Turns) -> Option<web_time::Instant> {
        let wake_at = earliest(
            self.wake_deadline(&mut turns.deadline),
            self.drag.deadline(),
        );
        earliest(wake_at, self.text.deadline())
    }

    /// Hand the turn's events to the features and the windows: those the
    /// connection has, [`MAX_TURN_EVENTS`] at most, so a server that sends
    /// without end leaves the rest of the turn its time. A handler's panic
    /// leaves the events after it heard: they are off the connection, and
    /// nothing else would hear them; the first panic is raised again after
    /// the last.
    fn dispatch_events(&self, turns: &mut Turns) -> Result<(), PlatformError> {
        // A session started since the last turn took its window's press:
        // the input ends it before the events after it, the session's now.
        // The drag handed the windows over, so each hears it though a
        // handler of another panicked.
        let mut failure = FirstPanic::new();
        for window in self.drag.pointers_taken() {
            failure.contain(|| self.input.pointer_taken(window));
        }
        // Nothing is off the connection yet: a failure goes on from here.
        failure.resume();
        let events = turn_events(self.connection.xcb(), &mut turns.queued).map_err(|error| {
            PlatformError::EventLoop {
                message: format!("the X server connection failed: {error}"),
            }
        })?;
        self.hear_all(events);
        Ok(())
    }

    /// Hand `events`, a turn's, to the features and the windows. They are
    /// off the connection, and nothing else would hear them: whatever one of
    /// them runs (a handler, a listener, a `tracing` subscriber) and however
    /// it fails, the events after it are heard, and the first failure goes
    /// on after the last.
    fn hear_all(&self, events: Vec<Event>) {
        let mut failure = FirstPanic::new();
        for heard in fold_motions(events) {
            failure.contain(|| self.hear(&heard));
        }
        // What the events marked and left to be read once.
        failure.contain(|| self.input.settle());
        failure.resume();
    }

    /// Hand one event to the features that hear it, in their order: a run
    /// of motions is its last to every feature but the input, which reads
    /// the whole run.
    fn hear(&self, heard: &TurnEvent) {
        let event = heard.event();
        if let Event::Error(error) = event {
            // A request on a window the server no longer has (a property
            // read racing a close): the request's own reply says so too.
            tracing::debug!(?error, "the X server refused a request");
            return;
        }
        // A drag message, or the answer to a fetch, is the drag's alone;
        // the drag contains what a window's handler raises as it hears it.
        if self.drag.event(event) {
            return;
        }
        // A press a window takes for its chrome reaches neither the text
        // input nor the input, nor does its release.
        if self.windows.chrome_event(event) {
            return;
        }
        // The text input hears a focus change before the input ends the
        // keyboard's keys there, so a key it holds reaches the window
        // first. A handler's panic in what the text input or the input
        // delivered still leaves the windows the event, a focus change
        // they follow too; the first panic is raised again after.
        let mut failure = FirstPanic::new();
        failure.contain(|| self.text.event(event));
        let input = || match heard {
            TurnEvent::Motions(run) => self.input.motions(run),
            TurnEvent::Other(event) => self.input.event(event),
        };
        if failure.contain(input) != Some(true) {
            failure.contain(|| self.windows.event(event));
        }
        failure.resume();
    }

    /// Deliver what the system's sources queued for the owner: the host's
    /// notices, the portal's colour scheme, and a change of the displays,
    /// which every window reads itself again for.
    fn read_sources(&self, turns: &mut Turns, host_notices: Option<&dyn LinuxNotices>) {
        if let Some(host_notices) = host_notices {
            host_notices.deliver(self);
        }
        self.deliver_forwarded();
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

    /// Deliver the launches later processes forwarded: what each opens
    /// reaches the application as running, then a window comes forward at
    /// the time the launch's startup id names.
    fn deliver_forwarded(&self) {
        let Some(instance) = &self.instance else {
            return;
        };
        // One launch at a time, each claimed only once the reopen before it
        // ran: one waiting behind a handler that blocks stays withdrawable,
        // so its caller never runs on its own while this instance opens it.
        while instance.has_waiting() {
            // An application that shows no window (one that outlives its
            // last, or hid it) is asked to reopen, as a Dock click asks on
            // macOS.
            if !self.windows.shows_any() {
                self.app
                    .reopen()
                    .or_contain((), "a launch another instance forwarded");
            }
            let Some((forwarded, delivery)) = instance.take_next() else {
                break;
            };
            if crate::owner::single_instance::hand_on(&self.app, forwarded.opened(), delivery) {
                self.windows.activate_forwarded(forwarded.token.as_deref());
            }
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
                turns.queued = Some(event);
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
        while self.turn(&mut turns, host_notices)? == TurnEnd::Wait {
            let wake_at = self.wake_at(&mut turns);
            self.wait(&mut turns, wake_at)?;
        }
        Ok(())
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

/// The earlier of two instants to wake at, either of which may be none.
fn earliest(
    first: Option<web_time::Instant>,
    second: Option<web_time::Instant>,
) -> Option<web_time::Instant> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (first, second) => first.or(second),
    }
}

/// What a run of the loop keeps from one turn to the next.
#[derive(Default)]
struct Turns {
    /// The event the last wait found already read, where `poll` cannot see
    /// it: the next turn's first.
    queued: Option<Event>,
    /// The display snapshot the windows last read themselves against.
    displays: Option<DisplayRevision>,
    /// The application's wake deadline as the turns armed it.
    deadline: WakeDeadline,
}

/// How a turn of the loop ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnEnd {
    /// The loop waits for the next event, wake or deadline.
    Wait,
    /// The application quit, or its last window closed and its exit policy
    /// lets the loop end.
    Quit,
}

/// The most events one turn takes off the connection. A read of the socket
/// brings at most 4 KiB, some 128 events, and a burst of input between two
/// frames is a few dozen; a server that sends more than this in one turn is
/// served over several, with the frames, the owner turn and the deadlines
/// between them.
pub(crate) const MAX_TURN_EVENTS: usize = 256;

/// The events of one turn: the one the last wait found, then those the
/// connection has read or reads now, [`MAX_TURN_EVENTS`] at most. What is
/// left stays with the connection, where the wait finds it before it sleeps.
fn turn_events<S: WaitSource>(
    source: &S,
    queued: &mut Option<S::Event>,
) -> Result<Vec<S::Event>, S::Error> {
    let mut events: Vec<S::Event> = queued.take().into_iter().collect();
    while events.len() < MAX_TURN_EVENTS
        && let Some(event) = source.take_event()?
    {
        events.push(event);
    }
    Ok(events)
}

/// One event of a turn as the features hear it.
#[derive(Debug)]
enum TurnEvent {
    /// A run of one pointer's motions over one window: the input reads the
    /// run, every other feature its last motion.
    Motions(PointerRun),
    /// Any other event.
    Other(Event),
}

impl TurnEvent {
    /// What every feature but the input hears: for a run, where the pointer
    /// is.
    fn event(&self) -> &Event {
        match self {
            Self::Motions(run) => run.last(),
            Self::Other(event) => event,
        }
    }
}

/// The motions of one pointer over one window that the server sent in one
/// turn with nothing between them, oldest first: XInput 2 motions of one
/// device, from one source, to one window. Only `fold_motions` builds
/// one, and [`continued_by`](Self::continued_by) is the one place that says
/// which motions belong together.
#[derive(Debug)]
pub(crate) struct PointerRun {
    /// The motions before the last: the positions the pointer passed
    /// through on its way to it.
    passed: Vec<Event>,
    /// The last motion: where the pointer is.
    last: Event,
}

impl PointerRun {
    /// Whether `event` is the next motion of the run's pointer, from the
    /// same source, over the same window.
    fn continued_by(&self, event: &Event) -> bool {
        match (&self.last, event) {
            (Event::XinputMotion(earlier), Event::XinputMotion(later)) => {
                (earlier.event, earlier.deviceid, earlier.sourceid)
                    == (later.event, later.deviceid, later.sourceid)
            }
            _ => false,
        }
    }

    /// The motions before the last, oldest first.
    pub(crate) fn passed(&self) -> &[Event] {
        &self.passed
    }

    /// The last motion: where the pointer is.
    pub(crate) fn last(&self) -> &Event {
        &self.last
    }
}

/// `events` with each run of one pointer's motions folded into one
/// [`PointerRun`], in order: every feature hears where the pointer is, once,
/// and the input alone reads what it passed through. Any other event between
/// two motions keeps them apart, so nothing is heard out of its order.
fn fold_motions(events: Vec<Event>) -> Vec<TurnEvent> {
    let mut folded: Vec<TurnEvent> = Vec::with_capacity(events.len());
    for event in events {
        if let Some(TurnEvent::Motions(run)) = folded.last_mut()
            && run.continued_by(&event)
        {
            run.passed.push(std::mem::replace(&mut run.last, event));
            continue;
        }
        folded.push(match event {
            Event::XinputMotion(_) => TurnEvent::Motions(PointerRun {
                passed: Vec::new(),
                last: event,
            }),
            other => TurnEvent::Other(other),
        });
    }
    folded
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
        platform.signal.close_in(&platform.turn);
        // Admission is closed, so a change a source still reports wakes
        // nobody; now the sources end.
        drop(self.display_watch.take());
        platform.portal.set(None);
        drop(self.preferences.take());
        drop(self.host_notices.take());
        contain_owner_callback(|| platform.text.release());
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
        self: Rc<Self>,
        platform: Rc<dyn Platform>,
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
        let hooks: Rc<dyn OwnerHooks> = Rc::new(DirectOwnerHooks::with_signal(
            Rc::clone(&platform),
            Arc::clone(&self.signal),
            Rc::clone(&self.turn),
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
        // A window the bootstrap opens starts in the portal's colour scheme,
        // not in the light one it would otherwise wait a turn to leave.
        self.deliver_appearance();
        self.app
            .ready(OwnerPlatform::new(Rc::clone(&platform), Rc::clone(&hooks)))
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
        self.exit_reevaluation.set(true);
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
        // A loop that stopped admitting work answers no read: the callbacks
        // it retires as it ends, and code after `run`, see it closed, as on
        // the Wayland host.
        if !owner.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "the system's preferences require the live X11 loop".into(),
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
    fn launch_value(&self, key: flui_platform::LaunchKey) -> Option<String> {
        host::environment_launch_value(key)
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

    use x11rb::protocol::Event;
    use x11rb::protocol::xinput::ButtonPressEvent;

    use super::{
        BeforeWait, MAX_TURN_EVENTS, TurnEvent, WaitSource, before_wait, fold_motions, turn_events,
        wait_for,
    };

    /// A window of `platform` and its X id. Hidden: the server is other
    /// cases' too, and a window mapped on it would lie under their pointer.
    fn hidden_window(
        platform: &crate::X11Platform,
    ) -> (std::rc::Rc<dyn crate::host_window::HostWindow>, u32) {
        use flui_platform::WindowOptions;
        use flui_testing::window_handler::RecordingWindowHandler;
        use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

        use crate::platform::Platform as _;

        let window = platform
            .open_window(
                WindowOptions::default().visible(false),
                RecordingWindowHandler::new(),
            )
            .expect("an X11 window opens");
        let target = window.surface_target();
        let handle = target.window_handle().expect("an open window has a handle");
        let RawWindowHandle::Xcb(handle) = handle.as_raw() else {
            panic!("an X11 window hands out an XCB handle");
        };
        (window, handle.window.get())
    }

    /// The server configured `window`: an event that may change its snapshot.
    fn configured(window: u32) -> Event {
        use x11rb::protocol::xproto::ConfigureNotifyEvent;

        Event::ConfigureNotify(ConfigureNotifyEvent {
            event: window,
            window,
            ..ConfigureNotifyEvent::default()
        })
    }

    /// Whatever an event of a turn runs, and however it fails, the events
    /// after it are heard and the failure goes on after the last: they are
    /// off the connection, and nothing else would hear them. Here the server
    /// refused a request, the loop reports it, and the `tracing` subscriber
    /// (the application's code) panics; the configure behind it still marks
    /// its window. On the X server `DISPLAY` names; passes as untested where
    /// none answers.
    #[test]
    fn a_failure_while_an_event_is_heard_leaves_the_rest_of_the_turn_heard() {
        use flui_foundation::panic::{FirstPanic, payload_text};
        use x11rb::protocol::ErrorKind;
        use x11rb::protocol::xproto::ConnectionExt as _;
        use x11rb::x11_utils::X11Error;

        /// A subscriber that fails at every diagnostic.
        struct FailsAtADiagnostic;

        impl tracing::Subscriber for FailsAtADiagnostic {
            fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, _: &tracing::Event<'_>) {
                panic!("the subscriber failed");
            }
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }

        let Ok(platform) = crate::X11Platform::try_new() else {
            eprintln!("no X server answers: nothing to run");
            return;
        };
        let owner = platform.owner();
        let xcb = owner.connection.xcb();
        let (_window, xid) = hidden_window(&platform);
        let sequence = || {
            let cookie = xcb.get_input_focus().expect("request");
            let sequence = cookie.sequence_number();
            cookie.reply().expect("the server answers");
            sequence
        };
        // What the window opened with is read.
        owner.windows.read_stale();

        let refused = Event::Error(X11Error {
            error_kind: ErrorKind::Window,
            error_code: 3,
            sequence: 1,
            bad_value: 0,
            minor_opcode: 0,
            major_opcode: 20,
            extension_name: None,
            request_name: None,
        });
        let mut failure = FirstPanic::new();
        tracing::subscriber::with_default(FailsAtADiagnostic, || {
            failure.contain(|| owner.hear_all(vec![refused, configured(xid)]));
        });
        assert_eq!(
            failure.held().and_then(payload_text),
            Some("the subscriber failed"),
            "the failure goes on after the turn's events"
        );
        let before = sequence();
        owner.windows.read_stale();
        assert!(
            sequence() - before > 1,
            "the configure behind the failure marked its window, which reads itself"
        );
    }

    /// A command that reads its window anew publishes a revision nobody has
    /// been told of yet, and asks the loop for a turn: a command that then
    /// ends early (refused, failed, unwound) leaves the announcement to that
    /// turn, not to whatever else wakes the loop. Here the window was moved
    /// behind its back and the move heard, and a command reads it. On the X
    /// server `DISPLAY` names; passes as untested where none answers.
    #[test]
    fn a_command_that_reads_its_window_anew_asks_for_a_turn() {
        use flui_platform::WindowCommand;
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt as _};

        let Ok(platform) = crate::X11Platform::try_new() else {
            eprintln!("no X server answers: nothing to run");
            return;
        };
        let owner = platform.owner();
        let (window, xid) = hidden_window(&platform);
        owner.windows.read_stale();
        let title = WindowCommand::SetTitle("read anew".to_owned())
            .admit(&window.state().capabilities)
            .expect("a window takes a title");

        let xcb = owner.connection.xcb();
        xcb.configure_window(xid, &ConfigureWindowAux::new().x(37).y(41))
            .expect("request");
        xcb.get_input_focus()
            .expect("request")
            .reply()
            .expect("the server answers");
        owner.windows.event(&configured(xid));
        let rung = || {
            let mut fds = [PollFd::new(&*owner.doorbell.0, PollFlags::IN)];
            let now = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            poll(&mut fds, Some(&now)).expect("the doorbell can be polled") > 0
        };
        owner.doorbell.answer();
        assert!(!rung(), "nothing rang since the doorbell was answered");
        window.apply(title).expect("the window takes its title");
        assert!(rung(), "the read asked the loop for a turn");
    }

    /// A flood is served turn after turn with no sleep between: a turn that
    /// stopped at its cap left events the connection has already read, where
    /// `poll` cannot see them, and the step before the wait takes one of
    /// them instead of sleeping.
    #[test]
    fn a_flood_is_served_turn_after_turn_with_no_sleep_between() {
        let sent = u32::try_from(3 * MAX_TURN_EVENTS + 10).expect("the flood fits");
        let source = Wire::default();
        // Everything is read already: the socket is quiet.
        source.queued.borrow_mut().extend(0..sent);
        let mut found = None;
        let mut served = Vec::new();
        let mut turns = 0;
        loop {
            let Ok(events) = turn_events(&source, &mut found);
            assert!(events.len() <= MAX_TURN_EVENTS, "{}", events.len());
            served.extend(events);
            turns += 1;
            let Ok(next) = before_wait(&source, || false);
            match next {
                BeforeWait::Event(event) => found = Some(event),
                BeforeWait::Frame => unreachable!("no window waits for a frame"),
                BeforeWait::Sleep => break,
            }
        }
        assert_eq!(
            served,
            (0..sent).collect::<Vec<_>>(),
            "every event, in order"
        );
        assert_eq!(turns, 4, "the loop slept only once nothing was left");
    }

    /// An event that may change a window's snapshot makes no request: the
    /// window asks the server for its state once, after the turn's events,
    /// however many of them named it, and a change of what the window
    /// manager supports is read once the same way. Counted in the requests
    /// the platform's connection makes, each of which takes the next
    /// sequence number, on the X server `DISPLAY` names; passes as untested
    /// where none answers.
    #[test]
    fn a_burst_of_state_events_reads_each_window_once() {
        use x11rb::protocol::xinput::{self, ConnectionExt as _, DeviceType, EnterEvent};
        use x11rb::protocol::xproto::{
            self, ConnectionExt as _, FocusInEvent, PropertyNotifyEvent,
        };

        let Ok(platform) = crate::X11Platform::try_new() else {
            eprintln!("no X server answers: nothing to run");
            return;
        };
        let owner = platform.owner();
        let xcb = owner.connection.xcb();
        let root = owner.connection.root();
        let (_first_window, first) = hidden_window(&platform);
        let (_second_window, second) = hidden_window(&platform);
        let atom = |name: &[u8]| {
            xcb.intern_atom(false, name)
                .expect("request")
                .reply()
                .expect("the atom")
                .atom
        };
        let active_window = atom(b"_NET_ACTIVE_WINDOW");
        let supported = atom(b"_NET_SUPPORTED");
        let net_wm_state = atom(b"_NET_WM_STATE");
        let property = |window: xproto::Window, atom: xproto::Atom| {
            Event::PropertyNotify(PropertyNotifyEvent {
                window,
                atom,
                ..PropertyNotifyEvent::default()
            })
        };
        let focused = |window: xproto::Window| {
            Event::FocusIn(FocusInEvent {
                event: window,
                ..FocusInEvent::default()
            })
        };
        // The pointer crossing a window's edge: a master pointer's crossing,
        // from a device attached to it. The input reports the hover, which
        // is part of the window's snapshot.
        let devices = xcb
            .xinput_xi_query_device(xinput::Device::ALL)
            .expect("request")
            .reply()
            .expect("the devices")
            .infos;
        let master = devices
            .iter()
            .find(|device| device.type_ == DeviceType::MASTER_POINTER)
            .expect("a master pointer")
            .deviceid;
        let attached = devices
            .iter()
            .find(|device| device.type_ == DeviceType::SLAVE_POINTER && device.attachment == master)
            .expect("a device attached to the master pointer")
            .deviceid;
        let crossed = |window: xproto::Window, entered: bool| {
            let crossing = EnterEvent {
                event: window,
                deviceid: master,
                sourceid: attached,
                ..EnterEvent::default()
            };
            if entered {
                Event::XinputEnter(crossing)
            } else {
                Event::XinputLeave(crossing)
            }
        };
        // Each event goes to who hears it: a crossing to the input, the rest
        // to the windows.
        let hear = |event: &Event| match event {
            Event::XinputEnter(_) | Event::XinputLeave(_) => {
                owner.input.event(event);
            }
            _ => owner.windows.event(event),
        };
        let probes = Cell::new(0);
        // The requests made so far, these probes aside.
        let requests_made = || {
            let cookie = xcb.get_input_focus().expect("request");
            let sequence = cookie.sequence_number();
            cookie.reply().expect("the server answers");
            probes.set(probes.get() + 1);
            sequence - probes.get()
        };
        // What the windows opened with is read.
        owner.windows.read_stale();

        // One window's read, and the read of what the window manager
        // supports, each measured on its own.
        let before = requests_made();
        owner.windows.event(&configured(first));
        owner.windows.read_stale();
        let one_window = requests_made() - before;
        assert!(one_window > 0, "a marked window reads itself");
        let before = requests_made();
        owner.windows.event(&property(root, supported));
        owner.windows.read_stale();
        let support = requests_made() - before - 2 * one_window;
        assert!(support > 0, "a change of the support is read");

        // (case, the events of a turn, the windows read, whether the support
        //  is read)
        let table: [(&str, Vec<Event>, u64, u64); 6] = [
            ("nothing", vec![], 0, 0),
            (
                "the pointer crossing a window's edge a thousand times",
                [crossed(first, true), crossed(first, false)]
                    .into_iter()
                    .cycle()
                    .take(1_000)
                    .collect(),
                1,
                0,
            ),
            (
                "a burst of one window's events",
                [
                    configured(first),
                    focused(first),
                    property(first, net_wm_state),
                ]
                .into_iter()
                .cycle()
                .take(1_000)
                .collect(),
                1,
                0,
            ),
            (
                "both windows' events",
                [configured(first), configured(second)]
                    .into_iter()
                    .cycle()
                    .take(1_000)
                    .collect(),
                2,
                0,
            ),
            (
                "the active window changing a thousand times",
                vec![property(root, active_window); 1_000],
                2,
                0,
            ),
            (
                "the support changing a thousand times",
                vec![property(root, supported); 1_000],
                2,
                1,
            ),
        ];
        for (case, events, windows, support_reads) in table {
            let before = requests_made();
            events.iter().for_each(hear);
            assert_eq!(requests_made(), before, "{case}: an event reads nothing");
            owner.windows.read_stale();
            assert_eq!(
                requests_made() - before,
                windows * one_window + support_reads * support,
                "{case}: read once the events are heard"
            );
            owner.windows.read_stale();
            assert_eq!(
                requests_made() - before,
                windows * one_window + support_reads * support,
                "{case}: nothing is left to read"
            );
        }
    }

    /// A server that sends without end has a turn take [`MAX_TURN_EVENTS`]
    /// events, the one the last wait found first, and leaves the rest with
    /// the connection for the turns after: the frames, the owner turn and the
    /// deadlines come between.
    #[test]
    fn a_turn_takes_a_bounded_number_of_events() {
        /// A case, the event the last wait found, how many events the
        /// connection has, the first and last event taken, how many are left.
        type Row = (&'static str, Option<u32>, u32, Option<(u32, u32)>, usize);

        let cap = u32::try_from(MAX_TURN_EVENTS).expect("the cap fits");
        let table: [Row; 5] = [
            ("nothing", None, 0, None, 0),
            ("the event the wait found", Some(7), 0, Some((7, 7)), 0),
            ("a few", None, 3, Some((0, 2)), 0),
            (
                "a flood",
                None,
                10_000,
                Some((0, cap - 1)),
                10_000 - MAX_TURN_EVENTS,
            ),
            (
                "a flood behind the event the wait found",
                Some(77_777),
                10_000,
                Some((77_777, cap - 2)),
                10_001 - MAX_TURN_EVENTS,
            ),
        ];
        for (case, found, sent, ends, left) in table {
            let source = Wire::default();
            source.queued.borrow_mut().extend(0..sent);
            let mut queued = found;
            let Ok(events) = turn_events(&source, &mut queued);
            assert_eq!(
                events.first().copied().zip(events.last().copied()),
                ends,
                "{case}"
            );
            assert!(events.len() <= MAX_TURN_EVENTS, "{case}: {}", events.len());
            assert_eq!(queued, None, "{case}: the wait's event is taken");
            assert_eq!(source.queued.borrow().len(), left, "{case}: left");
        }
    }

    /// A motion of device `device`, from `source`, over `window`, at `x`.
    fn motion(window: u32, device: u16, source: u16, x: i32) -> Event {
        Event::XinputMotion(ButtonPressEvent {
            event: window,
            deviceid: device,
            sourceid: source,
            event_x: x << 16,
            ..ButtonPressEvent::default()
        })
    }

    /// A press over `window`.
    fn press(window: u32) -> Event {
        Event::XinputButtonPress(ButtonPressEvent {
            event: window,
            deviceid: 2,
            sourceid: 9,
            ..ButtonPressEvent::default()
        })
    }

    /// One pointer's motions over one window with nothing between them are
    /// one event, the last, which carries the earlier ones; anything between
    /// two motions keeps them apart, so every event is heard in its order.
    #[test]
    fn a_pointers_motions_fold_into_the_last() {
        /// An event as it is heard: where its motion is, `None` for a press,
        /// and where the motions it carries are.
        type Heard = (Option<i32>, Vec<i32>);

        fn heard(events: Vec<Event>) -> Vec<Heard> {
            let x = |event: &Event| match event {
                Event::XinputMotion(motion) => Some(motion.event_x >> 16),
                _ => None,
            };
            fold_motions(events)
                .iter()
                .map(|folded| {
                    let passed = match folded {
                        TurnEvent::Motions(run) => run.passed().iter().filter_map(x).collect(),
                        TurnEvent::Other(_) => Vec::new(),
                    };
                    (x(folded.event()), passed)
                })
                .collect()
        }
        let at = |x| motion(1, 2, 9, x);
        // (case, the turn's events, each event as it is heard)
        let table: [(&str, Vec<Event>, Vec<Heard>); 7] = [
            ("nothing", vec![], vec![]),
            ("one motion", vec![at(1)], vec![(Some(1), vec![])]),
            (
                "a burst",
                vec![at(1), at(2), at(3)],
                vec![(Some(3), vec![1, 2])],
            ),
            (
                "a press between two bursts",
                vec![at(1), at(2), press(1), at(3), at(4)],
                vec![(Some(2), vec![1]), (None, vec![]), (Some(4), vec![3])],
            ),
            (
                "another window",
                vec![at(1), motion(5, 2, 9, 2)],
                vec![(Some(1), vec![]), (Some(2), vec![])],
            ),
            (
                "another master pointer",
                vec![at(1), motion(1, 12, 9, 2)],
                vec![(Some(1), vec![]), (Some(2), vec![])],
            ),
            (
                "another device of the same master",
                vec![at(1), motion(1, 2, 11, 2)],
                vec![(Some(1), vec![]), (Some(2), vec![])],
            ),
        ];
        for (case, events, expected) in table {
            assert_eq!(heard(events), expected, "{case}");
        }
    }

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
        type Error = !;

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
