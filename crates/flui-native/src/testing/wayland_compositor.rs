//! A Wayland compositor a test scripts: it speaks the wire protocol over a
//! socket of its own to an unmodified Wayland host, and does on demand what
//! no installed compositor does: a seat older than `repeat_info`, a
//! `configure_bounds`, a scale change while the host runs, a fractional scale
//! no screen has, a toplevel it never configures, a frame callback it never
//! answers, an output that goes with no `leave`, a burst of configures, a
//! connection it closes in the middle of a request.
//!
//! It runs on a thread of its own. A test connects the host to
//! [`ScriptedCompositor::socket`] (`WaylandPlatform::try_new_at`), plays it
//! [`Step`]s, and reads back what the host asked of it ([`Heard`]). Steps are
//! carried out in the order played, at the compositor's next turn, which comes
//! as soon as they are played.
//!
//! What it offers is an [`Offer`]: the globals and their versions, chosen per
//! test. It enforces these rules of the protocol, and no others, answering a
//! client that breaks one with the protocol error that ends the connection,
//! as a strict compositor does:
//!
//! - `xdg_surface.invalid_serial`: an `ack_configure` of a serial it never
//!   sent the surface, or of one older than a serial already acknowledged;
//! - `xdg_surface.unconfigured_buffer`: a toplevel's buffer committed before
//!   its first configure is acknowledged;
//! - `wl_surface.invalid_scale`: a buffer scale below one;
//! - `wl_surface.invalid_size`: a buffer whose sides its buffer scale does
//!   not divide, with no viewport destination to size the surface;
//! - `wl_pointer.role`: a cursor set on a surface that is a toplevel.
//!
//! It draws nothing; a committed buffer is held until the next one replaces
//! it, and then released.

use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rustix::event::{EventfdFlags, PollFd, PollFlags, eventfd, poll};
use wayland_protocols::wp::fractional_scale::v1::server::wp_fractional_scale_manager_v1::{
    self, WpFractionalScaleManagerV1,
};
use wayland_protocols::wp::fractional_scale::v1::server::wp_fractional_scale_v1::{
    self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::server::wp_viewport::{self, WpViewport};
use wayland_protocols::wp::viewporter::server::wp_viewporter::{self, WpViewporter};
use wayland_protocols::xdg::shell::server::xdg_positioner::{self, XdgPositioner};
use wayland_protocols::xdg::shell::server::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::server::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::server::xdg_wm_base::{self, XdgWmBase};
use wayland_server::backend::{ClientData, ClientId, DisconnectReason, GlobalId};
use wayland_server::protocol::wl_buffer::{self, WlBuffer};
use wayland_server::protocol::wl_callback::WlCallback;
use wayland_server::protocol::wl_compositor::{self, WlCompositor};
use wayland_server::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_server::protocol::wl_output::{self, WlOutput};
use wayland_server::protocol::wl_pointer::{self, WlPointer};
use wayland_server::protocol::wl_region::{self, WlRegion};
use wayland_server::protocol::wl_seat::{self, WlSeat};
use wayland_server::protocol::wl_shm::{self, WlShm};
use wayland_server::protocol::wl_shm_pool::{self, WlShmPool};
use wayland_server::protocol::wl_surface::{self, WlSurface};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, ListeningSocket, New,
    Resource,
};

pub use wayland_protocols::xdg::shell::server::xdg_toplevel::State as ToplevelState;

/// The keymap the seat's keyboard shares: one key, `a` (evdev 30), spelled out
/// in full so it compiles on any xkeyboard-config.
const KEYMAP: &[u8] = b"xkb_keymap {
xkb_keycodes \"flui\" { minimum = 8; maximum = 255; <AC01> = 38; };
xkb_types \"flui\" { type \"ONE_LEVEL\" { modifiers = none; level_name[Level1] = \"Any\"; }; };
xkb_compatibility \"flui\" { };
xkb_symbols \"flui\" { name[Group1] = \"FLUI\"; key <AC01> { type = \"ONE_LEVEL\", [ a ] }; };
};
\0";

/// The evdev code of the keymap's one key, `a`.
pub const KEY_A: u32 = 30;

/// How long the compositor's thread waits for a client, a step or its end
/// before it looks again.
const TURN: Duration = Duration::from_millis(50);

/// One output the compositor announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutputOffer {
    /// Its mode, in pixels.
    pub size: (i32, i32),
    /// Its whole scale (`wl_output.scale`).
    pub scale: i32,
}

impl OutputOffer {
    /// An output of `width` by `height` pixels at `scale`.
    #[must_use]
    pub const fn new(width: i32, height: i32, scale: i32) -> Self {
        Self {
            size: (width, height),
            scale,
        }
    }
}

