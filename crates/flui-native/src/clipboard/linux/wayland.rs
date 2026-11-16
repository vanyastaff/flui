//! The Wayland host's clipboard: the seats' selection through
//! `wl_data_device` (ADR-0151 §10, ADR-0152).
//!
//! The loop's clipboard hook (`owner::linux::wayland::WaylandClipboard`),
//! which the host's table fills. The seats' data devices are the loop's
//! (`transfer::linux::wayland`), shared with drag and drop: the selection a
//! seat holds is the last offer its device was told.
//!
//! - **A read** answers at once while the application owns the selection of
//!   the seat it reads (the last input's, else the first holding one):
//!   the text it wrote is the selection, and asking the compositor for it
//!   would have the owner thread both write and wait on the same pipe. Else
//!   it asks that seat's offer for its text in the form
//!   [`TextForm::preferred`] picks, under the MIME type the owner named,
//!   and a thread of its own reads the pipe, bounded in time and size, and
//!   answers the read's resolver. Reads of one offer share that read and
//!   its answer, and no more than a few wait at once ([`reads`]).
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
//!   the old one first means the new one). A concealed write never goes
//!   out through a source without the hint: where the compositor keeps
//!   such a source, the write is not put on the clipboard, with a warning. Each `send` is written by a thread of its own, at most
//!   [`MAX_WRITERS`] at once, from the text encoded once per form;
//!   `cancelled` (another client took the selection) ends that seat's
//!   ownership.
//! - **No input in focus.** A write with no user input in focus to set it
//!   under (none yet, or the keyboard left the window it went to) answers
//!   `Unavailable` and keeps nothing (ADR-0337 §1): the window that holds
//!   the keyboard again has a serial, the keyboard's `enter` included.
//!   Keeping the text to offer later could overwrite a copy the user made
//!   in another application meanwhile, since a compositor tells only the
//!   focused client about selections.
//! - **No system clipboard on the seat.** With no data device on the seat,
//!   or no data device manager, the text is kept here and the write answers
//!   `ApplicationOnly`: every read in the application answers it until a
//!   newer write or the first selection that arrives, and it is never
//!   offered later ([`Owners`]).
//! - **After the loop releases the clipboard** every read and write answers
//!   [`ClipboardError::Gone`].

use std::os::fd::{BorrowedFd, OwnedFd};
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment,
    capability::Resolver,
};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use web_time::Instant;

use self::reads::{Admitted, Reads};
use super::owners::Owners;
use crate::clipboard::text::{CONCEALED_HINT, CONCEALED_HINT_VALUE, TextForm};
use crate::host::{MAX_TEXT, NativeClipboard, NativePrimarySelection, owned_text};
use crate::owner::linux::wayland::{LastInput, SeatInput, WaylandClipboard, WaylandState};
use crate::owner::slot::OwnerCell;
use crate::transfer::linux::wayland::pipe::{self, PipeError};
use crate::transfer::linux::wayland::{DataDevices, DeviceListener, OfferData};

mod primary;
mod reads;

/// How long a read waits for the selection's owner to write its text, and a
/// write for the reader to take it. An owner that hangs answers nothing.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);
/// The most `send`s the application answers at once. Each holds a thread
/// until its reader has the text or the deadline passes; a client asking
/// faster than it reads is answered with nothing past this.
const MAX_WRITERS: usize = 4;

/// The selection's bytes through `reader`, at most [`MAX_TEXT`], a failure
/// told as the clipboard tells it.
fn read_selection(reader: OwnedFd) -> Result<Vec<u8>, ClipboardError> {
    pipe::read(reader, Instant::now() + TRANSFER_TIMEOUT, MAX_TEXT).map_err(|error| match error {
        PipeError::Timeout => {
            tracing::debug!("a clipboard read timed out");
            ClipboardError::Timeout
        }
        PipeError::TooLarge { .. } => {
            tracing::warn!("a clipboard read past {MAX_TEXT} bytes was dropped");
            ClipboardError::TooLarge
        }
        PipeError::Failed => ClipboardError::Unavailable,
    })
}

