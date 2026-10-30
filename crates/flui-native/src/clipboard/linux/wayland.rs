//! The Wayland host's clipboard: the seats' selection through
//! `wl_data_device` (ADR-0151 §10, ADR-0152).
//!
//! The loop's clipboard hook (`owner::linux::wayland::WaylandClipboard`),
//! which the host's table fills. It binds `wl_data_device_manager` as the
//! compositor announces it and keeps one data device on each seat the input
//! holds, which the loop hands it every turn.
//!
//! - **The selection a seat holds** reaches its device as a data offer named
//!   by `selection`, sent before the keyboard enters one of the client's
//!   surfaces and whenever it changes while the keyboard is in one. The
//!   device keeps the last; a drag's offer (`enter`) is let go of at
//!   `leave`, as this host takes no drop yet.
//! - **A read** answers at once while the application owns the selection:
//!   the text it wrote is the selection, and asking the compositor for it
//!   would have the owner thread both write and wait on the same pipe. Else
//!   it asks the offer of the seat of the last user input (or the first seat
//!   holding one) for its text in the form [`TextForm::preferred`] picks,
//!   and a thread of its own reads the pipe, bounded in time and size, and
//!   answers the read's resolver.
//! - **A write** makes a data source offering the text in every form of
//!   [`TextForm::OFFERED`] and sets it as the selection of the last user
//!   input's seat, under that input's serial: a compositor takes a selection
//!   only from a client the user is working in. Each `send` is written by a
//!   thread of its own; `cancelled` (another client took the selection) ends
//!   the ownership. With no user input yet the text is held here only, and a
//!   read answers it until another client's selection arrives.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_platform::capability::Resolver;
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{self, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use web_time::Instant;

use super::text::TextForm;
use crate::host::NativeClipboard;
use crate::owner::linux::wayland::{LastInput, WaylandClipboard, WaylandState};
use crate::owner::slot::OwnerCell;

/// The newest `wl_data_device_manager`: version 3 adds the drag actions,
/// version 2 lets a device be released.
const MANAGER_VERSION: u32 = 3;
/// How long a read waits for the selection's owner to write its text, and a
/// write for the reader to take it. An owner that hangs answers nothing.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);
/// The most text a read takes: past it the selection is not read.
const MAX_TEXT: usize = 64 * 1024 * 1024;

/// The seats' clipboard: the loop's clipboard hook. See the module docs.
pub(crate) struct SeatClipboard {
    shared: Arc<Shared>,
}

/// What the clipboard holds, reached on the owner thread.
struct Shared {
    state: OwnerCell<State>,
    /// The last user input, whose seat and serial a write is set under.
    last_input: LastInput,
}

#[derive(Default)]
struct State {
    /// `wl_data_device_manager`, by its global's name.
    manager: Option<(u32, WlDataDeviceManager)>,
    /// The queue the devices and sources report to.
    queue: Option<QueueHandle<WaylandState>>,
    devices: Vec<SeatDevice>,
    /// The text the application owns the selection with.
    owned: Option<Owned>,
}

/// One seat's data device and the offers it holds.
struct SeatDevice {
    seat: WlSeat,
    device: WlDataDevice,
    /// The offer of the seat's selection; `None` when it holds none.
    selection: Option<WlDataOffer>,
    /// The offer of a drag over one of the client's surfaces.
    dragged: Option<WlDataOffer>,
}

impl SeatDevice {
    fn release(self) {
        for offer in [self.selection, self.dragged].into_iter().flatten() {
            offer.destroy();
        }
        // `release` exists from version 2; below it the proxy is just
        // dropped.
        if self.device.version() >= 2 {
            self.device.release();
        }
    }
}

/// The application's selection: its text, and the source the compositor
/// asks for it; `None` while no seat took it.
struct Owned {
    text: Arc<str>,
    source: Option<WlDataSource>,
}

impl Owned {
    fn release(self) {
        if let Some(source) = self.source {
            source.destroy();
        }
    }
}

impl SeatClipboard {
    /// The clipboard of a loop on the calling thread, with no global bound
    /// yet, setting its selection under the inputs `last_input` records.
    pub(crate) fn new(last_input: LastInput) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: OwnerCell::new(),
                last_input,
            }),
        }
    }
}