/// What the compositor offers: its globals and their versions. The
/// default is a current desktop compositor's floor with no seat:
/// `wl_compositor` 6, `xdg_wm_base` 6, `wl_shm`, `wp_viewporter`, and one
/// output of 1920 by 1080 at scale 1.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Offer {
    compositor: u32,
    shell: u32,
    seat: Option<u32>,
    viewporter: bool,
    fractional_scale: bool,
    outputs: Vec<OutputOffer>,
    first: Option<Configure>,
    withhold_frame_callbacks: bool,
    hang_up: Option<HangUp>,
}

impl Default for Offer {
    fn default() -> Self {
        Self {
            compositor: 6,
            shell: 6,
            seat: None,
            viewporter: true,
            fractional_scale: false,
            outputs: vec![OutputOffer::new(1920, 1080, 1)],
            first: Some(Configure::default().state(ToplevelState::Activated)),
            withhold_frame_callbacks: false,
            hang_up: None,
        }
    }
}

impl Offer {
    /// `wl_compositor` at `version` (1 to 6): below 6 a surface is told no
    /// preferred scale, below 4 it has no `damage_buffer`.
    #[must_use]
    pub fn compositor(mut self, version: u32) -> Self {
        self.compositor = version.clamp(1, 6);
        self
    }

    /// `xdg_wm_base` at `version` (1 to 6): from 4 a toplevel is told
    /// `configure_bounds`, from 5 its window manager's capabilities.
    #[must_use]
    pub fn shell(mut self, version: u32) -> Self {
        self.shell = version.clamp(1, 6);
        self
    }

    /// A seat with a pointer and a keyboard, at `version` (1 to 9): below 4
    /// its keyboard is told no `repeat_info`.
    #[must_use]
    pub fn seat(mut self, version: u32) -> Self {
        self.seat = Some(version.clamp(1, 9));
        self
    }

    /// Offer `wp_viewporter`, or not.
    #[must_use]
    pub fn viewporter(mut self, offered: bool) -> Self {
        self.viewporter = offered;
        self
    }

    /// Offer `wp_fractional_scale_manager_v1`, whose scales
    /// [`Step::PreferFractionalScale`] sends.
    #[must_use]
    pub fn fractional_scale(mut self) -> Self {
        self.fractional_scale = true;
        self
    }

    /// The outputs announced, in order; a [`Step`] names one by its place.
    #[must_use]
    pub fn outputs(mut self, outputs: impl IntoIterator<Item = OutputOffer>) -> Self {
        self.outputs = outputs.into_iter().collect();
        self
    }

    /// The configure a toplevel's first commit is answered with; `None`
    /// answers it with none, so the toplevel waits for one a [`Step`] sends.
    #[must_use]
    pub fn first_configure(mut self, configure: Option<Configure>) -> Self {
        self.first = configure;
        self
    }

    /// Hold back every frame callback from the start, as a compositor does
    /// for a surface it does not draw ([`Step::WithholdFrameCallbacks`]).
    #[must_use]
    pub fn withhold_frame_callbacks(mut self) -> Self {
        self.withhold_frame_callbacks = true;
        self
    }

    /// Close the client's connection at `moment`, with no error.
    #[must_use]
    pub fn hang_up(mut self, moment: HangUp) -> Self {
        self.hang_up = Some(moment);
        self
    }
}

/// A request in the middle of which the compositor closes the connection,
/// answering nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HangUp {
    /// The client asks for an `xdg_toplevel`: in the middle of opening a
    /// window, before its first configure.
    OnToplevel,
    /// The client commits its toplevel's first buffer: in the middle of its
    /// first frame.
    OnFirstFrame,
}

/// A toplevel configure: the size, the states and the bounds before it.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Configure {
    size: (i32, i32),
    states: Vec<ToplevelState>,
    bounds: Option<(i32, i32)>,
}

impl Configure {
    /// The size, in logical pixels; `0` on an axis leaves it to the window.
    #[must_use]
    pub fn size(mut self, width: i32, height: i32) -> Self {
        self.size = (width, height);
        self
    }

    /// Add `state` to the states.
    #[must_use]
    pub fn state(mut self, state: ToplevelState) -> Self {
        self.states.push(state);
        self
    }

    /// Send `configure_bounds` of `width` by `height` first (`xdg_wm_base`
    /// version 4 and later).
    #[must_use]
    pub fn bounds(mut self, width: i32, height: i32) -> Self {
        self.bounds = Some((width, height));
        self
    }
}

/// One thing the compositor does when a test plays it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Step {
    /// Configure the last toplevel the client made.
    Configure(Configure),
    /// Prefer this whole scale for the toplevel's surface
    /// (`wl_surface.preferred_buffer_scale`, `wl_compositor` version 6).
    PreferBufferScale(i32),
    /// The toplevel's surface enters the output at this place of the offer.
    Enter(usize),
    /// The toplevel's surface leaves the output at this place of the offer.
    Leave(usize),
    /// Remove the output at this place of the offer, with no `leave` to the
    /// surfaces on it.
    RemoveOutput(usize),
    /// The seat's pointer enters the toplevel's surface at this point.
    PointerEnter {
        /// Logical x on the surface.
        x: f64,
        /// Logical y on the surface.
        y: f64,
    },
    /// The seat's keyboard enters the toplevel's surface.
    KeyboardEnter,
    /// A key of the seat's keyboard, by evdev code, goes down or up.
    Key {
        /// The evdev code ([`KEY_A`] is the keymap's one key).
        code: u32,
        /// Down or up.
        down: bool,
    },
    /// Prefer this fractional scale, a numerator over 120, for the
    /// toplevel's surface (`wp_fractional_scale_v1.preferred_scale`): any
    /// number, zero and ones past any screen's included.
    PreferFractionalScale(u32),
    /// Hold back every frame callback from now (`true`), or answer those
    /// held and every later one (`false`).
    WithholdFrameCallbacks(bool),
    /// Close the client's connection now, with no error.
    HangUp,
}

