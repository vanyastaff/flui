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
//! - **A read** answers at once while the application owns the selection of
//!   the seat it reads (the last input's, else the first holding one):
//!   the text it wrote is the selection, and asking the compositor for it
//!   would have the owner thread both write and wait on the same pipe. Else
//!   it asks that seat's offer for its text in the form
//!   [`TextForm::preferred`] picks, under the MIME type the owner named,
//!   and a thread of its own reads the pipe, bounded in time and size, and
//!   answers the read's resolver.
//! - **A write** makes a data source offering the text in every form of
//!   [`TextForm::OFFERED`] and sets it as the selection of the last user
//!   input's seat, under that input's serial: a compositor takes a selection
//!   only from a client the user is working in, and only that seat's earlier
//!   selection is replaced: another seat keeps its own. A write under the
//!   serial the seat's selection was set under changes what that source
//!   answers instead of setting a second one, which Weston up to 14 drops:
//!   the next paste reads the new text, and a clipboard manager that copied
//!   the selection as it was set keeps the copy it took. A write whose
//!   concealment differs offers other types, which only a new source can:
//!   it is set over the current one, which answers the new write too until
//!   a sync tells which of the two the compositor holds (a `cancelled` to
//!   the old one first means the new one). A concealed write that a
//!   compositor keeps the old source for goes without the hint, with a
//!   warning. Each `send` is written by a thread of its own, at most
//!   [`MAX_WRITERS`] at once, from the text encoded once per form;
//!   `cancelled` (another client took the selection) ends that seat's
//!   ownership. With no user input yet the text is held here only, and a
//!   read answers it until another client's selection arrives.

use std::os::fd::{AsFd, OwnedFd};
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, Concealment, capability::Resolver,
};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{self, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::{self, WlDataOffer};
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use web_time::Instant;

use super::owners::Owners;
use super::text::{CONCEALED_HINT, CONCEALED_HINT_VALUE, TextForm};
use crate::host::{MAX_TEXT, NativeClipboard, NativePrimarySelection, owned_text};
use crate::owner::linux::wayland::{LastInput, WaylandClipboard, WaylandState};
use crate::owner::slot::OwnerCell;

mod primary;

/// The newest `wl_data_device_manager`: version 3 adds the drag actions,
/// version 2 lets a device be released.
const MANAGER_VERSION: u32 = 3;
/// How long a read waits for the selection's owner to write its text, and a
/// write for the reader to take it. An owner that hangs answers nothing.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);
/// The most `send`s the application answers at once. Each holds a thread
/// until its reader has the text or the deadline passes; a client asking
/// faster than it reads is answered with nothing past this.
const MAX_WRITERS: usize = 4;

/// The seats' clipboard: the loop's clipboard hook. See the module docs.
pub(crate) struct SeatClipboard {
    shared: Arc<Shared>,
}

/// What the clipboard holds, reached on the owner thread.
struct Shared {
    state: OwnerCell<State>,
    /// The last user input, whose seat and serial a write is set under.
    last_input: LastInput,
    /// The primary selection, beside the clipboard on devices of its own.
    primary: OwnerCell<primary::Primary>,
    /// The `send`s being answered now.
    writers: transfer::Writers,
}

#[derive(Default)]
struct State {
    /// `wl_data_device_manager`, by its global's name.
    manager: Option<(u32, WlDataDeviceManager)>,
    /// The queue the devices and sources report to.
    queue: Option<QueueHandle<WaylandState>>,
    devices: Vec<SeatDevice>,
    /// The text the application owns each seat's selection with.
    owned: Owners<WlSeat, Owned>,
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

/// One of the application's selections: its text, and the source the
/// compositor asks for it through, with the serial it was set under; `None`
/// while it is held in this process alone, under no seat.
struct Owned {
    text: Arc<str>,
    source: Option<WlDataSource>,
    serial: Option<u32>,
    /// The source `source` was set over under the same serial, answering
    /// the same write, until the compositor says which of the two it holds:
    /// `cancelled` here means it took `source`, a sync answered first means
    /// it kept this one (see [`SelectionCheck`]).
    superseded: Option<WlDataSource>,
}

impl Owned {
    fn release(self) {
        for source in [self.source, self.superseded].into_iter().flatten() {
            source.destroy();
        }
    }

    /// Answer with `text` from now on, through every source this selection
    /// holds.
    fn replace(&mut self, text: Arc<str>, concealed: bool) {
        for source in [&self.source, &self.superseded].into_iter().flatten() {
            if let Some(data) = source.data::<SourceData>() {
                data.replace(Arc::clone(&text), concealed);
            }
        }
        self.text = text;
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
                primary: OwnerCell::new(),
                writers: transfer::Writers::default(),
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
        self.shared
            .primary
            .with(|primary| primary.global(registry, name, interface, queue));
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
        let primary_gone = self
            .shared
            .primary
            .with(|primary| primary.global_removed(name))
            .unwrap_or_default();
        primary_gone
            .into_iter()
            .for_each(primary::PrimaryDeviceGone::release);
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
        let weak = Arc::downgrade(&self.shared);
        let primary_gone = self
            .shared
            .primary
            .with(|primary| primary.seats(seats, &weak))
            .unwrap_or_default();
        primary_gone
            .into_iter()
            .for_each(primary::PrimaryDeviceGone::release);
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
            owned.into_items().into_iter().for_each(Owned::release);
        }
        if let Some(primary) = self.shared.primary.with(primary::Primary::take) {
            primary.release();
        }
    }

    #[cfg(any(test, feature = "testing"))]
    fn selection_offers_hint(&self) -> Option<bool> {
        let seat = self.shared.last_input.latest().map(|input| input.seat);
        self.shared
            .state
            .with(|state| {
                let device = state
                    .devices
                    .iter()
                    .find(|device| Some(&device.seat) == seat.as_ref())?;
                let data = device.selection.as_ref()?.data::<OfferData>()?;
                data.hint.with(|hint| *hint)
            })
            .flatten()
    }

    fn has_primary_selection(&self) -> bool {
        self.shared
            .primary
            .with(|primary| primary.has_manager())
            .unwrap_or(false)
    }

    fn deliver(&self) {
        self.deliver_primary();
    }
}

impl NativePrimarySelection for SeatClipboard {
    fn offer(&self, text: Rc<dyn Fn() -> Option<String>>) {
        self.offer_primary(text);
    }

    fn withdraw(&self) {
        self.withdraw_primary();
    }

    fn owns(&self) -> bool {
        self.owns_primary()
    }

    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        self.read_primary(answer);
    }
}

