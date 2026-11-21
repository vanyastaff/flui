//! The Wayland loop over `wayland-client`: the connection, the owner thread's turns and the hooks
//! it keeps through the host's table.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};
use std::thread::ThreadId;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_platform::transfer::data::DragTransport;
use flui_platform::{DisplaySnapshot, SessionEndAnswer, SessionEndPhase, SystemNotice, WindowId};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::{Connection, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1;
use wayland_protocols::xdg::foreign::zv2::client::zxdg_exporter_v2::ZxdgExporterV2;
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;
use web_time::Instant;

use super::PointerOffer;
use super::activation::{ActivationToken, LAUNCH_TOKEN_VARIABLE, launch_token};
use super::wait::{BeforePoll, Flush, LoopTurn, before_poll, deadline_wake, poll_timeout};
use crate::error::PlatformError;
use crate::executor::NativeExecutor;
use crate::host;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::call_point::Called;
use crate::owner::linux::single_instance::FirstInstance;
use crate::owner::linux::{LinuxNotices, LinuxPreferences, PortalSlot, SessionLoop};
use crate::owner::notices::{LifecycleHooks, NoticeChannel};
use crate::owner::panic_boundary::contain_owner_callback;
use crate::owner::signal::{OwnerSignal, OwnerTurnSlot};
use crate::owner::wake_deadline::{FrameCount, TurnDeadline, WakeDeadline};
use crate::owner::watch::Watch;
use crate::owner::{DirectOwnerHooks, OpenWindowError, OwnerHooks, OwnerPlatform};
use crate::platform::{AppHandler, ExitAnswer, Platform};
use crate::transfer::linux::wayland::DataDevices;

mod host_hooks;
mod last_input;
mod output_names;
mod presentation;
mod seat;
mod state;
mod waker;
mod windows;

pub(crate) use host_hooks::{WaylandDisplays, WaylandHost};
pub(crate) use last_input::{LastInput, SeatInput};
pub(crate) use output_names::LiveOutputs;
pub use output_names::OutputName;
pub(crate) use presentation::{PresentationClock, PresentationTime};
pub use seat::SeatWindowText;
#[cfg(any(test, feature = "testing"))]
pub(crate) use seat::ShownCursor;
pub(crate) use seat::{
    CursorGlobals, DragWindow, FramePointer, SeatText, SeatWindow, TextEvent, TextRecorder,
    TextWire, WaylandClipboard, WaylandDrag, WaylandInput,
};
use state::LoopGlobals;
pub(crate) use state::{MAX_OUTPUTS, SUSPENSION_VERSION, WaylandState, bindable};
pub(crate) use waker::LoopWaker;
pub(crate) use windows::{LoopWindow, WindowTable};

/// The globals a window is built from, and the queue its objects report
/// to.
#[derive(Clone)]
pub(crate) struct WaylandProtocol {
    /// The connection, which a surface target keeps alive.
    pub(crate) connection: Connection,
    /// The owner's event queue.
    pub(crate) queue: QueueHandle<WaylandState>,
    /// `wl_compositor`, which makes surfaces.
    pub(crate) compositor: WlCompositor,
    /// `xdg_wm_base`, which gives a surface the toplevel role.
    pub(crate) wm_base: XdgWmBase,
    /// `wp_viewporter`, which sizes a surface apart from its buffer;
    /// `None` where the compositor has none.
    pub(crate) viewporter: Option<WpViewporter>,
    /// `wl_subcompositor`, which places a window's own frame around its
    /// content; `None` where the compositor has none.
    pub(crate) subcompositor: Option<WlSubcompositor>,
    /// `wl_shm`, the shared memory a window's own frame is drawn in (and
    /// a test's frames, `testing::wayland::present`); `None` where the
    /// compositor has none.
    pub(crate) shm: Option<WlShm>,
    /// `zxdg_decoration_manager_v1`, through which a window asks the
    /// compositor to draw its frame; `None` where the compositor draws
    /// none.
    pub(crate) decoration: Option<ZxdgDecorationManagerV1>,
    /// `wp_fractional_scale_manager_v1`, which tells a surface the scale
    /// the compositor prefers to the 120th. Whether the window draws at
    /// it is `scale::buffer_scale`'s to decide: only through a viewport.
    pub(crate) fractional_scale: Option<WpFractionalScaleManagerV1>,
    /// `zxdg_exporter_v2`, which hands out a handle another client
    /// (a portal's dialog) parents its window to.
    pub(crate) exporter: Option<ZxdgExporterV2>,
    /// `wp_presentation`, which tells a surface when its commits are
    /// shown; `None` where the compositor tells nothing.
    pub(crate) presentation: Option<PresentationTime>,
    /// `xdg_activation_v1`, through which a window asks the compositor to
    /// activate it; `None` where the compositor takes no such request.
    pub(crate) activation: Option<XdgActivationV1>,
    /// The last user input the seats reported, which an activation token
    /// names.
    pub(crate) last_input: LastInput,
    /// The outputs the compositor announces now.
    pub(crate) live: LiveOutputs,
}

/// What a window opens with, which the loop hands the window feature
/// (`WaylandLoop::seed`).
pub(crate) struct WindowSeed {
    /// The window's identity.
    pub(crate) id: WindowId,
    /// The globals and the owner's queue.
    pub(crate) protocol: WaylandProtocol,
    /// The displays, by which the window names its output.
    pub(crate) displays: Arc<dyn WaylandDisplays>,
    /// The portal's colour scheme.
    pub(crate) portal: PortalSlot,
    /// The loop's windows, which the window leaves when it closes.
    pub(crate) table: std::rc::Weak<WindowTable>,
    /// The owner signal, which delivers what other threads raise.
    pub(crate) signal: Arc<OwnerSignal>,
    /// What a redraw asked for on another thread wakes.
    pub(crate) waker: Weak<LoopWaker>,
    /// The channel a wake from sleep marks the window's state stale
    /// through.
    pub(crate) notices: Arc<dyn NoticeChannel>,
    /// The loop's frame count, which the window's handler counts into
    /// each frame it produces.
    pub(crate) frames: Arc<AtomicU64>,
    /// The seats' input, which the window joins as it opens and leaves
    /// as it closes, and which shows its cursor.
    pub(crate) input: Arc<dyn WaylandInput>,
    /// Drag and drop, which the window joins as it opens and leaves as it
    /// closes.
    pub(crate) drag: Arc<dyn WaylandDrag>,
    /// The loop's background work, on which a window's own frame has
    /// the host's fonts read once.
    pub(crate) executor: NativeExecutor,
}

/// The owner's event queue and what it dispatches into, kept together:
/// dispatching needs both.
struct Dispatcher {
    queue: EventQueue<WaylandState>,
    state: WaylandState,
}

/// What the loop starts as it runs and stops as it ends.
#[derive(Default)]
struct Sources {
    preferences: Option<Arc<dyn LinuxPreferences>>,
    host_notices: Option<Arc<dyn LinuxNotices>>,
}

/// The Wayland host's loop: the connection to the compositor and what the
/// owner thread keeps for the positions of its table.
pub struct WaylandLoop {
    connection: Connection,
    /// The event queue: owner-only, so a dispatch re-entered from a
    /// handler fails loudly instead of waiting on itself.
    dispatcher: RefCell<Dispatcher>,
    protocol: WaylandProtocol,
    waker: Arc<LoopWaker>,
    signal: Arc<OwnerSignal>,
    /// The application's wake callback between owner turns.
    turn: Rc<OwnerTurnSlot>,
    app: Arc<AppDelivery>,
    windows: Rc<WindowTable>,
    /// The thread that made the platform, which owns its windows and
    /// runs its loop.
    owner_thread: ThreadId,
    displays: Arc<dyn WaylandDisplays>,
    input: Arc<dyn WaylandInput>,
    clipboard: Arc<dyn WaylandClipboard>,
    devices: DataDevices,
    drag: Arc<dyn WaylandDrag>,
    notices: Arc<dyn NoticeChannel>,
    portal: PortalSlot,
    start_preferences: fn(&NativeExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxPreferences>,
    start_host_notices: fn(&NativeExecutor, &Arc<OwnerSignal>) -> Arc<dyn LinuxNotices>,
    /// The sources the loop keeps while it runs.
    sources: RefCell<Sources>,
    /// The display watch, which the owner thread starts and stops.
    display_watch: RefCell<Option<Watch>>,
    executor: NativeExecutor,
    /// The wake deadline armed and the last one delivered.
    deadline: RefCell<WakeDeadline>,
    /// The application's turn deadline as the loop armed it.
    turn_deadline: RefCell<TurnDeadline>,
    /// How many frames the windows' handlers produced, which tells the
    /// deadline a delivery a frame served from one that reached nothing.
    frames: FrameCount,
    /// The exit policy is asked again on the next turn.
    reevaluate: Cell<bool>,
    /// The loop ran once; a platform runs once.
    ran: Cell<bool>,
    /// The application's name on the session bus, while this process is
    /// its one instance: the launches later processes hand over.
    instance: Option<FirstInstance>,
}
// Owner-local (ADR-0327 §5): the loop and its window table never leave the
// thread that made them.
static_assertions::assert_not_impl_any!(WaylandLoop: Send, Sync);

impl std::fmt::Debug for WaylandLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandLoop").finish_non_exhaustive()
    }
}

