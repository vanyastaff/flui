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
//! - **The primary selection is lazy.** The application offers it with a
//!   closure the owner thread alone may call, so the thread holds only the
//!   offer's [`Generation`]. A request for its text waits while the thread
//!   asks the owner through the [`Notice`], which wakes the owner's loop; at
//!   its turn the loop asks the closure and sends the text back
//!   ([`Command::Answer`]), and every request waiting on that offer is
//!   answered with it. One that waits past [`TRANSFER_TIMEOUT`], or whose
//!   offer stopped being the selection, is refused.
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
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_platform::capability::Resolver;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use web_time::Instant;
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
    GetPropertyReply, PropMode, Property, SelectionClearEvent, SelectionNotifyEvent,
    SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use crate::clipboard::linux::text::TextForm;

/// How long a transfer may wait for its peer: a whole read, the next chunk
/// of an `INCR` write, or the owner thread's text for the primary
/// selection.
pub(super) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);

/// The most text a read takes; an owner offering more answers nothing.
const MAX_TEXT: usize = 64 * 1024 * 1024;

/// The most `INCR` writes at once: a requestor that never deletes its
/// property holds one until its timeout, and more are refused.
const MAX_WRITERS: usize = 4;

/// The most reads of a selection waiting at once: each is answered with its
/// own copy of the text, so more are answered nothing at once.
pub(super) const MAX_READERS: usize = 4;

/// The most bytes of text the `INCR` writes hold at once, each shared text
/// counted once: room for the text owned now and one an ended ownership
/// left, which a transfer keeps writing (ICCCM §2.2). Each encodes its form
/// a chunk at a time, so the texts are all it holds.
const TRANSFER_BUDGET: usize = 2 * MAX_TEXT;

