//! The X11 adapter: a connection of its own that selects RandR's change events
//! on the root window and reads again what an event says may have changed.
//!
//! RandR 1.5 is required: its monitors are the displays. The adapter reads the
//! state (monitors, the outputs and CRTCs behind them, the modes, the
//! `Xft.dpi` resource and the window manager's work area) into a
//! [`RandrState`] and leaves everything else to that model. A change is a
//! different state, so the several events one reconfiguration sends (screen,
//! CRTC and output notifications) collapse into one report.
//!
//! The server is a peer, so what a read costs does not grow with what it
//! lists: the monitors and the outputs asked about are capped
//! (`x11_model::MAX_MONITORS`, `MAX_OUTPUTS`), of the modes only those a CRTC
//! runs are kept, and each property is read to a fixed length. A RandR event
//! reads the whole state; a change of the resources reads the scale alone, and
//! a change of the desktop or of the work area the work area alone
//! ([`Stale`]). A step hears at most [`MAX_STEP_EVENTS`] events before it
//! reads.

use std::os::fd::{AsFd, BorrowedFd};

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::Event;
use x11rb::protocol::randr;
use x11rb::protocol::xproto::{self, AtomEnum, ChangeWindowAttributesAux, EventMask};
use x11rb::rust_connection::RustConnection;

use super::session::{DisplayError, Session, Step, wait_readable};
use super::view::OutputView;
use super::x11_model::{
    RandrCrtc, RandrMode, RandrMonitor, RandrOutput, RandrState, bounded_monitors,
    current_work_area, modes_run, outputs_to_read, xft_scale_factor,
};

const PROTOCOL: &str = "X11";
/// The events one step hears before it reads what they said changed. A
/// reconfiguration sends a handful (screen, CRTC and output notifications),
/// and a session changes a property of the root window now and then, so a
/// step that heard this many is being sent a stream: it reads what they said
/// so far, or comes straight back for the rest, and a server that sends
/// without end is still read. It is not the owner loop's `MAX_TURN_EVENTS`:
/// this connection hears no input, which comes in bursts, and serves no
/// frames or deadlines between its steps.
const MAX_STEP_EVENTS: usize = 64;
/// `RESOURCE_MANAGER` is read this far (in 4-byte units): 256 KiB.
const RESOURCES_LENGTH: u32 = 1 << 16;
/// An output's EDID is read this far (in 4-byte units): the 128-byte base block.
const EDID_LENGTH: u32 = 32;
/// `_NET_WORKAREA` is read for this many desktops (four values each).
const WORK_AREA_DESKTOPS: u32 = 64;

fn error(error: impl std::fmt::Display) -> DisplayError {
    DisplayError::connection(PROTOCOL, error)
}

/// The native X11 host's displays: the source reads the X server the host's
/// windows are on, on a connection of its own, and wakes the owner when the
/// outputs change.
impl crate::owner::linux::x11::HostDisplays for super::source::LinuxDisplays {
    fn snapshot(&self) -> Result<flui_platform::DisplaySnapshot, crate::PlatformError> {
        super::source::LinuxDisplays::snapshot(self)
    }

    /// A source whose thread cannot start answers every snapshot with why,
    /// and is not tried again: no wake comes that would make it succeed.
    fn observe(
        self: std::sync::Arc<Self>,
        signal: &std::sync::Arc<crate::owner::signal::OwnerSignal>,
    ) -> crate::owner::watch::Watch {
        match self.watch(super::protocol::Protocol::X11, signal) {
            Ok(watcher) => crate::owner::watch::Watch::new(watcher),
            Err(error) => {
                tracing::warn!(%error, "the X11 display source could not start");
                crate::owner::watch::Watch::new(())
            }
        }
    }
}

/// The atoms whose changes can change the displays.
struct Atoms {
    work_area: xproto::Atom,
    current_desktop: xproto::Atom,
    /// The output property that holds the panel's EDID.
    edid: xproto::Atom,
}