/// A protocol error the compositor posted to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PostedError {
    /// The interface of the object it was posted on.
    pub interface: &'static str,
    /// The error's code in that interface.
    pub code: u32,
    /// What the compositor said.
    pub message: String,
}

/// One buffer committed to a surface the client made its cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CursorImage {
    /// The buffer's size, in pixels.
    pub size: (i32, i32),
    /// The surface's buffer scale.
    pub scale: i32,
    /// The surface's viewport destination, in logical pixels, if it has one.
    pub destination: Option<(i32, i32)>,
}

/// What the compositor heard from its client.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Heard {
    /// How many connections are open now: the host's own, and its display
    /// source's while that one is open (a first read of the outputs closes
    /// again once it has them).
    pub clients: u32,
    /// How many connections ended.
    pub disconnects: u32,
    /// The protocol errors the compositor posted.
    pub errors: Vec<PostedError>,
    /// The serials of the configures sent, in order.
    pub configures: Vec<u32>,
    /// The serials the client acknowledged, in order.
    pub acknowledged: Vec<u32>,
    /// How many commits of the toplevel's surface carried a new buffer.
    pub frames: u32,
    /// The toplevel's last buffer, in pixels.
    pub buffer: Option<(i32, i32)>,
    /// How many frame callbacks the toplevel's surface asked for.
    pub frame_callbacks: u32,
    /// How many of them were held back and are still unanswered.
    pub withheld: u32,
    /// Every buffer committed to a cursor surface, in order.
    pub cursor: Vec<CursorImage>,
}

/// What the test thread and the compositor's thread share.
#[derive(Debug, Default)]
struct Shared {
    heard: Mutex<Heard>,
}

impl Shared {
    fn note(&self, change: impl FnOnce(&mut Heard)) {
        change(&mut self.heard.lock());
    }
}

/// A Wayland compositor on a thread of its own, at a socket of its own,
/// playing what a test scripts. Dropping it stops the thread and removes
/// the socket.
#[derive(Debug)]
pub struct ScriptedCompositor {
    socket: PathBuf,
    steps: Sender<Control>,
    wake: Arc<OwnedFd>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    _directory: tempfile::TempDir,
}

/// What the test thread sends the compositor's.
#[derive(Debug)]
enum Control {
    Play(Step),
    Stop,
}

impl ScriptedCompositor {
    /// Start a compositor offering `offer`, listening on a socket in a
    /// private directory of its own.
    ///
    /// # Errors
    /// The directory, the socket, the thread's wake descriptor or the thread
    /// could not be made.
    pub fn start(offer: Offer) -> std::io::Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("flui-scripted-compositor")
            .tempdir()?;
        let socket = directory.path().join("wayland");
        let listening =
            ListeningSocket::bind_absolute(socket.clone()).map_err(std::io::Error::other)?;
        let wake = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?);
        let shared = Arc::new(Shared::default());
        let (steps, received) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("flui-scripted-compositor".into())
            .spawn({
                let wake = Arc::clone(&wake);
                let shared = Arc::clone(&shared);
                move || serve(&offer, &listening, &wake, &received, &shared)
            })?;
        Ok(Self {
            socket,
            steps,
            wake,
            shared,
            thread: Some(thread),
            _directory: directory,
        })
    }

    /// The socket's path, which a host connects to.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Play `step` at the compositor's next turn, after the steps played
    /// before it.
    pub fn play(&self, step: Step) {
        if self.steps.send(Control::Play(step)).is_ok() {
            self.ring();
        }
    }

    /// What the compositor has heard so far.
    #[must_use]
    pub fn heard(&self) -> Heard {
        self.shared.heard.lock().clone()
    }

    fn ring(&self) {
        let _ = rustix::io::write(self.wake.as_fd(), &1_u64.to_ne_bytes());
    }
}