/// The seats' clipboard: the loop's clipboard hook. See the module docs.
pub(crate) struct SeatClipboard {
    shared: Arc<Shared>,
}

/// What the clipboard holds, reached on the owner thread.
struct Shared {
    state: OwnerCell<State>,
    /// The seats' data devices, the loop's.
    devices: DataDevices,
    /// The last user input, whose seat and serial a write is set under.
    last_input: LastInput,
    /// The primary selection, beside the clipboard on devices of its own.
    primary: OwnerCell<primary::Primary>,
    /// The `send`s being answered now.
    writers: pipe::Writers,
}

#[derive(Default)]
struct State {
    /// The text the application owns each seat's selection with.
    owned: Owners<WlSeat, Owned>,
    /// The reads of other clients' selections under way.
    reads: SelectionReads,
    /// The loop released the clipboard: nothing it is asked can succeed.
    released: bool,
}

/// The reads of one selection under way, each of the offer it asked.
type SelectionReads = Reads<ObjectId, Resolver<ClipboardRead>>;

/// Read `offer`'s text as `form` for `reader`. `admit` admits the read
/// among those under way, on the owner thread: a read of the offer already
/// under way answers this one too, and only the first of an offer starts
/// one, which `receive` asks the offer's owner for and a thread named
/// `thread` reads. Past the most that wait, the read is refused at once.
fn read_offer(
    admitted: Option<Admitted<Resolver<ClipboardRead>>>,
    form: TextForm,
    thread: &str,
    receive: impl FnOnce(BorrowedFd<'_>),
) {
    match admitted {
        // Off the owner thread nothing was admitted, and the reader went
        // with the attempt; a read that joined waits on the one under way.
        None | Some(Admitted::Joined) => {}
        Some(Admitted::Started(end)) => reads::start(end, form, thread, receive),
        Some(Admitted::Refused(reader)) => {
            // A retired owner has nobody left to answer. The read is settled
            // before the diagnostic, which runs user code.
            let _ = reader.resolve(Err(ClipboardError::Unavailable));
            tracing::debug!(
                "a selection read past {} waiting was refused",
                crate::clipboard::linux::MAX_READERS
            );
        }
    }
}

impl DeviceListener for Shared {
    /// Text kept in this process gives way to the first selection that
    /// arrives (see [`Owners`]).
    fn selection_arrived(&self) {
        let replaced = self.state.with(|state| state.owned.take_kept()).flatten();
        replaced.into_iter().for_each(Owned::release);
    }
}

impl Shared {
    /// A source answering with `text`, set as `input.seat`'s selection under
    /// its serial; `None` with no data device manager or no device on the
    /// seat, which leaves every selection as it was.
    fn place(
        self: &Arc<Self>,
        text: &Arc<str>,
        concealed: bool,
        input: &SeatInput,
    ) -> Option<WlDataSource> {
        let source = self.devices.create_source(SourceData::new(
            Arc::downgrade(self),
            Arc::clone(text),
            concealed,
        ))?;
        for mime in offered(concealed) {
            source.offer(mime.to_owned());
        }
        if self
            .devices
            .set_selection(&input.seat, &source, input.serial)
        {
            Some(source)
        } else {
            source.destroy();
            None
        }
    }
}

/// One of the application's selections: its text, and the source the
/// compositor asks for it through, with the serial it was set under; `None`
/// while it is kept in this process alone, under no seat.
struct Owned {
    text: Arc<str>,
    source: Option<WlDataSource>,
    serial: Option<u32>,
    /// The sources set before `source` under the same serial, oldest first,
    /// until the compositor says which one it holds: `cancelled` to one of
    /// them means it took a later one, a [`SelectionCheck`] answered while
    /// one is still here means it kept that one.
    superseded: Vec<WlDataSource>,
}

impl Owned {
    fn release(self) {
        for source in self.source.into_iter().chain(self.superseded) {
            source.destroy();
        }
    }

    /// Text kept in this process alone, under no seat.
    fn kept(text: Arc<str>) -> Self {
        Self {
            text,
            source: None,
            serial: None,
            superseded: Vec::new(),
        }
    }

    /// Text set as a seat's selection through `source` under `serial`.
    fn seated(text: Arc<str>, source: WlDataSource, serial: u32) -> Self {
        Self {
            text,
            source: Some(source),
            serial: Some(serial),
            superseded: Vec::new(),
        }
    }

    /// Answer with `text` from now on, through every source this selection
    /// holds that can carry it: a concealed write never goes out through a
    /// source that does not offer the hint.
    fn replace(&mut self, text: Arc<str>, concealed: bool) {
        for source in self.source.iter().chain(&self.superseded) {
            if let Some(data) = source.data::<SourceData>()
                && data.carries(concealed)
            {
                data.replace(Arc::clone(&text), concealed);
            }
        }
        self.text = text;
    }
}

impl SeatClipboard {
    /// The clipboard of a loop on the calling thread, setting its selection
    /// on `devices` under the inputs `last_input` records.
    pub(crate) fn new(last_input: LastInput, devices: DataDevices) -> Self {
        let shared = Arc::new(Shared {
            state: OwnerCell::new(),
            devices: devices.clone(),
            last_input,
            primary: OwnerCell::new(),
            writers: pipe::Writers::new(MAX_WRITERS),
        });
        let listener: Weak<Shared> = Arc::downgrade(&shared);
        devices.listen(listener);
        Self { shared }
    }
}

impl WaylandClipboard for SeatClipboard {
    fn global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        _: u32,
        queue: &QueueHandle<WaylandState>,
    ) {
        self.shared
            .primary
            .with(|primary| primary.global(registry, name, interface, queue));
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
    }

    fn release(&self) {
        let released = State {
            released: true,
            ..State::default()
        };
        let held = self
            .shared
            .state
            .with(|state| std::mem::replace(state, released));
        // A read under way ends on its own thread, which holds its readers.
        if let Some(State { owned, .. }) = held {
            owned.into_items().into_iter().for_each(Owned::release);
        }
        if let Some(primary) = self.shared.primary.with(primary::Primary::take) {
            primary.release();
        }
    }

    #[cfg(any(test, feature = "testing"))]
    fn selection_offers_hint(&self) -> Option<bool> {
        let seat = self.shared.last_input.latest()?.seat;
        let offer = self.shared.devices.selection_of(&seat)?;
        offer
            .data::<OfferData>()?
            .types(|types| types.iter().any(|mime| mime == CONCEALED_HINT))
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
    /// The application owns the selection and wrote this text: admitted once
    /// the state is let go of, since a refusal logs.
    Owned(Arc<str>),
    /// Another client owns the selection: its offer is asked for the text
    /// in this form, under the MIME type its owner offered it under.
    Offer(WlDataOffer, TextForm, String),
}

impl NativeClipboard for SeatClipboard {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        let last_seat = self.shared.last_input.latest().map(|input| input.seat);
        // The seat read: the last input's, or else the first holding a
        // selection.
        let selection = self.shared.devices.selection(last_seat.as_ref());
        let read = self
            .shared
            .state
            .with(|state| {
                if state.released {
                    return Read::Now(Err(ClipboardError::Gone));
                }
                let seat = selection.as_ref().map(|selection| &selection.seat);
                if let Some(owned) = state.owned.read(seat) {
                    return Read::Owned(Arc::clone(&owned.text));
                }
                selection
                    .as_ref()
                    .and_then(|selection| selection.offer.as_ref())
                    .and_then(|offer| {
                        let (form, mime) = preferred(offer.data::<OfferData>()?)?;
                        Some(Read::Offer(offer.clone(), form, mime))
                    })
                    .unwrap_or(Read::Now(Ok(None)))
            })
            // Off the owner thread no read can reach the selections.
            .unwrap_or(Read::Now(Err(ClipboardError::Gone)));
        match read {
            // A retired owner has nobody left to answer.
            Read::Now(text) => drop(answer.resolve(text)),
            Read::Owned(text) => drop(answer.resolve(owned_text(&text))),
            Read::Offer(offer, form, mime) => {
                let admitted = self
                    .shared
                    .state
                    .with(|state| state.reads.admit(offer.id(), answer));
                // The type as the owner offered it: an owner may match what
                // it is asked for exactly.
                read_offer(admitted, form, "flui-wayland-clipboard", |pipe| {
                    offer.receive(mime, pipe);
                });
            }
        }
    }

    /// A concealed write is offered with KDE's password-manager hint, which
    /// only a clipboard manager that reads it honours.
    fn concealment(&self) -> Concealment {
        Concealment::HintOnly
    }

    /// Answered at once: `Ok` once the seat's selection is set to the
    /// write's source (the compositor may still cancel it for a newer one);
    /// [`ClipboardError::Unavailable`] with no user input in focus to set it
    /// under (none yet, or the keyboard left the window it went to), which
    /// keeps nothing: the window that holds the keyboard again has a serial;
    /// [`ClipboardError::ApplicationOnly`] with no data device on the seat or
    /// no data device manager: there is no system clipboard to go to, and
    /// the text is kept for the application's reads until a newer write or
    /// the first selection that arrives (see the module docs); and
    /// [`ClipboardError::Gone`] once the loop released the clipboard.
    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        let concealed = write.is_concealed();
        let text: Arc<str> = write.into_text().into();
        let last = self.shared.last_input.latest_in_focus();
        let outcome = self.shared.state.with(|state| {
            if state.released {
                return (Vec::new(), Err(ClipboardError::Gone));
            }
            let owned = &mut state.owned;
            let input = last.as_ref();
            // The seat's selection set under this input's serial, which a
            // write may change in place (see the module docs).
            let mut held = input.and_then(|input| {
                owned
                    .get_mut(&input.seat)
                    .filter(|held| held.serial == Some(input.serial) && held.source.is_some())
            });
            let held_hint = held.as_ref().map(|held| {
                held.source
                    .as_ref()
                    .and_then(|source| source.data::<SourceData>())
                    .is_some_and(|data| data.offers_hint)
            });
            // The offered types change with concealment, and only a new
            // source changes them.
            let fresh = input
                .filter(|_| held_hint != Some(concealed))
                .and_then(|input| self.shared.place(&text, concealed, input));
            let effect = write_effect(input.is_some(), held_hint, concealed, fresh.is_some());
            match (effect, fresh) {
                // Nothing is kept: the window that holds the keyboard again
                // has a serial.
                (WriteEffect::Refuse(error), _) => (Vec::new(), Err(error)),
                // No system clipboard on this seat: the text is kept for the
                // application, never offered later.
                (WriteEffect::KeepForApplication, _) => {
                    let previous = owned.keep(Owned::kept(text));
                    (
                        previous.into_iter().collect(),
                        Err(ClipboardError::ApplicationOnly),
                    )
                }
                (WriteEffect::ReplaceInPlace, _) => {
                    if let Some(held) = held.as_mut() {
                        held.replace(Arc::clone(&text), concealed);
                    }
                    (owned.take_kept().into_iter().collect(), Ok(()))
                }
                // The current source keeps answering the new write in case
                // the compositor keeps it, unless the write is concealed and
                // it offers no hint.
                (WriteEffect::SetOver, Some(source)) => {
                    if let Some(held) = held.as_mut() {
                        held.replace(Arc::clone(&text), concealed);
                        if let Some(current) = held.source.replace(source) {
                            SelectionCheck::send(
                                &current,
                                Arc::downgrade(&self.shared),
                                self.shared.devices.queue().as_ref(),
                            );
                            held.superseded.push(current);
                        }
                    }
                    (owned.take_kept().into_iter().collect(), Ok(()))
                }
                // Only this seat's earlier selection is replaced: another
                // seat keeps the source it reads.
                (WriteEffect::SetFresh, Some(source)) => {
                    let input = input.expect("BUG: a new source is set only under an input");
                    let previous = owned
                        .set(
                            input.seat.clone(),
                            Owned::seated(text, source, input.serial),
                        )
                        .collect::<Vec<Owned>>();
                    (previous, Ok(()))
                }
                (WriteEffect::SetOver | WriteEffect::SetFresh, None) => {
                    unreachable!("BUG: a write sets a new source only once it is made")
                }
            }
        });
        // Off the owner thread no write can reach the selections.
        let (previous, written) =
            outcome.unwrap_or_else(|| (Vec::new(), Err(ClipboardError::Gone)));
        previous.into_iter().for_each(Owned::release);
        match written {
            Err(ClipboardError::ApplicationOnly) => tracing::debug!(
                "no data device to set the selection on: the text is kept for the application"
            ),
            Err(ClipboardError::Unavailable) => {
                tracing::debug!("no input in focus to set the selection under: nothing was kept");
            }
            _ => {}
        }
        // A retired owner has nobody left to answer.
        let _ = answer.resolve(written);
    }
}

