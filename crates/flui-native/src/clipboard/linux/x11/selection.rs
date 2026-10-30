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
//!   its text at once. Otherwise the thread asks the owner to convert the
//!   selection into its window's property, in each of [`READ_ORDER`] until
//!   one is answered, and reads the property (deleting it, which for `INCR`
//!   asks for the next chunk). Reads that arrive meanwhile join the one
//!   under way. A read gets [`TRANSFER_TIMEOUT`] in all and at most
//!   [`MAX_TEXT`] bytes.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::mpsc::Receiver;
use std::time::Duration;

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
    /// The window's property a read is converted into.
    destination: xproto::Atom,
    /// The window's property a write appends to for a timestamp.
    clock: xproto::Atom,
    /// Each of [`TextForm::OFFERED`], by its position there.
    forms: [xproto::Atom; 4],
}

impl Atoms {
    fn intern(connection: &RustConnection) -> Result<Self, ReplyOrIdError> {
        let names: [&[u8]; 7] = [
            b"CLIPBOARD",
            b"TARGETS",
            b"TIMESTAMP",
            b"INCR",
            b"TEXT",
            b"FLUI_CLIPBOARD",
            b"FLUI_CLIPBOARD_CLOCK",
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
            destination,
            clock,
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
            destination,
            clock,
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

/// The text the application owns the selection with, and since when.
struct Owned {
    text: String,
    time: xproto::Timestamp,
}

/// An `INCR` write: the requestor's property, filled a chunk at a time.
struct Transfer {
    requestor: xproto::Window,
    property: xproto::Atom,
    kind: xproto::Atom,
    bytes: Vec<u8>,
    written: usize,
    /// Whether the zero-length chunk that ends it is written.
    ended: bool,
    deadline: Instant,
}

/// The read under way: the form asked for and what came so far.
struct Reading {
    form: TextForm,
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
    /// The reads waiting for the one under way.
    readers: Vec<Resolver<Option<String>>>,
    reading: Option<Reading>,
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
            readers: Vec::new(),
            reading: None,
            transfers: Vec::new(),
            chunk,
        })
    }

    /// Carry out `command`; `false` once it asks the thread to end.
    fn command(&mut self, command: Command) -> Result<bool, ReplyOrIdError> {
        match command {
            Command::Write(text) => {
                self.pending = Some(text);
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
                if let Some(owned) = &self.owned {
                    // A retired owner has nobody left to answer.
                    let _ = answer.resolve(Some(owned.text.clone()));
                } else {
                    self.readers.push(answer);
                    if self.reading.is_none() {
                        self.convert(READ_ORDER[0], Instant::now() + TRANSFER_TIMEOUT)?;
                    }
                }
            }
            Command::Stop => return Ok(false),
        }
        Ok(true)
    }

    /// Ask the owner for the selection as `form`.
    fn convert(&mut self, form: TextForm, deadline: Instant) -> Result<(), ReplyOrIdError> {
        self.connection.convert_selection(
            self.window,
            self.atoms.clipboard,
            self.atoms.form(form),
            self.atoms.destination,
            CURRENT_TIME,
        )?;
        self.reading = Some(Reading {
            form,
            incremental: None,
            deadline,
        });
        Ok(())
    }

    /// End the read under way, answering every reader with `text`.
    fn answer(&mut self, text: Option<String>) {
        self.reading = None;
        for reader in self.readers.drain(..) {
            let _ = reader.resolve(text.clone());
        }
    }

    fn event(&mut self, event: Event) -> Result<(), ReplyOrIdError> {
        match event {
            Event::PropertyNotify(event) => {
                if event.window == self.window && event.atom == self.atoms.clock {
                    self.own(event.time)?;
                } else if event.window == self.window
                    && event.atom == self.atoms.destination
                    && event.state == Property::NEW_VALUE
                {
                    self.read_chunk()?;
                } else if event.state == Property::DELETE {
                    self.write_chunk(event.window, event.atom)?;
                }
            }
            Event::SelectionNotify(event) => self.converted(&event)?,
            Event::SelectionRequest(event) => self.request(&event)?,
            Event::SelectionClear(event) => self.cleared(&event),
            // A requestor that went away fails the writes to it; its
            // transfer ends at its deadline.
            _ => {}
        }
        Ok(())
    }