/// What a read does once the state is let go of.
enum Read {
    /// The answer is known now.
    Now(ClipboardRead),
    /// The offer's owner writes the text in `form` to the pipe.
    Pipe(OwnedFd, TextForm),
}

impl NativeClipboard for SeatClipboard {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        let last_seat = self.shared.last_input.latest().map(|input| input.seat);
        let read = self
            .shared
            .state
            .with(|state| {
                // The seat read: the last input's, or else the first holding
                // a selection.
                let device = state
                    .devices
                    .iter()
                    .find(|device| Some(&device.seat) == last_seat.as_ref())
                    .or_else(|| {
                        state
                            .devices
                            .iter()
                            .find(|device| device.selection.is_some())
                    });
                if let Some(owned) = state.owned.read(device.map(|device| &device.seat)) {
                    return Read::Now(owned_text(&owned.text));
                }
                let Some((offer, (form, mime))) = device
                    .and_then(|device| device.selection.as_ref())
                    .and_then(|offer| Some((offer, offer.data::<OfferData>()?.preferred()?)))
                else {
                    return Read::Now(Ok(None));
                };
                match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
                    Ok((reader, writer)) => {
                        // The type as the owner offered it: an owner may match
                        // what it is asked for exactly.
                        offer.receive(mime, writer.as_fd());
                        // The owner holds the write end now; the loop's next
                        // flush sends the request.
                        drop(writer);
                        Read::Pipe(reader, form)
                    }
                    Err(error) => {
                        tracing::warn!(%error, "no pipe for a clipboard read");
                        Read::Now(Err(ClipboardError::Unavailable))
                    }
                }
            })
            .unwrap_or(Read::Now(Err(ClipboardError::Unavailable)));
        match read {
            // A retired owner has nobody left to answer.
            Read::Now(text) => drop(answer.resolve(text)),
            Read::Pipe(reader, form) => {
                let spawned = std::thread::Builder::new()
                    .name("flui-wayland-clipboard".to_owned())
                    .spawn(move || {
                        let text = transfer::read(reader, Instant::now() + TRANSFER_TIMEOUT)
                            .and_then(|bytes| form.answer(bytes));
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

    /// A concealed write is offered with KDE's password-manager hint, which
    /// only a clipboard manager that reads it honours.
    fn concealment(&self) -> Concealment {
        Concealment::HintOnly
    }

    fn write(&self, write: ClipboardWrite) {
        let concealed = write.is_concealed();
        let text: Arc<str> = write.into_text().into();
        let last = self.shared.last_input.latest();
        let previous = self.shared.state.with(|state| {
            let State {
                manager,
                queue,
                devices,
                owned,
            } = state;
            let set = |source: &WlDataSource, seat: &WlSeat, serial: u32| {
                let device = devices.iter().find(|device| device.seat == *seat)?;
                device.device.set_selection(Some(source), serial);
                Some(())
            };
            let create = |text: &Arc<str>| {
                let (_, manager) = manager.as_ref()?;
                let source = manager.create_data_source(
                    queue.as_ref()?,
                    SourceData::new(Arc::downgrade(&self.shared), Arc::clone(text), concealed),
                );
                for mime in offered(concealed) {
                    source.offer(mime.to_owned());
                }
                Some(source)
            };
            // A write under the serial the seat's selection was set under
            // (see the module docs).
            if let Some(input) = &last
                && let Some(held) = owned.get_mut(&input.seat)
                && held.serial == Some(input.serial)
                && let Some(current) = held.source.clone()
            {
                let offers_hint = current
                    .data::<SourceData>()
                    .is_some_and(|data| data.offers_hint);
                held.replace(Arc::clone(&text), concealed);
                // The offered types change with concealment, and only a new
                // source changes them. The current one keeps answering the
                // new write in case the compositor keeps it; a check already
                // pending keeps the two it weighs.
                if offers_hint != concealed
                    && held.superseded.is_none()
                    && let Some(source) = create(&text)
                    && set(&source, &input.seat, input.serial).is_some()
                {
                    held.superseded = held.source.replace(source);
                    SelectionCheck::send(&current, Arc::downgrade(&self.shared), queue.as_ref());
                }
                return None;
            }
            let source = last.as_ref().and_then(|input| {
                let source = create(&text)?;
                if set(&source, &input.seat, input.serial).is_none() {
                    source.destroy();
                    return None;
                }
                Some((input.seat.clone(), source, input.serial))
            });
            if source.is_none() {
                tracing::debug!(
                    "no user input to set the selection under: the text is held in this process"
                );
            }
            let (seat, source, serial) = match source {
                Some((seat, source, serial)) => (Some(seat), Some(source), Some(serial)),
                None => (None, None, None),
            };
            // Only this seat's earlier selection is replaced: another seat
            // keeps the source it reads.
            owned.set(
                seat,
                Owned {
                    text,
                    source,
                    serial,
                    superseded: None,
                },
            )
        });
        if let Some(previous) = previous.flatten() {
            previous.release();
        }
    }
}

/// A sync sent after a source was set over another under the same serial:
/// the compositor answers it after the `cancelled` it sends the superseded
/// source if it took the new one, so a source still superseded when it
/// answers is the one the compositor kept (Weston up to 13 takes no second
/// selection under one serial).
pub(crate) struct SelectionCheck {
    shared: Weak<Shared>,
    superseded: WlDataSource,
}

impl SelectionCheck {
    /// Ask the compositor, through `superseded`'s connection, to answer once
    /// it has handled the requests sent before.
    fn send(
        superseded: &WlDataSource,
        shared: Weak<Shared>,
        queue: Option<&QueueHandle<WaylandState>>,
    ) {
        let (Some(queue), Some(backend)) = (queue, superseded.backend().upgrade()) else {
            return;
        };
        Connection::from_backend(backend).display().sync(
            queue,
            Self {
                shared,
                superseded: superseded.clone(),
            },
        );
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
    /// Each text form offered, once, with the MIME type it was first offered
    /// under: at most one entry per [`TextForm`], however often the owner
    /// repeats an `offer` event.
    forms: OwnerCell<Vec<(TextForm, String)>>,
    /// Whether the offer names [`CONCEALED_HINT`].
    hint: OwnerCell<bool>,
}

impl OfferData {
    /// An offer's data, made by the dispatch on the owner thread.
    fn new() -> Self {
        Self {
            forms: OwnerCell::new(),
            hint: OwnerCell::new(),
        }
    }

    /// Record an `offer` event: a text form not offered before, under the
    /// MIME type named, or the concealment hint. Other types and repeats
    /// keep nothing.
    fn offer(&self, mime_type: String) {
        if mime_type == CONCEALED_HINT {
            self.hint.with(|hint| *hint = true);
            return;
        }
        let Some(form) = TextForm::named(&mime_type) else {
            return;
        };
        self.forms.with(|forms| {
            if forms.iter().all(|(offered, _)| *offered != form) {
                forms.push((form, mime_type));
            }
        });
    }

    /// The form a read asks the offer for, with the MIME type the owner
    /// offered it under.
    fn preferred(&self) -> Option<(TextForm, String)> {
        self.forms
            .with(|forms| {
                let form = TextForm::preferred(forms.iter().map(|(_, mime)| mime.as_str()))?;
                forms.iter().find(|(offered, _)| *offered == form).cloned()
            })
            .flatten()
    }
}

/// The MIME types a source offers: each text form, and [`CONCEALED_HINT`]
/// for a concealed write (ADR-0152 §6).
fn offered(concealed: bool) -> impl Iterator<Item = &'static str> {
    TextForm::OFFERED
        .into_iter()
        .map(TextForm::name)
        .chain(concealed.then_some(CONCEALED_HINT))
}

/// A data source's user data: the text it answers with.
pub(crate) struct SourceData {
    shared: Weak<Shared>,
    /// Whether the source offers [`CONCEALED_HINT`], fixed as it is set: a
    /// type offered after `set_selection` reaches no offer already made.
    offers_hint: bool,
    /// What the source answers with now, read on the owner thread alone. A
    /// later write under the same serial replaces it (see
    /// [`SeatClipboard::write`](NativeClipboard::write)).
    content: OwnerCell<Content>,
}

/// The write a source answers with.
#[derive(Default)]
struct Content {
    text: Arc<str>,
    /// Whether the write was concealed.
    concealed: bool,
    /// The text in each form asked for so far, encoded once and shared by
    /// every `send` of that form.
    encoded: Vec<(TextForm, Arc<[u8]>)>,
}

impl SourceData {
    /// A source's data, answering with `text` and offering the hint when the
    /// write is `concealed`; made on the owner thread.
    fn new(shared: Weak<Shared>, text: Arc<str>, concealed: bool) -> Self {
        let data = Self {
            shared,
            offers_hint: concealed,
            content: OwnerCell::new(),
        };
        data.replace(text, concealed);
        data
    }

    /// Answer with `text` from now on, a write `concealed` or not.
    fn replace(&self, text: Arc<str>, concealed: bool) {
        self.content.with(|content| {
            *content = Content {
                text,
                concealed,
                encoded: Vec::new(),
            };
        });
    }

    /// Whether the write the source answers is concealed while it offers no
    /// hint: a clipboard manager sees it as an ordinary one.
    fn conceals_unmarked(&self) -> bool {
        !self.offers_hint && self.content.with(|content| content.concealed) == Some(true)
    }

    /// What a `send` of `mime` answers: the text in that form, the hint's
    /// value while the write it answers with is concealed, or `None` for a
    /// type not offered.
    fn answer(&self, mime: &str) -> Option<Arc<[u8]>> {
        if mime == CONCEALED_HINT {
            let concealed = self.content.with(|content| content.concealed)?;
            return (self.offers_hint && concealed)
                .then(|| Arc::<[u8]>::from(CONCEALED_HINT_VALUE));
        }
        TextForm::named(mime).and_then(|form| self.bytes(form))
    }

    /// The text in `form`.
    fn bytes(&self, form: TextForm) -> Option<Arc<[u8]>> {
        self.content.with(|content| {
            if let Some((_, bytes)) = content.encoded.iter().find(|(held, _)| *held == form) {
                return Arc::clone(bytes);
            }
            let bytes: Arc<[u8]> = form.encode(&content.text).into();
            content.encoded.push((form, Arc::clone(&bytes)));
            bytes
        })
    }
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
                        if foreign {
                            state.owned.remove_unseated();
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
        if let wl_data_offer::Event::Offer { mime_type } = event {
            data.offer(mime_type);
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
                let bytes = data.answer(&mime_type);
                let (Some(shared), Some(bytes)) = (data.shared.upgrade(), bytes) else {
                    return;
                };
                let Some(permit) = shared.writers.permit() else {
                    tracing::debug!("a clipboard request past {MAX_WRITERS} at once was refused");
                    return;
                };
                let spawned = std::thread::Builder::new()
                    .name("flui-wayland-clipboard".to_owned())
                    .spawn(move || {
                        transfer::write(fd, &bytes, Instant::now() + TRANSFER_TIMEOUT);
                        drop(permit);
                    });
                if let Err(error) = spawned {
                    tracing::warn!(%error, "no thread for a clipboard write");
                }
            }
            wl_data_source::Event::Cancelled => {
                if let Some(shared) = data.shared.upgrade() {
                    shared.state.with(|state| {
                        // A superseded source cancelled: the compositor took
                        // the one set over it, which goes on.
                        if let Some(held) = state
                            .owned
                            .find_mut(|owned| owned.superseded.as_ref() == Some(source))
                        {
                            held.superseded = None;
                            return;
                        }
                        // Only this source's seat lost its selection; it is
                        // destroyed below, not by the entry's release, and a
                        // source it superseded goes with it.
                        let lost = state
                            .owned
                            .remove(|owned| owned.source.as_ref() == Some(source));
                        if let Some(superseded) = lost.and_then(|lost| lost.superseded) {
                            superseded.destroy();
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

impl Dispatch<WlCallback, SelectionCheck> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        check: &SelectionCheck,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_callback::Event::Done { .. } = event else {
            return;
        };
        let Some(shared) = check.shared.upgrade() else {
            return;
        };
        // Still superseded once the compositor has handled the `set_selection`:
        // it kept this source and dropped the one set over it, which no event
        // reports. The kept source answers the latest write already.
        let unmarked = shared.state.with(|state| {
            let held = state
                .owned
                .find_mut(|owned| owned.superseded.as_ref() == Some(&check.superseded))?;
            let kept = held.superseded.take()?;
            // The kept source offers the types of the write it was set for.
            let unmarked = kept
                .data::<SourceData>()
                .is_some_and(SourceData::conceals_unmarked);
            if let Some(dropped) = held.source.replace(kept) {
                dropped.destroy();
            }
            Some(unmarked)
        });
        // The subscriber runs with the clipboard's state let go.
        if unmarked.flatten() == Some(true) {
            tracing::warn!(
                target: "flui.clipboard.wayland",
                "the compositor kept the selection set earlier under this input: the concealed \
                 write it now answers is offered without the password-manager hint"
            );
        }
    }
}

pub(crate) mod transfer {
    //! A selection's bytes through the pipe its reader and owner share,
    //! bounded in time and size: what the threads of a read and a write run,
    //! and how many writes run at once.

    use std::os::fd::OwnedFd;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use web_time::Instant;

    use flui_platform::ClipboardError;

    use super::{MAX_TEXT, MAX_WRITERS};

    /// The writes running now, at most [`MAX_WRITERS`].
    #[derive(Debug, Default)]
    pub(crate) struct Writers(Arc<AtomicUsize>);

    /// One write's place among [`Writers`], given back when dropped.
    #[derive(Debug)]
    pub(crate) struct Permit(Arc<AtomicUsize>);

    impl Writers {
        /// A place for one more write; `None` while [`MAX_WRITERS`] run.
        pub(crate) fn permit(&self) -> Option<Permit> {
            self.0
                .try_update(Ordering::AcqRel, Ordering::Acquire, |running| {
                    (running < MAX_WRITERS).then_some(running + 1)
                })
                .ok()
                .map(|_| Permit(Arc::clone(&self.0)))
        }
    }

    impl Drop for Permit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

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

    /// Everything the owner writes to `reader` until it closes its end:
    /// [`ClipboardError::Timeout`] when it is still writing at `deadline`,
    /// [`ClipboardError::TooLarge`] past [`MAX_TEXT`], and
    /// [`ClipboardError::Unavailable`] when the read fails.
    pub(crate) fn read(reader: OwnedFd, deadline: Instant) -> Result<Vec<u8>, ClipboardError> {
        let mut bytes = Vec::new();
        let mut chunk = vec![0_u8; 64 * 1024];
        loop {
            if !ready(&reader, PollFlags::IN, deadline) {
                tracing::debug!("a clipboard read timed out");
                return Err(ClipboardError::Timeout);
            }
            match rustix::io::read(&reader, &mut chunk) {
                Ok(0) => return Ok(bytes),
                Ok(count) => {
                    if bytes.len() + count > MAX_TEXT {
                        tracing::warn!("a clipboard read past {MAX_TEXT} bytes was dropped");
                        return Err(ClipboardError::TooLarge);
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                }
                Err(Errno::INTR | Errno::AGAIN) => {}
                Err(error) => {
                    tracing::debug!(%error, "a clipboard read failed");
                    return Err(ClipboardError::Unavailable);
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

        /// At most [`MAX_WRITERS`] writes run at once; each that ends gives
        /// its place to the next.
        #[test]
        fn writes_past_the_limit_wait_for_a_place() {
            let writers = Writers::default();
            let mut held: Vec<Permit> = (0..MAX_WRITERS)
                .map(|_| writers.permit().expect("a place below the limit"))
                .collect();
            assert!(writers.permit().is_none(), "the limit is full");
            drop(held.pop().expect("a held place"));
            assert_eq!(held.len(), MAX_WRITERS - 1);
            assert!(writers.permit().is_some(), "an ended write frees its place");
        }

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
            assert_eq!(read(reader, deadline), Ok(text));
            writing.join().expect("the writer ends");
        }

        /// An owner that never closes its end answers nothing once the
        /// deadline passes; a reader gone ends the write at once.
        #[test]
        fn a_transfer_ends_at_its_deadline_or_its_peer() {
            let (reader, writer) = pipe();
            let started = Instant::now();
            assert_eq!(
                read(reader, started + Duration::from_millis(50)),
                Err(ClipboardError::Timeout)
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An owner that repeats its `offer` events grows nothing past one entry
    /// per text form, and a read still asks for the spelling offered first.
    #[test]
    fn repeated_offers_keep_one_entry_per_form() {
        let data = OfferData::new();
        for _ in 0..10_000 {
            data.offer("text/plain;charset=UTF-8".to_owned());
            data.offer("text/plain;charset=utf-8".to_owned());
            data.offer("UTF8_STRING".to_owned());
            data.offer("image/png".to_owned());
        }
        assert_eq!(data.forms.with(|forms| forms.len()), Some(2));
        assert_eq!(
            data.preferred(),
            Some((TextForm::PlainUtf8, "text/plain;charset=UTF-8".to_owned()))
        );
    }

    /// A concealed write offers KDE's password-manager hint beside its text
    /// and answers it `secret`; an ordinary write offers and answers neither
    /// (ADR-0152 §6).
    #[test]
    fn a_concealed_source_offers_and_answers_the_hint() {
        for concealed in [false, true] {
            let offers = offered(concealed).collect::<Vec<&str>>();
            assert_eq!(offers.contains(&CONCEALED_HINT), concealed, "{concealed}");
            let data = SourceData::new(Weak::new(), Arc::<str>::from("hunter2"), concealed);
            assert_eq!(
                data.answer(CONCEALED_HINT).as_deref(),
                concealed.then_some(CONCEALED_HINT_VALUE),
                "{concealed}"
            );
            assert_eq!(
                data.answer("text/plain;charset=utf-8").as_deref(),
                Some(&b"hunter2"[..]),
                "{concealed}"
            );
        }
    }

    /// A source changed in place answers the new text in every form, and the
    /// hint only while it offers it and the write it answers is concealed.
    #[test]
    fn a_source_changed_in_place_answers_the_new_write() {
        for (offered_hint, concealed) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            let data = SourceData::new(Weak::new(), Arc::<str>::from("first"), offered_hint);
            assert_eq!(data.answer("UTF8_STRING").as_deref(), Some(&b"first"[..]));
            data.replace(Arc::<str>::from("second"), concealed);
            let row = format!("offered {offered_hint}, concealed {concealed}");
            assert_eq!(
                data.answer("UTF8_STRING").as_deref(),
                Some(&b"second"[..]),
                "{row}"
            );
            assert_eq!(
                data.answer(CONCEALED_HINT).as_deref(),
                (offered_hint && concealed).then_some(CONCEALED_HINT_VALUE),
                "{row}"
            );
            assert_eq!(
                data.conceals_unmarked(),
                !offered_hint && concealed,
                "{row}"
            );
        }
    }
}