/// A connection to the X server and the RandR state last read from it.
pub(super) struct X11Session {
    connection: RustConnection,
    root: xproto::Window,
    atoms: Atoms,
    state: RandrState,
}

impl X11Session {
    /// Connect to the server `DISPLAY` names and read its RandR state.
    pub(super) fn connect() -> Result<Self, DisplayError> {
        let (connection, screen) = RustConnection::connect(None).map_err(error)?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| error("the server lists no screen of the display number"))?
            .root;
        let version = randr::query_version(&connection, 1, 5)
            .map_err(error)?
            .reply()
            .map_err(|source| error(format!("the server has no RandR extension: {source}")))?;
        if (version.major_version, version.minor_version) < (1, 5) {
            return Err(error(format!(
                "RandR {}.{} is too old; the monitor list needs 1.5",
                version.major_version, version.minor_version
            )));
        }
        randr::select_input(
            &connection,
            root,
            randr::NotifyMask::SCREEN_CHANGE
                | randr::NotifyMask::CRTC_CHANGE
                | randr::NotifyMask::OUTPUT_CHANGE,
        )
        .map_err(error)?
        .check()
        .map_err(error)?;
        // The `Xft.dpi` resource and the work area change as properties of the
        // root window.
        xproto::change_window_attributes(
            &connection,
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(error)?
        .check()
        .map_err(error)?;
        let atoms = Atoms {
            work_area: intern(&connection, b"_NET_WORKAREA")?,
            current_desktop: intern(&connection, b"_NET_CURRENT_DESKTOP")?,
            edid: intern(&connection, b"EDID")?,
        };
        let state = read_state(&connection, root, &atoms)?
            .ok_or_else(|| error("the RandR configuration kept changing during the first read"))?;
        Ok(Self {
            connection,
            root,
            atoms,
            state,
        })
    }

    /// What of the displays `event` says may have changed.
    fn stale_after(&self, event: &Event) -> Stale {
        match event {
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => Stale::OUTPUTS,
            Event::PropertyNotify(property) if property.window == self.root => {
                if property.atom == xproto::Atom::from(AtomEnum::RESOURCE_MANAGER) {
                    Stale::SCALE
                } else if [self.atoms.work_area, self.atoms.current_desktop]
                    .contains(&property.atom)
                {
                    Stale::WORK_AREA
                } else {
                    Stale::default()
                }
            }
            _ => Stale::default(),
        }
    }

    /// The state with what `stale` names read again; `None` when the read met
    /// a configuration in flux. Only a change of the outputs reads the RandR
    /// state: the scale and the work area are a property each.
    fn read_again(&self, stale: Stale) -> Result<Option<RandrState>, DisplayError> {
        if stale.outputs {
            return read_state(&self.connection, self.root, &self.atoms);
        }
        let mut state = self.state.clone();
        if stale.scale {
            state.scale_factor = resource_scale(&self.connection, self.root);
        }
        if stale.work_area {
            state.work_area = work_area(&self.connection, self.root, &self.atoms);
        }
        Ok(Some(state))
    }
}

/// What of the displays the events of a step said may have changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Stale {
    /// The monitors, outputs, CRTCs or modes: the whole state is read.
    outputs: bool,
    /// The `Xft.dpi` resource.
    scale: bool,
    /// The work area, or the desktop whose work area it is.
    work_area: bool,
}

impl Stale {
    const OUTPUTS: Self = Self {
        outputs: true,
        scale: false,
        work_area: false,
    };
    const SCALE: Self = Self {
        outputs: false,
        scale: true,
        work_area: false,
    };
    const WORK_AREA: Self = Self {
        outputs: false,
        scale: false,
        work_area: true,
    };

    /// Both are stale.
    fn and(self, other: Self) -> Self {
        Self {
            outputs: self.outputs || other.outputs,
            scale: self.scale || other.scale,
            work_area: self.work_area || other.work_area,
        }
    }
}

impl Session for X11Session {
    fn views(&self) -> Vec<OutputView> {
        self.state.views()
    }