/// The connection was lost, or a request the loop made failed.
fn lost(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::EventLoop {
        message: format!("the Wayland connection: {error}"),
    }
}

/// `duration` as `poll` takes it.
fn timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: duration.subsec_nanos().into(),
    }
}

impl WaylandLoop {
    /// Connect to the compositor the environment names and bind its
    /// globals, for host `H`, whose table makes the displays, the notice
    /// channel and the sources the loop starts. `instance`, when this
    /// process is its application's one instance, hands the loop the
    /// launches later processes forward.
    ///
    /// # Errors
    /// [`PlatformError::Init`] when no compositor answers, libwayland
    /// cannot be loaded, or the compositor lacks `wl_compositor`;
    /// [`PlatformError::MissingRequirement`] when it lacks `xdg_wm_base`,
    /// part of Linux's floor (ADR-0244 §3).
    pub(crate) fn try_new<H: WaylandHost>(
        socket: &Path,
        executor: NativeExecutor,
        instance: Option<FirstInstance>,
    ) -> Result<Self, PlatformError> {
        let init = |what: &str, error: &dyn std::fmt::Display| PlatformError::Init {
            message: format!("Wayland: {what}: {error}"),
        };
        let stream = std::os::unix::net::UnixStream::connect(socket)
            .map_err(|error| init(&format!("no compositor at {}", socket.display()), &error))?;
        let connection =
            Connection::from_socket(stream).map_err(|error| init("the connection", &error))?;
        let mut queue = connection.new_event_queue::<WaylandState>();
        let handle = queue.handle();
        let live = LiveOutputs::default();
        let last_input = LastInput::default();
        let input = H::input(last_input.clone());
        let devices = DataDevices::new();
        let clipboard = H::clipboard(last_input.clone(), devices.clone());
        let mut state = WaylandState {
            outputs: Vec::new(),
            live: live.clone(),
            input: Arc::clone(&input),
            clipboard: Arc::clone(&clipboard),
            devices: devices.clone(),
            starting: Some(LoopGlobals::default()),
        };
        // The compositor announces every global it has before it answers
        // the round trip: the registry's dispatch binds each as it comes
        // and keeps none.
        let _registry = connection.display().get_registry(&handle, ());
        queue
            .roundtrip(&mut state)
            .map_err(|error| init("the registry", &error))?;
        let LoopGlobals {
            compositor,
            wm_base,
            viewporter,
            subcompositor,
            shm,
            decoration,
            fractional_scale,
            exporter,
            presentation,
            activation,
        } = state
            .starting
            .take()
            .expect("BUG: the loop's globals are taken once, here");
        let compositor =
            compositor.ok_or_else(|| init("wl_compositor", &"the compositor announces none"))?;
        let wm_base = wm_base.ok_or_else(|| {
            tracing::error!("Wayland: the compositor announces no xdg_wm_base");
            crate::version::requirement::missing(crate::version::requirement::XDG_WM_BASE)
        })?;
        // A theme cursor is drawn with the loop's own globals.
        if let Some(shm) = &shm {
            input.cursor_globals(CursorGlobals {
                connection: connection.clone(),
                queue: handle.clone(),
                compositor: compositor.clone(),
                shm: shm.clone(),
                viewporter: viewporter.clone(),
            });
        }
        // The seats bound so far get their data devices now: the first
        // window opens before the loop's first turn, and a seat's
        // selection reaches only a device that exists when the keyboard
        // enters.
        devices.seats(&input.seats());
        clipboard.seats(&input.seats());
        let waker = Arc::new(
            LoopWaker::try_new().map_err(|error| init("the loop's wake descriptor", &error))?,
        );
        let signal = {
            let waker = Arc::downgrade(&waker);
            OwnerSignal::new(Arc::new(move || {
                if let Some(waker) = waker.upgrade() {
                    waker.wake();
                }
                Ok(())
            }))
        };
        let drag = {
            let waker = Arc::downgrade(&waker);
            H::drag(
                devices.clone(),
                last_input.clone(),
                Box::new(move || {
                    if let Some(waker) = waker.upgrade() {
                        waker.wake();
                    }
                }),
            )
        };
        if let Some(instance) = &instance {
            instance.attach(&signal);
        }
        let app = Arc::new(AppDelivery::new());
        // What launched the application waits for its `ready`.
        app.opened(crate::owner::launch_arguments::launched_by(
            H::launch_arguments(),
        ))
        .or_contain((), "the launch's arguments");
        Ok(Self {
            protocol: WaylandProtocol {
                connection: connection.clone(),
                queue: handle,
                compositor,
                wm_base,
                viewporter,
                subcompositor,
                shm,
                decoration,
                fractional_scale,
                exporter,
                presentation,
                activation,
                last_input,
                live,
            },
            connection,
            dispatcher: RefCell::new(Dispatcher { queue, state }),
            waker,
            signal,
            turn: Rc::default(),
            notices: H::notices(&app),
            app,
            // The variable stays set: unsetting it races every thread
            // reading the environment. A process the application starts
            // inherits it, though the first window has used it up.
            windows: Rc::new(WindowTable::new(launch_token(std::env::var_os(
                LAUNCH_TOKEN_VARIABLE,
            )))),
            owner_thread: std::thread::current().id(),
            displays: H::displays(socket),
            input,
            clipboard,
            drag,
            devices,
            portal: PortalSlot::default(),
            start_preferences: H::preferences,
            start_host_notices: H::host_notices,
            sources: RefCell::default(),
            display_watch: RefCell::default(),
            executor,
            deadline: RefCell::default(),
            turn_deadline: RefCell::default(),
            frames: FrameCount::default(),
            reevaluate: Cell::new(false),
            ran: Cell::new(false),
            instance,
        })
    }

