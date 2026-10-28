//! The Wayland adapter: a connection of its own that binds every `wl_output`
//! (and the xdg-output of each) and feeds their events to the
//! [`WaylandOutputs`] model.
//!
//! The adapter holds no policy. It maps the protocol's events onto
//! [`OutputEvent`]s, keeps the bound objects alive for as long as their
//! globals exist, and reports whether an event changed the list of outputs.
//!
//! - `wl_output` is bound at version 4 where offered (the connector name) and
//!   at the compositor's version below that. Version 1 has no `done` event to
//!   close a burst and is not bound.
//! - `zxdg_output_v1` is bound where offered, at version 3 at most; it carries
//!   the logical position and size. Its own `done` closes a burst only below
//!   version 3, where `wl_output.done` does not cover it.
//!
//! [`WaylandDisplaySource`] is the native Wayland host's displays position over
//! the source: the snapshot, the display and scale of an output a window's
//! surface entered (both connections name an output by its global, which the
//! compositor numbers for every client alike), and the watch that keeps them
//! current.

use std::os::fd::BorrowedFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use flui_platform::DisplaySnapshot;

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::{ReadEventsGuard, WaylandError},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

use super::protocol::{Environment, Protocol};
use super::session::{DisplayError, Session, Step, wait_readable};
use super::source::LinuxDisplays;
use super::view::OutputView;
use super::wayland_model::{OutputEvent, WaylandOutputs};
use crate::error::PlatformError;
use crate::owner::linux::wayland::{OutputFacts, WaylandDisplays};
use crate::owner::notices::StaleOnWake;
use crate::owner::signal::OwnerSignal;
use crate::owner::watch::Watch;

/// The native Wayland host's displays: the compositor's outputs as the
/// display source reads them on a connection of its own. Public where it is
/// defined, as the host's table names it, and nameable only inside the crate.
pub struct WaylandDisplaySource {
    native: Arc<LinuxDisplays>,
}

impl std::fmt::Debug for WaylandDisplaySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandDisplaySource")
            .finish_non_exhaustive()
    }
}

impl WaylandDisplaySource {
    /// The source, which reads the compositor on first use.
    pub(crate) fn new() -> Self {
        Self {
            native: LinuxDisplays::new(),
        }
    }
}

/// A wake marks the outputs stale.
impl StaleOnWake for WaylandDisplaySource {
    fn mark_stale(&self) {
        self.native.mark_stale();
    }
}

impl WaylandDisplays for WaylandDisplaySource {
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        self.native.snapshot()
    }

    fn output(&self, name: u32) -> Option<OutputFacts> {
        let snapshot = self.native.snapshot().ok()?;
        let display = self.native.display_of_native(Protocol::Wayland, name)?;
        let info = snapshot.displays().iter().find(|info| info.id == display)?;
        Some(OutputFacts {
            display,
            scale: info.scale_factor,
        })
    }

    /// The display thread: dropping the watch stops it and closes its
    /// connection. A source that cannot connect says so in the log and
    /// answers its snapshots with the error.
    fn observe(self: Arc<Self>, signal: &Arc<OwnerSignal>) -> Watch {
        match self.native.watch(Protocol::Wayland, signal) {
            Ok(watcher) => Watch::new(watcher),
            Err(error) => {
                tracing::warn!(%error, "the Wayland display source could not start");
                Watch::new(())
            }
        }
    }
}

const PROTOCOL: &str = "Wayland";
/// The newest `wl_output` this adapter understands (it adds `name` and
/// `description`).
const WL_OUTPUT_VERSION: u32 = 4;
/// The newest `zxdg_output_manager_v1` it understands (it deprecates the
/// xdg-output `done` in favour of `wl_output.done`).
const XDG_OUTPUT_VERSION: u32 = 3;

fn error(error: impl std::fmt::Display) -> DisplayError {
    DisplayError::connection(PROTOCOL, error)
}

/// One bound `wl_output` and its xdg-output.
struct BoundOutput {
    global: u32,
    output: WlOutput,
    xdg: Option<ZxdgOutputV1>,
}

struct State {
    outputs: WaylandOutputs,
    bound: Vec<BoundOutput>,
    manager: Option<ZxdgOutputManagerV1>,
    /// Set when an event changed the list; the session reports it and clears it.
    changed: bool,
}

impl State {
    fn bind_global(
        &mut self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<Self>,
    ) {
        if interface == WlOutput::interface().name {
            // Version 1 has no `done`: its burst never ends, so it never applies.
            if version < 2 || self.bound.iter().any(|bound| bound.global == name) {
                return;
            }
            let output = registry.bind::<WlOutput, u32, Self>(
                name,
                version.min(WL_OUTPUT_VERSION),
                queue,
                name,
            );
            let xdg = self
                .manager
                .as_ref()
                .map(|manager| manager.get_xdg_output(&output, queue, name));
            self.outputs.add(name);
            if xdg.is_some() {
                self.outputs.expect_logical_size(name);
            }
            self.bound.push(BoundOutput {
                global: name,
                output,
                xdg,
            });
        } else if interface == ZxdgOutputManagerV1::interface().name && self.manager.is_none() {
            let manager = registry.bind::<ZxdgOutputManagerV1, (), Self>(
                name,
                version.min(XDG_OUTPUT_VERSION),
                queue,
                (),
            );
            for bound in &mut self.bound {
                if bound.xdg.is_none() {
                    bound.xdg = Some(manager.get_xdg_output(&bound.output, queue, bound.global));
                    self.outputs.expect_logical_size(bound.global);
                }
            }
            self.manager = Some(manager);
        }
    }

