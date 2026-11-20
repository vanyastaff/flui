//! The lane's libei connection, through `reis`'s protocol objects: the handshake as a sender, each
//! seat bound to the interfaces the lane sends (an absolute pointer, buttons, scroll, and text,
//! which carries keys as keysyms so no keymap is needed), and each event sent in a frame of its
//! own.
//!
//! A device is sent to only once the server resumed it; the lane waits [`READY_WITHIN`] for one
//! that has what an event needs, and a server that offers none is a capability the lane lacks,
//! named before any event. The connection's state lives on the lane's thread alone.

use std::collections::HashMap;
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use reis::ei;
use reis::{Interface, PendingRequestResult};

use super::mapping::{EiRegion, LANE};
use crate::error::{Error, Result};

/// How long the lane waits for the server to resume a device it can send to.
pub(super) const READY_WITHIN: Duration = Duration::from_secs(5);

/// The interfaces the lane binds a seat to.
const WANTED: [&str; 4] = [
    ei::PointerAbsolute::NAME,
    ei::Button::NAME,
    ei::Scroll::NAME,
    ei::Text::NAME,
];

fn broken(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: message.into(),
    }
}

/// A seat the server announced: the mask of each interface it offers.
#[derive(Default)]
struct SeatOffer {
    capabilities: HashMap<String, u64>,
}

/// A device the server announced.
struct DeviceOffer {
    device: ei::Device,
    interfaces: HashMap<String, reis::Object>,
    regions: Vec<EiRegion>,
    done: bool,
    resumed: bool,
}

/// A libei connection: see the [module docs](self).
pub(super) struct EiSession {
    context: ei::Context,
    seats: Vec<(ei::Seat, SeatOffer)>,
    devices: Vec<DeviceOffer>,
    last_serial: u32,
    sequence: u32,
    started: Instant,
}

impl EiSession {
    /// Shake hands as a sender over `socket`.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the socket or the handshake fails.
    pub(super) fn connect(socket: UnixStream) -> Result<Self> {
        let context =
            ei::Context::new(socket).map_err(|error| broken(format!("the EIS socket: {error}")))?;
        let handshake = reis::handshake::ei_handshake_blocking(
            &context,
            "flui-driver",
            ei::handshake::ContextType::Sender,
        )
        .map_err(|error| broken(format!("the libei handshake failed: {error:?}")))?;
        Ok(Self {
            context,
            seats: Vec::new(),
            devices: Vec::new(),
            last_serial: handshake.serial,
            sequence: 0,
            started: Instant::now(),
        })
    }

    /// Read what the server sent within `wait`, and act on it.
    fn pump(&mut self, wait: Duration) -> Result<()> {
        let millis = i64::try_from(wait.as_millis()).unwrap_or(i64::MAX);
        let timeout = rustix::event::Timespec {
            tv_sec: millis / 1000,
            tv_nsec: (millis % 1000) * 1_000_000,
        };
        let fd = self.context.as_fd();
        let mut fds = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        let ready = rustix::event::poll(&mut fds, Some(&timeout))
            .map_err(|error| broken(format!("waiting on the EIS socket: {error}")))?;
        if ready > 0 {
            self.context
                .read()
                .map_err(|error| broken(format!("the EIS socket: {error}")))?;
        }
        while let Some(pending) = self.context.pending_event() {
            match pending {
                PendingRequestResult::Request(event) => self.handle(event)?,
                PendingRequestResult::ParseError(error) => {
                    return Err(broken(format!(
                        "the EIS server sent what parses as no event: {error}"
                    )));
                }
                PendingRequestResult::InvalidObject(_) => {}
            }
        }
        self.flush()
    }

