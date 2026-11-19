//! The clipboard thread: the `CLIPBOARD` and `PRIMARY` selections as ICCCM
//! §2 has an owner and a requestor exchange them.
//!
//! - **Owning.** A write takes a timestamp from the server first (a
//!   zero-length append to one of the window's own properties, whose
//!   `PropertyNotify` carries the time), then sets the window as the
//!   selection's owner at that time and checks that it took. The selection
//!   is held until another client takes it (`SelectionClear`).
//! - **Answering.** A `SelectionRequest` for `TARGETS`, `TIMESTAMP` or a
//!   text form is answered by writing the requestor's property and sending
//!   it `SelectionNotify`; anything else, a request older than the
//!   ownership, or a `MULTIPLE`, is refused with property `None`. Text
//!   larger than one request goes `INCR`: the property says how much is
//!   coming, and each time the requestor deletes it the next chunk is
//!   written, a zero-length one last. At most [`MAX_WRITERS`] such
//!   transfers run at once, each given [`TRANSFER_TIMEOUT`] between
//!   chunks.
//! - **A requestor that goes is not answered.** The server may hand a gone
//!   client's ids to the next client that connects, and a gone window's id
//!   to that client's window, which did not ask. A request answered in the
//!   step that heard it is answered before any later event is handled.
//!   Whatever outlives that step (a request waiting on the owner thread, an
//!   `INCR` transfer, which starts only once its requestor is heard) hears
//!   the requestor's window from then on, and the window's end drops it.
//!   The answers decided later are written at the end of the step, and on
//!   every exit of the thread's loop that leaves the connection working:
//!   first a round trip to the server, then every event read until its
//!   reply handled, then the answers, with no round trip among them. A
//!   window that ended before that round trip reached the server is
//!   therefore never answered, nor refused. A requestor already gone when
//!   the thread starts to hear it is not answered; one the server will not
//!   let the thread hear for another reason is refused. What X cannot tell
//!   apart is a window that ends, and whose id a new window takes, after
//!   the thread last met the server (the request itself, for an answer in
//!   the step that heard it; the round trip, for one decided later) and
//!   before the server carries out the write: only a server grab would
//!   close that.
//! - **The primary selection is lazy.** The application offers it with a
//!   closure the owner thread alone may call, so the thread holds only the
//!   offer's [`Generation`]. A request for its text waits while the thread
//!   asks the owner through the [`Notice`], which wakes the owner's loop; at
//!   its turn the loop asks the closure and sends the text back
//!   ([`Command::Answer`]), and every request waiting on that offer is
//!   answered with it. While [`MAX_WRITERS`] transfers run, a request is
//!   refused without asking the closure. One that waits past
//!   [`TRANSFER_TIMEOUT`], or whose offer stopped being the selection, is
//!   refused.
//! - **Clearing an expiring entry.** A clear waits for the write asked
//!   before it, then gives the clipboard up at the time the window took it,
//!   if the clipboard still holds the entry it names (ADR-0363): the server
//!   ignores the release once another client took the selection later, so
//!   no copy made since is lost.
//! - **Reading.** While the application owns the clipboard a read answers
//!   its text at once; a read sent after a write waits for the write to own
//!   it. Otherwise the thread takes a server timestamp, as a write does, and
//!   asks the owner to convert the selection at that time into one of its
//!   window's properties, in each of [`READ_ORDER`] until one is answered.
//!   It reads the property, deleting it, which for `INCR` asks for the next
//!   chunk. An answer counts only if it names the read's time, form and
//!   property, and a read that times out retires its property, so an owner
//!   that answers late never answers a later read. Reads that arrive
//!   meanwhile join the one under way. A read gets [`TRANSFER_TIMEOUT`] in
//!   all and at most [`MAX_TEXT`] bytes. The primary selection is read so
//!   too when the application's offer is not the selection; while it is,
//!   the owner thread asks its closure itself and sends no read.

use std::cell::OnceCell;
use std::collections::VecDeque;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_platform::{
    ClipboardEntryId, ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten,
    capability::Resolver,
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use web_time::Instant;
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
    GetPropertyReply, PropMode, Property, SelectionClearEvent, SelectionNotifyEvent,
    SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use crate::clipboard::linux::MAX_READERS;
use crate::clipboard::text::{CONCEALED_HINT, CONCEALED_HINT_VALUE, TextForm};
use crate::host::{ExpiredClear, ExpiredEntry, MAX_TEXT, admit_text, owned_text};
use crate::owner::linux::x11::{WindowSelection, select_checked};

/// How long a transfer may wait for its peer: a whole read, the next chunk
/// of an `INCR` write, or the owner thread's text for the primary
/// selection.
pub(super) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);

/// The most `INCR` writes at once: a requestor that never deletes its
/// property holds one until its timeout, and more are refused.
pub(super) const MAX_WRITERS: usize = 4;

/// The most bytes of text the `INCR` writes hold at once, each shared text
/// counted once: room for the text owned now and one an ended ownership
/// left, which a transfer keeps writing (ICCCM §2.2). Each encodes its form
/// a chunk at a time, so the texts are all it holds.
const TRANSFER_BUDGET: usize = 2 * MAX_TEXT;

/// The most requests waiting on the owner thread for the primary
/// selection's text, counted with those whose answer waits for the end of
/// the step; more are refused.
const MAX_ASKS: usize = 16;

/// What the thread hears of a requestor's window while a transfer writes to
/// it: the deletions of its property, each asking for the next chunk, and
/// its end.
fn transfer_events() -> EventMask {
    EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY
}

/// The largest chunk written in one request, below the server's request
/// limit whatever it is.
const MAX_CHUNK: usize = 256 * 1024;

/// How many properties a read is converted into in turn.
const DESTINATIONS: usize = 4;

/// The forms a read asks for, in order: UTF-8 under X11's name first, which
/// every toolkit answers, then the MIME name, then ISO 8859-1.
const READ_ORDER: [TextForm; 3] = [TextForm::Utf8String, TextForm::PlainUtf8, TextForm::Latin1];

/// One of the two selections the thread speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Name {
    /// `CLIPBOARD`, which a copy writes.
    Clipboard,
    /// `PRIMARY`, the text selected last.
    Primary,
}

/// An offer of the application's primary selection, numbered by the owner
/// thread in the order it made them, from 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Generation(u64);

impl Generation {
    /// The offer after this one.
    pub(super) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("BUG: an application makes fewer than 2^64 offers"),
        )
    }
}

/// What the handle asks of the thread.
pub(super) enum Command {
    /// Own the clipboard with this write, and answer whether it owned.
    Write(ClipboardWrite, Owed<()>),
    /// Own the primary selection with the application's offer, whose text
    /// the owner thread answers.
    Offer(Generation),
    /// Let go of the primary selection if this offer holds it.
    Withdraw(Generation),
    /// The owner thread's text for an offer the thread asked about; `None`
    /// when the offer is no longer the application's or holds no text.
    Answer(Generation, Option<String>),
    /// Answer with the selection's text.
    Read(Name, Owed<Option<Arc<str>>>),
    /// Let go of the clipboard if it still holds this expiring entry, once
    /// the write asked before it owned or failed (ADR-0363).
    ClearExpired(ExpiredEntry, OwedClear),
    /// Let go of everything and end.
    Stop,
}

/// The answer a conditional clear owes its handle: taken to settle, or
/// [`ExpiredClear::Failed`] with [`ClipboardError::Gone`] when the command
/// drops untaken, as [`Owed`] does for a read or write.
pub(super) struct OwedClear(Option<Resolver<ExpiredClear>>);

impl OwedClear {
    /// The answer `resolver` owes.
    pub(super) fn new(resolver: Resolver<ExpiredClear>) -> Self {
        Self(Some(resolver))
    }

    /// Settle with `cleared`. A retired owner has nobody left to answer.
    fn settle(mut self, cleared: ExpiredClear) {
        if let Some(resolver) = self.0.take() {
            let _ = resolver.resolve(cleared);
        }
    }
}

impl Drop for OwedClear {
    fn drop(&mut self) {
        if let Some(resolver) = self.0.take() {
            // A retired owner has nobody left to answer.
            let _ = resolver.resolve(ExpiredClear::Failed(ClipboardError::Gone));
        }
    }
}

