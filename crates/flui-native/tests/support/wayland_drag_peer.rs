//! Another application dragging and dropping on the harness's compositor: a
//! client of its own on a thread of its own, with one toplevel, as a file
//! manager or a browser would be. As a source it starts a drag from its
//! surface when its pointer button goes down there and reports what its data
//! source hears; as a target it accepts a type of the drags over it, reads
//! a drop's bytes and finishes it, and reports what its data device hears.

use std::os::fd::{AsFd as _, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{
    Connection, Dispatch, Proxy as _, QueueHandle, WEnum, delegate_noop, event_created_child,
};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

/// What the peer does.
#[derive(Clone)]
pub(crate) enum Role {
    /// Start a drag offering this from a press on its surface.
    Source(Offered),
    /// Take drops over its surface.
    Target(Wanted),
}

/// What a drag offers.
#[derive(Clone)]
pub(crate) struct Offered {
    /// Each MIME type and the bytes it is answered with; `None` holds the
    /// pipe open and writes nothing.
    pub(crate) types: Vec<(String, Option<Vec<u8>>)>,
    /// The actions the drag permits.
    pub(crate) actions: DndAction,
}

/// What a target takes.
#[derive(Clone)]
pub(crate) struct Wanted {
    /// The type it accepts where a drag offers it; `None` refuses every drag.
    pub(crate) mime: Option<String>,
    /// The action it takes.
    pub(crate) action: DndAction,
}

/// What the peer's data source or data device heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Heard {
    /// The drag started.
    Started,
    /// The pointer entered the surface.
    Pointed,
    /// The target accepts this type, or none.
    Target(Option<String>),
    /// The compositor chose this action.
    Action(DndAction),
    /// The target asked for this type.
    Send(String),
    /// The user dropped where the target accepts.
    DropPerformed,
    /// The target finished with the drop.
    Finished,
    /// The drag ended untaken.
    Cancelled,
    /// A drag entered the target, offering these types.
    Entered(Vec<String>),
    /// The drag left the target.
    Left,
    /// The drag was dropped on the target.
    Dropped,
    /// The target read these bytes of the drop and finished it.
    Received(Vec<u8>),
}

/// The peer, until dropped.
pub(crate) struct Peer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    heard: mpsc::Receiver<Heard>,
    /// Everything heard so far, in order.
    log: std::cell::RefCell<Vec<Heard>>,
}

impl Peer {
    /// Connect and show a toplevel playing `role`; returns once the
    /// compositor shows it.
    pub(crate) fn open(role: Role) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (tell, heard) = mpsc::channel();
        let (shown, is_shown) = mpsc::channel();
        let thread = std::thread::spawn({
            let stop = Arc::clone(&stop);
            move || run(&role, &stop, &tell, &shown)
        });
        is_shown
            .recv_timeout(Duration::from_secs(10))
            .expect("the peer's toplevel is shown");
        Self {
            stop,
            thread: Some(thread),
            heard,
            log: std::cell::RefCell::default(),
        }
    }

    /// Everything the peer heard so far, in order.
    pub(crate) fn heard(&self) -> Vec<Heard> {
        let mut log = self.log.borrow_mut();
        log.extend(self.heard.try_iter());
        log.clone()
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    role: Role,
    tell: mpsc::Sender<Heard>,
    compositor: WlCompositor,
    shm: WlShm,
    manager: WlDataDeviceManager,
    surface: WlSurface,
    device: Option<WlDataDevice>,
    /// Held for its button presses.
    _pointer: WlPointer,
    buffer: Option<WlBuffer>,
    shown: bool,
    /// Pipes the source holds open, writing nothing.
    held: Vec<OwnedFd>,
    /// The offer of the drag over the target.
    over: Option<WlDataOffer>,
}