/// What a write under the last input does with the seat's selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WriteEffect {
    /// The source set under this serial answers the new text in place.
    ReplaceInPlace,
    /// The new source is set over the one set under this serial.
    SetOver,
    /// The new source is the seat's selection, set under this serial.
    SetFresh,
    /// No device to set it on, so no system clipboard on this seat: the
    /// text is kept for the application (`ApplicationOnly`).
    KeepForApplication,
    /// No serial to set it under: refused, nothing kept.
    Refuse(ClipboardError),
}

/// What a write does, from what it found: whether an input in focus gives
/// it a serial (`has_input`), the hint offered by the source set under the
/// input's serial, if any (`held_hint`), whether the write is concealed,
/// and whether a new source was made and set (`fresh_set`: only no data
/// device manager or no device on the seat stops that). A write answers
/// `Ok` only where the clipboard serves what it asked: a concealed write is
/// never served by a source that offers no hint, while a plain write a
/// hinted source serves is the plain text, marked secret.
fn write_effect(
    has_input: bool,
    held_hint: Option<bool>,
    concealed: bool,
    fresh_set: bool,
) -> WriteEffect {
    if !has_input {
        return WriteEffect::Refuse(ClipboardError::Unavailable);
    }
    match (held_hint, fresh_set) {
        (Some(hint), _) if hint == concealed => WriteEffect::ReplaceInPlace,
        (Some(_), true) => WriteEffect::SetOver,
        (None, true) => WriteEffect::SetFresh,
        (Some(true), false) if !concealed => WriteEffect::ReplaceInPlace,
        (_, false) => WriteEffect::KeepForApplication,
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

/// The form a read asks `offer` for, with the MIME type its owner offered
/// it under first: an owner may match what it is asked for exactly.
fn preferred(offer: &OfferData) -> Option<(TextForm, String)> {
    offer
        .types(|types| {
            let form = TextForm::preferred(types.iter().map(String::as_str))?;
            let mime = types
                .iter()
                .find(|mime| TextForm::named(mime) == Some(form))?;
            Some((form, mime.clone()))
        })
        .flatten()
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

    /// Whether the source can answer a write `concealed` or not: a concealed
    /// one only while it offers the hint, or a clipboard manager would take
    /// it for an ordinary one.
    fn carries(&self, concealed: bool) -> bool {
        self.offers_hint || !concealed
    }

    /// The text the source answers with.
    fn text(&self) -> Option<Arc<str>> {
        self.content.with(|content| Arc::clone(&content.text))
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

// ---- The dispatch of the clipboard's objects on the owner's queue: each
// records what the compositor said, and runs no handler.

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
                        pipe::write(fd, &bytes, Instant::now() + TRANSFER_TIMEOUT);
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
                            .find_mut(|owned| owned.superseded.contains(source))
                        {
                            held.superseded.retain(|held| held != source);
                            return;
                        }
                        // Only this source's seat lost its selection; it is
                        // destroyed below, not by the entry's release, and the
                        // sources it superseded go with it.
                        let lost = state
                            .owned
                            .remove(|owned| owned.source.as_ref() == Some(source));
                        for superseded in lost.into_iter().flat_map(|lost| lost.superseded) {
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
        // it kept this source and dropped every one set over it, which no
        // event reports. The selection's text is what the kept source
        // answers: a concealed write it cannot carry was not put on the
        // clipboard.
        let dropped_write = shared.state.with(|state| {
            let held = state
                .owned
                .find_mut(|owned| owned.superseded.contains(&check.superseded))?;
            let at = held
                .superseded
                .iter()
                .position(|source| *source == check.superseded)?;
            let mut sources = std::mem::take(&mut held.superseded);
            let kept = sources.remove(at);
            let replaced = held.source.replace(kept.clone());
            for dropped in sources.into_iter().chain(replaced) {
                dropped.destroy();
            }
            let text = kept.data::<SourceData>().and_then(SourceData::text)?;
            let dropped_write = !Arc::ptr_eq(&text, &held.text);
            held.text = text;
            Some(dropped_write)
        });
        // The subscriber runs with the clipboard's state let go.
        if dropped_write.flatten() == Some(true) {
            tracing::warn!(
                target: "flui.clipboard.wayland",
                "the compositor kept the selection set earlier under this input: the concealed \
                 write made since was not put on the clipboard"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A write answers `Ok` only where the clipboard serves what it asked,
    /// and one no device can take is kept for the application rather than
    /// refused: a concealed write under the serial of a plain selection
    /// whose new source could not be set is kept, never served unmarked by
    /// the plain source, and a write on a seat with no data device, or under
    /// a compositor with no data device manager, is kept too.
    #[test]
    fn a_write_answers_ok_only_where_the_clipboard_serves_it() {
        use WriteEffect::*;
        for (held_hint, concealed, fresh_set, effect) in [
            (Some(false), false, false, ReplaceInPlace),
            (Some(true), true, false, ReplaceInPlace),
            (Some(false), true, true, SetOver),
            (Some(false), true, false, KeepForApplication),
            (Some(true), false, true, SetOver),
            (Some(true), false, false, ReplaceInPlace),
            (None, false, true, SetFresh),
            (None, true, true, SetFresh),
            (None, false, false, KeepForApplication),
            (None, true, false, KeepForApplication),
        ] {
            assert_eq!(
                write_effect(true, held_hint, concealed, fresh_set),
                effect,
                "{held_hint:?} {concealed} {fresh_set}"
            );
        }
        // With no input in focus, whatever else holds, the write is refused
        // `Unavailable` and nothing is kept: the window that holds the
        // keyboard again has a serial, and a selection another client set
        // meanwhile is what a paste reads.
        for (held_hint, concealed) in [(None, false), (None, true), (Some(false), true)] {
            assert_eq!(
                write_effect(false, held_hint, concealed, false),
                Refuse(ClipboardError::Unavailable)
            );
        }
    }

    /// A read asks for the text form [`TextForm::preferred`] picks, under the
    /// spelling the owner offered first, whatever else it offers.
    #[test]
    fn a_read_asks_for_the_spelling_offered_first() {
        let data = OfferData::new();
        for mime in [
            "image/png",
            "UTF8_STRING",
            "text/plain;charset=UTF-8",
            "text/plain;charset=utf-8",
        ] {
            data.offer(mime.to_owned());
        }
        assert_eq!(
            preferred(&data),
            Some((TextForm::PlainUtf8, "text/plain;charset=UTF-8".to_owned()))
        );
        assert_eq!(preferred(&OfferData::new()), None);
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
            assert_eq!(data.carries(concealed), offered_hint || !concealed, "{row}");
        }
    }
}