/// The answer a read or write command owes its handle. The thread takes the
/// resolver to settle; a command that drops untaken (sent to a thread that
/// has ended, or left in its queue as it ends) answers
/// [`ClipboardError::Gone`], so no exit leaves it without a typed answer.
pub(super) struct Owed<V: 'static>(Option<Resolver<Result<V, ClipboardError>>>);

impl<V: 'static> Owed<V> {
    /// The answer `resolver` owes.
    pub(super) fn new(resolver: Resolver<Result<V, ClipboardError>>) -> Self {
        Self(Some(resolver))
    }

    /// The resolver, which the thread settles from here.
    fn take(mut self) -> Resolver<Result<V, ClipboardError>> {
        self.0
            .take()
            .expect("BUG: an owed answer is taken once, as its command is carried out")
    }
}

impl<V: 'static> Drop for Owed<V> {
    fn drop(&mut self) {
        if let Some(resolver) = self.0.take() {
            // A retired owner has nobody left to answer.
            let _ = resolver.resolve(Err(ClipboardError::Gone));
        }
    }
}

/// What the thread tells the owner thread about its primary selection, and
/// the wake that has the owner's loop read it.
pub(super) struct Notice {
    state: parking_lot::Mutex<NoticeState>,
    wake: Box<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct NoticeState {
    /// The offer whose text a requestor waits for.
    asked: Option<Generation>,
    /// The newest offer another client took the selection from, or that
    /// never became the selection.
    lost: Generation,
    /// The thread ended: its connection closed, and with it every
    /// selection it owned, so no offer is the selection any more.
    ended: bool,
}

impl Notice {
    /// A notice that calls `wake` for the owner's loop to read it.
    pub(super) fn new(wake: Box<dyn Fn() + Send + Sync>) -> Self {
        Self {
            state: parking_lot::Mutex::new(NoticeState::default()),
            wake,
        }
    }

    /// A requestor waits for `offer`'s text.
    fn ask(&self, offer: Generation) {
        self.state.lock().asked = Some(offer);
        (self.wake)();
    }

    /// `offer` is not the selection any more.
    fn lose(&self, offer: Generation) {
        {
            let mut state = self.state.lock();
            state.lost = state.lost.max(offer);
        }
        (self.wake)();
    }

    /// The thread ended: no offer, made or to come, is the selection.
    fn end(&self) {
        {
            let mut state = self.state.lock();
            state.ended = true;
            state.asked = None;
        }
        (self.wake)();
    }

    /// The offer a requestor waits for, once.
    pub(super) fn take_asked(&self) -> Option<Generation> {
        self.state.lock().asked.take()
    }

    /// Whether `offer` may still be the selection: no client took it, and
    /// the thread that owns it still runs.
    pub(super) fn holds(&self, offer: Generation) -> bool {
        let state = self.state.lock();
        !state.ended && offer > state.lost
    }
}

/// Intern `names`, every request sent before the first reply is read.
fn intern<const N: usize>(
    connection: &RustConnection,
    names: [&[u8]; N],
) -> Result<[xproto::Atom; N], ReplyOrIdError> {
    let cookies = names
        .map(|name| connection.intern_atom(false, name))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let atoms = cookies
        .into_iter()
        .map(|cookie| Ok(cookie.reply()?.atom))
        .collect::<Result<Vec<xproto::Atom>, ReplyOrIdError>>()?;
    Ok(<[xproto::Atom; N]>::try_from(atoms)
        .unwrap_or_else(|_| unreachable!("BUG: one atom is interned for each name")))
}

/// The atoms both selections speak.
struct Atoms {
    targets: xproto::Atom,
    timestamp: xproto::Atom,
    incr: xproto::Atom,
    text: xproto::Atom,
    /// [`CONCEALED_HINT`], offered with a concealed write's text.
    concealed_hint: xproto::Atom,
    /// Each of [`TextForm::OFFERED`], by its position there.
    forms: [xproto::Atom; 4],
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, ReplyOrIdError> {
        let [targets, timestamp, incr, text, concealed_hint] = intern(
            connection,
            [
                b"TARGETS",
                b"TIMESTAMP",
                b"INCR",
                b"TEXT",
                CONCEALED_HINT.as_bytes(),
            ],
        )?;
        let forms = intern(
            connection,
            TextForm::OFFERED.map(|form| form.name().as_bytes()),
        )?;
        Ok(Self {
            targets,
            timestamp,
            incr,
            text,
            concealed_hint,
            forms,
        })
    }

    fn form(&self, form: TextForm) -> xproto::Atom {
        let at = TextForm::OFFERED
            .iter()
            .position(|offered| *offered == form)
            .expect("BUG: every form is offered");
        self.forms[at]
    }

    /// The form a target names; `TEXT` is answered as UTF-8.
    fn form_of(&self, target: xproto::Atom) -> Option<TextForm> {
        if target == self.text {
            return Some(TextForm::Utf8String);
        }
        let at = self.forms.iter().position(|atom| *atom == target)?;
        Some(TextForm::OFFERED[at])
    }
}

/// What the application owns a selection with.
enum Content {
    /// The clipboard's text.
    Text(Encodings),
    /// The primary selection's offer, whose text the owner thread answers.
    Offered(Generation),
}

/// Text for every transfer to share: each encodes its own form a chunk at a
/// time, so no form of it is ever held whole besides the text.
struct Encodings {
    text: Arc<str>,
    /// Whether the write was concealed: [`CONCEALED_HINT`] is offered too.
    concealed: bool,
    /// The entry an expiring write makes, which a conditional clear names.
    entry: Option<ClipboardEntryId>,
    /// The text's length in ISO 8859-1, counted once: each request for the
    /// form would otherwise scan the whole text again.
    latin1_len: OnceCell<usize>,
}

impl Encodings {
    fn new(write: ClipboardWrite) -> Self {
        Self {
            concealed: write.is_concealed(),
            entry: write.entry(),
            text: Arc::<str>::from(write.into_text()),
            latin1_len: OnceCell::new(),
        }
    }

    /// How many bytes the text takes in `form`.
    fn len_in(&self, form: TextForm) -> usize {
        match form {
            TextForm::Latin1 => *self.latin1_len.get_or_init(|| form.encoded_len(&self.text)),
            TextForm::PlainUtf8 | TextForm::Utf8String | TextForm::Plain => {
                form.encoded_len(&self.text)
            }
        }
    }
}

/// What the application owns a selection with, and since when.
struct Owned {
    content: Content,
    time: xproto::Timestamp,
}

impl Owned {
    /// The clipboard's text and its length in `form`; `None` for an offer,
    /// whose text the owner thread answers.
    fn text_in(&self, form: TextForm) -> Option<(Arc<str>, usize)> {
        match &self.content {
            Content::Text(encodings) => Some((Arc::clone(&encodings.text), encodings.len_in(form))),
            Content::Offered(_) => None,
        }
    }
}

/// An `INCR` write: the requestor's property, filled a chunk at a time.
struct Transfer {
    requestor: xproto::Window,
    property: xproto::Atom,
    kind: xproto::Atom,
    form: TextForm,
    text: Arc<str>,
    /// The byte of `text` the next chunk starts at.
    written: usize,
    /// Whether the zero-length chunk that ends it is written.
    ended: bool,
    deadline: Instant,
}

/// A request for the primary selection's text, waiting on the owner
/// thread.
struct Ask {
    request: SelectionRequestEvent,
    property: xproto::Atom,
    form: TextForm,
    offer: Generation,
    deadline: Instant,
}

/// A request whose answer is decided but written later, at the end of the
/// thread's step ([`Selections::write_decided`]): one that waited on the
/// owner thread, or text that goes `INCR` to a requestor the thread had to
/// start hearing first.
struct Decided {
    request: SelectionRequestEvent,
    property: xproto::Atom,
    form: TextForm,
    /// The text and its length in `form`; `None` refuses the request.
    text: Option<(Arc<str>, usize)>,
}

impl Decided {
    /// `ask`, answered with `text`.
    fn answer(ask: Ask, text: Option<(Arc<str>, usize)>) -> Self {
        Self {
            request: ask.request,
            property: ask.property,
            form: ask.form,
            text,
        }
    }
}

/// The read under way: the form asked for and what came so far.
struct Reading {
    form: TextForm,
    /// The server's time the read asks at; `None` until it arrives.
    time: Option<xproto::Timestamp>,
    /// The bytes of an `INCR` read, once the owner announced one.
    incremental: Option<Vec<u8>>,
    deadline: Instant,
}

/// One selection: what the application owns it with, and the reads of it.
struct Selection {
    atom: xproto::Atom,
    /// The window's property a write appends to for a timestamp.
    clock: xproto::Atom,
    /// The window's property a read appends to for a timestamp, which tells
    /// its answers from a late answer to an earlier read.
    read_clock: xproto::Atom,
    /// The window's properties a read is converted into, one at a time: a
    /// read that timed out retires its property, so an owner that answers
    /// it late writes where no read looks.
    destinations: [xproto::Atom; DESTINATIONS],
    owned: Option<Owned>,
    /// A write waiting for its timestamp; a newer write replaces it.
    pending: Option<Content>,
    /// The answers owed to the clipboard writes waiting for their timestamp:
    /// the pending one's, and those of the writes it replaced, which the
    /// selection then holds the application's newer text for.
    written: Vec<Resolver<ClipboardWritten>>,
    /// The writes' clock appends the server has not stamped yet: only the
    /// last one's time is the pending write's.
    clocks: usize,
    /// The reads waiting for the read under way, or for one to start.
    readers: Vec<Resolver<ClipboardRead>>,
    /// The reads sent after a pending write: they answer what the write
    /// leaves, so they join no read that began before it.
    after_write: Vec<Resolver<ClipboardRead>>,
    reading: Option<Reading>,
    /// Which of [`Self::destinations`] the reads are converted into now.
    destination: usize,
}

impl Selection {
    /// Selection `atom`, whose properties on the window are named after
    /// `prefix`.
    fn try_new(
        connection: &RustConnection,
        atom: xproto::Atom,
        prefix: &str,
    ) -> Result<Self, ReplyOrIdError> {
        let names = ["CLOCK", "READ_CLOCK", "0", "1", "2", "3"]
            .map(|suffix| format!("FLUI_{prefix}_{suffix}"));
        let [clock, read_clock, first, second, third, fourth] =
            intern(connection, names.each_ref().map(String::as_bytes))?;
        Ok(Self {
            atom,
            clock,
            read_clock,
            destinations: [first, second, third, fourth],
            owned: None,
            pending: None,
            written: Vec::new(),
            clocks: 0,
            readers: Vec::new(),
            after_write: Vec::new(),
            reading: None,
            destination: 0,
        })
    }