impl Drop for ScriptedCompositor {
    fn drop(&mut self) {
        let _ = self.steps.send(Control::Stop);
        self.ring();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The compositor's thread: accept clients, read their requests, play the
/// steps the test sends, until it says stop.
fn serve(
    offer: &Offer,
    listening: &ListeningSocket,
    wake: &OwnedFd,
    steps: &Receiver<Control>,
    shared: &Arc<Shared>,
) {
    let Ok(mut display) = Display::<Compositor>::new() else {
        return;
    };
    let mut state = Compositor::new(offer, &display.handle(), Arc::clone(shared));
    let Ok(timeout) = rustix::time::Timespec::try_from(TURN) else {
        return;
    };
    loop {
        let (accept, requests, woken) = {
            let mut fds = [
                PollFd::new(listening, PollFlags::IN),
                PollFd::new(&display, PollFlags::IN),
                PollFd::new(wake, PollFlags::IN),
            ];
            if poll(&mut fds, Some(&timeout)).is_err() {
                continue;
            }
            let ready = |fd: &PollFd<'_>| !fd.revents().is_empty();
            (ready(&fds[0]), ready(&fds[1]), ready(&fds[2]))
        };
        if woken {
            let _ = rustix::io::read(wake, &mut [0_u8; 8]);
        }
        if accept
            && let Ok(Some(stream)) = listening.accept()
            && let Ok(hang_up) = stream.try_clone()
        {
            let client = ClientState {
                shared: Arc::clone(shared),
            };
            if display
                .handle()
                .insert_client(stream, Arc::new(client))
                .is_ok()
            {
                state.connections.push(hang_up);
                shared.note(|heard| heard.clients = heard.clients.saturating_add(1));
            }
        }
        if requests {
            let _ = display.dispatch_clients(&mut state);
        }
        loop {
            match steps.try_recv() {
                Ok(Control::Play(step)) => state.play(step, &display.handle()),
                Ok(Control::Stop) | Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => break,
            }
        }
        state.hang_up_if_asked();
        let _ = display.flush_clients();
    }
}

/// A connection's own data: it tells the shared record when it ends.
struct ClientState {
    shared: Arc<Shared>,
}

impl ClientData for ClientState {
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {
        self.shared.note(|heard| {
            heard.clients = heard.clients.saturating_sub(1);
            heard.disconnects = heard.disconnects.saturating_add(1);
        });
    }
}

/// A state a surface's next commit applies: set by a request since the last
/// commit, or left as it is. What it is set to may itself be nothing (a
/// detached buffer, an unset destination).
enum Pending<T> {
    Unchanged,
    Set(T),
}

impl<T> Pending<T> {
    /// What the commit applies, leaving nothing pending.
    fn take(&mut self) -> Option<T> {
        match std::mem::replace(self, Self::Unchanged) {
            Self::Set(value) => Some(value),
            Self::Unchanged => None,
        }
    }
}

/// The role a surface was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    None,
    Toplevel,
    Cursor,
}

/// What a surface holds: the state its next commit applies, and the state it
/// shows.
struct SurfaceState {
    surface: WlSurface,
    role: Role,
    pending_buffer: Pending<Option<WlBuffer>>,
    buffer: Option<WlBuffer>,
    pending_scale: Option<i32>,
    scale: i32,
    viewport: Option<WpViewport>,
    pending_destination: Pending<Option<(i32, i32)>>,
    destination: Option<(i32, i32)>,
    frames: Vec<WlCallback>,
}

impl SurfaceState {
    /// The buffer the surface shows, as a cursor shows it.
    fn cursor_image(&self) -> Option<CursorImage> {
        let size = self.buffer.as_ref()?.data::<BufferSize>()?;
        Some(CursorImage {
            size: (size.width, size.height),
            scale: self.scale,
            destination: self.destination,
        })
    }
}

/// The client's last toplevel.
struct ShellWindow {
    surface: WlSurface,
    xdg: XdgSurface,
    xdg_toplevel: XdgToplevel,
    /// Whether its first configure was sent.
    first_sent: bool,
    /// The serials of the configures sent and not yet acknowledged.
    unacknowledged: Vec<u32>,
    /// The serial last acknowledged; `None` before the first, while the
    /// toplevel may commit no buffer.
    acknowledged: Option<u32>,
}

/// One announced output.
struct OutputSlot {
    offer: OutputOffer,
    global: Option<GlobalId>,
    bound: Vec<WlOutput>,
}

/// A buffer's size, the user data of its `wl_buffer`.
#[derive(Debug, Clone, Copy)]
struct BufferSize {
    width: i32,
    height: i32,
}

/// The compositor's state, on its own thread.
struct Compositor {
    offer: Offer,
    shared: Arc<Shared>,
    started: Instant,
    serial: u32,
    withhold: bool,
    hang_up_now: bool,
    /// The compositor's end of each connection, to close it by.
    connections: Vec<UnixStream>,
    surfaces: Vec<SurfaceState>,
    toplevel: Option<ShellWindow>,
    outputs: Vec<OutputSlot>,
    pointers: Vec<WlPointer>,
    keyboards: Vec<WlKeyboard>,
    withheld: Vec<WlCallback>,
    /// Each surface's fractional-scale object.
    fractional: Vec<(WlSurface, WpFractionalScaleV1)>,
}

impl Compositor {
    fn new(offer: &Offer, display: &DisplayHandle, shared: Arc<Shared>) -> Self {
        display.create_global::<Self, WlCompositor, ()>(offer.compositor, ());
        display.create_global::<Self, XdgWmBase, ()>(offer.shell, ());
        display.create_global::<Self, WlShm, ()>(1, ());
        if offer.viewporter {
            display.create_global::<Self, WpViewporter, ()>(1, ());
        }
        if offer.fractional_scale {
            display.create_global::<Self, WpFractionalScaleManagerV1, ()>(1, ());
        }
        if let Some(version) = offer.seat {
            display.create_global::<Self, WlSeat, ()>(version, ());
        }
        let outputs = offer
            .outputs
            .iter()
            .enumerate()
            .map(|(place, output)| OutputSlot {
                offer: *output,
                global: Some(display.create_global::<Self, WlOutput, usize>(4, place)),
                bound: Vec::new(),
            })
            .collect();
        Self {
            withhold: offer.withhold_frame_callbacks,
            offer: offer.clone(),
            shared,
            started: Instant::now(),
            serial: 0,
            hang_up_now: false,
            connections: Vec::new(),
            surfaces: Vec::new(),
            toplevel: None,
            outputs,
            pointers: Vec::new(),
            keyboards: Vec::new(),
            withheld: Vec::new(),
            fractional: Vec::new(),
        }
    }

    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }

    fn time(&self) -> u32 {
        u32::try_from(self.started.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    fn surface_mut(&mut self, surface: &WlSurface) -> Option<&mut SurfaceState> {
        self.surfaces
            .iter_mut()
            .find(|held| held.surface.id() == surface.id())
    }

    /// Send the toplevel `configure`, its bounds first where the shell's
    /// version has them.
    fn configure(&mut self, configure: &Configure) {
        let serial = self.next_serial();
        let Some(toplevel) = &mut self.toplevel else {
            return;
        };
        if let Some((width, height)) = configure.bounds
            && toplevel.xdg_toplevel.version() >= 4
        {
            toplevel.xdg_toplevel.configure_bounds(width, height);
        }
        let states = configure
            .states
            .iter()
            .flat_map(|state| (*state as u32).to_ne_bytes())
            .collect();
        toplevel
            .xdg_toplevel
            .configure(configure.size.0, configure.size.1, states);
        toplevel.xdg.configure(serial);
        toplevel.first_sent = true;
        toplevel.unacknowledged.push(serial);
        self.shared.note(|heard| heard.configures.push(serial));
    }

    fn play(&mut self, step: Step, display: &DisplayHandle) {
        match step {
            Step::Configure(configure) => self.configure(&configure),
            Step::PreferBufferScale(scale) => {
                if let Some(toplevel) = &self.toplevel
                    && toplevel.surface.version() >= 6
                {
                    toplevel.surface.preferred_buffer_scale(scale);
                }
            }
            Step::Enter(place) | Step::Leave(place) => {
                let Some(toplevel) = &self.toplevel else {
                    return;
                };
                let Some(slot) = self.outputs.get(place) else {
                    return;
                };
                for output in slot
                    .bound
                    .iter()
                    .filter(|output| output.client() == toplevel.surface.client())
                {
                    if matches!(step, Step::Enter(_)) {
                        toplevel.surface.enter(output);
                    } else {
                        toplevel.surface.leave(output);
                    }
                }
            }
            Step::RemoveOutput(place) => {
                if let Some(global) = self
                    .outputs
                    .get_mut(place)
                    .and_then(|slot| slot.global.take())
                {
                    display.remove_global::<Self>(global);
                }
            }
            Step::PointerEnter { x, y } => {
                let serial = self.next_serial();
                let Some(toplevel) = &self.toplevel else {
                    return;
                };
                for pointer in &self.pointers {
                    pointer.enter(serial, &toplevel.surface, x, y);
                    if pointer.version() >= 5 {
                        pointer.frame();
                    }
                }
            }
            Step::KeyboardEnter => {
                let serial = self.next_serial();
                let Some(toplevel) = &self.toplevel else {
                    return;
                };
                for keyboard in &self.keyboards {
                    keyboard.enter(serial, &toplevel.surface, Vec::new());
                    keyboard.modifiers(serial, 0, 0, 0, 0);
                }
            }
            Step::Key { code, down } => {
                let serial = self.next_serial();
                let time = self.time();
                let state = if down {
                    wl_keyboard::KeyState::Pressed
                } else {
                    wl_keyboard::KeyState::Released
                };
                for keyboard in &self.keyboards {
                    keyboard.key(serial, time, code, state);
                }
            }
            Step::WithholdFrameCallbacks(withhold) => {
                self.withhold = withhold;
                if !withhold {
                    let time = self.time();
                    for callback in self.withheld.drain(..) {
                        callback.done(time);
                    }
                    self.shared.note(|heard| heard.withheld = 0);
                }
            }
            Step::PreferFractionalScale(numerator) => {
                let Some(toplevel) = &self.toplevel else {
                    return;
                };
                for (_, scale) in self
                    .fractional
                    .iter()
                    .filter(|(surface, _)| surface.id() == toplevel.surface.id())
                {
                    scale.preferred_scale(numerator);
                }
            }
            Step::HangUp => self.hang_up_now = true,
        }
    }

    /// Close every connection, if a step or the offer asked for it: each
    /// client reads the end of its socket at once, whatever it waits for.
    fn hang_up_if_asked(&mut self) {
        if !std::mem::take(&mut self.hang_up_now) {
            return;
        }
        for connection in self.connections.drain(..) {
            let _ = connection.shutdown(std::net::Shutdown::Both);
        }
    }

    /// Post `code` on `resource`, and keep a record of it.
    fn post<I: Resource>(&self, display: &DisplayHandle, resource: &I, code: u32, message: String) {
        self.shared.note(|heard| {
            heard.errors.push(PostedError {
                interface: I::interface().name,
                code,
                message: message.clone(),
            });
        });
        display.post_error(resource, code, message);
    }

    /// Apply what `surface` holds pending, as its commit does.
    fn commit(&mut self, surface: &WlSurface, display: &DisplayHandle) {
        let withhold = self.withhold;
        let time = self.time();
        let Some(held) = self.surface_mut(surface) else {
            return;
        };
        let mut replaced = None;
        let mut new_buffer = false;
        if let Some(buffer) = held.pending_buffer.take() {
            new_buffer = buffer.is_some();
            replaced = std::mem::replace(&mut held.buffer, buffer);
        }
        if let Some(scale) = held.pending_scale.take() {
            held.scale = scale;
        }
        if let Some(destination) = held.pending_destination.take() {
            held.destination = destination;
        }
        let role = held.role;
        let scale = held.scale;
        let destination = held.destination;
        let size = held
            .buffer
            .as_ref()
            .and_then(|buffer| buffer.data::<BufferSize>().copied());
        let frames = std::mem::take(&mut held.frames);
        if let Some(old) = replaced
            && held.buffer.as_ref().is_none_or(|now| now.id() != old.id())
            && old.is_alive()
        {
            old.release();
        }
        // A strict compositor's rule: with no viewport destination, a
        // buffer's sides are multiples of its scale.
        if let Some(size) = size
            && destination.is_none()
            && scale > 0
            && (size.width % scale != 0 || size.height % scale != 0)
        {
            self.post(
                display,
                surface,
                wl_surface::Error::InvalidSize as u32,
                format!(
                    "a buffer of {}x{} is not a multiple of its scale {scale}",
                    size.width, size.height
                ),
            );
            return;
        }
        // A toplevel draws nothing before it acknowledged its first configure.
        if role == Role::Toplevel
            && new_buffer
            && let Some(toplevel) = &self.toplevel
            && toplevel.surface.id() == surface.id()
            && toplevel.acknowledged.is_none()
        {
            let xdg = toplevel.xdg.clone();
            self.post(
                display,
                &xdg,
                xdg_surface::Error::UnconfiguredBuffer as u32,
                "a buffer committed before the first configure was acknowledged".to_owned(),
            );
            return;
        }
        let asked = u32::try_from(frames.len()).unwrap_or(u32::MAX);
        if withhold {
            self.withheld.extend(frames);
        } else {
            for callback in frames {
                callback.done(time);
            }
        }
        match role {
            Role::Toplevel => {
                let first_configure = self
                    .toplevel
                    .as_ref()
                    .is_some_and(|toplevel| !toplevel.first_sent && !new_buffer);
                self.shared.note(|heard| {
                    heard.frame_callbacks = heard.frame_callbacks.saturating_add(asked);
                    if withhold {
                        heard.withheld = heard.withheld.saturating_add(asked);
                    }
                    if new_buffer {
                        heard.frames = heard.frames.saturating_add(1);
                        heard.buffer = size.map(|size| (size.width, size.height));
                    }
                });
                if new_buffer && self.offer.hang_up == Some(HangUp::OnFirstFrame) {
                    self.offer.hang_up = None;
                    self.hang_up_now = true;
                }
                if first_configure && let Some(first) = self.offer.first.clone() {
                    self.configure(&first);
                }
            }
            Role::Cursor => {
                if let Some(size) = size
                    && new_buffer
                {
                    self.shared.note(|heard| {
                        heard.cursor.push(CursorImage {
                            size: (size.width, size.height),
                            scale,
                            destination,
                        });
                    });
                }
            }
            Role::None => {}
        }
    }
}

impl GlobalDispatch<WlCompositor, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlCompositor>,
        (): &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlCompositor, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &WlCompositor,
        request: wl_compositor::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                let surface = data_init.init(id, ());
                state.surfaces.push(SurfaceState {
                    surface,
                    role: Role::None,
                    pending_buffer: Pending::Unchanged,
                    buffer: None,
                    pending_scale: None,
                    scale: 1,
                    viewport: None,
                    pending_destination: Pending::Unchanged,
                    destination: None,
                    frames: Vec::new(),
                });
            }
            wl_compositor::Request::CreateRegion { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegion, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlRegion,
        _: wl_region::Request,
        (): &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        surface: &WlSurface,
        request: wl_surface::Request,
        (): &(),
        display: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_surface::Request::Attach { buffer, .. } => {
                if let Some(held) = state.surface_mut(surface) {
                    held.pending_buffer = Pending::Set(buffer);
                }
            }
            wl_surface::Request::Frame { callback } => {
                let callback = data_init.init(callback, ());
                if let Some(held) = state.surface_mut(surface) {
                    held.frames.push(callback);
                }
            }
            wl_surface::Request::SetBufferScale { scale } if scale < 1 => {
                state.post(
                    display,
                    surface,
                    wl_surface::Error::InvalidScale as u32,
                    format!("a buffer scale of {scale}"),
                );
            }
            wl_surface::Request::SetBufferScale { scale } => {
                if let Some(held) = state.surface_mut(surface) {
                    held.pending_scale = Some(scale);
                }
            }
            wl_surface::Request::Commit => state.commit(surface, display),
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, surface: &WlSurface, (): &()) {
        state
            .surfaces
            .retain(|held| held.surface.id() != surface.id());
    }
}