fn run(role: &Role, stop: &AtomicBool, tell: &mpsc::Sender<Heard>, shown: &mpsc::Sender<()>) {
    let connection = Connection::connect_to_env().expect("the harness's compositor answers");
    let (globals, mut queue) =
        registry_queue_init::<State>(&connection).expect("the registry answers");
    let handle = queue.handle();
    let compositor: WlCompositor = globals.bind(&handle, 4..=4, ()).expect("wl_compositor");
    let wm_base: XdgWmBase = globals.bind(&handle, 1..=1, ()).expect("xdg_wm_base");
    let shm: WlShm = globals.bind(&handle, 1..=1, ()).expect("wl_shm");
    let seat: WlSeat = globals.bind(&handle, 1..=5, ()).expect("wl_seat");
    let manager: WlDataDeviceManager = globals
        .bind(&handle, 3..=3, ())
        .expect("wl_data_device_manager 3");
    let surface = compositor.create_surface(&handle, ());
    let xdg_surface = wm_base.get_xdg_surface(&surface, &handle, ());
    let toplevel = xdg_surface.get_toplevel(&handle, ());
    toplevel.set_title("drag peer".to_owned());
    surface.commit();
    let mut state = State {
        role: role.clone(),
        tell: tell.clone(),
        compositor,
        shm,
        manager,
        surface,
        device: None,
        _pointer: seat.get_pointer(&handle, ()),
        buffer: None,
        shown: false,
        held: Vec::new(),
        over: None,
    };
    state.device = Some(state.manager.get_data_device(&seat, &handle, ()));
    let mut told_shown = false;
    while !stop.load(Ordering::Acquire) {
        queue
            .dispatch_pending(&mut state)
            .expect("the peer dispatches");
        if state.shown && !told_shown {
            // The buffer is on screen once the compositor answered.
            queue.roundtrip(&mut state).expect("a roundtrip");
            let _ = shown.send(());
            told_shown = true;
        }
        connection.flush().expect("the peer flushes");
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut descriptors = [PollFd::new(&fd, PollFlags::IN)];
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: 20_000_000,
            };
            let ready = poll(&mut descriptors, Some(&timeout)).unwrap_or(0);
            if ready > 0 {
                let _ = guard.read();
            }
        }
    }
    drop((toplevel, xdg_surface));
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlBuffer);

delegate_noop!(State: ignore WlDataDeviceManager);
delegate_noop!(State: ignore XdgToplevel);

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        (): &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = event else {
            return;
        };
        xdg_surface.ack_configure(serial);
        if state.buffer.is_none() {
            // A small grey square: the compositor stretches nothing, but a
            // surface with a buffer is shown and takes the pointer.
            let (width, height) = (256_i32, 256_i32);
            let len = usize::try_from(width * height * 4).expect("a small buffer");
            let memory =
                rustix::fs::memfd_create("flui-drag-source", rustix::fs::MemfdFlags::CLOEXEC)
                    .expect("a memfd");
            rustix::fs::ftruncate(&memory, u64::try_from(len).expect("fits"))
                .expect("the memfd is sized");
            rustix::io::write(&memory, &vec![0x80_u8; len]).expect("the buffer is painted");
            let pool =
                state
                    .shm
                    .create_pool(memory.as_fd(), i32::try_from(len).expect("fits"), queue, ());
            let buffer =
                pool.create_buffer(0, width, height, width * 4, Format::Xrgb8888, queue, ());
            pool.destroy();
            state.surface.attach(Some(&buffer), 0, 0);
            state.surface.damage(0, 0, width, height);
            state.buffer = Some(buffer);
            state.shown = true;
        }
        state.surface.commit();
        let _ = &state.compositor;
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlPointer,
        event: wl_pointer::Event,
        (): &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wl_pointer::Event::Enter { .. } = event {
            let _ = state.tell.send(Heard::Pointed);
            return;
        }
        let wl_pointer::Event::Button {
            serial,
            state: WEnum::Value(wl_pointer::ButtonState::Pressed),
            ..
        } = event
        else {
            return;
        };
        let Role::Source(offered) = &state.role else {
            return;
        };
        let source = state.manager.create_data_source(queue, ());
        for (mime, _) in &offered.types {
            source.offer(mime.clone());
        }
        source.set_actions(offered.actions);
        state
            .device
            .as_ref()
            .expect("the device is made at start")
            .start_drag(Some(&source), &state.surface, None, serial);
        let _ = state.tell.send(Heard::Started);
    }
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        (): &(),
        connection: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A source's own drag over its own surface, and selections: not the
        // case's.
        let Role::Target(wanted) = &state.role else {
            return;
        };
        match event {
            wl_data_device::Event::Enter {
                serial,
                id: Some(offer),
                ..
            } => {
                let types = offer
                    .data::<OfferTypes>()
                    .map(|types| types.0.lock().expect("the types").clone())
                    .unwrap_or_default();
                let accepted = wanted.mime.clone().filter(|mime| types.contains(mime));
                offer.set_actions(wanted.action, wanted.action);
                offer.accept(serial, accepted);
                state.over = Some(offer);
                let _ = state.tell.send(Heard::Entered(types));
            }
            wl_data_device::Event::Leave => {
                if let Some(offer) = state.over.take() {
                    offer.destroy();
                }
                let _ = state.tell.send(Heard::Left);
            }
            wl_data_device::Event::Drop => {
                let _ = state.tell.send(Heard::Dropped);
                let (Some(offer), Some(mime)) = (state.over.take(), wanted.mime.clone()) else {
                    return;
                };
                let (reader, writer) = rustix::pipe::pipe().expect("a pipe");
                offer.receive(mime, writer.as_fd());
                drop(writer);
                connection.flush().expect("the peer flushes");
                let bytes = read_all(&reader);
                offer.finish();
                offer.destroy();
                let _ = state.tell.send(Heard::Received(bytes));
            }
            _ => {}
        }
    }

    event_created_child!(State, WlDataDevice, [
        wayland_client::protocol::wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferTypes::default()),
    ]);
}