    /// The offer the application owns the selection with, if any.
    fn offered(&self) -> Option<Generation> {
        match self.owned {
            Some(Owned {
                content: Content::Offered(offer),
                ..
            }) => Some(offer),
            _ => None,
        }
    }

    /// Answer the waiting reads: with the application's text while it owns
    /// the clipboard, otherwise through the read under way or a new one.
    fn serve(
        &mut self,
        connection: &RustConnection,
        window: xproto::Window,
    ) -> Result<(), ReplyOrIdError> {
        if self.readers.is_empty() {
            return Ok(());
        }
        if let Some(owned) = &self.owned {
            // The owner thread reads its own offer itself: a read that
            // reaches the thread found the offer gone, and the application
            // holds no text.
            let text = match &owned.content {
                Content::Text(encodings) => owned_text(&encodings.text),
                Content::Offered(_) => Ok(None),
            };
            if self.reading.is_some() {
                self.retire_destination();
            }
            self.answer(text);
            return Ok(());
        }
        if self.reading.is_none() {
            self.reading = Some(Reading {
                form: READ_ORDER[0],
                time: None,
                incremental: None,
                deadline: Instant::now() + TRANSFER_TIMEOUT,
            });
            // The server stamps the property's change with its time.
            connection.change_property8(
                PropMode::APPEND,
                window,
                self.read_clock,
                AtomEnum::INTEGER,
                &[],
            )?;
        }
        Ok(())
    }

    /// The property reads are converted into now.
    fn destination(&self) -> xproto::Atom {
        self.destinations[self.destination]
    }

    /// Leave the read under way unanswered by the owner: the next read is
    /// converted into another property, so an answer to this one written
    /// late lands where no read looks.
    fn retire_destination(&mut self) {
        self.reading = None;
        self.destination = (self.destination + 1) % DESTINATIONS;
    }

    /// Ask the owner for the selection as the read's form, at the read's
    /// time.
    fn convert(
        &self,
        connection: &RustConnection,
        window: xproto::Window,
        atoms: &Atoms,
    ) -> Result<(), ReplyOrIdError> {
        let Some(Reading {
            form,
            time: Some(time),
            ..
        }) = self.reading
        else {
            return Ok(());
        };
        connection.convert_selection(
            window,
            self.atom,
            atoms.form(form),
            self.destination(),
            time,
        )?;
        Ok(())
    }

    /// End the read under way, answering every reader with `text`, one
    /// shared copy of it. A delivery that panics (the owner's wake hook)
    /// leaves the others answered, and the first failure goes on.
    fn answer(&mut self, text: ClipboardRead) {
        self.reading = None;
        let mut failure = FirstPanic::new();
        for reader in std::mem::take(&mut self.readers) {
            // A retired owner has nobody left to answer.
            failure.contain(|| reader.resolve(text.clone()));
        }
        failure.resume();
    }

    /// The pending write owned the selection or lost it: answer every write
    /// waiting for it. A delivery that panics leaves the others answered,
    /// and the first failure goes on.
    fn answer_writes(&mut self, written: ClipboardWritten) {
        let mut failure = FirstPanic::new();
        for answer in std::mem::take(&mut self.written) {
            // A retired owner has nobody left to answer.
            failure.contain(|| answer.resolve(written));
        }
        failure.resume();
    }

    /// The thread ends: every read and write still waiting is answered
    /// [`ClipboardError::Gone`], each delivery contained, so one that panics
    /// leaves the others answered too, and the first failure goes on.
    fn abandon(&mut self) {
        let mut failure = FirstPanic::new();
        let waiting = std::mem::take(&mut self.readers);
        for reader in waiting
            .into_iter()
            .chain(std::mem::take(&mut self.after_write))
        {
            // A retired owner has nobody left to answer.
            failure.contain(|| {
                let _ = reader.resolve(Err(ClipboardError::Gone));
            });
        }
        for answer in std::mem::take(&mut self.written) {
            failure.contain(|| {
                let _ = answer.resolve(Err(ClipboardError::Gone));
            });
        }
        failure.resume();
    }

    /// The read's timestamp arrived: ask for the conversion at it.
    fn stamped(
        &mut self,
        connection: &RustConnection,
        window: xproto::Window,
        atoms: &Atoms,
        time: xproto::Timestamp,
    ) -> Result<(), ReplyOrIdError> {
        if let Some(reading) = &mut self.reading
            && reading.time.is_none()
        {
            reading.time = Some(time);
            self.convert(connection, window, atoms)?;
        }
        Ok(())
    }

    /// The owner answered a conversion: read its property, or ask for the
    /// next form if it refused.
    fn converted(
        &mut self,
        connection: &RustConnection,
        window: xproto::Window,
        atoms: &Atoms,
        event: &SelectionNotifyEvent,
    ) -> Result<(), ReplyOrIdError> {
        let destination = self.destination();
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        // An answer to this read names its form, its time (an owner that
        // ignores the time sends the server's current one) and its property.
        let ours = reading
            .time
            .is_some_and(|time| event.time == time || event.time == CURRENT_TIME)
            && event.target == atoms.form(reading.form)
            && (event.property == NONE || event.property == destination);
        if !ours {
            return Ok(());
        }
        if event.property == NONE {
            return self.ask_next_form(connection, window, atoms);
        }
        let form = reading.form;
        let reply = self.take_property(connection, window, MAX_TEXT)?;
        if reply.type_ == atoms.incr {
            // The owner says how much is coming, at least: room for that is
            // taken once, so honest chunks land without growing the buffer.
            let announced = reply
                .value32()
                .and_then(|mut value| value.next())
                .and_then(|size| usize::try_from(size).ok())
                .unwrap_or(0);
            // The header is let go before the room is taken: an owner may pad
            // it to the read's limit.
            drop(reply);
            if let Err(refused) = admit_text(announced) {
                // Deleting the property asked for the first chunk: it lands
                // where no read looks.
                self.retire_destination();
                self.answer(Err(refused));
                return Ok(());
            }
            // Deleting the property asked for the first chunk.
            if let Some(reading) = &mut self.reading {
                reading.incremental = Some(Vec::with_capacity(announced));
            }
            return Ok(());
        }
        if !is_form(&reply, atoms.form(form)) {
            // The owner answered, but not with the text asked for: bytes of
            // another type or width are not that text, whatever they decode
            // to.
            return self.ask_next_form(connection, window, atoms);
        }
        let text = if reply.bytes_after == 0 {
            form.answer(reply.value)
        } else {
            Err(ClipboardError::TooLarge)
        };
        self.answer(text);
        Ok(())
    }