impl Dispatch<WlCallback, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlCallback,
        _: wayland_server::protocol::wl_callback::Request,
        (): &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, callback: &WlCallback, (): &()) {
        state.withheld.retain(|held| held.id() != callback.id());
    }
}

impl GlobalDispatch<WlShm, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlShm>,
        (): &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let shm = data_init.init(resource, ());
        shm.format(wl_shm::Format::Argb8888);
        shm.format(wl_shm::Format::Xrgb8888);
    }
}

impl Dispatch<WlShm, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlShm,
        request: wl_shm::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, .. } = request {
            // The pool's memory is never read: the compositor draws nothing.
            data_init.init(id, ());
        }
    }
}

impl Dispatch<WlShmPool, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlShmPool,
        request: wl_shm_pool::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm_pool::Request::CreateBuffer {
            id, width, height, ..
        } = request
        {
            data_init.init(id, BufferSize { width, height });
        }
    }
}

impl Dispatch<WlBuffer, BufferSize> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlBuffer,
        _: wl_buffer::Request,
        _: &BufferSize,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<WpViewporter, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpViewporter>,
        (): &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpViewporter, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &WpViewporter,
        request: wp_viewporter::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_viewporter::Request::GetViewport { id, surface } = request {
            let viewport = data_init.init(id, surface.clone());
            if let Some(held) = state.surface_mut(&surface) {
                held.viewport = Some(viewport);
            }
        }
    }
}