/// The types an offer named.
#[derive(Default)]
struct OfferTypes(std::sync::Mutex<Vec<String>>);

impl Dispatch<WlDataOffer, OfferTypes> for State {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: wl_data_offer::Event,
        types: &OfferTypes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            types.0.lock().expect("the types").push(mime_type);
        }
    }
}

/// Everything `reader` holds until its writer closes it, waiting at most ten
/// seconds for each part.
fn read_all(reader: &OwnedFd) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let mut descriptors = [PollFd::new(reader, PollFlags::IN)];
        let timeout = Timespec {
            tv_sec: 10,
            tv_nsec: 0,
        };
        if poll(&mut descriptors, Some(&timeout)).unwrap_or(0) == 0 {
            return bytes;
        }
        match rustix::io::read(reader, &mut buffer) {
            Ok(0) | Err(_) => return bytes,
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
        }
    }
}

impl Dispatch<WlDataSource, ()> for State {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let heard = match event {
            wl_data_source::Event::Target { mime_type } => Heard::Target(mime_type),
            wl_data_source::Event::Action {
                dnd_action: WEnum::Value(action),
            } => Heard::Action(action),
            wl_data_source::Event::Send { mime_type, fd } => {
                let answer = match &state.role {
                    Role::Source(offered) => offered
                        .types
                        .iter()
                        .find(|(mime, _)| *mime == mime_type)
                        .and_then(|(_, bytes)| bytes.clone()),
                    Role::Target(_) => None,
                };
                match answer {
                    Some(bytes) => {
                        std::thread::spawn(move || {
                            let mut left = &bytes[..];
                            while let Ok(count @ 1..) = rustix::io::write(&fd, left) {
                                left = &left[count..];
                            }
                        });
                    }
                    None => state.held.push(fd),
                }
                Heard::Send(mime_type)
            }
            wl_data_source::Event::DndDropPerformed => Heard::DropPerformed,
            wl_data_source::Event::DndFinished => {
                source.destroy();
                Heard::Finished
            }
            wl_data_source::Event::Cancelled => {
                source.destroy();
                Heard::Cancelled
            }
            _ => return,
        };
        let _ = state.tell.send(heard);
    }
}
delegate_noop!(State: ignore WlSeat);