    /// The owner did not give the form asked for (it refused, or answered
    /// with something else): ask for the next of [`READ_ORDER`], or answer
    /// that the selection holds no text once each was asked.
    fn ask_next_form(
        &mut self,
        connection: &RustConnection,
        window: xproto::Window,
        atoms: &Atoms,
    ) -> Result<(), ReplyOrIdError> {
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        let next = READ_ORDER
            .iter()
            .skip_while(|form| **form != reading.form)
            .nth(1);
        let Some(&form) = next else {
            self.answer(Ok(None));
            return Ok(());
        };
        reading.form = form;
        reading.incremental = None;
        self.convert(connection, window, atoms)
    }

    /// The owner wrote an `INCR` chunk: read at most what the read has left
    /// of [`MAX_TEXT`], the first chunk kept as it came.
    fn read_chunk(
        &mut self,
        connection: &RustConnection,
        window: xproto::Window,
        atoms: &Atoms,
    ) -> Result<(), ReplyOrIdError> {
        let Some(received) = self
            .reading
            .as_ref()
            .and_then(|reading| reading.incremental.as_ref())
            .map(Vec::len)
        else {
            return Ok(());
        };
        let reply = self.take_property(connection, window, MAX_TEXT.saturating_sub(received))?;
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        let Some(bytes) = &mut reading.incremental else {
            return Ok(());
        };
        if !reply.value.is_empty() && !is_form(&reply, atoms.form(reading.form)) {
            // A chunk that is not the text asked for: the owner's transfer
            // goes on where no read looks, and the next form is asked.
            self.destination = (self.destination + 1) % DESTINATIONS;
            return self.ask_next_form(connection, window, atoms);
        }
        // A chunk past what the read has left reads as empty with bytes after
        // it, which is not the end of the transfer.
        let admitted = admit_text(bytes.len() + reply.value.len());
        if admitted.is_err() || reply.bytes_after != 0 {
            // The owner's transfer goes on: its chunks land where no read looks.
            self.retire_destination();
            self.answer(Err(ClipboardError::TooLarge));
        } else if reply.value.is_empty() {
            let form = reading.form;
            let bytes = std::mem::take(bytes);
            self.answer(form.answer(bytes));
        } else if bytes.capacity() == 0 {
            // Nothing announced: the first chunk is kept as it came.
            *bytes = reply.value;
        } else {
            append(bytes, &reply.value);
        }
        Ok(())
    }

    /// Read and delete the window's destination property, at most `limit`
    /// bytes of it, rounded up to whole 32-bit units; a longer value says
    /// so in `bytes_after`.
    fn take_property(
        &self,
        connection: &RustConnection,
        window: xproto::Window,
        limit: usize,
    ) -> Result<GetPropertyReply, ReplyOrIdError> {
        let longs = u32::try_from(limit.div_ceil(4)).expect("BUG: the text limit fits a request");
        Ok(connection
            .get_property(true, window, self.destination(), AtomEnum::ANY, 0, longs)?
            .reply()?)
    }

    /// End the read under way if it waited past `now`.
    fn expire(&mut self, now: Instant) {
        if self
            .reading
            .as_ref()
            .is_some_and(|reading| reading.deadline <= now)
        {
            tracing::debug!("an X11 selection read timed out");
            self.retire_destination();
            self.answer(Err(ClipboardError::Timeout));
        }
    }
}

/// Whether `reply` holds text in the form whose target is `form`: bytes,
/// typed as that form, as an owner writes its answer to a conversion (ICCCM
/// §2.6.2). A property of another type or width (a list of atoms, an image)
/// is not text in that form, though its bytes would decode as some.
fn is_form(reply: &GetPropertyReply, form: xproto::Atom) -> bool {
    reply.format == 8 && reply.type_ == form
}

/// What the requestor is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// Its property holds the answer.
    Written,
    /// Refused, with property `None`.
    Refused,
}

/// When a request is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Now.
    Now(Reply),
    /// At the end of the step, or once the owner thread sends the text.
    Later,
    /// Never: the requestor's window is gone, and the window that holds its
    /// id next did not ask.
    Never,
}

/// Whether the thread hears a requestor's window, which anything that
/// waits on the requestor needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watched {
    /// It does: the window's end will be heard.
    Heard,
    /// The window is gone (`BadWindow`): its request is never answered.
    Gone,
    /// The server refused otherwise (an allocation failure, a security
    /// policy): the requestor is there and waits, so it is refused.
    Unheard,
}

impl Watched {
    /// What selecting on the requestor's window came to.
    fn of(selection: WindowSelection) -> Result<Self, ReplyOrIdError> {
        match selection {
            WindowSelection::Selected => Ok(Self::Heard),
            WindowSelection::Gone => Ok(Self::Gone),
            WindowSelection::Failed(ReplyError::X11Error(_)) => Ok(Self::Unheard),
            WindowSelection::Failed(ReplyError::ConnectionError(error)) => Err(error.into()),
        }
    }

    /// How a request is answered now because its requestor cannot be heard;
    /// `None` when it is heard and may wait.
    fn unheard_answer(self) -> Option<Answer> {
        match self {
            Self::Heard => None,
            Self::Gone => Some(Answer::Never),
            Self::Unheard => Some(Answer::Now(Reply::Refused)),
        }
    }
}

/// The clipboard thread's state over its connection.
pub(super) struct Selections {
    connection: RustConnection,
    window: xproto::Window,
    atoms: Atoms,
    clipboard: Selection,
    primary: Selection,
    /// What the owner thread hears of the primary selection.
    notice: Arc<Notice>,
    /// The requests waiting on the owner thread for the primary
    /// selection's text.
    asks: Vec<Ask>,
    /// The requests that waited, whose answers are written at the end of
    /// the step.
    decided: VecDeque<Decided>,
    /// The conditional clears asked while a clipboard write waits for its
    /// timestamp: served once it owned or failed, in order.
    clears: Vec<(ExpiredEntry, OwedClear)>,
    transfers: Vec<Transfer>,
    chunk: usize,
}

impl Selections {
    /// Make the selection window on the root of `screen`, which hears its
    /// own properties change.
    pub(super) fn try_new(
        connection: RustConnection,
        screen: usize,
        notice: Arc<Notice>,
    ) -> Result<Self, ReplyOrIdError> {
        let root = connection
            .setup()
            .roots
            .get(screen)
            .map_or(NONE, |screen| screen.root);
        let window = connection.generate_id()?;
        connection.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        let atoms = Atoms::intern(&connection)?;
        let [clipboard] = intern(&connection, [b"CLIPBOARD"])?;
        let clipboard = Selection::try_new(&connection, clipboard, "CLIPBOARD")?;
        let primary = Selection::try_new(
            &connection,
            xproto::Atom::from(AtomEnum::PRIMARY),
            "PRIMARY",
        )?;
        let chunk = connection
            .maximum_request_bytes()
            .saturating_sub(64)
            .clamp(1, MAX_CHUNK);
        connection.flush()?;
        Ok(Self {
            connection,
            window,
            atoms,
            clipboard,
            primary,
            notice,
            asks: Vec::new(),
            decided: VecDeque::new(),
            clears: Vec::new(),
            transfers: Vec::new(),
            chunk,
        })
    }

    fn selection(&self, name: Name) -> &Selection {
        match name {
            Name::Clipboard => &self.clipboard,
            Name::Primary => &self.primary,
        }
    }

    /// The connection, the window and the atoms beside selection `name`,
    /// for a step of the selection's own.
    fn split(&mut self, name: Name) -> (&RustConnection, xproto::Window, &Atoms, &mut Selection) {
        let selection = match name {
            Name::Clipboard => &mut self.clipboard,
            Name::Primary => &mut self.primary,
        };
        (&self.connection, self.window, &self.atoms, selection)
    }

    /// The selection `atom` names, if it is one the thread speaks.
    fn named(&self, atom: xproto::Atom) -> Option<Name> {
        if atom == self.clipboard.atom {
            Some(Name::Clipboard)
        } else if atom == self.primary.atom {
            Some(Name::Primary)
        } else {
            None
        }
    }