impl WaylandClipboard for SeatClipboard {
    fn global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    ) {
        if interface != WlDataDeviceManager::interface().name {
            return;
        }
        self.shared.state.with(|state| {
            // A second announcement would bind an object nothing destroys.
            if state.manager.is_some() {
                return;
            }
            let manager = registry.bind::<WlDataDeviceManager, (), WaylandState>(
                name,
                version.min(MANAGER_VERSION),
                queue,
                (),
            );
            state.manager = Some((name, manager));
            state.queue = Some(queue.clone());
        });
    }

    fn global_removed(&self, name: u32) {
        let gone = self
            .shared
            .state
            .with(|state| {
                if state
                    .manager
                    .as_ref()
                    .is_some_and(|(held, _)| *held == name)
                {
                    state.manager = None;
                    std::mem::take(&mut state.devices)
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default();
        gone.into_iter().for_each(SeatDevice::release);
    }

    fn seats(&self, seats: &[WlSeat]) {
        let gone = self
            .shared
            .state
            .with(|state| {
                let (kept, gone) = std::mem::take(&mut state.devices)
                    .into_iter()
                    .partition(|device| seats.contains(&device.seat));
                state.devices = kept;
                if let (Some((_, manager)), Some(queue)) = (&state.manager, &state.queue) {
                    for seat in seats {
                        if state.devices.iter().any(|device| device.seat == *seat) {
                            continue;
                        }
                        let device = manager.get_data_device(
                            seat,
                            queue,
                            DeviceData {
                                shared: Arc::downgrade(&self.shared),
                            },
                        );
                        state.devices.push(SeatDevice {
                            seat: seat.clone(),
                            device,
                            selection: None,
                            dragged: None,
                        });
                    }
                }
                gone
            })
            .unwrap_or_default();
        gone.into_iter().for_each(SeatDevice::release);
    }

    fn release(&self) {
        let held = self.shared.state.with(std::mem::take);
        if let Some(State { devices, owned, .. }) = held {
            devices.into_iter().for_each(SeatDevice::release);
            if let Some(owned) = owned {
                owned.release();
            }
        }
    }
}

/// What a read does once the state is let go of.
enum Read {
    /// The answer is known now.
    Now(Option<String>),
    /// The offer's owner writes the text in `form` to the pipe.
    Pipe(OwnedFd, TextForm),
}

impl NativeClipboard for SeatClipboard {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        let last_seat = self.shared.last_input.latest().map(|input| input.seat);
        let read = self
            .shared
            .state
            .with(|state| {
                if let Some(owned) = &state.owned {
                    return Read::Now(Some(owned.text.to_string()));
                }
                let offer = state
                    .devices
                    .iter()
                    .filter(|device| device.selection.is_some())
                    .min_by_key(|device| Some(&device.seat) != last_seat.as_ref())
                    .and_then(|device| device.selection.as_ref());
                let Some((offer, form)) = offer.and_then(|offer| {
                    let form = offer.data::<OfferData>()?.preferred()?;
                    Some((offer, form))
                }) else {
                    return Read::Now(None);
                };
                match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
                    Ok((reader, writer)) => {
                        offer.receive(form.name().to_owned(), writer.as_fd());
                        // The owner holds the write end now; the loop's next
                        // flush sends the request.
                        drop(writer);
                        Read::Pipe(reader, form)
                    }
                    Err(error) => {
                        tracing::warn!(%error, "no pipe for a clipboard read");
                        Read::Now(None)
                    }
                }
            })
            .unwrap_or(Read::Now(None));
        match read {
            // A retired owner has nobody left to answer.
            Read::Now(text) => drop(answer.resolve(text)),
            Read::Pipe(reader, form) => {
                let spawned = std::thread::Builder::new()
                    .name("flui-wayland-clipboard".to_owned())
                    .spawn(move || {
                        let text = transfer::read(reader, Instant::now() + TRANSFER_TIMEOUT)
                            .map(|bytes| form.decode(bytes));
                        let _ = answer.resolve(text);
                    });
                // The closure, and the resolver in it, is dropped: the read
                // ends as abandoned by its provider.
                if let Err(error) = spawned {
                    tracing::warn!(%error, "no thread for a clipboard read");
                }
            }
        }
    }

    fn write_text(&self, text: String) {
        let text: Arc<str> = text.into();
        let last = self.shared.last_input.latest();
        let previous = self.shared.state.with(|state| {
            let source = last.and_then(|input| {
                let (_, manager) = state.manager.as_ref()?;
                let queue = state.queue.as_ref()?;
                let device = state
                    .devices
                    .iter()
                    .find(|device| device.seat == input.seat)?;
                let source = manager.create_data_source(
                    queue,
                    SourceData {
                        shared: Arc::downgrade(&self.shared),
                        text: Arc::clone(&text),
                    },
                );
                for form in TextForm::OFFERED {
                    source.offer(form.name().to_owned());
                }
                device.device.set_selection(Some(&source), input.serial);
                Some(source)
            });
            if source.is_none() {
                tracing::debug!(
                    "no user input to set the selection under: the text is held in this process"
                );
            }
            state.owned.replace(Owned { text, source })
        });
        if let Some(previous) = previous.flatten() {
            previous.release();
        }
    }
}