/// The most requests waiting on the owner thread for the primary
/// selection's text; more are refused.
const MAX_ASKS: usize = 16;

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
    /// Own the clipboard with this text.
    Write(String),
    /// Own the primary selection with the application's offer, whose text
    /// the owner thread answers.
    Offer(Generation),
    /// Let go of the primary selection if this offer holds it.
    Withdraw(Generation),
    /// The owner thread's text for an offer the thread asked about; `None`
    /// when the offer is no longer the application's or holds no text.
    Answer(Generation, Option<String>),
    /// Answer with the selection's text.
    Read(Name, Resolver<Option<String>>),
    /// Let go of everything and end.
    Stop,
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

    /// The offer a requestor waits for, once.
    pub(super) fn take_asked(&self) -> Option<Generation> {
        self.state.lock().asked.take()
    }

    /// The newest offer that is not the selection.
    pub(super) fn lost(&self) -> Generation {
        self.state.lock().lost
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
    /// Each of [`TextForm::OFFERED`], by its position there.
    forms: [xproto::Atom; 4],
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, ReplyOrIdError> {
        let [targets, timestamp, incr, text] =
            intern(connection, [b"TARGETS", b"TIMESTAMP", b"INCR", b"TEXT"])?;
        let forms = intern(
            connection,
            TextForm::OFFERED.map(|form| form.name().as_bytes()),
        )?;
        Ok(Self {
            targets,
            timestamp,
            incr,
            text,
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
    /// The text's length in ISO 8859-1, counted once: each request for the
    /// form would otherwise scan the whole text again.
    latin1_len: OnceCell<usize>,
}

impl Encodings {
    fn new(text: Arc<str>) -> Self {
        Self {
            text,
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
    /// The writes' clock appends the server has not stamped yet: only the
    /// last one's time is the pending write's.
    clocks: usize,
    /// The reads waiting for the read under way, or for one to start.
    readers: Vec<Resolver<Option<String>>>,
    /// The reads sent after a pending write: they answer what the write
    /// leaves, so they join no read that began before it.
    after_write: Vec<Resolver<Option<String>>>,
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
                Content::Text(encodings) => Some(encodings.text.to_string()),
                Content::Offered(_) => None,
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

    /// End the read under way, answering every reader with `text`, the last
    /// with `text` itself rather than a copy. A delivery that panics (the
    /// owner's wake hook) leaves the others answered, and the first failure
    /// goes on.
    fn answer(&mut self, mut text: Option<String>) {
        self.reading = None;
        let mut failure = FirstPanic::new();
        let mut readers = std::mem::take(&mut self.readers).into_iter().peekable();
        while let Some(reader) = readers.next() {
            let answer = if readers.peek().is_some() {
                text.clone()
            } else {
                text.take()
            };
            // A retired owner has nobody left to answer.
            failure.contain(|| reader.resolve(answer));
        }
        failure.resume();
    }

    /// The thread ends: every read still waiting is abandoned, each drop
    /// contained, so one whose abandonment panics leaves the others
    /// abandoned too, and the first failure goes on.
    fn abandon(&mut self) {
        let mut failure = FirstPanic::new();
        let waiting = std::mem::take(&mut self.readers);
        for reader in waiting
            .into_iter()
            .chain(std::mem::take(&mut self.after_write))
        {
            failure.contain(|| drop(reader));
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
            let next = READ_ORDER
                .iter()
                .skip_while(|form| **form != reading.form)
                .nth(1);
            match next {
                Some(&form) => {
                    reading.form = form;
                    self.convert(connection, window, atoms)?;
                }
                None => self.answer(None),
            }
            return Ok(());
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
            if announced > MAX_TEXT {
                // Deleting the property asked for the first chunk: it lands
                // where no read looks.
                self.retire_destination();
                self.answer(None);
                return Ok(());
            }
            // Deleting the property asked for the first chunk.
            if let Some(reading) = &mut self.reading {
                reading.incremental = Some(Vec::with_capacity(announced));
            }
            return Ok(());
        }
        let text = if reply.bytes_after == 0 {
            decode(form, reply.value)
        } else {
            None
        };
        self.answer(text);
        Ok(())
    }

    /// The owner wrote an `INCR` chunk: read at most what the read has left
    /// of [`MAX_TEXT`], the first chunk kept as it came.
    fn read_chunk(
        &mut self,
        connection: &RustConnection,
        window: xproto::Window,
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
        // A chunk past what the read has left reads as empty with bytes after
        // it, which is not the end of the transfer.
        if bytes.len() + reply.value.len() > MAX_TEXT || reply.bytes_after != 0 {
            // The owner's transfer goes on: its chunks land where no read looks.
            self.retire_destination();
            self.answer(None);
        } else if reply.value.is_empty() {
            let form = reading.form;
            let bytes = std::mem::take(bytes);
            self.answer(decode(form, bytes));
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
            self.answer(None);
        }
    }
}

/// How a request is answered.
enum Reply {
    /// Now: `true` when its property holds the answer, `false` when it is
    /// refused.
    Now(bool),
    /// Once the owner thread sends the text.
    Later,
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
        let primary = Selection::try_new(&connection, AtomEnum::PRIMARY.into(), "PRIMARY")?;
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
            Command::Write(text) => {
                self.claim(Name::Clipboard, Content::Text(Encodings::new(text.into())))?;
            }
            Command::Offer(offer) => self.claim(Name::Primary, Content::Offered(offer))?,
            Command::Withdraw(offer) => self.withdraw(offer)?,
            Command::Answer(offer, text) => self.answered(offer, text)?,
            Command::Read(name, answer) => {
                // Each waiting read is answered with a copy of the text: past
                // a few, a read answers nothing rather than hold another.
                let selection = self.split(name).3;
                if selection.readers.len() + selection.after_write.len() >= MAX_READERS {
                    // A retired owner has nobody left to answer. The read is
                    // settled before the diagnostic, which runs user code.
                    let _ = answer.resolve(None);
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
                        self.lost(name)?;
                    }
                    let (connection, window, _, selection) = self.split(name);
                    selection.readers.push(answer);
                    selection.serve(connection, window)?;
                }
            }
            Command::Stop => return Ok(false),
        }
        Ok(true)
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
        self.settle_asks()
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
                            return selection.read_chunk(connection, window);
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
            // A requestor that went away fails the writes to it; its
            // transfer ends at its deadline.
            _ => {}
        }
        Ok(())
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
        if owner == window {
            selection.owned = Some(Owned { content, time });
        } else {
            // A client took it at a later time than ours.
            selection.owned = None;
            tracing::debug!("an X11 selection was taken before the write could own it");
            if let Content::Offered(offer) = content {
                self.notice.lose(offer);
            }
        }
        // The reads that waited for the write.
        let (connection, window, _, selection) = self.split(name);
        selection.readers.append(&mut selection.after_write);
        selection.serve(connection, window)?;
        self.settle_asks()
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
        if self.still_owned(name)? {
            return Ok(());
        }
        self.lost(name)
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
    fn lost(&mut self, name: Name) -> Result<(), ReplyOrIdError> {
        if let Some(Owned {
            content: Content::Offered(offer),
            ..
        }) = self.split(name).3.owned.take()
        {
            self.notice.lose(offer);
        }
        self.settle_asks()
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
            Reply::Now(true) => self.notify(request, property),
            Reply::Now(false) => self.notify(request, NONE),
            Reply::Later => Ok(()),
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
    /// hold it for the owner thread's text.
    fn answer_request(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
    ) -> Result<Reply, ReplyOrIdError> {
        let Some(name) = self.named(request.selection) else {
            return Ok(Reply::Now(false));
        };
        let Some(owned) = &self.selection(name).owned else {
            return Ok(Reply::Now(false));
        };
        let current = request.owner == self.window
            && (request.time == CURRENT_TIME || not_before(request.time, owned.time));
        if !current {
            return Ok(Reply::Now(false));
        }
        let target = request.target;
        if target == self.atoms.targets {
            let mut targets = vec![self.atoms.targets, self.atoms.timestamp, self.atoms.text];
            targets.extend(self.atoms.forms);
            self.connection.change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::ATOM,
                &targets,
            )?;
            return Ok(Reply::Now(true));
        }
        if target == self.atoms.timestamp {
            self.connection.change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::INTEGER,
                &[owned.time],
            )?;
            return Ok(Reply::Now(true));
        }
        let Some(form) = self.atoms.form_of(target) else {
            return Ok(Reply::Now(false));
        };
        let Content::Offered(offer) = owned.content else {
            let (_, _, _, selection) = self.split(name);
            let Some((text, len)) = selection
                .owned
                .as_ref()
                .and_then(|owned| owned.text_in(form))
            else {
                return Ok(Reply::Now(false));
            };
            let answered = self.write_text(request, property, form, text, len)?;
            return Ok(Reply::Now(answered));
        };
        if self.asks.len() >= MAX_ASKS {
            tracing::debug!("an X11 primary selection request past {MAX_ASKS} waiting was refused");
            return Ok(Reply::Now(false));
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
        Ok(Reply::Later)
    }

    /// The owner thread answered `offer` with `text`: answer the requests
    /// that waited on it, if it is still the selection.
    fn answered(&mut self, offer: Generation, text: Option<String>) -> Result<(), ReplyOrIdError> {
        let (ready, waiting): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| ask.offer == offer);
        self.asks = waiting;
        let encodings = text
            .filter(|_| self.primary.offered() == Some(offer))
            .map(|text| Encodings::new(text.into()));
        for ask in ready {
            let answered = match &encodings {
                Some(encodings) => {
                    let text = Arc::clone(&encodings.text);
                    let len = encodings.len_in(ask.form);
                    self.write_text(&ask.request, ask.property, ask.form, text, len)?
                }
                None => false,
            };
            self.notify(&ask.request, if answered { ask.property } else { NONE })?;
        }
        Ok(())
    }

    /// Refuse the requests waiting on an offer that is no longer the
    /// primary selection.
    fn settle_asks(&mut self) -> Result<(), ReplyOrIdError> {
        let current = self.primary.offered();
        let (stale, kept): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| Some(ask.offer) != current);
        self.asks = kept;
        for ask in stale {
            self.notify(&ask.request, NONE)?;
        }
        Ok(())
    }

    /// Write `text` in `form`, `len` bytes of it, into the requestor's
    /// `property`: whole, or `INCR` when larger than one request, each chunk
    /// encoded as it is written. `false` when it is refused.
    fn write_text(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
        form: TextForm,
        text: Arc<str>,
        len: usize,
    ) -> Result<bool, ReplyOrIdError> {
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
            return Ok(true);
        }
        self.transfers.retain(|transfer| {
            transfer.requestor != request.requestor || transfer.property != property
        });
        if self.transfers.len() >= MAX_WRITERS {
            tracing::debug!("an X11 selection transfer past {MAX_WRITERS} at once was refused");
            return Ok(false);
        }
        if !self.within_budget(&text) {
            tracing::debug!("an X11 selection transfer past the retained-text budget was refused");
            return Ok(false);
        }
        let Ok(size) = u32::try_from(len) else {
            return Ok(false);
        };
        // The requestor's deletions of the property ask for each chunk.
        self.connection.change_window_attributes(
            request.requestor,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
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
        Ok(true)
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
            let transfer = self.transfers.swap_remove(at);
            self.forget(transfer.requestor)?;
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

    /// Stop hearing `requestor`'s properties once no transfer writes to it.
    fn forget(&self, requestor: xproto::Window) -> Result<(), ReplyOrIdError> {
        // The window's own changes are heard for as long as it lives.
        let unheard = requestor != self.window
            && self
                .transfers
                .iter()
                .all(|transfer| transfer.requestor != requestor);
        if unheard {
            self.connection.change_window_attributes(
                requestor,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT),
            )?;
        }
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
            self.forget(transfer.requestor)?;
        }
        let (expired, kept): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| ask.deadline <= now);
        self.asks = kept;
        for ask in expired {
            tracing::debug!("the owner thread left an X11 primary selection request unanswered");
            self.notify(&ask.request, NONE)?;
        }
        Ok(())
    }

    /// The thread ends: abandon the reads still waiting on either
    /// selection, each drop contained, the first failure going on after
    /// both.
    fn abandon(&mut self) {
        let mut failure = FirstPanic::new();
        failure.contain(|| self.clipboard.abandon());
        failure.contain(|| self.primary.abandon());
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
/// reads waiting are abandoned, each drop contained, before the state
/// drops; so are the commands still queued, whose reads were never taken.
/// What ended the thread is reported after the abandonment, so a report
/// that fails cannot skip it.
pub(super) fn run(mut selections: Selections, commands: Receiver<Command>, bell: &OwnedFd) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        steps(&mut selections, &commands, bell)
    }));
    let abandoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut failure = FirstPanic::new();
        failure.contain(|| selections.abandon());
        while let Ok(command) = commands.try_recv() {
            failure.contain(|| drop(command));
        }
        // A command sent after the drain drops with the queue.
        failure.contain(|| drop(commands));
        failure.resume();
    }));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            report(|| tracing::error!("the X11 clipboard's connection failed: {error}"));
        }
        Err(payload) => contained(payload),
    }
    if let Err(payload) = abandoned {
        contained(payload);
    }
}

/// Report a panic the thread contained. The payload may own anything; it is
/// kept rather than dropped where its destructor could fail too (ADR-0127).
fn contained(payload: Box<dyn std::any::Any + Send>) {
    report(|| {
        tracing::error!(
            "the X11 clipboard's thread panicked: {}",
            crate::owner::panic_boundary::panic_payload_message(payload.as_ref())
        );
    });
    std::mem::forget(payload);
}

/// Run a diagnostic: a subscriber that panics is contained, its payload kept.
fn report(diagnostic: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(diagnostic)) {
        std::mem::forget(payload);
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

/// `bytes` received in `form` as text, or `None` past [`MAX_TEXT`] bytes of
/// it: ISO 8859-1 and ill-formed UTF-8 grow as they decode.
fn decode(form: TextForm, bytes: Vec<u8>) -> Option<String> {
    (form.decoded_len(&bytes) <= MAX_TEXT).then(|| form.decode(bytes))
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
    use super::{MAX_TEXT, append, not_before};

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