    /// Carry out `command`; `false` once it asks the thread to end.
    fn command(&mut self, command: Command) -> Result<bool, ReplyOrIdError> {
        match command {
            Command::Write(write, answer) => {
                self.clipboard.written.push(answer.take());
                self.claim(Name::Clipboard, Content::Text(Encodings::new(write)))?;
            }
            Command::Offer(offer) => self.claim(Name::Primary, Content::Offered(offer))?,
            Command::Withdraw(offer) => self.withdraw(offer)?,
            Command::Answer(offer, text) => self.answered(offer, text),
            Command::Read(name, answer) => {
                let answer = answer.take();
                // Past a few handles' reads waiting, a read is refused rather
                // than queue behind them.
                let selection = self.split(name).3;
                if selection.readers.len() + selection.after_write.len() >= MAX_READERS {
                    // A retired owner has nobody left to answer. The read is
                    // settled before the diagnostic, which runs user code.
                    let _ = answer.resolve(Err(ClipboardError::Unavailable));
                    tracing::debug!("an X11 selection read past {MAX_READERS} waiting was refused");
                    return Ok(true);
                }
                // A read after a write answers what the write left: it waits
                // for the write to own the selection or fail to.
                if self.split(name).3.pending.is_some() {
                    self.split(name).3.after_write.push(answer);
                } else {
                    // A clear queued behind this read would answer it from
                    // what the window no longer owns.
                    if !self.still_owned(name)? {
                        self.lost(name);
                    }
                    let (connection, window, _, selection) = self.split(name);
                    selection.readers.push(answer);
                    selection.serve(connection, window)?;
                }
            }
            Command::ClearExpired(entry, answer) => {
                if self.clipboard.pending.is_some() {
                    self.clears.push((entry, answer));
                } else {
                    let cleared = self.clear_expired(entry)?;
                    answer.settle(cleared);
                }
            }
            Command::Stop => return Ok(false),
        }
        Ok(true)
    }

    /// Let go of the clipboard if it still holds the expiring entry `entry`
    /// names. The window gives up the selection at the time it took it:
    /// the server ignores a `SetSelectionOwner` at a time earlier than the
    /// selection's last change, so a client that took the clipboard since,
    /// even one whose `SelectionClear` has not reached the thread, keeps it.
    /// The check and the release are one step of the server.
    fn clear_expired(&mut self, entry: ExpiredEntry) -> Result<ExpiredClear, ReplyOrIdError> {
        let held = match &self.clipboard.owned {
            Some(Owned {
                content: Content::Text(encodings),
                time,
            }) => encodings.entry.map(|held| (held, *time)),
            _ => None,
        };
        let Some((_, time)) = held.filter(|(held, _)| entry.names(*held)) else {
            return Ok(ExpiredClear::NotHeld);
        };
        if !self.still_owned(Name::Clipboard)? {
            self.lost(Name::Clipboard);
            return Ok(ExpiredClear::NotHeld);
        }
        let (connection, _, _, clipboard) = self.split(Name::Clipboard);
        clipboard.owned = None;
        connection.set_selection_owner(NONE, clipboard.atom, time)?;
        connection.flush()?;
        Ok(ExpiredClear::Cleared)
    }

    /// The clipboard write a clear waited for owned or failed: serve the
    /// clears asked meanwhile, in order.
    fn serve_clears(&mut self) -> Result<(), ReplyOrIdError> {
        for (entry, answer) in std::mem::take(&mut self.clears) {
            let cleared = self.clear_expired(entry)?;
            answer.settle(cleared);
        }
        Ok(())
    }

    /// Own selection `name` with `content` once the server's time arrives.
    fn claim(&mut self, name: Name, content: Content) -> Result<(), ReplyOrIdError> {
        let (connection, window, _, selection) = self.split(name);
        selection.pending = Some(content);
        selection.clocks += 1;
        // The server stamps the property's change with its time.
        connection.change_property8(
            PropMode::APPEND,
            window,
            selection.clock,
            AtomEnum::INTEGER,
            &[],
        )?;
        Ok(())
    }

    /// The owner thread withdrew `offer`: let go of the primary selection
    /// if `offer` holds it, or never take it if `offer` waits to.
    fn withdraw(&mut self, offer: Generation) -> Result<(), ReplyOrIdError> {
        let (connection, window, _, primary) = self.split(Name::Primary);
        if matches!(primary.pending, Some(Content::Offered(pending)) if pending == offer) {
            primary.pending = None;
        }
        // An older offer the withdrawn one replaced is no longer the
        // application's either, even when its replacement never owned.
        if primary.offered().is_some_and(|owned| owned <= offer)
            && let Some(owned) = primary.owned.take()
        {
            connection.set_selection_owner(NONE, primary.atom, owned.time)?;
        }
        if primary.pending.is_none() {
            // The reads that waited for the withdrawn offer.
            primary.readers.append(&mut primary.after_write);
            primary.serve(connection, window)?;
        }
        self.settle_asks();
        Ok(())
    }

    fn event(&mut self, event: Event) -> Result<(), ReplyOrIdError> {
        match event {
            Event::PropertyNotify(event) => {
                if event.window == self.window {
                    for name in [Name::Clipboard, Name::Primary] {
                        let (connection, window, atoms, selection) = self.split(name);
                        if event.atom == selection.clock {
                            return self.own(name, event.time);
                        }
                        if event.atom == selection.read_clock {
                            return selection.stamped(connection, window, atoms, event.time);
                        }
                        if event.atom == selection.destination()
                            && event.state == Property::NEW_VALUE
                        {
                            return selection.read_chunk(connection, window, atoms);
                        }
                    }
                }
                if event.state == Property::DELETE {
                    self.write_chunk(event.window, event.atom)?;
                }
            }
            Event::SelectionNotify(event) => {
                if event.requestor == self.window
                    && let Some(name) = self.named(event.selection)
                {
                    let (connection, window, atoms, selection) = self.split(name);
                    selection.converted(connection, window, atoms, &event)?;
                }
            }
            Event::SelectionRequest(event) => self.request(&event)?,
            Event::SelectionClear(event) => self.cleared(&event)?,
            Event::DestroyNotify(event) => self.gone(event.window),
            _ => {}
        }
        Ok(())
    }

    /// A requestor's window went (its client crashed, was killed, or let it
    /// go): nothing more is written to it, since the window that takes its
    /// id next did not ask. Its transfers end now, their writers and text
    /// free for the next, and its requests waiting on the owner thread, or
    /// for their answer to be written, are dropped. Nothing is selected on a
    /// window that is not there.
    fn gone(&mut self, requestor: xproto::Window) {
        self.transfers
            .retain(|transfer| transfer.requestor != requestor);
        self.asks.retain(|ask| ask.request.requestor != requestor);
        self.decided
            .retain(|decided| decided.request.requestor != requestor);
    }

    /// The server's time arrived: own selection `name` with what waits.
    fn own(&mut self, name: Name, time: xproto::Timestamp) -> Result<(), ReplyOrIdError> {
        let (connection, window, _, selection) = self.split(name);
        // A stamp of a write a newer one replaced: the newer write takes the
        // stamp of its own append, which follows.
        selection.clocks = selection.clocks.saturating_sub(1);
        if selection.clocks > 0 {
            return Ok(());
        }
        let Some(content) = selection.pending.take() else {
            return Ok(());
        };
        connection.set_selection_owner(window, selection.atom, time)?;
        let owner = connection
            .get_selection_owner(selection.atom)?
            .reply()?
            .owner;
        let written = if owner == window {
            selection.owned = Some(Owned { content, time });
            Ok(())
        } else {
            // A client took it at a later time than ours.
            selection.owned = None;
            tracing::debug!("an X11 selection was taken before the write could own it");
            if let Content::Offered(offer) = content {
                self.notice.lose(offer);
            }
            Err(ClipboardError::Unavailable)
        };
        // The reads that waited for the write.
        let (connection, window, _, selection) = self.split(name);
        selection.readers.append(&mut selection.after_write);
        let served = selection.serve(connection, window);
        self.settle_asks();
        // The writes' answers go last: a delivery that panics leaves the
        // reads above served and the connection's state settled.
        self.split(name).3.answer_writes(written);
        served?;
        // The clears asked after the write are checked against what it left.
        if name == Name::Clipboard {
            self.serve_clears()?;
        }
        Ok(())
    }