    /// What a window opens with: its identity, the globals, and what it
    /// reports to.
    ///
    /// # Errors
    /// [`OpenWindowError::Unavailable`] off the thread that owns the
    /// platform, [`OpenWindowError::OwnerGone`] once the loop has ended,
    /// [`OpenWindowError::Backend`] when the window identities are used
    /// up.
    pub(crate) fn seed(&self) -> Result<WindowSeed, OpenWindowError> {
        if std::thread::current().id() != self.owner_thread {
            return Err(OpenWindowError::Unavailable {
                message: "a Wayland window opens on the thread that owns its platform, \
                          which its handler lives on"
                    .to_owned(),
            });
        }
        if !self.signal.accepting() {
            return Err(OpenWindowError::OwnerGone { rejected: None });
        }
        let id = self.windows.issue()?;
        Ok(WindowSeed {
            id,
            protocol: self.protocol.clone(),
            displays: Arc::clone(&self.displays),
            portal: self.portal.clone(),
            table: Rc::downgrade(&self.windows),
            signal: Arc::clone(&self.signal),
            waker: Arc::downgrade(&self.waker),
            notices: Arc::clone(&self.notices),
            frames: self.frames.counter(),
            input: Arc::clone(&self.input),
            drag: Arc::clone(&self.drag),
            executor: self.executor.clone(),
        })
    }

