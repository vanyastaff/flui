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
//! - `wp_color_manager_v1` is bound where offered, at version 2 at most. Each
//!   output's image description is asked for, its information read, and asked
//!   for again on `image_description_changed`; each description is numbered so
//!   the model keeps only the newest.
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

use flui_platform::ColorGamut;
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::{ReadEventsGuard, WaylandError},
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
    },
};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_output_v1::{self, WpColorManagementOutputV1},
    wp_color_manager_v1::{self, Primaries, TransferFunction, WpColorManagerV1},
    wp_image_description_info_v1::{self, WpImageDescriptionInfoV1},
    wp_image_description_v1::{self, WpImageDescriptionV1},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

use super::protocol::{Environment, Protocol};
use super::session::{DisplayError, Session, Step, wait_readable};
use super::source::LinuxDisplays;
use super::view::OutputView;
use super::wayland_model::{ColorEvent, OutputEvent, TransferRange, WaylandOutputs};
use crate::error::PlatformError;
use crate::owner::linux::wayland::{MAX_OUTPUTS, OutputFacts, OutputName, WaylandDisplays};
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

    fn output(&self, output: OutputName) -> Option<OutputFacts> {
        let snapshot = self.native.snapshot().ok()?;
        let display = self
            .native
            .display_of_native(Protocol::Wayland, output.get())?;
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
/// The newest `wp_color_manager_v1` it understands (version 2 makes an image
/// description announce itself with `ready2`).
const COLOR_MANAGER_VERSION: u32 = 2;

fn error(error: impl std::fmt::Display) -> DisplayError {
    DisplayError::connection(PROTOCOL, error)
}

/// One bound `wl_output`, its xdg-output and its colour.
struct BoundOutput {
    global: u32,
    output: WlOutput,
    xdg: Option<ZxdgOutputV1>,
    /// The output's color-management object, where the compositor has the
    /// protocol.
    color: Option<WpColorManagementOutputV1>,
    /// The image description asked for last; a newer one replaces it.
    description: Option<WpImageDescriptionV1>,
    /// The number of the description still being read: asked for, and its
    /// information not yet done. One is in flight per output at a time, so a
    /// compositor announcing changes without end, or never finishing one,
    /// leaves at most one description and one information object live.
    reading: Option<u64>,
    /// Its information was asked for.
    informed: bool,
    /// A change was announced while one was being read: the next is asked
    /// for once that one ends.
    stale: bool,
}

impl BoundOutput {
    fn release(self) {
        if let Some(description) = self.description {
            description.destroy();
        }
        if let Some(color) = self.color {
            color.destroy();
        }
        if let Some(xdg) = self.xdg {
            xdg.destroy();
        }
        // `release` exists from version 3; below that the proxy is just dropped.
        if self.output.version() >= 3 {
            self.output.release();
        }
    }
}

/// The user data of an image description and of its information: the output's
/// global and the description's number.
type Described = (u32, u64);

struct State {
    outputs: WaylandOutputs,
    bound: Vec<BoundOutput>,
    manager: Option<ZxdgOutputManagerV1>,
    color_manager: Option<WpColorManagerV1>,
    /// The number the next image description is given. Rising, so a late event
    /// of an older description is told from the newest one's; at 2^64 the
    /// colour is no longer followed rather than numbered again.
    next_description: Option<u64>,
    /// Set when an event changed the list; the session reports it and clears it.
    changed: bool,
}

impl State {
    /// Follow the colour of the output at `index` of `bound`: its
    /// color-management object, and a first description.
    fn watch_color(&mut self, index: usize, queue: &QueueHandle<Self>) {
        let Some(manager) = &self.color_manager else {
            return;
        };
        let bound = &mut self.bound[index];
        if bound.color.is_none() {
            bound.color = Some(manager.get_output(&bound.output, queue, bound.global));
        }
        let global = bound.global;
        self.describe(global, queue);
    }

    /// Ask for the current image description of the output `global`. The
    /// description it replaces is destroyed, and the model drops its events.
    fn describe(&mut self, global: u32, queue: &QueueHandle<Self>) {
        let Some(number) = self.next_description else {
            return;
        };
        let Some(bound) = self.bound.iter_mut().find(|bound| bound.global == global) else {
            return;
        };
        let Some(color) = &bound.color else {
            return;
        };
        if bound.reading.is_some() {
            bound.stale = true;
            return;
        }
        self.next_description = number.checked_add(1);
        let description = color.get_image_description(queue, (global, number));
        if let Some(previous) = bound.description.replace(description) {
            previous.destroy();
        }
        bound.reading = Some(number);
        bound.informed = false;
        self.outputs.describe(global, number);
    }

    /// The description `described` is ready: ask for its information, once,
    /// while it is the one being read.
    fn inform(
        &mut self,
        (global, number): Described,
        description: &WpImageDescriptionV1,
        queue: &QueueHandle<Self>,
    ) {
        let Some(bound) = self.bound.iter_mut().find(|bound| bound.global == global) else {
            return;
        };
        if bound.reading != Some(number) || bound.informed {
            return;
        }
        bound.informed = true;
        // The information object destroys itself after its `done`.
        let _info = description.get_information(queue, (global, number));
    }

    /// The description `described` was read or failed: ask for the next if a
    /// change was announced meanwhile.
    fn read(&mut self, (global, number): Described, queue: &QueueHandle<Self>) {
        let Some(bound) = self.bound.iter_mut().find(|bound| bound.global == global) else {
            return;
        };
        if bound.reading != Some(number) {
            return;
        }
        bound.reading = None;
        if std::mem::take(&mut bound.stale) {
            self.describe(global, queue);
        }
    }

    fn apply_color(&mut self, (global, number): Described, event: ColorEvent) {
        self.changed |= self.outputs.apply_color(global, number, event);
    }
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
            // The loop binds no more, so a display past them is one no
            // window could name.
            if self.bound.len() >= MAX_OUTPUTS {
                tracing::debug!(name, "a Wayland output past {MAX_OUTPUTS} was not read");
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
                color: None,
                description: None,
                reading: None,
                informed: false,
                stale: false,
            });
            self.watch_color(self.bound.len() - 1, queue);
        } else if interface == WpColorManagerV1::interface().name && self.color_manager.is_none() {
            self.color_manager = Some(registry.bind::<WpColorManagerV1, (), Self>(
                name,
                version.min(COLOR_MANAGER_VERSION),
                queue,
                (),
            ));
            for index in 0..self.bound.len() {
                self.watch_color(index, queue);
            }
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
        self.bound.remove(index).release();
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

impl Dispatch<WpColorManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpColorManagerV1,
        _: wp_color_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // What the compositor supports for surfaces; an output's description
        // says what it is.
    }
}