    /// Another client took a selection the window owned, unless the window
    /// owns it again: a clear queued behind the write that took it back is
    /// stale.
    fn cleared(&mut self, event: &SelectionClearEvent) -> Result<(), ReplyOrIdError> {
        if event.owner != self.window {
            return Ok(());
        }
        let Some(name) = self.named(event.selection) else {
            return Ok(());
        };
        if !self.still_owned(name)? {
            self.lost(name);
        }
        Ok(())
    }

    /// Whether the window owns selection `name` as far as the server says,
    /// or owns nothing to lose.
    fn still_owned(&mut self, name: Name) -> Result<bool, ReplyOrIdError> {
        let (connection, window, _, selection) = self.split(name);
        if selection.owned.is_none() {
            return Ok(true);
        }
        let owner = connection
            .get_selection_owner(selection.atom)?
            .reply()?
            .owner;
        Ok(owner == window)
    }

    /// Another client took selection `name`: the ownership ends, and an
    /// offer that held it is lost.
    fn lost(&mut self, name: Name) {
        if let Some(Owned {
            content: Content::Offered(offer),
            ..
        }) = self.split(name).3.owned.take()
        {
            self.notice.lose(offer);
        }
        self.settle_asks();
    }

    /// Another client asks for a selection.
    fn request(&mut self, request: &SelectionRequestEvent) -> Result<(), ReplyOrIdError> {
        // An obsolete client names no property: the target is its name.
        let property = if request.property == NONE {
            request.target
        } else {
            request.property
        };
        match self.answer_request(request, property)? {
            Answer::Now(reply) => self.send_reply(request, property, reply),
            Answer::Later | Answer::Never => Ok(()),
        }
    }

    /// Tell `request`'s requestor `reply`.
    fn send_reply(
        &self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
        reply: Reply,
    ) -> Result<(), ReplyOrIdError> {
        match reply {
            Reply::Written => self.notify(request, property),
            Reply::Refused => self.notify(request, NONE),
        }
    }

