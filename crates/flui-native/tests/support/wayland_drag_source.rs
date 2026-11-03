//! Another application dragging on the harness's compositor: a client of
//! its own on a thread of its own, with one toplevel, which starts a drag
//! from its surface when its pointer button goes down there, as a file
//! manager or a browser would, and reports what its data source hears.

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
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_pointer::{self, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum, delegate_noop, event_created_child,
};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

/// What the drag offers.
#[derive(Clone)]
pub(crate) struct Offered {
    /// Each MIME type and the bytes it is answered with; `None` holds the
    /// pipe open and writes nothing.
    pub(crate) types: Vec<(String, Option<Vec<u8>>)>,
    /// The actions the drag permits.
    pub(crate) actions: DndAction,
}

/// What the drag's data source heard.
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
}

/// The client, until dropped.
pub(crate) struct DragSource {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    heard: mpsc::Receiver<Heard>,
    /// Everything heard so far, in order.
    log: std::cell::RefCell<Vec<Heard>>,
}

impl DragSource {
    /// Connect and show a toplevel, which drags `offered` from a press on
    /// it; returns once the compositor shows it.
    pub(crate) fn open(offered: Offered) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (tell, heard) = mpsc::channel();
        let (shown, is_shown) = mpsc::channel();
        let thread = std::thread::spawn({
            let stop = Arc::clone(&stop);
            move || run(&offered, &stop, &tell, &shown)
        });
        is_shown
            .recv_timeout(Duration::from_secs(10))
            .expect("the drag source's toplevel is shown");
        Self {
            stop,
            thread: Some(thread),
            heard,
            log: std::cell::RefCell::default(),
        }
    }

    /// Everything the source heard so far, in order.
    pub(crate) fn heard(&self) -> Vec<Heard> {
        let mut log = self.log.borrow_mut();
        log.extend(self.heard.try_iter());
        log.clone()
    }
}

impl Drop for DragSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    offered: Offered,
    tell: mpsc::Sender<Heard>,
    compositor: WlCompositor,
    shm: WlShm,
    manager: WlDataDeviceManager,
    surface: WlSurface,
    device: Option<WlDataDevice>,
    /// Held for its button presses.
    _pointer: WlPointer,
    buffer: Option<WlBuffer>,
    /// The size the last `configure` of the toplevel named, zero where it
    /// left the size to the client.
    configured: (i32, i32),
    /// The size of the buffer attached.
    drawn: Option<(i32, i32)>,
    shown: bool,
    /// Pipes the source holds open, writing nothing.
    held: Vec<OwnedFd>,
}

fn run(offered: &Offered, stop: &AtomicBool, tell: &mpsc::Sender<Heard>, shown: &mpsc::Sender<()>) {
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
    toplevel.set_title("drag source".to_owned());
    surface.commit();
    let mut state = State {
        offered: offered.clone(),
        tell: tell.clone(),
        compositor,
        shm,
        manager,
        surface,
        device: None,
        _pointer: seat.get_pointer(&handle, ()),
        buffer: None,
        configured: (0, 0),
        drawn: None,
        shown: false,
        held: Vec::new(),
    };
    state.device = Some(state.manager.get_data_device(&seat, &handle, ()));
    let mut told_shown = false;
    while !stop.load(Ordering::Acquire) {
        queue
            .dispatch_pending(&mut state)
            .expect("the drag source dispatches");
        if state.shown && !told_shown {
            // The buffer is on screen once the compositor answered.
            queue.roundtrip(&mut state).expect("a roundtrip");
            let _ = shown.send(());
            told_shown = true;
        }
        connection.flush().expect("the drag source flushes");
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
delegate_noop!(State: ignore WlDataOffer);

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

impl Dispatch<XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        _: &XdgToplevel,
        event: xdg_toplevel::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure { width, height, .. } = event {
            state.configured = (width, height);
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
        // The size the compositor tiled the toplevel at, or a square of
        // the client's choice where it left the size to the client: a
        // buffer that fills the tile takes the pointer wherever the
        // compositor places it.
        let size = match state.configured {
            (width, height) if width > 0 && height > 0 => (width, height),
            _ => (256, 256),
        };
        if state.drawn != Some(size) {
            state.paint(size, queue);
        }
        state.surface.commit();
        let _ = &state.compositor;
    }
}

impl State {
    /// Attach a grey buffer of `size` to the surface.
    fn paint(&mut self, (width, height): (i32, i32), queue: &QueueHandle<Self>) {
        let len = usize::try_from(width * height * 4).expect("a buffer of a tile's size");
        let memory = rustix::fs::memfd_create("flui-drag-source", rustix::fs::MemfdFlags::CLOEXEC)
            .expect("a memfd");
        rustix::fs::ftruncate(&memory, u64::try_from(len).expect("fits"))
            .expect("the memfd is sized");
        rustix::io::write(&memory, &vec![0x80_u8; len]).expect("the buffer is painted");
        let pool =
            self.shm
                .create_pool(memory.as_fd(), i32::try_from(len).expect("fits"), queue, ());
        let buffer = pool.create_buffer(0, width, height, width * 4, Format::Xrgb8888, queue, ());
        pool.destroy();
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.damage(0, 0, width, height);
        if let Some(old) = self.buffer.replace(buffer) {
            old.destroy();
        }
        self.drawn = Some((width, height));
        self.shown = true;
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
        let source = state.manager.create_data_source(queue, ());
        for (mime, _) in &state.offered.types {
            source.offer(mime.clone());
        }
        source.set_actions(state.offered.actions);
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
        _: &mut Self,
        _: &WlDataDevice,
        _: wl_data_device::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Its own drag over its own surface, and selections: not the case's.
    }

    event_created_child!(State, WlDataDevice, [
        wayland_client::protocol::wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
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
                let answer = state
                    .offered
                    .types
                    .iter()
                    .find(|(mime, _)| *mime == mime_type)
                    .and_then(|(_, bytes)| bytes.clone());
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