    fn remove_global(&mut self, name: u32) {
        let Some(index) = self.bound.iter().position(|bound| bound.global == name) else {
            return;
        };
        let bound = self.bound.remove(index);
        if let Some(xdg) = bound.xdg {
            xdg.destroy();
        }
        // `release` exists from version 3; below that the proxy is just dropped.
        if bound.output.version() >= 3 {
            bound.output.release();
        }
        self.changed |= self.outputs.remove(name);
    }

    fn apply(&mut self, global: u32, event: OutputEvent) {
        self.changed |= self.outputs.apply(global, event);
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => state.bind_global(registry, name, &interface, version, queue),
            wl_registry::Event::GlobalRemove { name } => state.remove_global(name),
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let event = match event {
            wl_output::Event::Geometry {
                x,
                y,
                transform,
                make,
                model,
                ..
            } => OutputEvent::Geometry {
                x,
                y,
                swaps_axes: matches!(
                    transform,
                    WEnum::Value(
                        wl_output::Transform::_90
                            | wl_output::Transform::_270
                            | wl_output::Transform::Flipped90
                            | wl_output::Transform::Flipped270
                    )
                ),
                make,
                model,
            },
            wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh,
            } => OutputEvent::Mode {
                current: matches!(flags, WEnum::Value(flags) if flags.contains(wl_output::Mode::Current)),
                width,
                height,
                refresh_millihertz: refresh,
            },
            wl_output::Event::Scale { factor } => OutputEvent::Scale(factor),
            wl_output::Event::Name { name } => OutputEvent::Name(name),
            wl_output::Event::Description { description } => OutputEvent::Description(description),
            wl_output::Event::Done => OutputEvent::Done,
            _ => return,
        };
        state.apply(*global, event);
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: <ZxdgOutputManagerV1 as Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<ZxdgOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        xdg: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let event = match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                OutputEvent::LogicalPosition { x, y }
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                OutputEvent::LogicalSize { width, height }
            }
            // From version 3 `wl_output.done` closes the burst; here it would
            // apply the output's half-delivered state.
            zxdg_output_v1::Event::Done if xdg.version() < 3 => OutputEvent::Done,
            _ => return,
        };
        state.apply(*global, event);
    }
}

/// A connection to the compositor and the outputs it announced.
pub(super) struct WaylandSession {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

impl WaylandSession {
    /// Connect to the socket `WAYLAND_DISPLAY` names and read every output.
    pub(super) fn connect(environment: &Environment) -> Result<Self, DisplayError> {
        let path = environment.wayland_socket_path().ok_or_else(|| {
            error("WAYLAND_DISPLAY does not name a socket (it is a bare name and XDG_RUNTIME_DIR is not set, or only WAYLAND_SOCKET is)")
        })?;
        let stream = UnixStream::connect(&path)
            .map_err(|source| error(format!("cannot connect to {}: {source}", path.display())))?;
        let connection = Connection::from_socket(stream).map_err(error)?;
        let (globals, mut queue) = registry_queue_init::<State>(&connection).map_err(error)?;
        let mut state = State {
            outputs: WaylandOutputs::default(),
            bound: Vec::new(),
            manager: None,
            changed: false,
        };
        let queue_handle = queue.handle();
        let advertised = globals.contents().with_list(<[_]>::to_vec);
        // The manager is bound with the outputs, in whatever order they come.
        for global in advertised {
            state.bind_global(
                globals.registry(),
                global.name,
                &global.interface,
                global.version,
                &queue_handle,
            );
        }
        // The first round trip delivers the outputs' bursts, which request
        // their xdg-outputs; the second delivers those.
        queue.roundtrip(&mut state).map_err(error)?;
        queue.roundtrip(&mut state).map_err(error)?;
        state.changed = false;
        Ok(Self {
            connection,
            queue,
            state,
        })
    }
}

/// Move what the socket holds into the event queue, on a guard the caller took
/// after the socket reported data.
fn read(guard: ReadEventsGuard) -> Result<(), DisplayError> {
    match guard.read() {
        Ok(_) => Ok(()),
        Err(WaylandError::Io(source)) if source.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
        Err(source) => Err(error(source)),
    }
}

impl Session for WaylandSession {
    fn views(&self) -> Vec<OutputView> {
        self.state.outputs.views()
    }

    fn step(&mut self, stop: BorrowedFd<'_>) -> Result<Step, DisplayError> {
        // Events already read from the socket come first.
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(error)?;
        if std::mem::take(&mut self.state.changed) {
            return Ok(Step::Changed);
        }
        match self.connection.flush() {
            Ok(()) => {}
            // A full socket buffer: the rest goes out on a later turn.
            Err(WaylandError::Io(source)) if source.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(source) => return Err(error(source)),
        }
        // `None` means events were queued meanwhile; the next turn dispatches them.
        let Some(guard) = self.queue.prepare_read() else {
            return Ok(Step::Idle);
        };
        let ready = wait_readable(guard.connection_fd(), stop).map_err(error)?;
        if ready.stop {
            // Dropping the guard cancels the read.
            return Ok(Step::Stopped);
        }
        if ready.connection {
            read(guard)?;
        }
        Ok(Step::Idle)
    }
}