    /// Tell `request`'s requestor its answer is in `property`, or with
    /// `NONE` that it is refused.
    fn notify(
        &self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
    ) -> Result<(), ReplyOrIdError> {
        let notify = SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property,
        };
        self.connection
            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
        Ok(())
    }

    /// Write the answer to `request` into the requestor's `property`, or
    /// hold it for the end of the step or the owner thread's text.
    fn answer_request(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
    ) -> Result<Answer, ReplyOrIdError> {
        const REFUSED: Answer = Answer::Now(Reply::Refused);
        const WRITTEN: Answer = Answer::Now(Reply::Written);
        let Some(name) = self.named(request.selection) else {
            return Ok(REFUSED);
        };
        let Some(owned) = &self.selection(name).owned else {
            return Ok(REFUSED);
        };
        let current = request.owner == self.window
            && (request.time == CURRENT_TIME || not_before(request.time, owned.time));
        if !current {
            return Ok(REFUSED);
        }
        let target = request.target;
        let concealed = matches!(&owned.content, Content::Text(encodings) if encodings.concealed);
        if target == self.atoms.targets {
            let mut targets = vec![self.atoms.targets, self.atoms.timestamp, self.atoms.text];
            targets.extend(self.atoms.forms);
            if concealed {
                targets.push(self.atoms.concealed_hint);
            }
            self.connection.change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::ATOM,
                &targets,
            )?;
            return Ok(WRITTEN);
        }
        if target == self.atoms.timestamp {
            self.connection.change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::INTEGER,
                &[owned.time],
            )?;
            return Ok(WRITTEN);
        }
        if target == self.atoms.concealed_hint {
            if !concealed {
                return Ok(REFUSED);
            }
            self.connection.change_property8(
                PropMode::REPLACE,
                request.requestor,
                property,
                self.atoms.concealed_hint,
                CONCEALED_HINT_VALUE,
            )?;
            return Ok(WRITTEN);
        }
        let Some(form) = self.atoms.form_of(target) else {
            return Ok(REFUSED);
        };
        let Content::Offered(offer) = owned.content else {
            let (_, _, _, selection) = self.split(name);
            let Some((text, len)) = selection
                .owned
                .as_ref()
                .and_then(|owned| owned.text_in(form))
            else {
                return Ok(REFUSED);
            };
            return self.answer_text(request, property, form, text, len);
        };
        // With every transfer held, a long text could not be written: the
        // closure is not asked at all, so a requestor that holds the writers
        // and keeps asking costs the application no work.
        if self.transfers.len() >= MAX_WRITERS {
            tracing::debug!(
                "an X11 primary selection request while {MAX_WRITERS} transfers run was refused"
            );
            return Ok(REFUSED);
        }
        if self.asks.len() + self.decided.len() >= MAX_ASKS {
            tracing::debug!("an X11 primary selection request past {MAX_ASKS} waiting was refused");
            return Ok(REFUSED);
        }
        // The request outlives this step: its requestor's end has to be
        // heard, or the answer could reach a window that took its id.
        if let Some(answer) = self.watch(request.requestor)?.unheard_answer() {
            return Ok(answer);
        }
        // One ask covers every request waiting on the offer.
        let asked = self.asks.iter().any(|ask| ask.offer == offer);
        self.asks.push(Ask {
            request: *request,
            property,
            form,
            offer,
            deadline: Instant::now() + TRANSFER_TIMEOUT,
        });
        if !asked {
            self.notice.ask(offer);
        }
        Ok(Answer::Later)
    }

    /// Answer `request` with `text` in `form`, `len` bytes of it: whole now,
    /// or `INCR` once the requestor is heard, the transfer started at the end
    /// of the step. Hearing it takes a round trip, which may read the
    /// window's end; the step handles that before it writes.
    fn answer_text(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
        form: TextForm,
        text: Arc<str>,
        len: usize,
    ) -> Result<Answer, ReplyOrIdError> {
        if len <= self.chunk {
            return Ok(Answer::Now(
                self.write_text(request, property, form, text, len)?,
            ));
        }
        if self.asks.len() + self.decided.len() >= MAX_ASKS {
            tracing::debug!("an X11 selection transfer past {MAX_ASKS} waiting was refused");
            return Ok(Answer::Now(Reply::Refused));
        }
        if let Some(answer) = self.watch(request.requestor)?.unheard_answer() {
            return Ok(answer);
        }
        self.decided.push_back(Decided {
            request: *request,
            property,
            form,
            text: Some((text, len)),
        });
        Ok(Answer::Later)
    }

    /// The owner thread answered `offer` with `text`: the requests that
    /// waited on it are answered with it, if it is still the selection, at
    /// the end of the step.
    fn answered(&mut self, offer: Generation, text: Option<String>) {
        let (ready, waiting): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| ask.offer == offer);
        self.asks = waiting;
        let encodings = text
            .filter(|_| self.primary.offered() == Some(offer))
            .map(|text| Encodings::new(ClipboardWrite::text(text)));
        self.decided.extend(ready.into_iter().map(|ask| {
            let text = encodings
                .as_ref()
                .map(|encodings| (Arc::clone(&encodings.text), encodings.len_in(ask.form)));
            Decided::answer(ask, text)
        }));
    }

    /// The requests waiting on an offer that is no longer the primary
    /// selection are refused at the end of the step.
    fn settle_asks(&mut self) {
        let current = self.primary.offered();
        let (stale, kept): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| Some(ask.offer) != current);
        self.asks = kept;
        self.decided
            .extend(stale.into_iter().map(|ask| Decided::answer(ask, None)));
    }

    /// Write the answers decided this step. A round trip comes first, and
    /// every event read until its reply is handled, so a requestor whose
    /// window ended before the round trip reached the server is dropped by
    /// then ([`Self::gone`]). The answers follow with no round trip among
    /// them: each is written while its entry still holds its requestor
    /// heard, so an `INCR` start waits on no reply, and no event is handled
    /// between that drain and the last write. `true` when there was
    /// something to write.
    fn write_decided(&mut self) -> Result<bool, ReplyOrIdError> {
        if self.decided.is_empty() {
            return Ok(false);
        }
        self.connection.get_input_focus()?.reply()?;
        while let Some(event) = self.connection.poll_for_event()? {
            self.event(event)?;
        }
        while let Some(decided) = self.decided.front() {
            let (request, property, form) = (decided.request, decided.property, decided.form);
            let reply = match decided.text.clone() {
                Some((text, len)) => self.write_text(&request, property, form, text, len)?,
                None => Reply::Refused,
            };
            self.send_reply(&request, property, reply)?;
            let before = self.needs(request.requestor);
            self.decided.pop_front();
            self.release(request.requestor, before)?;
        }
        Ok(true)
    }

    /// Write `text` in `form`, `len` bytes of it, into the requestor's
    /// `property`: whole, or `INCR` when larger than one request, each chunk
    /// encoded as it is written. An `INCR` start needs its requestor heard
    /// already ([`Self::watch`]), so it reads no event.
    fn write_text(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
        form: TextForm,
        text: Arc<str>,
        len: usize,
    ) -> Result<Reply, ReplyOrIdError> {
        let kind = self.atoms.form(form);
        if len <= self.chunk {
            let (bytes, _) = form.encode_chunk(&text, 0, len);
            self.connection.change_property8(
                PropMode::REPLACE,
                request.requestor,
                property,
                kind,
                &bytes,
            )?;
            return Ok(Reply::Written);
        }
        self.transfers.retain(|transfer| {
            transfer.requestor != request.requestor || transfer.property != property
        });
        if self.transfers.len() >= MAX_WRITERS {
            tracing::debug!("an X11 selection transfer past {MAX_WRITERS} at once was refused");
            return Ok(Reply::Refused);
        }
        if !self.within_budget(&text) {
            tracing::debug!("an X11 selection transfer past the retained-text budget was refused");
            return Ok(Reply::Refused);
        }
        let Ok(size) = u32::try_from(len) else {
            return Ok(Reply::Refused);
        };
        // The requestor's deletions of the property ask for each chunk: the
        // selection reaches the server before the header they answer.
        self.hear_deletions(request.requestor)?;
        self.connection.change_property32(
            PropMode::REPLACE,
            request.requestor,
            property,
            self.atoms.incr,
            &[size],
        )?;
        self.transfers.push(Transfer {
            requestor: request.requestor,
            property,
            kind,
            form,
            text,
            written: 0,
            ended: false,
            deadline: Instant::now() + TRANSFER_TIMEOUT,
        });
        Ok(Reply::Written)
    }

    /// A requestor deleted `property`: write the next chunk of its transfer,
    /// or end it after the zero-length one.
    fn write_chunk(
        &mut self,
        requestor: xproto::Window,
        property: xproto::Atom,
    ) -> Result<(), ReplyOrIdError> {
        let Some(at) = self
            .transfers
            .iter()
            .position(|transfer| transfer.requestor == requestor && transfer.property == property)
        else {
            return Ok(());
        };
        let transfer = &mut self.transfers[at];
        if transfer.ended {
            self.transfers.swap_remove(at);
            self.release(requestor, transfer_events())?;
            return Ok(());
        }
        let (chunk, next) =
            transfer
                .form
                .encode_chunk(&transfer.text, transfer.written, self.chunk);
        transfer.ended = chunk.is_empty();
        transfer.written = next;
        transfer.deadline = Instant::now() + TRANSFER_TIMEOUT;
        self.connection.change_property8(
            PropMode::REPLACE,
            requestor,
            property,
            transfer.kind,
            &chunk,
        )?;
        Ok(())
    }

    /// Whether a transfer of `text` may start. A transfer goes on after the
    /// ownership whose text it writes ends (ICCCM §2.2), so what the
    /// transfers hold is bounded when each starts: [`TRANSFER_BUDGET`] bytes
    /// in all, each shared text counted once.
    fn within_budget(&self, text: &Arc<str>) -> bool {
        let mut held: Vec<&Arc<str>> = Vec::new();
        for transfer in &self.transfers {
            if !held.iter().any(|kept| Arc::ptr_eq(kept, &transfer.text)) {
                held.push(&transfer.text);
            }
        }
        if held.iter().any(|kept| Arc::ptr_eq(kept, text)) {
            return true;
        }
        let total: usize = held.iter().map(|kept| kept.len()).sum();
        total.saturating_add(text.len()) <= TRANSFER_BUDGET
    }

    /// What the thread needs to hear of `requestor`'s window for what waits
    /// on it: its end while a request waits on the owner thread or for its
    /// answer to be written, and its property deletions too while a transfer
    /// writes to it. X keeps one event mask per client and window, a later
    /// selection replacing the earlier, so the mask selected is always this,
    /// moved as it changes ([`Self::release`], [`Self::hear_deletions`]).
    fn needs(&self, requestor: xproto::Window) -> EventMask {
        if self
            .transfers
            .iter()
            .any(|transfer| transfer.requestor == requestor)
        {
            return transfer_events();
        }
        let waits = self
            .asks
            .iter()
            .any(|ask| ask.request.requestor == requestor)
            || self
                .decided
                .iter()
                .any(|decided| decided.request.requestor == requestor);
        if waits {
            EventMask::STRUCTURE_NOTIFY
        } else {
            EventMask::NO_EVENT
        }
    }

    /// Whether the thread hears `requestor`'s window: something waits on
    /// it, or it is the thread's own, heard for as long as it lives.
    fn hears(&self, requestor: xproto::Window) -> bool {
        requestor == self.window || self.needs(requestor) != EventMask::NO_EVENT
    }

    /// Hear `requestor`'s end, for a request about to wait on it, until
    /// nothing does ([`Self::release`]).
    fn watch(&self, requestor: xproto::Window) -> Result<Watched, ReplyOrIdError> {
        if self.hears(requestor) {
            return Ok(Watched::Heard);
        }
        Watched::of(select_checked(
            &self.connection,
            requestor,
            EventMask::STRUCTURE_NOTIFY,
        ))
    }

    /// Hear the property deletions of `requestor`, already heard, as well as
    /// its end, for a transfer about to write to it.
    fn hear_deletions(&self, requestor: xproto::Window) -> Result<(), ReplyOrIdError> {
        if requestor != self.window && self.needs(requestor) != transfer_events() {
            self.select(requestor, transfer_events())?;
        }
        Ok(())
    }

    /// Something that waited on `requestor` ended: select what its window
    /// needs now if that is not `before`, nothing once nothing waits on it.
    fn release(&self, requestor: xproto::Window, before: EventMask) -> Result<(), ReplyOrIdError> {
        let now = self.needs(requestor);
        if requestor != self.window && now != before {
            self.select(requestor, now)?;
        }
        Ok(())
    }

    /// Select `mask` on `requestor`'s window, unchecked: a window gone
    /// meanwhile answers an error event, which the thread ignores.
    fn select(&self, requestor: xproto::Window, mask: EventMask) -> Result<(), ReplyOrIdError> {
        self.connection.change_window_attributes(
            requestor,
            &ChangeWindowAttributesAux::new().event_mask(mask),
        )?;
        Ok(())
    }

    /// End what waited past its deadline.
    fn expire(&mut self, now: Instant) -> Result<(), ReplyOrIdError> {
        self.clipboard.expire(now);
        self.primary.expire(now);
        let (expired, kept): (Vec<Transfer>, Vec<Transfer>) = std::mem::take(&mut self.transfers)
            .into_iter()
            .partition(|transfer| transfer.deadline <= now);
        self.transfers = kept;
        for transfer in expired {
            tracing::debug!("an X11 selection transfer timed out");
            self.release(transfer.requestor, transfer_events())?;
        }
        let (expired, kept): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| ask.deadline <= now);
        self.asks = kept;
        if !expired.is_empty() {
            tracing::debug!("the owner thread left an X11 primary selection request unanswered");
        }
        self.decided
            .extend(expired.into_iter().map(|ask| Decided::answer(ask, None)));
        Ok(())
    }

    /// The thread ends: abandon the reads still waiting on either
    /// selection, each drop contained, the first failure going on after
    /// both.
    fn abandon(&mut self) {
        let mut failure = FirstPanic::new();
        failure.contain(|| self.clipboard.abandon());
        failure.contain(|| self.primary.abandon());
        // A clear still waiting answers `Gone` as it drops.
        failure.contain(|| drop(std::mem::take(&mut self.clears)));
        failure.resume();
    }

    /// The earliest deadline waiting.
    fn deadline(&self) -> Option<Instant> {
        [&self.clipboard, &self.primary]
            .into_iter()
            .filter_map(|selection| selection.reading.as_ref())
            .map(|reading| reading.deadline)
            .chain(self.transfers.iter().map(|transfer| transfer.deadline))
            .chain(self.asks.iter().map(|ask| ask.deadline))
            .min()
    }
}