    /// Hold the window `id` until it closes. The first window held is
    /// given the activation token the process was launched with.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when the table holds `id` already.
    pub(crate) fn register(
        &self,
        id: WindowId,
        window: Rc<dyn LoopWindow>,
    ) -> Result<Option<ActivationToken>, OpenWindowError> {
        self.windows.register(id, window)
    }

    /// Send what was asked and wait until the compositor has answered it,
    /// dispatching what it sent meanwhile. Runs no handler: the windows
    /// settle on the loop's next turn.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when the connection failed.
    pub(crate) fn roundtrip(&self) -> Result<(), OpenWindowError> {
        self.dispatching(|queue, state| queue.roundtrip(state).map(drop))
            .map_err(|error| OpenWindowError::Backend {
                message: format!("the Wayland connection: {error}"),
            })
    }

    /// Run `body` on the event queue and what it dispatches into. The
    /// queue is borrowed for `body` alone, which runs no handler: a
    /// dispatch re-entered from inside one panics on the borrow rather
    /// than waiting on itself.
    fn dispatching<R>(
        &self,
        body: impl FnOnce(&mut EventQueue<WaylandState>, &mut WaylandState) -> R,
    ) -> R {
        let mut dispatcher = self.dispatcher.borrow_mut();
        let Dispatcher { queue, state } = &mut *dispatcher;
        body(queue, state)
    }