    fn step(&mut self, stop: BorrowedFd<'_>) -> Result<Step, DisplayError> {
        // Events already read from the socket come first: the wait below only
        // sees data the connection has not read yet. The flush comes before
        // them, since sending may read what the server sent meanwhile.
        self.connection.flush().map_err(error)?;
        let mut stale = Stale::default();
        let mut heard = 0;
        while heard < MAX_STEP_EVENTS
            && let Some(event) = self.connection.poll_for_event().map_err(error)?
        {
            heard += 1;
            stale = stale.and(self.stale_after(&event));
        }
        if stale != Stale::default() {
            // The old state stays when the read met a configuration in flux.
            if let Some(state) = self.read_again(stale)? {
                // A monitor with no output (made by `xrandr --setmonitor`)
                // changes the state without changing a display, and wakes
                // nobody.
                let changed = state.views() != self.state.views();
                self.state = state;
                if changed {
                    return Ok(Step::Changed);
                }
            }
            // The read waited for replies, and events that arrived meanwhile
            // sit in the connection's queue, not on the socket the wait below
            // watches. Turn again to drain them first.
            return Ok(Step::Idle);
        }
        if heard == MAX_STEP_EVENTS {
            // The step stopped hearing at its cap, not at an empty queue: the
            // events after it are in the connection's queue, where the wait
            // below would sleep on them. The next step hears them.
            return Ok(Step::Idle);
        }
        let ready = wait_readable(self.connection.stream().as_fd(), stop).map_err(error)?;
        Ok(if ready.stop {
            Step::Stopped
        } else {
            Step::Idle
        })
    }
}

fn intern(connection: &RustConnection, name: &[u8]) -> Result<xproto::Atom, DisplayError> {
    Ok(xproto::intern_atom(connection, false, name)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .atom)
}

/// The server's RandR state, the `Xft.dpi` scale and the work area.
fn read_state(
    connection: &RustConnection,
    root: xproto::Window,
    atoms: &Atoms,
) -> Result<Option<RandrState>, DisplayError> {
    let Some(resources) = settle(
        randr::get_screen_resources_current(connection, root)
            .map_err(error)?
            .reply(),
    )?
    else {
        return Ok(None);
    };
    let Some(listed) = settle(
        randr::get_monitors(connection, root, true)
            .map_err(error)?
            .reply(),
    )?
    else {
        return Ok(None);
    };

    // Each list is read as far as its cap: a monitor is named, and an output
    // asked about, only if the state keeps it.
    let monitors = bounded_monitors(listed.monitors.iter().map(|monitor| RandrMonitor {
        name: atom_name(connection, monitor.name),
        is_primary: monitor.primary,
        bounds: Bounds::new(
            Point::new(i32::from(monitor.x), i32::from(monitor.y)),
            Size::new(i32::from(monitor.width), i32::from(monitor.height)),
        ),
        outputs: monitor.outputs.clone(),
    }));
    let mut outputs: Vec<RandrOutput> = Vec::new();
    let mut crtcs: Vec<RandrCrtc> = Vec::new();
    for output in outputs_to_read(&monitors) {
        let Some(info) = settle(
            randr::get_output_info(connection, output, resources.config_timestamp)
                .map_err(error)?
                .reply(),
        )?
        else {
            return Ok(None);
        };
        let crtc = if info.status == randr::SetConfig::SUCCESS {
            info.crtc
        } else {
            0
        };
        outputs.push(RandrOutput {
            id: output,
            crtc,
            name: String::from_utf8_lossy(&info.name).into_owned(),
            edid: output_edid(connection, output, atoms.edid),
        });
        if crtc != 0 && !crtcs.iter().any(|known| known.id == crtc) {
            let Some(info) = settle(
                randr::get_crtc_info(connection, crtc, resources.config_timestamp)
                    .map_err(error)?
                    .reply(),
            )?
            else {
                return Ok(None);
            };
            let mode = if info.status == randr::SetConfig::SUCCESS {
                info.mode
            } else {
                0
            };
            crtcs.push(RandrCrtc { id: crtc, mode });
        }
    }

    let modes = modes_run(
        resources.modes.iter().map(|mode| {
            let flags = u32::from(mode.mode_flags);
            RandrMode {
                id: mode.id,
                dot_clock: mode.dot_clock,
                htotal: mode.htotal,
                vtotal: mode.vtotal,
                interlace: flags & u32::from(randr::ModeFlag::INTERLACE) != 0,
                double_scan: flags & u32::from(randr::ModeFlag::DOUBLE_SCAN) != 0,
            }
        }),
        &crtcs,
    );

    Ok(Some(RandrState {
        monitors,
        outputs,
        crtcs,
        modes,
        scale_factor: resource_scale(connection, root),
        work_area: work_area(connection, root, atoms),
    }))
}

