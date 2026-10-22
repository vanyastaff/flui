//! The X11 adapter: a connection of its own that selects RandR's change events
//! on the root window and reads the whole RandR state again whenever one
//! arrives.
//!
//! RandR 1.5 is required: its monitors are the displays. The adapter reads the
//! state (monitors, the outputs and CRTCs behind them, the modes, the
//! `Xft.dpi` resource and the window manager's work area) into a
//! [`RandrState`] and leaves everything else to that model. A change is a
//! different state, so the several events one reconfiguration sends (screen,
//! CRTC and output notifications) collapse into one report.

use std::os::fd::{AsFd, BorrowedFd};

use flui_foundation::geometry::{Bounds, Point, Size};
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::Event;
use x11rb::protocol::randr;
use x11rb::protocol::xproto::{self, AtomEnum, ChangeWindowAttributesAux, EventMask};
use x11rb::rust_connection::RustConnection;

use super::session::{DisplayError, Session, Step, wait_readable};
use super::view::OutputView;
use super::x11_model::{
    RandrCrtc, RandrMode, RandrMonitor, RandrOutput, RandrState, current_work_area,
    xft_scale_factor,
};

const PROTOCOL: &str = "X11";
/// `RESOURCE_MANAGER` is read this far (in 4-byte units): 256 KiB.
const RESOURCES_LENGTH: u32 = 1 << 16;
/// An output's EDID is read this far (in 4-byte units): the 128-byte base block.
const EDID_LENGTH: u32 = 32;
/// `_NET_WORKAREA` is read for this many desktops (four values each).
const WORK_AREA_DESKTOPS: u32 = 64;

fn error(error: impl std::fmt::Display) -> DisplayError {
    DisplayError::connection(PROTOCOL, error)
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

    /// Whether `event` can change what the displays are.
    fn concerns_displays(&self, event: &Event) -> bool {
        match event {
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => true,
            Event::PropertyNotify(property) => {
                property.window == self.root
                    && [
                        xproto::Atom::from(AtomEnum::RESOURCE_MANAGER),
                        self.atoms.work_area,
                        self.atoms.current_desktop,
                    ]
                    .contains(&property.atom)
            }
            _ => false,
        }
    }
}

impl Session for X11Session {
    fn views(&self) -> Vec<OutputView> {
        self.state.views()
    }

    fn step(&mut self, stop: BorrowedFd<'_>) -> Result<Step, DisplayError> {
        // Events already read from the socket come first: the wait below only
        // sees data the connection has not read yet.
        let mut concerned = false;
        while let Some(event) = self.connection.poll_for_event().map_err(error)? {
            concerned |= self.concerns_displays(&event);
        }
        if concerned {
            // The old state stays when the read met a configuration in flux.
            if let Some(state) = read_state(&self.connection, self.root, &self.atoms)? {
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
        self.connection.flush().map_err(error)?;
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

    let mut monitors = Vec::with_capacity(listed.monitors.len());
    let mut outputs: Vec<RandrOutput> = Vec::new();
    let mut crtcs: Vec<RandrCrtc> = Vec::new();
    for monitor in &listed.monitors {
        for &output in &monitor.outputs {
            if outputs.iter().any(|known| known.id == output) {
                continue;
            }
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
        monitors.push(RandrMonitor {
            name: atom_name(connection, monitor.name),
            is_primary: monitor.primary,
            bounds: Bounds::new(
                Point::new(i32::from(monitor.x), i32::from(monitor.y)),
                Size::new(i32::from(monitor.width), i32::from(monitor.height)),
            ),
            outputs: monitor.outputs.clone(),
        });
    }

    let modes = resources
        .modes
        .iter()
        .map(|mode| {
            let flags = u32::from(mode.mode_flags);
            RandrMode {
                id: mode.id,
                dot_clock: mode.dot_clock,
                htotal: mode.htotal,
                vtotal: mode.vtotal,
                interlace: flags & u32::from(randr::ModeFlag::INTERLACE) != 0,
                double_scan: flags & u32::from(randr::ModeFlag::DOUBLE_SCAN) != 0,
            }
        })
        .collect();

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
fn resource_scale(connection: &RustConnection, root: xproto::Window) -> Option<f64> {
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