    /// The serial of the last user input the seats reported, which an
    /// activation token names, for a test.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn last_input_serial(&self) -> Option<u32> {
        self.protocol.last_input.latest().map(|input| input.serial)
    }

    /// Whether the seat's selection offer names the concealment hint,
    /// for a test.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn selection_offers_hint(&self) -> Option<bool> {
        self.clipboard.selection_offers_hint()
    }

    /// The cursor the seats' input last showed, for a test.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn shown_cursor(&self) -> Option<ShownCursor> {
        self.input.shown_cursor()
    }
    /// The connection to the compositor, which every window of the
    /// platform is made on.
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }
    /// Whether the compositor offers to draw a window's frame
    /// (`xdg-decoration`).
    pub(crate) fn offers_decorations(&self) -> bool {
        self.protocol.decoration.is_some()
    }

    /// Whether a window can draw a frame of its own: the compositor
    /// offers subsurfaces and shared memory.
    pub(crate) fn offers_frame_surfaces(&self) -> bool {
        self.protocol.subcompositor.is_some() && self.protocol.shm.is_some()
    }

    /// Whether the compositor tells a surface when its commits are shown
    /// (`wp_presentation`).
    pub(crate) fn offers_presentation(&self) -> bool {
        self.protocol.presentation.is_some()
    }
    /// Whether the compositor takes activation requests
    /// (`xdg-activation`).
    pub(crate) fn offers_activation(&self) -> bool {
        self.protocol.activation.is_some()
    }

    /// What the compositor offers a window's pointer: shared memory for
    /// a cursor image, the constraints and relative motion of a grab.
    pub(crate) fn pointer_offer(&self) -> PointerOffer {
        self.input.pointer_offer()
    }
    /// Whether the compositor's `xdg_wm_base` is new enough to say when it
    /// suspends a toplevel, so a window reports a covering.
    pub(crate) fn tells_suspension(&self) -> bool {
        self.protocol.wm_base.version() >= SUSPENSION_VERSION
    }

    /// Run turns for `span` on the owner thread, for a test that drives
    /// the loop's windows without running it.
    ///
    /// # Errors
    /// What a turn reports.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn turn_for(&self, span: Duration) -> Result<(), PlatformError> {
        let end = Instant::now() + span;
        loop {
            let now = Instant::now();
            if now >= end || self.turn(Some(end - now))? {
                return Ok(());
            }
        }
    }

    /// The globals of the outputs the compositor announces now, for a
    /// test that moves a window between them.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn outputs(&self) -> Vec<OutputName> {
        self.protocol.live.names()
    }

    /// The loop's binding of `output`, while the compositor announces it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn output_proxy(&self, output: OutputName) -> Option<WlOutput> {
        self.dispatching(|_, state| {
            state
                .outputs
                .iter()
                .find(|(name, _)| *name == output)
                .map(|(_, proxy)| proxy.clone())
        })
    }

    /// Dispatch what the compositor already sent: the windows record it.
    fn dispatch_pending(&self) -> Result<usize, PlatformError> {
        self.dispatching(EventQueue::dispatch_pending).map_err(lost)
    }

    /// One turn of the loop, blocking at most `limit`. `true` when the
    /// loop is to end.
    fn turn(&self, limit: Option<Duration>) -> Result<bool, PlatformError> {
        self.dispatch_pending()?;
        // Each seat the input holds has a data device before a window it
        // focuses is told the seat's selection.
        let seats = self.input.seats();
        self.devices.seats(&seats);
        self.clipboard.seats(&seats);
        // The input the seats reported reaches the windows before they
        // settle, so a frame they deliver follows the input it shows.
        // Neither holds a lock of the loop: a handler may open, close or
        // quit. What the dispatch recorded is off the queue and nothing
        // else would deliver it, so a handler's panic in one step leaves
        // the steps after it delivered, and the first is raised after the
        // last, as the X11 loop does with its turn's events.
        let mut failure = FirstPanic::new();
        failure.contain(|| self.input.deliver());
        // The application's selection is asked for its text here, at the
        // turn, not inside the dispatch that recorded the request.
        failure.contain(|| self.clipboard.deliver());
        // The drags the devices recorded reach their windows, and each
        // drag's source is answered, with no lock of the loop held.
        failure.contain(|| self.drag.deliver());
        for window in self.windows.values() {
            failure.contain(|| window.settle());
        }
        failure.resume();
        // A due turn deadline is the owner's turn below, window or not,
        // and asks no window for a frame.
        let turn_due = self.turn_deadline.borrow_mut().take_due(Instant::now());
        if turn_due && self.signal.accepting() {
            let _ = self.signal.wake();
        }
        let turn = self.signal.begin_turn();
        self.deliver_host_notices();
        if turn.drive_in(&self.turn) || self.exit_due() {
            return Ok(true);
        }
        let now = Instant::now();
        let windows = self.windows.values();
        let due = self.deadline.borrow_mut().take_due(now, self.frames.get());
        if due {
            for window in &windows {
                window.request_frame();
            }
        }
        let frame_due = windows.iter().any(|window| window.frame_due());
        let frame_wait = windows
            .iter()
            .filter_map(|window| window.frame_wait())
            .min();
        let mode_due = windows.iter().filter_map(|window| window.mode_due()).min();
        // The wake deadline is served by the windows' frames: the loop
        // wakes for it once one of them can take a frame.
        let first_release = windows.iter().map(|window| window.frame_hold()).min();
        drop(windows);
        // A loop that stopped accepting work asks for no deadline, as the
        // X11 and Win32 loops do.
        let answer = if self.signal.accepting() {
            self.app.wake_deadline().or_resume(None)
        } else {
            None
        };
        let wake_deadline = deadline_wake(
            self.deadline.borrow_mut().arm(answer, self.frames.get()),
            first_release,
        );
        let turn_answer = if self.signal.accepting() {
            self.app.turn_deadline().or_resume(None)
        } else {
            None
        };
        let turn_deadline = self.turn_deadline.borrow_mut().arm(turn_answer);
        let timeout = poll_timeout(
            LoopTurn {
                frame_due,
                frame_wait,
                input_due: self.input.next_due(),
                mode_due,
                wake_deadline: [wake_deadline, turn_deadline].into_iter().flatten().min(),
            },
            now,
        );
        let timeout = match (timeout, limit) {
            (Some(timeout), Some(limit)) => Some(timeout.min(limit)),
            (timeout, None) => timeout,
            (None, limit) => limit,
        };
        self.wait(timeout)?;
        Ok(false)
    }

    /// Deliver what the host's notice sources queued.
    fn deliver_host_notices(&self) {
        let source = self.sources.borrow_mut().host_notices.clone();
        if let Some(source) = source {
            source.deliver(self);
        }
        self.deliver_forwarded();
    }

    /// Deliver the launches later processes forwarded: what each opens
    /// reaches the application as running, then the window activated
    /// last comes forward with the token the launch's launcher gave it.
    fn deliver_forwarded(&self) {
        let Some(instance) = &self.instance else {
            return;
        };
        // One launch at a time, each answered before any handler it
        // reaches runs: one waiting behind a handler that blocks stays
        // withdrawable, so its caller never runs on its own while this
        // instance opens it. A launch that opens nothing asks an
        // application without a window (one that outlives its last) to
        // reopen.
        while instance.has_waiting() {
            let Some((forwarded, delivery)) = instance.take_next() else {
                break;
            };
            if crate::owner::single_instance::hand_on_forwarded(
                &self.app,
                forwarded.opened(),
                delivery,
                || !self.windows.is_empty(),
            ) {
                self.windows
                    .activate_forwarded(forwarded.token.map(ActivationToken::new));
            }
        }
    }

    /// Whether the loop ends because its last window closed, or the
    /// application asked for its exit policy again with no window left:
    /// the handler's answer, and the host's own when it leaves the
    /// decision to the host (a desktop loop ends with its last window).
    fn exit_due(&self) -> bool {
        let emptied = self.windows.take_emptied();
        let reevaluate = self.reevaluate.replace(false);
        if !(emptied || reevaluate) || !self.windows.is_empty() {
            return false;
        }
        self.app
            .exit_policy()
            .or_resume(ExitAnswer::HostDecides)
            .ends_loop(true)
            // Asked with no lock held, the policy may have opened a
            // window: one opened meanwhile keeps the loop, as on X11 and
            // Win32.
            && self.windows.is_empty()
    }

    /// Block until the compositor sends something, another producer
    /// wakes the loop, or `timeout` passes; then read what came.
    fn wait(&self, timeout: Option<Duration>) -> Result<(), PlatformError> {
        let (next, guard) = self.dispatching(|queue, state| {
            let dispatched = queue.dispatch_pending(state).map_err(lost)?;
            let flush = match self.connection.flush() {
                Ok(()) => Flush::Complete,
                // A full socket: the rest goes out once it takes more.
                Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    Flush::Partial
                }
                Err(error) => return Err(lost(error)),
            };
            let next = before_poll(dispatched, flush);
            // `None`: events were queued meanwhile, and the next turn
            // dispatches them.
            let guard = match next {
                BeforePoll::Apply => None,
                BeforePoll::Poll { .. } => queue.prepare_read(),
            };
            Ok((next, guard))
        })?;
        let (BeforePoll::Poll { writable }, Some(guard)) = (next, guard) else {
            return Ok(());
        };
        let (compositor, woken) = {
            let connection = guard.connection_fd();
            let watched = if writable {
                PollFlags::IN | PollFlags::OUT
            } else {
                PollFlags::IN
            };
            let mut descriptors = [
                PollFd::new(&connection, watched),
                PollFd::new(&self.waker.0, PollFlags::IN),
            ];
            let timeout = timeout.map(timespec);
            loop {
                match poll(&mut descriptors, timeout.as_ref()) {
                    Ok(_) => break,
                    Err(Errno::INTR) => {}
                    Err(error) => return Err(lost(error)),
                }
            }
            let ready = |descriptor: &PollFd<'_>| {
                descriptor
                    .revents()
                    .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
            };
            (ready(&descriptors[0]), ready(&descriptors[1]))
        };
        if compositor {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(lost(error)),
            }
        } else {
            // Dropping the guard cancels the read.
            drop(guard);
        }
        if woken {
            self.waker.drain();
        }
        Ok(())
    }

    /// Start what the loop keeps while it runs: the portal and the
    /// display watch. Before the application's bootstrap, so its first
    /// read sees them.
    fn start_sources(&self) {
        let preferences = (self.start_preferences)(&self.executor, &self.signal);
        self.portal.set(Some(&preferences));
        self.sources.borrow_mut().preferences = Some(preferences);
        let watch = Arc::clone(&self.displays).observe(&self.signal);
        *self.display_watch.borrow_mut() = Some(watch);
    }

    /// Start the sources of the host's notices, once the owner can
    /// deliver what they queue: a sleep that arrived during the bootstrap
    /// would hold logind's inhibitor with no turn to deliver it.
    fn start_host_notices(&self) {
        let notices = (self.start_host_notices)(&self.executor, &self.signal);
        self.sources.borrow_mut().host_notices = Some(notices);
    }

    /// Stop what the loop started: admission first, so a source that
    /// still reports wakes nobody, then the sources. The application code it
    /// runs (the wake callback and queued window requests dropped, their
    /// requesters answered) is contained, so an unwinding run calls it too.
    fn stop_sources(&self) {
        self.signal.close_in(&self.turn);
        let watch = self.display_watch.borrow_mut().take();
        let sources = std::mem::take(&mut *self.sources.borrow_mut());
        self.portal.set(None);
        contain_owner_callback(|| drop(watch));
        contain_owner_callback(|| drop(sources));
    }

    /// End the run: the sources stop, every window still open lets go of
    /// its handler and surface, and the application hears the quit, once,
    /// before its handler is released.
    fn finish(&self, mut failure: FirstPanic) {
        self.stop_sources();
        for window in self.windows.values() {
            window.release();
        }
        self.input.release();
        self.clipboard.release();
        self.drag.release();
        self.devices.release();
        let quit = self.app.quit_requested();
        self.app.release();
        // The exit policy's parked panic is the run's first failure: it
        // comes out ahead of the loop's (`failure`) and the quit's.
        if let Called::Panicked(quit) = quit {
            failure.record(quit.into_payload());
        }
        self.app.resume_after_run(failure);
    }
}