// ---- The dispatch of the clipboard's objects on the owner's queue: each
// records what the compositor said, and runs no handler.

/// A data device's user data.
pub(crate) struct DeviceData {
    shared: Weak<Shared>,
}

/// A data offer's user data: the forms of text it offers, as its `offer`
/// events name them, read on the owner thread alone.
pub(crate) struct OfferData {
    forms: OwnerCell<Vec<TextForm>>,
}

impl OfferData {
    /// An offer's data, made by the dispatch on the owner thread.
    fn new() -> Self {
        Self {
            forms: OwnerCell::new(),
        }
    }

    /// The form a read asks the offer for.
    fn preferred(&self) -> Option<TextForm> {
        self.forms
            .with(|forms| TextForm::preferred(forms.iter().map(|form| form.name())))
            .flatten()
    }
}

/// A data source's user data: the text it answers with.
pub(crate) struct SourceData {
    shared: Weak<Shared>,
    text: Arc<str>,
}

impl Dispatch<WlDataDeviceManager, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: wl_data_device_manager::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<WlDataDevice, DeviceData> for WaylandState {
    fn event(
        _: &mut Self,
        device: &WlDataDevice,
        event: wl_data_device::Event,
        data: &DeviceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let replaced = shared
            .state
            .with(|state| {
                // While the application holds no source, any selection that
                // arrives is another client's.
                let foreign = matches!(&event, wl_data_device::Event::Selection { id: Some(_) });
                let held = state
                    .devices
                    .iter_mut()
                    .find(|held| held.device == *device)?;
                let replaced = match event {
                    wl_data_device::Event::Selection { id } => {
                        let replaced = std::mem::replace(&mut held.selection, id);
                        // Text held in this process with no source is the
                        // selection until another client's arrives.
                        if foreign
                            && state
                                .owned
                                .as_ref()
                                .is_some_and(|owned| owned.source.is_none())
                        {
                            state.owned = None;
                        }
                        replaced
                    }
                    wl_data_device::Event::Enter { id, .. } => {
                        std::mem::replace(&mut held.dragged, id)
                    }
                    wl_data_device::Event::Leave | wl_data_device::Event::Drop => {
                        held.dragged.take()
                    }
                    _ => None,
                };
                // An offer named again is the one already held.
                replaced.filter(|offer| {
                    Some(offer) != held.selection.as_ref() && Some(offer) != held.dragged.as_ref()
                })
            })
            .flatten();
        if let Some(offer) = replaced {
            offer.destroy();
        }
    }

    event_created_child!(WaylandState, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferData::new()),
    ]);
}

impl Dispatch<WlDataOffer, OfferData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: wl_data_offer::Event,
        data: &OfferData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event
            && let Some(form) = TextForm::named(&mime_type)
        {
            data.forms.with(|forms| forms.push(form));
        }
    }
}

impl Dispatch<WlDataSource, SourceData> for WaylandState {
    fn event(
        _: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        data: &SourceData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                // A form the source did not offer is answered with nothing:
                // dropping the descriptor ends the reader's read.
                let Some(form) = TextForm::named(&mime_type) else {
                    return;
                };
                let bytes = form.encode(&data.text);
                let spawned = std::thread::Builder::new()
                    .name("flui-wayland-clipboard".to_owned())
                    .spawn(move || transfer::write(fd, &bytes, Instant::now() + TRANSFER_TIMEOUT));
                if let Err(error) = spawned {
                    tracing::warn!(%error, "no thread for a clipboard write");
                }
            }
            wl_data_source::Event::Cancelled => {
                if let Some(shared) = data.shared.upgrade() {
                    shared.state.with(|state| {
                        if state
                            .owned
                            .as_ref()
                            .is_some_and(|owned| owned.source.as_ref() == Some(source))
                        {
                            state.owned = None;
                        }
                    });
                }
                // After the clipboard's release the source is still the
                // protocol's to destroy.
                source.destroy();
            }
            _ => {}
        }
    }
}