impl Dispatch<WpViewport, WlSurface> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &WpViewport,
        request: wp_viewport::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wp_viewport::Request::SetDestination { width, height } = request
            && let Some(held) = state.surface_mut(surface)
        {
            // `-1` by `-1` unsets the destination.
            held.pending_destination =
                Pending::Set((width > 0 && height > 0).then_some((width, height)));
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, _: &WpViewport, surface: &WlSurface) {
        if let Some(held) = state.surface_mut(surface) {
            held.viewport = None;
            held.pending_destination = Pending::Set(None);
        }
    }
}

impl GlobalDispatch<WpFractionalScaleManagerV1, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WpFractionalScaleManagerV1>,
        (): &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &WpFractionalScaleManagerV1,
        request: wp_fractional_scale_manager_v1::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wp_fractional_scale_manager_v1::Request::GetFractionalScale { id, surface } = request
        {
            let scale = data_init.init(id, ());
            state.fractional.push((surface, scale));
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WpFractionalScaleV1,
        _: wp_fractional_scale_v1::Request,
        (): &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, scale: &WpFractionalScaleV1, (): &()) {
        state.fractional.retain(|(_, held)| held.id() != scale.id());
    }
}

impl GlobalDispatch<XdgWmBase, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<XdgWmBase>,
        (): &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<XdgWmBase, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &XdgWmBase,
        request: xdg_wm_base::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                data_init.init(id, surface);
            }
            xdg_wm_base::Request::CreatePositioner { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgPositioner, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &XdgPositioner,
        _: xdg_positioner::Request,
        (): &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }
}