/// Stops the sources of a run that unwinds; a run that ends calls
/// [`WaylandLoop::finish`] and disarms it.
struct RunGuard<'a>(Option<&'a WaylandLoop>);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.0.take() {
            owner.stop_sources();
        }
    }
}

impl host::Loop for WaylandLoop {
    fn run(
        self: Rc<Self>,
        platform: Rc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        if std::thread::current().id() != self.owner_thread {
            return Err(PlatformError::EventLoop {
                message: "a Wayland platform runs on the thread that made it, which owns its \
                          windows"
                    .to_owned(),
            });
        }
        if self.ran.replace(true) {
            return Err(PlatformError::EventLoop {
                message: "a Wayland platform runs once".to_owned(),
            });
        }
        tracing::info!("starting the Wayland loop");
        self.signal.bind_owner();
        let hooks: Rc<dyn OwnerHooks> = Rc::new(DirectOwnerHooks::with_signal(
            Rc::clone(&platform),
            Arc::clone(&self.signal),
            Rc::clone(&self.turn),
        ));
        // The delivery holds the run's hooks weakly and mints the owner
        // capability of every later call from them: the run keeps them.
        self.app.bind(handler, &platform, &hooks);
        self.app.resume_through_signal(&self.signal);
        let mut guard = RunGuard(Some(&self));
        self.start_sources();
        // A turn's panic leaves the loop here and the run still ends in
        // order; the exit policy's parked panic comes out ahead of it.
        let mut failure = FirstPanic::new();
        let outcome = failure.contain(|| {
            self.app
                .ready(OwnerPlatform::new(platform, Rc::clone(&hooks)))
                .map_err(PlatformError::bootstrap)
                .and_then(|()| {
                    self.signal
                        .start()
                        .map_err(|error| PlatformError::EventLoop {
                            message: error.to_string(),
                        })?;
                    self.start_host_notices();
                    while !self.turn(None)? {}
                    Ok(())
                })
        });
        guard.0 = None;
        tracing::info!("the Wayland loop ended");
        self.finish(failure);
        drop(hooks);
        outcome.expect("BUG: a loop that raised no panic returned its outcome")
    }