pub(crate) mod transfer {
    //! A selection's bytes through the pipe its reader and owner share,
    //! bounded in time and size: what the threads of a read and a write run.

    use std::os::fd::OwnedFd;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use web_time::Instant;

    use super::MAX_TEXT;

    /// Wait until `fd` is ready for `flags` or `deadline` passes; `false` on
    /// the deadline or a failed poll.
    fn ready(fd: &OwnedFd, flags: PollFlags, deadline: Instant) -> bool {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let timeout = Timespec {
                tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: left.subsec_nanos().into(),
            };
            let mut descriptors = [PollFd::new(fd, flags)];
            match poll(&mut descriptors, Some(&timeout)) {
                Ok(0) => return false,
                Ok(_) => return true,
                Err(Errno::INTR) => {}
                Err(_) => return false,
            }
        }
    }

    /// Everything the owner writes to `reader` until it closes its end; `None`
    /// when it is still writing at `deadline`, writes past [`MAX_TEXT`], or
    /// the read fails.
    pub(crate) fn read(reader: OwnedFd, deadline: Instant) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut chunk = vec![0_u8; 64 * 1024];
        loop {
            if !ready(&reader, PollFlags::IN, deadline) {
                tracing::debug!("a clipboard read timed out");
                return None;
            }
            match rustix::io::read(&reader, &mut chunk) {
                Ok(0) => return Some(bytes),
                Ok(count) => {
                    if bytes.len() + count > MAX_TEXT {
                        tracing::warn!("a clipboard read past {MAX_TEXT} bytes was dropped");
                        return None;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                }
                Err(Errno::INTR | Errno::AGAIN) => {}
                Err(error) => {
                    tracing::debug!(%error, "a clipboard read failed");
                    return None;
                }
            }
        }
    }

    /// Write `bytes` to `writer` and close it; stop at `deadline` or when the
    /// reader closed its end.
    pub(crate) fn write(writer: OwnedFd, bytes: &[u8], deadline: Instant) {
        // A full pipe answers `AGAIN` instead of holding the thread past its
        // deadline.
        if let Err(error) = rustix::fs::fcntl_setfl(&writer, rustix::fs::OFlags::NONBLOCK) {
            tracing::debug!(%error, "a clipboard write could not be made non-blocking");
            return;
        }
        let mut left = bytes;
        while !left.is_empty() {
            if !ready(&writer, PollFlags::OUT, deadline) {
                tracing::debug!("a clipboard write timed out");
                return;
            }
            // A reader gone answers `PIPE`: Rust's runtime ignores
            // `SIGPIPE`, so the write fails instead of ending the process.
            match rustix::io::write(&writer, left) {
                Ok(count) => left = &left[count..],
                Err(Errno::INTR | Errno::AGAIN) => {}
                Err(error) => {
                    tracing::debug!(%error, "a clipboard write ended early");
                    return;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::time::Duration;

        use super::*;

        fn pipe() -> (OwnedFd, OwnedFd) {
            rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).expect("a pipe")
        }

        /// What one end writes the other reads whole, past the pipe's own
        /// buffer, once the writer closes its end.
        #[test]
        fn a_selection_crosses_the_pipe_whole() {
            let (reader, writer) = pipe();
            let text = "selection ".repeat(100_000).into_bytes();
            let sent = text.clone();
            let deadline = Instant::now() + Duration::from_secs(5);
            let writing = std::thread::spawn(move || write(writer, &sent, deadline));
            assert_eq!(read(reader, deadline), Some(text));
            writing.join().expect("the writer ends");
        }

        /// An owner that never closes its end answers nothing once the
        /// deadline passes; a reader gone ends the write at once.
        #[test]
        fn a_transfer_ends_at_its_deadline_or_its_peer() {
            let (reader, writer) = pipe();
            let started = Instant::now();
            assert_eq!(read(reader, started + Duration::from_millis(50)), None);
            assert!(started.elapsed() < Duration::from_secs(2));
            drop(writer);

            let (reader, writer) = pipe();
            drop(reader);
            let started = Instant::now();
            write(writer, b"nobody reads", started + Duration::from_secs(5));
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "a closed reader ends the write before its deadline"
            );
        }
    }
}
