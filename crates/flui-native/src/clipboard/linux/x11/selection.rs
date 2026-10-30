//! The clipboard thread: the `CLIPBOARD` selection as ICCCM §2 has an owner
//! and a requestor exchange it.
//!
//! - **Owning.** A write takes a timestamp from the server first (a
//!   zero-length append to the window's own property, whose
//!   `PropertyNotify` carries the time), then sets the window as the
//!   selection's owner at that time and checks that it took. The text is
//!   held until another client takes the selection (`SelectionClear`).
//! - **Answering.** A `SelectionRequest` for `TARGETS`, `TIMESTAMP` or a
//!   text form is answered by writing the requestor's property and sending
//!   it `SelectionNotify`; anything else, a request older than the
//!   ownership, or a `MULTIPLE`, is refused with property `None`. Text
//!   larger than one request goes `INCR`: the property says how much is
//!   coming, and each time the requestor deletes it the next chunk is
//!   written, a zero-length one last. At most [`MAX_WRITERS`] such
//!   transfers run at once, each given [`TRANSFER_TIMEOUT`] between
//!   chunks.
//! - **Reading.** While the application owns the selection a read answers
//!   its text at once; a read sent after a write waits for the write to own
//!   it. Otherwise the thread takes a server timestamp, as a write does, and
//!   asks the owner to convert the selection at that time into one of its
//!   window's properties, in each of [`READ_ORDER`] until one is answered.
//!   It reads the property, deleting it, which for `INCR` asks for the next
//!   chunk. An answer counts only if it names the read's time, form and
//!   property, and a read that times out retires its property, so an owner
//!   that answers late never answers a later read. Reads that arrive
//!   meanwhile join the one under way. A read gets [`TRANSFER_TIMEOUT`] in
//!   all and at most [`MAX_TEXT`] bytes.

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

/// How long a transfer may wait for its peer: a whole read, or the next
/// chunk of an `INCR` write.
pub(super) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);

/// The most text a read takes; an owner offering more answers nothing.
const MAX_TEXT: usize = 64 * 1024 * 1024;

/// The most `INCR` writes at once: a requestor that never deletes its
/// property holds one until its timeout, and more are refused.
const MAX_WRITERS: usize = 4;

/// The largest chunk written in one request, below the server's request
/// limit whatever it is.
const MAX_CHUNK: usize = 256 * 1024;

/// How many properties a read is converted into in turn.
const DESTINATIONS: usize = 4;

/// The forms a read asks for, in order: UTF-8 under X11's name first, which
/// every toolkit answers, then the MIME name, then ISO 8859-1.
const READ_ORDER: [TextForm; 3] = [TextForm::Utf8String, TextForm::PlainUtf8, TextForm::Latin1];

/// What the handle asks of the thread.
pub(super) enum Command {
    /// Own the selection with this text.
    Write(String),
    /// Answer with the selection's text.
    Read(Resolver<Option<String>>),
    /// Let go of everything and end.
    Stop,
}