    fn quit(&self) {
        // A quit before the loop runs ends it as soon as it starts.
        if let Err(error) = self.signal.request_quit() {
            tracing::debug!(%error, "the Wayland loop has already ended");
        }
    }

    fn request_exit_policy_reevaluation(&self) {
        self.reevaluate.set(true);
        self.waker.wake();
    }
}

impl host::Launch for WaylandLoop {
    /// Only a page has an origin.
    fn page_origin(&self) -> Option<String> {
        None
    }

    fn launch_value(&self, key: flui_platform::LaunchKey) -> Option<String> {
        host::environment_launch_value(key)
    }
}

/// The displays position is the loop's: the source the table registered.
impl host::Displays<WaylandLoop> for WaylandLoop {
    fn snapshot(owner: &WaylandLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner.displays.snapshot()
    }
}

/// The clipboard position is the loop's: the seats' selection, through
/// the clipboard the table registered.
impl host::Clipboard<WaylandLoop> for WaylandLoop {
    fn clipboard(owner: &WaylandLoop) -> Rc<dyn host::NativeClipboard> {
        Rc::new(Arc::clone(&owner.clipboard))
    }
}

/// The drag position is the loop's: the drags over the windows, through
/// the drag the table registered, the platform's one data-transfer
/// source.
impl host::Drag<WaylandLoop> for WaylandLoop {
    fn data_transfer(owner: &WaylandLoop) -> Arc<dyn DragTransport> {
        Arc::clone(&owner.drag) as Arc<dyn DragTransport>
    }
}

/// The primary selection position is the loop's too, where the
/// compositor has `zwp_primary_selection_device_manager_v1`.
impl host::PrimarySelection<WaylandLoop> for WaylandLoop {
    fn primary_selection(owner: &WaylandLoop) -> Option<Rc<dyn host::NativePrimarySelection>> {
        owner
            .clipboard
            .has_primary_selection()
            .then(|| -> Rc<dyn host::NativePrimarySelection> {
                Rc::new(Arc::clone(&owner.clipboard))
            })
    }
}

/// The input position is the loop's: the seats' input the table
/// registered, whose keymap names the layout.
impl host::Input<WaylandLoop> for WaylandLoop {
    fn keyboard_layout(owner: &WaylandLoop) -> String {
        owner.input.keyboard_layout()
    }

    fn on_keyboard_layout_change(owner: &WaylandLoop, callback: Box<dyn FnMut()>) {
        owner.input.on_keyboard_layout_change(callback);
    }
}

/// The preference position is the loop's: the portal it starts as it
/// runs, read on the owner thread only.
impl host::Preferences<WaylandLoop> for WaylandLoop {
    fn sample(owner: &WaylandLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
        if std::thread::current().id() != owner.owner_thread || !owner.signal.accepting() {
            return Err(PlatformError::Preferences {
                message: "the system's preferences require the live Wayland loop's owner".into(),
            });
        }
        owner
            .portal
            .source()
            .ok_or(PlatformError::PreferencesDeferred)?
            .sample()
    }

    fn delivered(owner: &WaylandLoop, values: &flui_platform::SystemPreferences) {
        if let Some(source) = owner.portal.source() {
            source.delivered(values);
        }
    }
}

impl SessionLoop for WaylandLoop {
    fn notice(&self, notice: SystemNotice) {
        self.notices.dispatch(notice);
    }

    fn session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.app
            .session_end(phase)
            .or_resume(SessionEndAnswer::Proceed)
    }
}