/// A reply, or `None` when the server refused the request because the
/// configuration moved under the read (`BadRROutput` for an output unplugged
/// mid-read, say). That is not a failure of the session: the change that caused
/// it sends its own notify, and the state is read again then. Only a failed
/// connection ends the session.
fn settle<T>(reply: Result<T, ReplyError>) -> Result<Option<T>, DisplayError> {
    match reply {
        Ok(reply) => Ok(Some(reply)),
        Err(ReplyError::X11Error(refusal)) => {
            tracing::debug!(
                ?refusal,
                "RandR state changed during a read; reading again on the next notify"
            );
            Ok(None)
        }
        Err(ReplyError::ConnectionError(failure)) => Err(error(failure)),
    }
}

/// The EDID the server holds for `output`, `None` for an output with none (a
/// virtual one, or a driver that does not publish it).
fn output_edid(
    connection: &RustConnection,
    output: randr::Output,
    property: xproto::Atom,
) -> Option<Vec<u8>> {
    // The base block is 128 bytes; the first 16 hold the identity.
    let reply = randr::get_output_property(
        connection,
        output,
        property,
        AtomEnum::INTEGER,
        0,
        EDID_LENGTH,
        false,
        false,
    )
    .ok()?
    .reply()
    .ok()?;
    (reply.format == 8 && !reply.data.is_empty()).then_some(reply.data)
}

/// The name an atom stands for, or `"Display"` for one the server does not know.
fn atom_name(connection: &RustConnection, atom: xproto::Atom) -> String {
    xproto::get_atom_name(connection, atom)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| String::from_utf8_lossy(&reply.name).into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Display".to_owned())
}

/// `Xft.dpi / 96` from the root window's `RESOURCE_MANAGER`, if it sets one.
fn resource_scale(connection: &RustConnection, root: xproto::Window) -> Option<DevicePixelRatio> {
    let reply = xproto::get_property(
        connection,
        false,
        root,
        AtomEnum::RESOURCE_MANAGER,
        AtomEnum::STRING,
        0,
        RESOURCES_LENGTH,
    )
    .ok()?
    .reply()
    .ok()?;
    xft_scale_factor(&String::from_utf8_lossy(&reply.value))
}

/// The work area of the current desktop, if the window manager sets one.
fn work_area(
    connection: &RustConnection,
    root: xproto::Window,
    atoms: &Atoms,
) -> Option<Bounds<i32>> {
    let cardinals = |atom: xproto::Atom, length: u32| {
        xproto::get_property(connection, false, root, atom, AtomEnum::CARDINAL, 0, length)
            .ok()?
            .reply()
            .ok()
    };
    let areas = cardinals(atoms.work_area, WORK_AREA_DESKTOPS * 4)?;
    let values: Vec<i32> = areas
        .value32()?
        .map(|value| i32::try_from(value).unwrap_or(i32::MAX))
        .collect();
    let current = cardinals(atoms.current_desktop, 1).and_then(|reply| reply.value32()?.next());
    current_work_area(&values, current)
}

#[cfg(test)]
mod tests;