/// The thread's body: commands and events until `Stop`, or until the
/// connection fails. A panic is contained here: the thread ends, and the
/// reads and writes waiting are answered `Gone`, each delivery contained,
/// before the state drops; so are the commands still queued, whose reads
/// and writes were never taken. What ended the thread is reported after,
/// so a report that fails cannot skip it.
pub(super) fn run(mut selections: Selections, commands: Receiver<Command>, bell: &OwnedFd) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a panic ends the clipboard thread, which answers its reads and writes `Gone` first"
    )]
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let stepped = steps(&mut selections, &commands, bell);
        // The answers decided before the loop ended (at `Stop`, or as a
        // reply failed) are owed to requestors still waiting: written on
        // every exit, which a broken connection refuses at once. The loop's
        // own failure is the one reported.
        let written = selections.write_decided().and_then(|_| {
            // The connection closes next, and a server drops the requests
            // of a client that closed before it read them: a round trip has
            // it carry out the answers first.
            selections.connection.get_input_focus()?.reply()?;
            Ok(())
        });
        stepped.and(written)
    }));
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: waiting reads and writes are answered `Gone` even when the thread's body panicked"
    )]
    let abandoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut failure = FirstPanic::new();
        failure.contain(|| selections.abandon());
        // The connection closes with the thread, and the selections it
        // owned with it: the owner's turn retires the offers.
        failure.contain(|| selections.notice.end());
        // A read or write answers `Gone` as it drops, here or with the queue.
        while let Ok(command) = commands.try_recv() {
            failure.contain(|| drop(command));
        }
        failure.contain(|| drop(commands));
        failure.resume();
    }));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            report(|| tracing::error!(%error, "the X11 clipboard's connection failed"));
        }
        Err(payload) => contained(payload),
    }
    if let Err(payload) = abandoned {
        contained(payload);
    }
}

/// Report a panic the thread contained. The payload may own anything; it is
/// destroyed under containment (ADR-0281).
fn contained(payload: Box<dyn std::any::Any + Send>) {
    report(|| {
        tracing::error!(
            panic = %crate::owner::panic_boundary::panic_payload_message(payload.as_ref()),
            "the X11 clipboard's thread panicked"
        );
    });
    flui_foundation::panic::discard_payload(payload);
}

/// Run a diagnostic: a subscriber that panics is contained, its payload discarded.
fn report(diagnostic: impl FnOnce()) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a diagnostic subscriber's panic is discarded, not propagated"
    )]
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(diagnostic)) {
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Append `chunk` to an `INCR` read's `bytes`, which stay within
/// [`MAX_TEXT`] with it: past the room taken, the buffer grows exactly,
/// at most doubling and never past the limit, so a read holds less than
/// twice [`MAX_TEXT`] even while it grows.
fn append(bytes: &mut Vec<u8>, chunk: &[u8]) {
    let needed = bytes.len() + chunk.len();
    if needed > bytes.capacity() {
        let grown = bytes
            .capacity()
            .saturating_mul(2)
            .clamp(needed, MAX_TEXT.max(needed));
        bytes.reserve_exact(grown - bytes.len());
    }
    bytes.extend_from_slice(chunk);
}

fn steps(
    selections: &mut Selections,
    commands: &Receiver<Command>,
    bell: &OwnedFd,
) -> Result<(), ReplyOrIdError> {
    loop {
        // The bell is read before the commands it rang for, so a ring that
        // lands meanwhile leaves it readable for the next wait.
        let mut count = [0_u8; 8];
        let _ = rustix::io::read(bell, &mut count);
        while let Ok(command) = commands.try_recv() {
            if !selections.command(command)? {
                // [`run`] writes what this batch decided.
                return Ok(());
            }
        }
        // A reply read while an event was handled may queue events the
        // wait cannot see: read them all before waiting.
        let mut heard = false;
        while let Some(event) = selections.connection.poll_for_event()? {
            heard = true;
            selections.event(event)?;
        }
        selections.expire(Instant::now())?;
        // The answers decided this step go last, after the events read until
        // a round trip of their own.
        heard |= selections.write_decided()?;
        selections.connection.flush()?;
        if heard {
            continue;
        }
        wait(selections, bell)?;
    }
}

/// Wait for the connection, the bell or the earliest deadline.
fn wait(selections: &Selections, bell: &OwnedFd) -> Result<(), ReplyOrIdError> {
    let timeout = selections.deadline().map(|deadline| {
        let remaining = deadline.saturating_duration_since(Instant::now());
        // Rounded up, so a wait never wakes just before its deadline.
        Timespec::try_from(remaining + Duration::from_millis(1))
            .unwrap_or_else(|_| Timespec::try_from(TRANSFER_TIMEOUT).expect("BUG: it fits"))
    });
    let connection = selections.connection.stream().as_fd();
    let mut fds = [
        PollFd::new(&connection, PollFlags::IN),
        PollFd::new(bell, PollFlags::IN),
    ];
    match poll(&mut fds, timeout.as_ref()) {
        Ok(_) | Err(Errno::INTR) => Ok(()),
        Err(error) => Err(x11rb::errors::ConnectionError::IoError(error.into()).into()),
    }
}

/// Whether server time `time` is no earlier than `since`, across the
/// 32-bit millisecond clock's wrap every 49.7 days: the later of two
/// times is the one less than half the clock ahead.
fn not_before(time: xproto::Timestamp, since: xproto::Timestamp) -> bool {
    time.wrapping_sub(since) < 1 << 31
}

#[cfg(test)]
mod tests {
    use x11rb::errors::ReplyError;
    use x11rb::protocol::ErrorKind;
    use x11rb::x11_utils::X11Error;

    use super::{Answer, MAX_TEXT, Reply, Watched, WindowSelection, append, not_before};

    /// A requestor whose window is gone is never answered, since the window
    /// that takes its id did not ask; one the server will not let the thread
    /// hear for another reason is there and waiting, and is refused (ICCCM
    /// §2.2). Xvfb refuses this selection only with `BadWindow`, so the other
    /// refusals are fed in at the seam.
    #[test]
    fn a_requestor_that_cannot_be_heard_is_refused_unless_it_is_gone() {
        let refused = |error_kind| {
            WindowSelection::Failed(ReplyError::X11Error(X11Error {
                error_kind,
                error_code: 0,
                sequence: 0,
                bad_value: 0,
                minor_opcode: 0,
                major_opcode: 2,
                extension_name: None,
                request_name: Some("ChangeWindowAttributes"),
            }))
        };
        let table = [
            ("heard", WindowSelection::Selected, None),
            ("gone", WindowSelection::Gone, Some(Answer::Never)),
            (
                "access",
                refused(ErrorKind::Access),
                Some(Answer::Now(Reply::Refused)),
            ),
            (
                "alloc",
                refused(ErrorKind::Alloc),
                Some(Answer::Now(Reply::Refused)),
            ),
        ];
        for (case, selection, expected) in table {
            let watched = Watched::of(selection).expect("no connection error");
            assert_eq!(watched.unheard_answer(), expected, "{case}");
        }
    }

    /// A time just past the clock's wrap is later than one just before it.
    #[test]
    fn server_times_order_across_the_wrap() {
        assert!(not_before(0x10, 0xffff_fff0));
        assert!(!not_before(0xffff_fff0, 0x10));
        assert!(not_before(5, 5));
        assert!(not_before(6, 5));
        assert!(!not_before(5, 6));
    }

    /// An `INCR` read's buffer grows exactly within its limit: past the room
    /// taken it at most doubles, and never past [`MAX_TEXT`].
    #[test]
    fn a_read_buffer_grows_within_the_limit() {
        let mut bytes = Vec::with_capacity(4);
        append(&mut bytes, b"0123456789");
        assert_eq!(bytes.capacity(), 10, "grown to what the chunk needs");
        append(&mut bytes, b"ab");
        assert_eq!(bytes.capacity(), 20, "doubled");
        assert_eq!(bytes, b"0123456789ab");

        let mut bytes = vec![0_u8; MAX_TEXT * 3 / 4];
        append(&mut bytes, b"x");
        assert_eq!(bytes.capacity(), MAX_TEXT, "not doubled past the limit");
    }
}