    fn handle(&mut self, event: ei::Event) -> Result<()> {
        match event {
            ei::Event::Connection(_, ei::connection::Event::Seat { seat }) => {
                self.seats.push((seat, SeatOffer::default()));
            }
            ei::Event::Connection(_, ei::connection::Event::Ping { ping }) => ping.done(0),
            ei::Event::Connection(
                _,
                ei::connection::Event::Disconnected {
                    reason,
                    explanation,
                    ..
                },
            ) => {
                return Err(Error::Lane {
                    lane: LANE.to_owned(),
                    unavailable: true,
                    message: format!(
                        "the EIS server ended the connection ({reason:?}): {}",
                        explanation.unwrap_or_default()
                    ),
                });
            }
            ei::Event::Seat(seat, event) => {
                let Some((_, offer)) = self.seats.iter_mut().find(|(known, _)| *known == seat)
                else {
                    return Ok(());
                };
                match event {
                    ei::seat::Event::Capability { mask, interface } => {
                        offer.capabilities.insert(interface, mask);
                    }
                    ei::seat::Event::Done => {
                        let mask = WANTED
                            .iter()
                            .filter_map(|name| offer.capabilities.get(*name))
                            .fold(0, |mask, bit| mask | bit);
                        seat.bind(mask);
                    }
                    ei::seat::Event::Device { device } => self.devices.push(DeviceOffer {
                        device,
                        interfaces: HashMap::new(),
                        regions: Vec::new(),
                        done: false,
                        resumed: false,
                    }),
                    _ => {}
                }
            }
            ei::Event::Device(device, event) => {
                let sequence = self.sequence;
                let Some(offer) = self.devices.iter_mut().find(|known| known.device == device)
                else {
                    return Ok(());
                };
                match event {
                    ei::device::Event::Interface { object } => {
                        offer
                            .interfaces
                            .insert(object.interface().to_owned(), object);
                    }
                    ei::device::Event::Region {
                        offset_x,
                        offset_y,
                        width,
                        // libei's own spelling of the region's height field.
                        hight: height,
                        ..
                    } => offer.regions.push(EiRegion {
                        x: offset_x,
                        y: offset_y,
                        width,
                        height,
                    }),
                    ei::device::Event::Done => offer.done = true,
                    ei::device::Event::Resumed { serial } => {
                        self.last_serial = serial;
                        offer.resumed = true;
                        device.start_emulating(serial, sequence);
                        self.sequence = self.sequence.wrapping_add(1);
                    }
                    ei::device::Event::Paused { serial } => {
                        self.last_serial = serial;
                        offer.resumed = false;
                    }
                    ei::device::Event::Destroyed { serial } => {
                        self.last_serial = serial;
                        self.devices.retain(|known| known.device != device);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        self.context
            .flush()
            .map_err(|error| broken(format!("the EIS socket: {error}")))
    }

    /// The interface `T` of a resumed device that has it, waiting for the server to offer one,
    /// with the device and the regions it covers.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming the interface when none is offered in time.
    pub(super) fn interface<T: Interface>(&mut self) -> Result<(T, ei::Device, Vec<EiRegion>)> {
        let deadline = Instant::now() + READY_WITHIN;
        loop {
            self.pump(Duration::ZERO)?;
            let found = self.devices.iter().find_map(|offer| {
                if !(offer.done && offer.resumed) {
                    return None;
                }
                let interface = offer.interfaces.get(T::NAME)?.clone().downcast::<T>()?;
                Some((interface, offer.device.clone(), offer.regions.clone()))
            });
            if let Some(found) = found {
                return Ok(found);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::not_supported(
                    format!("wayland_{}", T::NAME),
                    format!(
                        "the EIS server offers no device with {} the lane may send to",
                        T::NAME
                    ),
                ));
            }
            self.pump(left.min(Duration::from_millis(20)))?;
        }
    }

    /// End a frame of `device`'s events, and send it.
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the socket fails.
    pub(super) fn frame(&mut self, device: &ei::Device) -> Result<()> {
        let micros = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        device.frame(self.last_serial, micros);
        self.flush()
    }
}