/// logind cannot be asked to wait for the user, so a session end the
/// application held has nothing to release.
impl LifecycleHooks for WaylandLoop {
    fn release_session_end(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use wayland_client::protocol::wl_registry;
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::protocol::wl_surface::WlSurface;

    use crate::owner::linux::wayland::user_input::UserInput;
    use crate::owner::linux::wayland::wait::FrameHold;

    /// A window the table holds that does nothing on a turn.
    struct Idle;

    impl LoopWindow for Idle {
        fn settle(&self) {}
        fn frame_due(&self) -> bool {
            false
        }
        fn frame_wait(&self) -> Option<Instant> {
            None
        }
        fn frame_hold(&self) -> FrameHold {
            FrameHold::Free
        }
        fn mode_due(&self) -> Option<Instant> {
            None
        }
        fn request_frame(&self) {}
        fn release(&self) {}
        fn activate_forwarded(&self, _token: Option<ActivationToken>) {}
    }

    /// The launcher's token goes to the first window the table holds,
    /// once: a window refused (here, one under an identity the table
    /// holds already) leaves it for the next, and no later window gets
    /// one.
    #[test]
    fn the_launch_token_goes_to_the_first_window_held() {
        let token = ActivationToken::new("launcher".to_owned());
        let table = WindowTable::new(Some(token.clone()));
        let issue = || table.issue().expect("the table issues an id");
        let (first, second, third) = (issue(), issue(), issue());
        assert!(
            table
                .windows
                .borrow_mut()
                .insert(first, Rc::new(Idle))
                .is_none(),
            "a window held before"
        );
        assert!(
            table.register(first, Rc::new(Idle)).is_err(),
            "an identity held already is refused"
        );
        assert_eq!(
            table.register(second, Rc::new(Idle)).ok(),
            Some(Some(token)),
            "the first window held takes it"
        );
        assert_eq!(table.register(third, Rc::new(Idle)).ok(), Some(None));
    }

    /// `bindable` drops a global announced at version zero and passes
    /// any other version and a removal. The test proves the function
    /// alone: its two callers, the loop's registry dispatch
    /// (`Dispatch<WlRegistry, ()> for WaylandState`) and the display
    /// watch's (`display::linux::wayland`), need a compositor to run,
    /// and dropping either call fails nothing here.
    #[test]
    fn a_global_of_version_zero_is_bound_by_none() {
        let global = |version| wl_registry::Event::Global {
            name: 7,
            interface: "wl_output".to_owned(),
            version,
        };
        assert!(bindable(global(0)).is_none(), "version zero");
        for version in [1, 4, u32::MAX] {
            assert!(
                matches!(
                    bindable(global(version)),
                    Some(wl_registry::Event::Global { version: told, .. }) if told == version
                ),
                "version {version}"
            );
        }
        assert!(matches!(
            bindable(wl_registry::Event::GlobalRemove { name: 7 }),
            Some(wl_registry::Event::GlobalRemove { name: 7 })
        ));
    }

    /// The last input recorded is the one an activation token names,
    /// on the owner thread only: another thread reads none.
    #[test]
    fn the_last_input_is_the_one_recorded_last() {
        let (ours, _theirs) = std::os::unix::net::UnixStream::pair().expect("a socket pair");
        let Ok(connection) = Connection::from_socket(ours) else {
            eprintln!("no libwayland-client to load; the case was not run");
            return;
        };
        let backend = connection.backend().downgrade();
        let input = |serial| UserInput {
            seat: WlSeat::inert(backend.clone()),
            serial,
            surface: WlSurface::inert(backend.clone()),
        };
        let last = LastInput::default();
        assert!(last.latest().is_none(), "nothing recorded yet");
        last.record(input(7));
        last.record(input(42));
        assert_eq!(last.latest().map(|input| input.serial), Some(42));
        let off_owner = std::thread::spawn(move || last.latest().map(|input| input.serial))
            .join()
            .expect("the other thread");
        assert_eq!(off_owner, None, "off the owner thread");
    }

    /// A grab is the first press a seat's pointer holds, kept until its
    /// last button goes up or the pointer leaves its surface; a watched
    /// grab ends with a release over its surface, not with a leave.
    #[test]
    fn a_grab_is_the_first_press_held_until_its_last_release() {
        let (ours, _theirs) = std::os::unix::net::UnixStream::pair().expect("a socket pair");
        let Ok(connection) = Connection::from_socket(ours) else {
            eprintln!("no libwayland-client to load; the case was not run");
            return;
        };
        let backend = connection.backend().downgrade();
        let seat = WlSeat::inert(backend.clone());
        let surface = WlSurface::inert(backend);
        let press = |serial| UserInput {
            seat: seat.clone(),
            serial,
            surface: surface.clone(),
        };
        let grabs = LastInput::default();
        let serial = |grabs: &LastInput| grabs.grab_on(&surface).map(|grab| grab.serial);
        assert_eq!(serial(&grabs), None, "no button held");
        grabs.pressed(press(3));
        grabs.pressed(press(4));
        assert_eq!(serial(&grabs), Some(3), "the grab's first press");
        grabs.watch(seat.clone(), 3);
        grabs.released(&seat);
        assert_eq!(serial(&grabs), Some(3), "a button still holds it");
        grabs.released(&seat);
        assert_eq!(serial(&grabs), None, "the last release ends it");
        assert!(
            grabs.watched_ended(&seat, 3),
            "a release ended the grab of 3"
        );
        grabs.pressed(press(9));
        grabs.watch(seat.clone(), 9);
        grabs.left(&seat);
        assert_eq!(serial(&grabs), None, "a leave ends it");
        assert!(
            !grabs.watched_ended(&seat, 9),
            "no release reached the surface the grab of 9 left"
        );
        grabs.seat_gone(&seat);
        assert!(grabs.watched_ended(&seat, 9), "its seat went");
    }
}