/// The atoms the selection speaks.
struct Atoms {
    clipboard: xproto::Atom,
    targets: xproto::Atom,
    timestamp: xproto::Atom,
    incr: xproto::Atom,
    text: xproto::Atom,
    /// The window's properties a read is converted into, one at a time: a
    /// read that timed out retires its property, so an owner that answers
    /// it late writes where no read looks.
    destinations: [xproto::Atom; DESTINATIONS],
    /// The window's property a write appends to for a timestamp.
    clock: xproto::Atom,
    /// The window's property a read appends to for a timestamp, which tells
    /// its answers from a late answer to an earlier read.
    read_clock: xproto::Atom,
    /// Each of [`TextForm::OFFERED`], by its position there.
    forms: [xproto::Atom; 4],
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, ReplyOrIdError> {
        let names: [&[u8]; 11] = [
            b"CLIPBOARD",
            b"TARGETS",
            b"TIMESTAMP",
            b"INCR",
            b"TEXT",
            b"FLUI_CLIPBOARD_CLOCK",
            b"FLUI_CLIPBOARD_READ_CLOCK",
            b"FLUI_CLIPBOARD_0",
            b"FLUI_CLIPBOARD_1",
            b"FLUI_CLIPBOARD_2",
            b"FLUI_CLIPBOARD_3",
        ];
        let cookies = names
            .map(|name| connection.intern_atom(false, name))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let forms = TextForm::OFFERED
            .map(|form| connection.intern_atom(false, form.name().as_bytes()))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let atoms = cookies
            .into_iter()
            .map(|cookie| Ok(cookie.reply()?.atom))
            .collect::<Result<Vec<xproto::Atom>, ReplyOrIdError>>()?;
        let forms = forms
            .into_iter()
            .map(|cookie| Ok(cookie.reply()?.atom))
            .collect::<Result<Vec<xproto::Atom>, ReplyOrIdError>>()?;
        let [
            clipboard,
            targets,
            timestamp,
            incr,
            text,
            clock,
            read_clock,
            first,
            second,
            third,
            fourth,
        ] = atoms[..]
        else {
            unreachable!("BUG: one atom is interned for each name");
        };
        let [plain_utf8, utf8_string, plain, latin1] = forms[..] else {
            unreachable!("BUG: one atom is interned for each form");
        };
        Ok(Self {
            clipboard,
            targets,
            timestamp,
            incr,
            text,
            destinations: [first, second, third, fourth],
            clock,
            read_clock,
            forms: [plain_utf8, utf8_string, plain, latin1],
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

/// The text the application owns the selection with, and since when. Its
/// UTF-8 forms are the text's own bytes, shared by every transfer; its ISO
/// 8859-1 form is encoded once, the first time a requestor asks for it.
struct Owned {
    text: Arc<str>,
    time: xproto::Timestamp,
    latin1: Option<Arc<[u8]>>,
}

impl Owned {
    fn new(text: String, time: xproto::Timestamp) -> Self {
        Self {
            text: text.into(),
            time,
            latin1: None,
        }
    }

    /// The text in `form`: at most one copy of it besides the text.
    fn encoded(&mut self, form: TextForm) -> Arc<[u8]> {
        if form.is_utf8() {
            return Arc::<[u8]>::from(Arc::clone(&self.text));
        }
        let text = &self.text;
        Arc::clone(self.latin1.get_or_insert_with(|| form.encode(text).into()))
    }
}

/// An `INCR` write: the requestor's property, filled a chunk at a time.
struct Transfer {
    requestor: xproto::Window,
    property: xproto::Atom,
    kind: xproto::Atom,
    bytes: Arc<[u8]>,
    written: usize,
    /// Whether the zero-length chunk that ends it is written.
    ended: bool,
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

/// The clipboard thread's state over its connection.
pub(super) struct Selections {
    connection: RustConnection,
    window: xproto::Window,
    atoms: Atoms,
    owned: Option<Owned>,
    /// A write waiting for its timestamp; a newer write replaces it.
    pending: Option<String>,
    /// The writes' clock appends the server has not stamped yet: only the
    /// last one's time is the pending write's.
    clocks: usize,
    /// The reads waiting for the read under way, or for one to start.
    readers: Vec<Resolver<Option<String>>>,
    /// The reads sent after a pending write: they answer what the write
    /// leaves, so they join no read that began before it.
    after_write: Vec<Resolver<Option<String>>>,
    reading: Option<Reading>,
    /// Which of [`Atoms::destinations`] the reads are converted into now.
    destination: usize,
    transfers: Vec<Transfer>,
    chunk: usize,
}

impl Selections {
    /// Make the selection window on the root of `screen`, which hears its
    /// own properties change.
    pub(super) fn try_new(
        connection: RustConnection,
        screen: usize,
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
        let chunk = connection
            .maximum_request_bytes()
            .saturating_sub(64)
            .clamp(1, MAX_CHUNK);
        connection.flush()?;
        Ok(Self {
            connection,
            window,
            atoms,
            owned: None,
            pending: None,
            clocks: 0,
            readers: Vec::new(),
            after_write: Vec::new(),
            reading: None,
            destination: 0,
            transfers: Vec::new(),
            chunk,
        })
    }

    /// Carry out `command`; `false` once it asks the thread to end.
    fn command(&mut self, command: Command) -> Result<bool, ReplyOrIdError> {
        match command {
            Command::Write(text) => {
                self.pending = Some(text);
                self.clocks += 1;
                // The server stamps the property's change with its time.
                self.connection.change_property8(
                    PropMode::APPEND,
                    self.window,
                    self.atoms.clock,
                    AtomEnum::INTEGER,
                    &[],
                )?;
            }
            Command::Read(answer) => {
                // A read after a write answers what the write left: it waits
                // for the write to own the selection or fail to.
                if self.pending.is_some() {
                    self.after_write.push(answer);
                } else {
                    self.readers.push(answer);
                    self.serve()?;
                }
            }
            Command::Stop => return Ok(false),
        }
        Ok(true)
    }

    /// Answer the waiting reads: with the application's text while it owns
    /// the selection, otherwise through the read under way or a new one.
    fn serve(&mut self) -> Result<(), ReplyOrIdError> {
        if self.readers.is_empty() {
            return Ok(());
        }
        if let Some(owned) = &self.owned {
            let text = owned.text.to_string();
            if self.reading.is_some() {
                self.retire_destination();
            }
            self.answer(Some(text));
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
            self.connection.change_property8(
                PropMode::APPEND,
                self.window,
                self.atoms.read_clock,
                AtomEnum::INTEGER,
                &[],
            )?;
        }
        Ok(())
    }

    /// The property reads are converted into now.
    fn destination(&self) -> xproto::Atom {
        self.atoms.destinations[self.destination]
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
    fn convert(&mut self) -> Result<(), ReplyOrIdError> {
        let Some(Reading {
            form,
            time: Some(time),
            ..
        }) = self.reading
        else {
            return Ok(());
        };
        self.connection.convert_selection(
            self.window,
            self.atoms.clipboard,
            self.atoms.form(form),
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

    fn event(&mut self, event: Event) -> Result<(), ReplyOrIdError> {
        match event {
            Event::PropertyNotify(event) => {
                if event.window == self.window && event.atom == self.atoms.clock {
                    self.own(event.time)?;
                } else if event.window == self.window && event.atom == self.atoms.read_clock {
                    if let Some(reading) = &mut self.reading
                        && reading.time.is_none()
                    {
                        reading.time = Some(event.time);
                        self.convert()?;
                    }
                } else if event.window == self.window
                    && event.atom == self.destination()
                    && event.state == Property::NEW_VALUE
                {
                    self.read_chunk()?;
                } else if event.state == Property::DELETE {
                    self.write_chunk(event.window, event.atom)?;
                }
            }
            Event::SelectionNotify(event) => self.converted(&event)?,
            Event::SelectionRequest(event) => self.request(&event)?,
            Event::SelectionClear(event) => self.cleared(&event)?,
            // A requestor that went away fails the writes to it; its
            // transfer ends at its deadline.
            _ => {}
        }
        Ok(())
    }

    /// The server's time arrived: own the selection with the pending text.
    fn own(&mut self, time: xproto::Timestamp) -> Result<(), ReplyOrIdError> {
        // A stamp of a write a newer one replaced: the newer write takes the
        // stamp of its own append, which follows.
        self.clocks = self.clocks.saturating_sub(1);
        if self.clocks > 0 {
            return Ok(());
        }
        let Some(text) = self.pending.take() else {
            return Ok(());
        };
        self.connection
            .set_selection_owner(self.window, self.atoms.clipboard, time)?;
        let owner = self
            .connection
            .get_selection_owner(self.atoms.clipboard)?
            .reply()?
            .owner;
        if owner == self.window {
            self.owned = Some(Owned::new(text, time));
        } else {
            // A client took it at a later time than ours.
            self.owned = None;
            tracing::debug!("the X11 clipboard was taken before the write could own it");
        }
        // The reads that waited for the write.
        self.readers.append(&mut self.after_write);
        self.serve()
    }

    /// Another client took the selection, unless the window owns it again:
    /// a clear queued behind the write that took it back is stale.
    fn cleared(&mut self, event: &SelectionClearEvent) -> Result<(), ReplyOrIdError> {
        if event.owner != self.window
            || event.selection != self.atoms.clipboard
            || self.owned.is_none()
        {
            return Ok(());
        }
        let owner = self
            .connection
            .get_selection_owner(self.atoms.clipboard)?
            .reply()?
            .owner;
        if owner != self.window {
            self.owned = None;
        }
        Ok(())
    }

    /// The owner answered a conversion: read its property, or ask for the
    /// next form if it refused.
    fn converted(&mut self, event: &SelectionNotifyEvent) -> Result<(), ReplyOrIdError> {
        if event.requestor != self.window || event.selection != self.atoms.clipboard {
            return Ok(());
        }
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        // An answer to this read names its form, its time (an owner that
        // ignores the time sends the server's current one) and its property.
        let ours = reading
            .time
            .is_some_and(|time| event.time == time || event.time == CURRENT_TIME)
            && event.target == self.atoms.form(reading.form)
            && (event.property == NONE
                || event.property == self.atoms.destinations[self.destination]);
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
                    self.convert()?;
                }
                None => self.answer(None),
            }
            return Ok(());
        }
        let form = reading.form;
        let reply = self.take_property(MAX_TEXT)?;
        if reply.type_ == self.atoms.incr {
            // Deleting the property asked for the first chunk.
            if let Some(reading) = &mut self.reading {
                reading.incremental = Some(Vec::new());
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
    fn read_chunk(&mut self) -> Result<(), ReplyOrIdError> {
        let Some(received) = self
            .reading
            .as_ref()
            .and_then(|reading| reading.incremental.as_ref())
            .map(Vec::len)
        else {
            return Ok(());
        };
        let reply = self.take_property(MAX_TEXT.saturating_sub(received))?;
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        let Some(bytes) = &mut reading.incremental else {
            return Ok(());
        };
        if reply.value.is_empty() {
            let form = reading.form;
            let bytes = std::mem::take(bytes);
            self.answer(decode(form, bytes));
        } else if bytes.len() + reply.value.len() > MAX_TEXT || reply.bytes_after != 0 {
            self.answer(None);
        } else if bytes.is_empty() {
            *bytes = reply.value;
        } else {
            bytes.extend_from_slice(&reply.value);
        }
        Ok(())
    }

    /// Read and delete the window's destination property, at most `limit`
    /// bytes of it, rounded up to whole 32-bit units; a longer value says
    /// so in `bytes_after`.
    fn take_property(&self, limit: usize) -> Result<GetPropertyReply, ReplyOrIdError> {
        let longs = u32::try_from(limit.div_ceil(4)).expect("BUG: the text limit fits a request");
        Ok(self
            .connection
            .get_property(
                true,
                self.window,
                self.destination(),
                AtomEnum::ANY,
                0,
                longs,
            )?
            .reply()?)
    }

    /// Another client asks for the selection.
    fn request(&mut self, request: &SelectionRequestEvent) -> Result<(), ReplyOrIdError> {
        // An obsolete client names no property: the target is its name.
        let property = if request.property == NONE {
            request.target
        } else {
            request.property
        };
        let answered = self.answer_request(request, property)?;
        let notify = SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if answered { property } else { NONE },
        };
        self.connection
            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
        Ok(())
    }

    /// Write the answer to `request` into the requestor's `property`;
    /// `false` when it is refused.
    fn answer_request(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
    ) -> Result<bool, ReplyOrIdError> {
        let Some(owned) = &self.owned else {
            return Ok(false);
        };
        let current = request.selection == self.atoms.clipboard
            && request.owner == self.window
            && (request.time == CURRENT_TIME || not_before(request.time, owned.time));
        if !current {
            return Ok(false);
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
            return Ok(true);
        }
        if target == self.atoms.timestamp {
            self.connection.change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::INTEGER,
                &[owned.time],
            )?;
            return Ok(true);
        }
        let Some(form) = self.atoms.form_of(target) else {
            return Ok(false);
        };
        let kind = self.atoms.form(form);
        let Some(bytes) = self.owned.as_mut().map(|owned| owned.encoded(form)) else {
            return Ok(false);
        };
        if bytes.len() <= self.chunk {
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
            tracing::debug!("an X11 clipboard transfer past {MAX_WRITERS} at once was refused");
            return Ok(false);
        }
        let Ok(size) = u32::try_from(bytes.len()) else {
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
            bytes,
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
        let end = (transfer.written + self.chunk).min(transfer.bytes.len());
        let chunk = &transfer.bytes[transfer.written..end];
        transfer.ended = chunk.is_empty();
        transfer.written = end;
        transfer.deadline = Instant::now() + TRANSFER_TIMEOUT;
        self.connection.change_property8(
            PropMode::REPLACE,
            requestor,
            property,
            transfer.kind,
            chunk,
        )?;
        Ok(())
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
        if self
            .reading
            .as_ref()
            .is_some_and(|reading| reading.deadline <= now)
        {
            tracing::debug!("an X11 clipboard read timed out");
            self.retire_destination();
            self.answer(None);
        }
        let (expired, kept): (Vec<Transfer>, Vec<Transfer>) = std::mem::take(&mut self.transfers)
            .into_iter()
            .partition(|transfer| transfer.deadline <= now);
        self.transfers = kept;
        for transfer in expired {
            tracing::debug!("an X11 clipboard transfer timed out");
            self.forget(transfer.requestor)?;
        }
        Ok(())
    }

    /// The earliest deadline waiting.
    fn deadline(&self) -> Option<Instant> {
        self.reading
            .iter()
            .map(|reading| reading.deadline)
            .chain(self.transfers.iter().map(|transfer| transfer.deadline))
            .min()
    }
}

/// The thread's body: commands and events until `Stop`, or until the
/// connection fails. A panic is contained here: the thread ends, and the
/// reads waiting are abandoned, each drop contained, before the state
/// drops; so are the commands still queued, whose reads were never taken.
pub(super) fn run(mut selections: Selections, commands: Receiver<Command>, bell: &OwnedFd) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        steps(&mut selections, &commands, bell)
    }));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!("the X11 clipboard's connection failed: {error}"),
        Err(payload) => contained(payload),
    }
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
    if let Err(payload) = abandoned {
        contained(payload);
    }
}

/// Report a panic the thread contained. The payload may own anything; it is
/// kept rather than dropped where its destructor could fail too (ADR-0127).
fn contained(payload: Box<dyn std::any::Any + Send>) {
    tracing::error!(
        "the X11 clipboard's thread panicked: {}",
        crate::owner::panic_boundary::panic_payload_message(payload.as_ref())
    );
    std::mem::forget(payload);
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
    use super::not_before;

    /// A time just past the clock's wrap is later than one just before it.
    #[test]
    fn server_times_order_across_the_wrap() {
        assert!(not_before(0x10, 0xffff_fff0));
        assert!(!not_before(0xffff_fff0, 0x10));
        assert!(not_before(5, 5));
        assert!(not_before(6, 5));
        assert!(!not_before(5, 6));
    }
}