impl Dispatch<WpColorManagementOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &WpColorManagementOutputV1,
        event: wp_color_management_output_v1::Event,
        global: &u32,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wp_color_management_output_v1::Event::ImageDescriptionChanged = event {
            state.describe(*global, queue);
        }
    }
}

impl Dispatch<WpImageDescriptionV1, Described> for State {
    fn event(
        state: &mut Self,
        description: &WpImageDescriptionV1,
        event: wp_image_description_v1::Event,
        described: &Described,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            wp_image_description_v1::Event::Ready { .. }
            | wp_image_description_v1::Event::Ready2 { .. } => {
                state.inform(*described, description, queue);
            }
            wp_image_description_v1::Event::Failed { .. } => {
                let (global, number) = *described;
                state.changed |= state.outputs.color_failed(global, number);
                state.read(*described, queue);
            }
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionInfoV1, Described> for State {
    fn event(
        state: &mut Self,
        _: &WpImageDescriptionInfoV1,
        event: wp_image_description_info_v1::Event,
        described: &Described,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        use wp_image_description_info_v1::Event;
        if let Event::Done = event {
            state.apply_color(*described, ColorEvent::Done);
            state.read(*described, queue);
            return;
        }
        let event = match event {
            Event::PrimariesNamed { primaries } => ColorEvent::NamedPrimaries(match primaries {
                WEnum::Value(Primaries::Srgb) => Some(ColorGamut::Srgb),
                WEnum::Value(Primaries::DisplayP3 | Primaries::DciP3) => {
                    Some(ColorGamut::DisplayP3)
                }
                WEnum::Value(Primaries::Bt2020) => Some(ColorGamut::Rec2020),
                _ => None,
            }),
            Event::Primaries { r_x, .. } => ColorEvent::Primaries { red_x: r_x },
            Event::TfNamed { tf } => ColorEvent::Transfer(match tf {
                WEnum::Value(TransferFunction::St2084Pq) => TransferRange::Pq,
                WEnum::Value(
                    TransferFunction::Hlg | TransferFunction::ExtLinear | TransferFunction::ExtSrgb,
                ) => TransferRange::Extended,
                _ => TransferRange::Standard,
            }),
            // A pure power curve ends at white.
            Event::TfPower { .. } => ColorEvent::Transfer(TransferRange::Standard),
            Event::Luminances {
                max_lum,
                reference_lum,
                ..
            } => ColorEvent::Luminances {
                max: max_lum,
                reference: reference_lum,
            },
            Event::TargetLuminance { max_lum, .. } => ColorEvent::TargetLuminance { max: max_lum },
            _ => return,
        };
        state.apply_color(*described, event);
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
            color_manager: None,
            next_description: Some(0),
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
        // The first round trip delivers the outputs' bursts and their image
        // descriptions' readiness, which request their information; the second
        // delivers that and the xdg-outputs.
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