    /// The server's time arrived: own the selection with the pending text.
    fn own(&mut self, time: xproto::Timestamp) -> Result<(), ReplyOrIdError> {
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
            self.owned = Some(Owned { text, time });
        } else {
            // A client took it at a later time than ours.
            self.owned = None;
            tracing::debug!("the X11 clipboard was taken before the write could own it");
        }
        Ok(())
    }

    fn cleared(&mut self, event: &SelectionClearEvent) {
        if event.owner == self.window && event.selection == self.atoms.clipboard {
            self.owned = None;
        }
    }

    /// The owner answered a conversion: read its property, or ask for the
    /// next form if it refused.
    fn converted(&mut self, event: &SelectionNotifyEvent) -> Result<(), ReplyOrIdError> {
        if event.requestor != self.window || event.selection != self.atoms.clipboard {
            return Ok(());
        }
        let Some(reading) = &self.reading else {
            return Ok(());
        };
        if event.property == NONE {
            let next = READ_ORDER
                .iter()
                .skip_while(|form| **form != reading.form)
                .nth(1);
            match next {
                Some(&form) => {
                    let deadline = reading.deadline;
                    self.convert(form, deadline)?;
                }
                None => self.answer(None),
            }
            return Ok(());
        }
        let reply = self.take_property()?;
        if reply.type_ == self.atoms.incr {
            // Deleting the property asked for the first chunk.
            if let Some(reading) = &mut self.reading {
                reading.incremental = Some(Vec::new());
            }
            return Ok(());
        }
        let form = reading.form;
        let text = (reply.bytes_after == 0).then(|| form.decode(reply.value));
        self.answer(text);
        Ok(())
    }

    /// The owner wrote an `INCR` chunk.
    fn read_chunk(&mut self) -> Result<(), ReplyOrIdError> {
        if self
            .reading
            .as_ref()
            .is_none_or(|reading| reading.incremental.is_none())
        {
            return Ok(());
        }
        let reply = self.take_property()?;
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        let Some(bytes) = &mut reading.incremental else {
            return Ok(());
        };
        if reply.value.is_empty() {
            let form = reading.form;
            let bytes = std::mem::take(bytes);
            self.answer(Some(form.decode(bytes)));
        } else if bytes.len() + reply.value.len() > MAX_TEXT || reply.bytes_after != 0 {
            self.answer(None);
        } else {
            bytes.extend_from_slice(&reply.value);
        }
        Ok(())
    }

    /// Read and delete the window's destination property, at most
    /// [`MAX_TEXT`] bytes of it.
    fn take_property(&self) -> Result<GetPropertyReply, ReplyOrIdError> {
        let longs = u32::try_from(MAX_TEXT / 4).expect("BUG: the text limit fits a request");
        Ok(self
            .connection
            .get_property(
                true,
                self.window,
                self.atoms.destination,
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
            && (request.time == CURRENT_TIME || request.time >= owned.time);
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
        let bytes = form.encode(&owned.text);
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
        if self
            .transfers
            .iter()
            .all(|transfer| transfer.requestor != requestor)
        {
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
/// reads waiting are dropped unanswered with the state.
pub(super) fn run(mut selections: Selections, commands: &Receiver<Command>, bell: &OwnedFd) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        steps(&mut selections, commands, bell)
    }));
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!("the X11 clipboard's connection failed: {error}"),
        Err(payload) => {
            tracing::error!(
                "the X11 clipboard's thread panicked: {}",
                crate::owner::panic_boundary::panic_payload_message(payload.as_ref())
            );
            // The payload may own anything; it is kept rather than dropped
            // where its destructor could fail too (ADR-0127).
            std::mem::forget(payload);
        }
    }
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