impl Dispatch<XdgSurface, WlSurface> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        xdg: &XdgSurface,
        request: xdg_surface::Request,
        surface: &WlSurface,
        display: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_surface::Request::GetToplevel { id } => {
                let toplevel = data_init.init(id, ());
                if let Some(held) = state.surface_mut(surface) {
                    held.role = Role::Toplevel;
                }
                state.toplevel = Some(ShellWindow {
                    surface: surface.clone(),
                    xdg: xdg.clone(),
                    xdg_toplevel: toplevel,
                    first_sent: false,
                    unacknowledged: Vec::new(),
                    acknowledged: None,
                });
                if state.offer.hang_up == Some(HangUp::OnToplevel) {
                    state.offer.hang_up = None;
                    state.hang_up_now = true;
                }
            }
            xdg_surface::Request::AckConfigure { serial } => {
                state.shared.note(|heard| heard.acknowledged.push(serial));
                let Some(toplevel) = state
                    .toplevel
                    .as_mut()
                    .filter(|toplevel| toplevel.xdg.id() == xdg.id())
                else {
                    return;
                };
                // A serial is acknowledged once, and acknowledging one
                // acknowledges the older ones with it.
                if toplevel.unacknowledged.contains(&serial) {
                    toplevel.unacknowledged.retain(|sent| *sent > serial);
                    toplevel.acknowledged = Some(serial);
                } else {
                    let acknowledged = toplevel.acknowledged;
                    state.post(
                        display,
                        xdg,
                        xdg_surface::Error::InvalidSerial as u32,
                        format!(
                            "serial {serial} was never sent, or is older than the {acknowledged:?} acknowledged"
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgToplevel, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &XdgToplevel,
        _: xdg_toplevel::Request,
        (): &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, toplevel: &XdgToplevel, (): &()) {
        if state
            .toplevel
            .as_ref()
            .is_some_and(|held| held.xdg_toplevel.id() == toplevel.id())
        {
            state.toplevel = None;
        }
    }
}

impl GlobalDispatch<WlOutput, usize> for Compositor {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlOutput>,
        place: &usize,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let output = data_init.init(resource, *place);
        let Some(slot) = state.outputs.get_mut(*place) else {
            return;
        };
        let OutputOffer {
            size: (width, height),
            scale,
        } = slot.offer;
        output.geometry(
            0,
            0,
            width / 4,
            height / 4,
            wl_output::Subpixel::Unknown,
            "FLUI".into(),
            format!("scripted output {place}"),
            wl_output::Transform::Normal,
        );
        output.mode(
            wl_output::Mode::Current | wl_output::Mode::Preferred,
            width,
            height,
            60_000,
        );
        if output.version() >= 2 {
            output.scale(scale);
        }
        if output.version() >= 4 {
            output.name(format!("SCRIPTED-{place}"));
        }
        if output.version() >= 2 {
            output.done();
        }
        slot.bound.push(output);
    }
}

impl Dispatch<WlOutput, usize> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlOutput,
        _: wl_output::Request,
        _: &usize,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, output: &WlOutput, place: &usize) {
        if let Some(slot) = state.outputs.get_mut(*place) {
            slot.bound.retain(|held| held.id() != output.id());
        }
    }
}

impl GlobalDispatch<WlSeat, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlSeat>,
        (): &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let seat = data_init.init(resource, ());
        seat.capabilities(wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard);
        if seat.version() >= 2 {
            seat.name("seat0".into());
        }
    }
}

impl Dispatch<WlSeat, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &WlSeat,
        request: wl_seat::Request,
        (): &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { id } => {
                let pointer = data_init.init(id, ());
                state.pointers.push(pointer);
            }
            wl_seat::Request::GetKeyboard { id } => {
                let keyboard = data_init.init(id, ());
                if let Some((file, size)) = keymap_file() {
                    keyboard.keymap(wl_keyboard::KeymapFormat::XkbV1, file.as_fd(), size);
                }
                // A seat older than version 4 has no word for the repeat.
                if keyboard.version() >= 4 {
                    keyboard.repeat_info(25, 600);
                }
                state.keyboards.push(keyboard);
            }
            _ => {}
        }
    }
}

/// [`KEYMAP`] in a file, as a keyboard shares it, with its size.
fn keymap_file() -> Option<(std::fs::File, u32)> {
    use std::io::Write as _;

    let mut file = tempfile::tempfile().ok()?;
    file.write_all(KEYMAP).ok()?;
    Some((file, u32::try_from(KEYMAP.len()).ok()?))
}

impl Dispatch<WlPointer, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        pointer: &WlPointer,
        request: wl_pointer::Request,
        (): &(),
        display: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // The cursor shows what the surface holds now: a client draws it
        // before it names it, and draws it again on the surface later.
        if let wl_pointer::Request::SetCursor {
            surface: Some(surface),
            ..
        } = request
            && let Some(held) = state.surface_mut(&surface)
        {
            if held.role == Role::Toplevel {
                state.post(
                    display,
                    pointer,
                    wl_pointer::Error::Role as u32,
                    "a cursor set on a toplevel's surface".to_owned(),
                );
                return;
            }
            held.role = Role::Cursor;
            if let Some(image) = held.cursor_image() {
                state.shared.note(|heard| heard.cursor.push(image));
            }
        }
    }

    fn destroyed(state: &mut Self, _: ClientId, pointer: &WlPointer, (): &()) {
        state.pointers.retain(|held| held.id() != pointer.id());
    }
}

impl Dispatch<WlKeyboard, ()> for Compositor {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &WlKeyboard,
        _: wl_keyboard::Request,
        (): &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(state: &mut Self, _: ClientId, keyboard: &WlKeyboard, (): &()) {
        state.keyboards.retain(|held| held.id() != keyboard.id());
    }
}
