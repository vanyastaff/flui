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
//!   `INCR` transfer, which starts only once the requestor's property
//!   deletions are heard) hears the requestor's window from then on, its
//!   selection checked, and the window's end drops it. An answer decided
//!   later is written once a mark sent after it comes back: a zero-length
//!   replace of a property of the thread's own window, whose
//!   `PropertyNotify` arrives in order with every other event, so every
//!   event before it is handled by then, and the answers follow with no
//!   round trip among them; only that mark's new value counts, and one that
//!   has not come back by [`REPLY_TIMEOUT`] when nothing is left to handle
//!   gives way to a bounded round trip. A window that ended before the mark reached the server is
//!   therefore never answered, nor refused. A requestor already gone when
//!   the thread starts to hear it is not answered; one the server will not
//!   let the thread hear for another reason, or does not answer about in
//!   time, is refused, and so is a request into the thread's own window. A
//!   step handles at most [`MAX_EVENTS_PER_STEP`] events, so a flood delays a
//!   decided answer only by the events ahead of its mark.
//! - **No wait for the server is unbounded.** Every reply the thread waits
//!   on ends at [`REPLY_TIMEOUT`] (the `connection` module), the request
//!   counting as failed: an ownership unconfirmed is given back, a read ends
//!   [`ClipboardError::Timeout`], a requestor that cannot be checked is
//!   refused. When the loop ends (at `Stop`, as a reply fails, or by a panic,
//!   which is carried past this), the requests still waiting on the owner
//!   thread are refused, no transfer starts and no write takes a selection,
//!   and the answers owed are written under a mark; the close waits on the
//!   server, marks and replies alike, at most [`CLOSE_TIMEOUT`], with a last
//!   mark to see the server carry them out, and reports the answers it had
//!   to drop. Reports go to `tracing` once the state is committed, each
//!   contained, so a subscriber that fails costs no answer.
//! - **What X cannot tell apart** is a window that ends, and whose id a new
//!   window takes, after the thread last met the server (the request itself,
//!   for an answer in the step that heard it; the mark, for one decided
//!   later) and before the server carries out the write: only a server grab
//!   would close that.
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

use super::connection::{Connection, REPLY_TIMEOUT, is_late, reply_within};
use flui_foundation::panic::FirstPanic;
use flui_platform::{
    ClipboardEntryId, ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten,
    capability::Resolver,
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use web_time::Instant;
use x11rb::connection::{Connection as _, RequestConnection as _, SequenceNumber};
use x11rb::errors::{ReplyError, ReplyOrIdError};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
    GetPropertyReply, PropMode, Property, SelectionClearEvent, SelectionNotifyEvent,
    SelectionRequestEvent, WindowClass,
};
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
/// selection's text, counted with those whose decided answer waits for its
/// mark; more are refused.
const MAX_ASKS: usize = 16;

/// The most events one step of the thread handles before it carries on with
/// its deadlines and marks: a client that floods it with requests delays
/// the answers decided before the flood by the events ahead of their mark,
/// never by the flood that follows.
const MAX_EVENTS_PER_STEP: usize = 64;

/// How long the thread, as it ends, waits on the server: for its marks to
/// come back (the answers still owed are written once the first does, and
/// the last shows the server carried them out before the connection closes)
/// and for any reply a handler waits on meanwhile. A server another client
/// holds, or a link that died, holds up the application's exit no longer
/// than this, once the thread reads its `Stop`: a reply the thread waited on
/// when it came ends first, at [`REPLY_TIMEOUT`].
const CLOSE_TIMEOUT: Duration = Duration::from_millis(300);

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
    connection: &Connection,
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
    /// The thread's window's properties marks replace, in turn ([`Pending`]).
    marks: [xproto::Atom; 2],
}

impl Atoms {
    fn intern(connection: &Connection) -> Result<Self, ReplyOrIdError> {
        let [
            targets,
            timestamp,
            incr,
            text,
            concealed_hint,
            first,
            second,
        ] = intern(
            connection,
            [
                b"TARGETS",
                b"TIMESTAMP",
                b"INCR",
                b"TEXT",
                CONCEALED_HINT.as_bytes(),
                b"FLUI_SELECTION_MARK_0",
                b"FLUI_SELECTION_MARK_1",
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
            marks: [first, second],
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

/// A request whose answer is decided but written later, once the mark sent
/// after it comes back ([`Pending`]): one that waited on the owner thread,
/// or text that goes `INCR` to a requestor the thread had to start hearing
/// first.
struct Decided {
    request: SelectionRequestEvent,
    property: xproto::Atom,
    form: TextForm,
    verdict: Verdict,
}

impl Decided {
    /// `ask`, answered with `verdict`.
    fn answer(ask: Ask, verdict: Verdict) -> Self {
        Self {
            request: ask.request,
            property: ask.property,
            form: ask.form,
            verdict,
        }
    }
}

/// What a decided request is told.
enum Verdict {
    /// Text that fits one request, and its length in the form asked for.
    Whole(Arc<str>, usize),
    /// Text that goes `INCR`, and its length, to a requestor whose property
    /// deletions the thread hears.
    Incremental(Arc<str>, usize, DeletionsHeard),
    /// Refused, with property `None`.
    Refused,
}

/// Why an `INCR` transfer did not start; its requestor is refused.
#[derive(Debug, Clone, Copy)]
enum TransferRefused {
    /// [`MAX_WRITERS`] transfers run.
    Writers,
    /// The text would take the transfers past [`TRANSFER_BUDGET`].
    Budget,
    /// The length does not fit the header's 32 bits.
    Length,
    /// The thread ends, and the transfer would not outlive its connection.
    Closing,
}

/// What the thread reports through `tracing`. A subscriber is user code,
/// so a report is queued while the state changes and made once the change is
/// committed and its requestors told ([`Selections::report`]), each
/// contained: a subscriber that panics loses no answer, refusal or mark.
#[derive(Debug, Clone, Copy)]
enum Diagnostic {
    /// An `INCR` transfer did not start.
    Transfer(TransferRefused),
    /// A request for the primary selection came while every transfer ran.
    PrimaryPastWriters,
    /// A request for the primary selection came past [`MAX_ASKS`] waiting.
    PrimaryPastAsks,
    /// Text that would go `INCR` was asked for past [`MAX_ASKS`] waiting.
    TransferPastAsks,
    /// The owner thread left requests for the primary selection past their
    /// deadline.
    OwnerSilent,
    /// A transfer's requestor took no chunk before the deadline.
    TransferTimedOut,
    /// Another client took a selection before the write could own it.
    TakenBeforeOwned,
    /// The server did not answer a request in time; it counted as failed.
    ServerLate,
    /// The thread ended before the server read its marks back: these many
    /// answers were never written.
    Dropped(usize),
}

impl Diagnostic {
    fn report(self) {
        match self {
            Self::Transfer(TransferRefused::Writers) => {
                tracing::debug!("an X11 selection transfer past {MAX_WRITERS} at once was refused");
            }
            Self::Transfer(TransferRefused::Budget) => {
                tracing::debug!(
                    "an X11 selection transfer past the retained-text budget was refused"
                );
            }
            Self::Transfer(TransferRefused::Length) => {
                tracing::debug!("an X11 selection transfer past 4 GiB was refused");
            }
            Self::Transfer(TransferRefused::Closing) => {
                tracing::debug!("an X11 selection transfer was refused as the clipboard ended");
            }
            Self::PrimaryPastWriters => tracing::debug!(
                "an X11 primary selection request while {MAX_WRITERS} transfers run was refused"
            ),
            Self::PrimaryPastAsks => {
                tracing::debug!(
                    "an X11 primary selection request past {MAX_ASKS} waiting was refused"
                );
            }
            Self::TransferPastAsks => {
                tracing::debug!("an X11 selection transfer past {MAX_ASKS} waiting was refused");
            }
            Self::OwnerSilent => {
                tracing::debug!(
                    "the owner thread left an X11 primary selection request unanswered"
                );
            }
            Self::TransferTimedOut => tracing::debug!("an X11 selection transfer timed out"),
            Self::TakenBeforeOwned => {
                tracing::debug!("an X11 selection was taken before the write could own it");
            }
            Self::ServerLate => tracing::debug!(
                "the X server did not answer the clipboard in time; the request was dropped"
            ),
            Self::Dropped(answers) => tracing::debug!(
                answers,
                "the X11 clipboard ended before the server read its marks back; answers dropped"
            ),
        }
    }
}

/// Proof that the thread hears a requestor window's property deletions,
/// which ask for an `INCR` transfer's chunks, and its end: made only where
/// the server took that selection ([`Selections::watch_deletions`]), so a
/// transfer starts on no window that cannot answer it. The window is the
/// request's, which the [`Decided`] that holds the proof names.
struct DeletionsHeard(());

/// The decided answers not written yet. A mark is a zero-length replace of
/// a property of the thread's own window, whose `PropertyNotify` comes back
/// in order with every other event: once it is read back, every event before
/// it is handled, a requestor whose window ended before it dropped
/// ([`Selections::gone`]), so the answers decided before it was sent go to
/// no window that took a gone one's id. At most one mark is in flight; marks
/// take turns between two properties, so one that comes back after its
/// deadline ([`Selections::mark_overdue`]) is not taken for the next.
#[derive(Default)]
struct Pending {
    /// Decided before the mark in flight, written as it comes back.
    marked: VecDeque<Decided>,
    /// Decided since; the next mark covers them.
    unmarked: VecDeque<Decided>,
    /// Whether a mark is in flight.
    in_flight: bool,
    /// Which of [`Atoms::marks`] the mark in flight, or the last, replaced.
    mark: usize,
    /// When the mark in flight is overdue.
    deadline: Option<Instant>,
    /// The sequence of a round trip that stands in for an overdue mark: once
    /// every event the server sent before its reply is handled, the mark
    /// counts as back.
    stand_in: Option<SequenceNumber>,
}

impl Pending {
    fn len(&self) -> usize {
        self.marked.len() + self.unmarked.len()
    }

    fn iter(&self) -> impl Iterator<Item = &Decided> {
        self.marked.iter().chain(&self.unmarked)
    }

    fn retain(&mut self, keep: impl Fn(&Decided) -> bool) {
        self.marked.retain(&keep);
        self.unmarked.retain(&keep);
    }
}

/// Whether the thread still serves, or ends: as it ends no request may wait
/// on an owner that is going, and no transfer starts that the connection
/// would not outlive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Serving,
    Closing,
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
        connection: &Connection,
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
        connection: &Connection,
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
        connection: &Connection,
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
        connection: &Connection,
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
        connection: &Connection,
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
        let Some(reply) = self.take_property(connection, window, MAX_TEXT)? else {
            return Ok(());
        };
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
        connection: &Connection,
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
        connection: &Connection,
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
        let Some(reply) =
            self.take_property(connection, window, MAX_TEXT.saturating_sub(received))?
        else {
            return Ok(());
        };
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
    /// so in `bytes_after`. A server that does not answer in time ends the
    /// read with [`ClipboardError::Timeout`]: `None`.
    fn take_property(
        &mut self,
        connection: &Connection,
        window: xproto::Window,
        limit: usize,
    ) -> Result<Option<GetPropertyReply>, ReplyOrIdError> {
        let longs = u32::try_from(limit.div_ceil(4)).expect("BUG: the text limit fits a request");
        let cookie =
            connection.get_property(true, window, self.destination(), AtomEnum::ANY, 0, longs)?;
        let reply = reply_within(connection, cookie)?;
        if reply.is_none() {
            // Its answer, should it come, lands where no read looks.
            self.retire_destination();
            self.answer(Err(ClipboardError::Timeout));
        }
        Ok(reply)
    }

    /// End the read under way if it waited past `now`; the diagnostic, user
    /// code, comes after its readers are answered.
    fn expire(&mut self, now: Instant) {
        if self
            .reading
            .as_ref()
            .is_some_and(|reading| reading.deadline <= now)
        {
            self.retire_destination();
            self.answer(Err(ClipboardError::Timeout));
            tracing::debug!("an X11 selection read timed out");
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

impl Answer {
    /// What the requestor is told now; `None` when nothing is sent yet, or
    /// ever.
    fn told(self) -> Option<Reply> {
        match self {
            Self::Now(reply) => Some(reply),
            Self::Later | Self::Never => None,
        }
    }
}

/// Why the thread does not hear a requestor's window, which anything that
/// waits on the requestor needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotHeard {
    /// The window is gone (`BadWindow`): its request is never answered.
    Gone,
    /// The server refused otherwise (an allocation failure, a security
    /// policy) or did not answer in time: the requestor is there and waits,
    /// so it is refused.
    Refused,
}

impl NotHeard {
    /// What selecting on the requestor's window came to: `Ok` when the
    /// thread hears it.
    fn of(selection: WindowSelection) -> Result<Result<(), Self>, ReplyOrIdError> {
        match selection {
            WindowSelection::Selected => Ok(Ok(())),
            WindowSelection::Gone => Ok(Err(Self::Gone)),
            WindowSelection::Failed(ReplyError::X11Error(_)) => Ok(Err(Self::Refused)),
            WindowSelection::Failed(ReplyError::ConnectionError(error)) if is_late(&error) => {
                Ok(Err(Self::Refused))
            }
            WindowSelection::Failed(ReplyError::ConnectionError(error)) => Err(error.into()),
        }
    }

    /// How the request is answered now.
    fn answer(self) -> Answer {
        match self {
            Self::Gone => Answer::Never,
            Self::Refused => Answer::Now(Reply::Refused),
        }
    }
}

/// The clipboard thread's state over its connection.
pub(super) struct Selections {
    connection: Connection,
    window: xproto::Window,
    atoms: Atoms,
    clipboard: Selection,
    primary: Selection,
    /// What the owner thread hears of the primary selection.
    notice: Arc<Notice>,
    /// The requests waiting on the owner thread for the primary
    /// selection's text.
    asks: Vec<Ask>,
    /// The decided answers, written as their mark comes back.
    pending: Pending,
    /// The conditional clears asked while a clipboard write waits for its
    /// timestamp: served once it owned or failed, in order.
    clears: Vec<(ExpiredEntry, OwedClear)>,
    transfers: Vec<Transfer>,
    chunk: usize,
    phase: Phase,
    /// The reports owed for this step's changes, made once they are
    /// committed ([`Self::report`]).
    diagnostics: Vec<Diagnostic>,
}

impl Selections {
    /// Make the selection window on the root of `screen`, which hears its
    /// own properties change.
    pub(super) fn try_new(
        connection: Connection,
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
            pending: Pending::default(),
            clears: Vec::new(),
            transfers: Vec::new(),
            chunk,
            phase: Phase::Serving,
            diagnostics: Vec::new(),
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
    fn split(&mut self, name: Name) -> (&Connection, xproto::Window, &Atoms, &mut Selection) {
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
            Command::Answer(offer, text) => self.answered(offer, text)?,
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
                    // what the window no longer owns. A server that does not
                    // say leaves the ownership as the thread knows it.
                    if self.still_owned(name)? == Some(false) {
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
        let owned = self.still_owned(Name::Clipboard)?;
        if owned == Some(false) {
            self.lost(Name::Clipboard);
            return Ok(ExpiredClear::NotHeld);
        }
        // The release is safe either way, being ignored once another client
        // took the clipboard later; unconfirmed, it is not reported done.
        let (connection, _, _, clipboard) = self.split(Name::Clipboard);
        clipboard.owned = None;
        connection.set_selection_owner(NONE, clipboard.atom, time)?;
        connection.flush()?;
        Ok(match owned {
            Some(_) => ExpiredClear::Cleared,
            None => ExpiredClear::Failed(ClipboardError::Timeout),
        })
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
                    if self.atoms.marks.contains(&event.atom) {
                        return self.mark_changed(&event);
                    }
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
    /// for their mark, are dropped. Nothing is selected on a window that is
    /// not there.
    fn gone(&mut self, requestor: xproto::Window) {
        self.transfers
            .retain(|transfer| transfer.requestor != requestor);
        self.asks.retain(|ask| ask.request.requestor != requestor);
        self.pending
            .retain(|decided| decided.request.requestor != requestor);
    }

    /// The server's time arrived: own selection `name` with what waits.
    fn own(&mut self, name: Name, time: xproto::Timestamp) -> Result<(), ReplyOrIdError> {
        let selection = self.split(name).3;
        // A stamp of a write a newer one replaced: the newer write takes the
        // stamp of its own append, which follows.
        selection.clocks = selection.clocks.saturating_sub(1);
        if selection.clocks > 0 {
            return Ok(());
        }
        let Some(content) = selection.pending.take() else {
            return Ok(());
        };
        let atom = selection.atom;
        // A clipboard that is ending takes no selection: its connection
        // closes a moment later, and the selection with it.
        let written = if self.phase == Phase::Closing {
            Err(ClipboardError::Gone)
        } else {
            self.connection
                .set_selection_owner(self.window, atom, time)?;
            let cookie = self.connection.get_selection_owner(atom)?;
            match reply_within(&self.connection, cookie)? {
                Some(reply) if reply.owner == self.window => Ok(()),
                Some(_) => {
                    // A client took it at a later time than ours.
                    self.diagnostics.push(Diagnostic::TakenBeforeOwned);
                    Err(ClipboardError::Unavailable)
                }
                None => {
                    // Whether it took is unknown: it is given back at the
                    // same time, which the server carries out in order.
                    self.connection.set_selection_owner(NONE, atom, time)?;
                    self.diagnostics.push(Diagnostic::ServerLate);
                    Err(ClipboardError::Timeout)
                }
            }
        };
        let selection = self.split(name).3;
        if written.is_ok() {
            selection.owned = Some(Owned { content, time });
        } else {
            selection.owned = None;
            if let Content::Offered(offer) = content {
                self.notice.lose(offer);
            }
        }
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
        // The server took it away, as far as anyone can tell.
        if self.still_owned(name)? != Some(true) {
            self.lost(name);
        }
        Ok(())
    }

    /// Whether the window owns selection `name` as far as the server says,
    /// or owns nothing to lose; `None` when the server did not say in time.
    fn still_owned(&mut self, name: Name) -> Result<Option<bool>, ReplyOrIdError> {
        let (connection, window, _, selection) = self.split(name);
        if selection.owned.is_none() {
            return Ok(Some(true));
        }
        let cookie = connection.get_selection_owner(selection.atom)?;
        let owned = reply_within(connection, cookie)?.map(|reply| reply.owner == window);
        if owned.is_none() {
            self.diagnostics.push(Diagnostic::ServerLate);
        }
        Ok(owned)
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
        match self.answer_request(request, property)?.told() {
            Some(reply) => self.send_reply(request, property, reply),
            None => Ok(()),
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
        // A request into the thread's own window would write its properties
        // (a mark's, a clock's, a read's destination): no client may, the
        // thread's own reads take the owned text without one.
        if request.requestor == self.window {
            return Ok(REFUSED);
        }
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
        // An owner that is going never answers.
        if self.phase == Phase::Closing {
            return Ok(REFUSED);
        }
        // With every transfer held, a long text could not be written: the
        // closure is not asked at all, so a requestor that holds the writers
        // and keeps asking costs the application no work.
        if self.transfers.len() >= MAX_WRITERS {
            self.diagnostics.push(Diagnostic::PrimaryPastWriters);
            return Ok(REFUSED);
        }
        if self.asks.len() + self.pending.len() >= MAX_ASKS {
            self.diagnostics.push(Diagnostic::PrimaryPastAsks);
            return Ok(REFUSED);
        }
        // The request outlives this step: its requestor's end has to be
        // heard, or the answer could reach a window that took its id.
        if let Err(not_heard) = self.watch(request.requestor)? {
            return Ok(not_heard.answer());
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
    /// or `INCR` once the thread hears the requestor's property deletions,
    /// the transfer started as the mark sent after it comes back.
    fn answer_text(
        &mut self,
        request: &SelectionRequestEvent,
        property: xproto::Atom,
        form: TextForm,
        text: Arc<str>,
        len: usize,
    ) -> Result<Answer, ReplyOrIdError> {
        if len <= self.chunk {
            self.write_whole(request.requestor, property, form, &text, len)?;
            return Ok(Answer::Now(Reply::Written));
        }
        if self.phase == Phase::Closing {
            return Ok(Answer::Now(Reply::Refused));
        }
        if self.asks.len() + self.pending.len() >= MAX_ASKS {
            self.diagnostics.push(Diagnostic::TransferPastAsks);
            return Ok(Answer::Now(Reply::Refused));
        }
        let heard = match self.watch_deletions(request.requestor)? {
            Ok(heard) => heard,
            Err(not_heard) => return Ok(not_heard.answer()),
        };
        self.pending.unmarked.push_back(Decided {
            request: *request,
            property,
            form,
            verdict: Verdict::Incremental(text, len, heard),
        });
        Ok(Answer::Later)
    }

    /// The owner thread answered `offer` with `text`: the requests that
    /// waited on it are answered with it, if it is still the selection, as
    /// the mark sent after them comes back. Text that goes `INCR` needs its
    /// requestor's property deletions heard first; a requestor gone by then
    /// is not answered.
    fn answered(&mut self, offer: Generation, text: Option<String>) -> Result<(), ReplyOrIdError> {
        let (ready, waiting): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| ask.offer == offer);
        self.asks = waiting;
        let encodings = text
            .filter(|_| self.primary.offered() == Some(offer))
            .map(|text| Encodings::new(ClipboardWrite::text(text)));
        for ask in ready {
            let verdict = match &encodings {
                None => Verdict::Refused,
                Some(encodings) => {
                    let (text, len) = (Arc::clone(&encodings.text), encodings.len_in(ask.form));
                    if len <= self.chunk {
                        Verdict::Whole(text, len)
                    } else {
                        match self.watch_deletions(ask.request.requestor)? {
                            Ok(heard) => Verdict::Incremental(text, len, heard),
                            Err(NotHeard::Gone) => continue,
                            Err(NotHeard::Refused) => Verdict::Refused,
                        }
                    }
                }
            };
            self.pending
                .unmarked
                .push_back(Decided::answer(ask, verdict));
        }
        Ok(())
    }

    /// The requests waiting on an offer that is no longer the primary
    /// selection are refused as the next mark comes back.
    fn settle_asks(&mut self) {
        let current = self.primary.offered();
        let (stale, kept): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| Some(ask.offer) != current);
        self.asks = kept;
        self.pending.unmarked.extend(
            stale
                .into_iter()
                .map(|ask| Decided::answer(ask, Verdict::Refused)),
        );
    }

    /// Send a mark for the answers decided since the last one, unless one
    /// is in flight: they are written as it comes back.
    fn mark_decided(&mut self) -> Result<(), ReplyOrIdError> {
        if self.pending.in_flight || self.pending.unmarked.is_empty() {
            return Ok(());
        }
        self.send_mark()
    }

    /// Send a mark covering every answer decided so far, on the other of
    /// [`Atoms::marks`] than the last, overdue at [`REPLY_TIMEOUT`].
    fn send_mark(&mut self) -> Result<(), ReplyOrIdError> {
        let unmarked = std::mem::take(&mut self.pending.unmarked);
        self.pending.marked.extend(unmarked);
        self.pending.in_flight = true;
        self.pending.stand_in = None;
        self.pending.mark = (self.pending.mark + 1) % self.atoms.marks.len();
        self.pending.deadline = Some(Instant::now() + REPLY_TIMEOUT);
        // A replace never fails on the type or format another client left
        // in the property, as an append would; the server stamps the
        // change, which the window hears.
        self.connection.change_property8(
            PropMode::REPLACE,
            self.window,
            self.atoms.marks[self.pending.mark],
            AtomEnum::INTEGER,
            &[],
        )?;
        Ok(())
    }

    /// A property of the thread's window that marks replace changed: the
    /// mark in flight comes back with the new value of its own property, and
    /// nothing else counts (another client's change or deletion, a mark that
    /// came back after its deadline).
    fn mark_changed(&mut self, event: &xproto::PropertyNotifyEvent) -> Result<(), ReplyOrIdError> {
        let ours = self.pending.in_flight
            && event.atom == self.atoms.marks[self.pending.mark]
            && event.state == Property::NEW_VALUE;
        if ours { self.mark_read() } else { Ok(()) }
    }

    /// The mark in flight did not come back by its deadline (the server is
    /// held, or lost it): one bounded round trip stands in for it. The
    /// answers the mark covers go out once every event the server sent before
    /// the reply is handled ([`Self::handle_events`]), on the same terms, a
    /// step's bound kept; a server that does not answer in time leaves them
    /// for the next deadline.
    fn mark_overdue(&mut self) -> Result<(), ReplyOrIdError> {
        let sync = self.connection.get_input_focus()?;
        let after = sync.sequence_number();
        if reply_within(&self.connection, sync)?.is_none() {
            self.diagnostics.push(Diagnostic::ServerLate);
            self.pending.deadline = Some(Instant::now() + REPLY_TIMEOUT);
        } else {
            self.pending.stand_in = Some(after);
        }
        Ok(())
    }

    /// Handle at most `limit` events; `true` when one was handled. An event
    /// the server sent after the reply standing in for a mark, or a queue
    /// emptied with such a reply read, counts as the mark back.
    fn handle_events(&mut self, limit: usize) -> Result<bool, ReplyOrIdError> {
        let mut handled = false;
        for _ in 0..limit {
            let Some((event, sequence)) = self.connection.poll_for_event_with_sequence()? else {
                self.stood_in(None)?;
                return Ok(handled);
            };
            handled = true;
            self.event(event)?;
            self.stood_in(Some(sequence))?;
        }
        Ok(handled)
    }

    /// The events read so far reached `sequence` (`None`: every one read is
    /// handled): a round trip standing in for the mark in flight counts as
    /// the mark back once they passed its reply.
    fn stood_in(&mut self, sequence: Option<SequenceNumber>) -> Result<(), ReplyOrIdError> {
        let passed = self
            .pending
            .stand_in
            .is_some_and(|after| sequence.is_none_or(|sequence| sequence >= after));
        if passed && self.pending.in_flight {
            self.mark_read()?;
        }
        Ok(())
    }

    /// The mark in flight came back: every event before it is handled, so
    /// the answers it covers are written, with no round trip among them.
    /// Each requestor stays heard until its answer is sent; a refused
    /// transfer is reported once the state is committed.
    fn mark_read(&mut self) -> Result<(), ReplyOrIdError> {
        self.pending.in_flight = false;
        self.pending.deadline = None;
        self.pending.stand_in = None;
        while let Some(requestor) = self
            .pending
            .marked
            .front()
            .map(|decided| decided.request.requestor)
        {
            let before = self.needs(requestor);
            let Some(decided) = self.pending.marked.pop_front() else {
                break;
            };
            let (request, property, form) = (decided.request, decided.property, decided.form);
            let reply = match decided.verdict {
                Verdict::Whole(text, len) => {
                    self.write_whole(requestor, property, form, &text, len)?;
                    Reply::Written
                }
                Verdict::Incremental(text, len, heard) => {
                    match self.start_transfer(requestor, property, form, text, len, heard)? {
                        Ok(()) => Reply::Written,
                        Err(refused) => {
                            self.diagnostics.push(Diagnostic::Transfer(refused));
                            Reply::Refused
                        }
                    }
                }
                Verdict::Refused => Reply::Refused,
            };
            self.send_reply(&request, property, reply)?;
            self.release(requestor, before)?;
        }
        Ok(())
    }

    /// Write `text` in `form`, `len` bytes of it, which fit one request,
    /// into `requestor`'s `property`.
    fn write_whole(
        &self,
        requestor: xproto::Window,
        property: xproto::Atom,
        form: TextForm,
        text: &str,
        len: usize,
    ) -> Result<(), ReplyOrIdError> {
        let (bytes, _) = form.encode_chunk(text, 0, len);
        self.connection.change_property8(
            PropMode::REPLACE,
            requestor,
            property,
            self.atoms.form(form),
            &bytes,
        )?;
        Ok(())
    }

    /// Start an `INCR` transfer of `text` in `form`, `len` bytes of it, into
    /// `requestor`'s `property`, its chunks encoded as they are written;
    /// `_heard` proves the requestor's deletions, which ask for them, are
    /// heard.
    fn start_transfer(
        &mut self,
        requestor: xproto::Window,
        property: xproto::Atom,
        form: TextForm,
        text: Arc<str>,
        len: usize,
        _heard: DeletionsHeard,
    ) -> Result<Result<(), TransferRefused>, ReplyOrIdError> {
        if self.phase == Phase::Closing {
            return Ok(Err(TransferRefused::Closing));
        }
        self.transfers
            .retain(|transfer| transfer.requestor != requestor || transfer.property != property);
        if self.transfers.len() >= MAX_WRITERS {
            return Ok(Err(TransferRefused::Writers));
        }
        if !self.within_budget(&text) {
            return Ok(Err(TransferRefused::Budget));
        }
        let Ok(size) = u32::try_from(len) else {
            return Ok(Err(TransferRefused::Length));
        };
        self.connection.change_property32(
            PropMode::REPLACE,
            requestor,
            property,
            self.atoms.incr,
            &[size],
        )?;
        self.transfers.push(Transfer {
            requestor,
            property,
            kind: self.atoms.form(form),
            form,
            text,
            written: 0,
            ended: false,
            deadline: Instant::now() + TRANSFER_TIMEOUT,
        });
        Ok(Ok(()))
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
    /// moved as it changes ([`Self::watch_deletions`], [`Self::release`]).
    fn needs(&self, requestor: xproto::Window) -> EventMask {
        let transfers = self
            .transfers
            .iter()
            .any(|transfer| transfer.requestor == requestor)
            || self.pending.iter().any(|decided| {
                decided.request.requestor == requestor
                    && matches!(decided.verdict, Verdict::Incremental(..))
            });
        if transfers {
            return transfer_events();
        }
        let waits = self
            .asks
            .iter()
            .any(|ask| ask.request.requestor == requestor)
            || self
                .pending
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
    fn watch(&self, requestor: xproto::Window) -> Result<Result<(), NotHeard>, ReplyOrIdError> {
        if self.hears(requestor) {
            return Ok(Ok(()));
        }
        NotHeard::of(select_checked(
            &self.connection,
            requestor,
            EventMask::STRUCTURE_NOTIFY,
        ))
    }

    /// Hear `requestor`'s property deletions as well as its end, for an
    /// `INCR` transfer about to be decided, its selection checked as
    /// [`Self::watch`]'s is: the proof that it took, or why it did not.
    fn watch_deletions(
        &self,
        requestor: xproto::Window,
    ) -> Result<Result<DeletionsHeard, NotHeard>, ReplyOrIdError> {
        if requestor == self.window || self.needs(requestor) == transfer_events() {
            return Ok(Ok(DeletionsHeard(())));
        }
        let heard = NotHeard::of(select_checked(
            &self.connection,
            requestor,
            transfer_events(),
        ))?;
        Ok(heard.map(|()| DeletionsHeard(())))
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

    /// End what waited past its deadline: reads, transfers and requests on
    /// the owner thread.
    fn expire(&mut self, now: Instant) -> Result<(), ReplyOrIdError> {
        self.clipboard.expire(now);
        self.primary.expire(now);
        let (expired, kept): (Vec<Ask>, Vec<Ask>) = std::mem::take(&mut self.asks)
            .into_iter()
            .partition(|ask| ask.deadline <= now);
        self.asks = kept;
        if !expired.is_empty() {
            self.diagnostics.push(Diagnostic::OwnerSilent);
        }
        self.pending.unmarked.extend(
            expired
                .into_iter()
                .map(|ask| Decided::answer(ask, Verdict::Refused)),
        );
        let (expired, kept): (Vec<Transfer>, Vec<Transfer>) = std::mem::take(&mut self.transfers)
            .into_iter()
            .partition(|transfer| transfer.deadline <= now);
        self.transfers = kept;
        for transfer in expired {
            self.diagnostics.push(Diagnostic::TransferTimedOut);
            self.release(transfer.requestor, transfer_events())?;
        }
        Ok(())
    }

    /// The mark in flight is overdue at `now` and nothing is left to
    /// handle ([`Self::mark_overdue`]). Behind events still to be read, a
    /// mark is not lost, only behind them: a round trip, which waits, would
    /// only fall further behind a flood.
    fn check_mark(&mut self, now: Instant) -> Result<(), ReplyOrIdError> {
        if self.pending.in_flight && self.pending.deadline.is_some_and(|due| due <= now) {
            self.mark_overdue()?;
        }
        Ok(())
    }

    /// Make the reports this step's committed changes owe, each contained;
    /// the first that fails goes on once every one is made.
    fn report(&mut self) {
        let mut failure = FirstPanic::new();
        for diagnostic in std::mem::take(&mut self.diagnostics) {
            failure.contain(|| diagnostic.report());
        }
        failure.resume();
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

    /// The loop ended (at `Stop`, as a reply failed, or by a panic): the
    /// answers owed go out while the connection works, within
    /// [`CLOSE_TIMEOUT`]. The requests waiting on the owner thread are
    /// refused, since the owner is going, and so is every request read
    /// meanwhile that would wait, and text that would start a transfer the
    /// connection would not outlive. A mark covers them, and they are written
    /// as it comes back; a last mark shows the server carried them out
    /// before the connection closes. A server that does not read the marks
    /// back in time is left, the answers dropped reported; every wait for a
    /// reply meanwhile ends at the same deadline. The reports come last,
    /// each contained, so one that fails costs no answer and no mark.
    fn close(&mut self) -> Result<(), ReplyOrIdError> {
        let deadline = Instant::now() + CLOSE_TIMEOUT;
        self.connection.stream().end_by(deadline);
        let delivered = self.deliver_owed(deadline);
        self.report();
        delivered
    }

    /// The close's answers, under marks read back by `deadline`.
    fn deliver_owed(&mut self, deadline: Instant) -> Result<(), ReplyOrIdError> {
        self.phase = Phase::Closing;
        let asks = std::mem::take(&mut self.asks);
        // Answers a panic left behind a mark that already came back are
        // owed first, under the next mark.
        if !self.pending.in_flight {
            let marked = std::mem::take(&mut self.pending.marked);
            let unmarked = std::mem::replace(&mut self.pending.unmarked, marked);
            self.pending.unmarked.extend(unmarked);
        }
        self.pending.unmarked.extend(
            asks.into_iter()
                .map(|ask| Decided::answer(ask, Verdict::Refused)),
        );
        while self.pending.in_flight || !self.pending.unmarked.is_empty() {
            self.mark_decided()?;
            if !self.read_marks(deadline)? {
                self.diagnostics
                    .push(Diagnostic::Dropped(self.pending.len()));
                return Ok(());
            }
        }
        self.send_mark()?;
        self.read_marks(deadline)?;
        Ok(())
    }

    /// Handle events until the mark in flight comes back; `false` when
    /// `deadline` passes first, a flood of events included.
    fn read_marks(&mut self, deadline: Instant) -> Result<bool, ReplyOrIdError> {
        self.connection.flush()?;
        while self.pending.in_flight {
            let now = Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            if !self.handle_events(1)? && self.pending.in_flight {
                self.connection.flush()?;
                wait_for_connection(&self.connection, deadline - now)?;
            }
        }
        Ok(true)
    }

    /// The earliest deadline waiting.
    fn deadline(&self) -> Option<Instant> {
        [&self.clipboard, &self.primary]
            .into_iter()
            .filter_map(|selection| selection.reading.as_ref())
            .map(|reading| reading.deadline)
            .chain(self.transfers.iter().map(|transfer| transfer.deadline))
            .chain(self.asks.iter().map(|ask| ask.deadline))
            .chain(self.pending.deadline)
            .min()
    }
}

/// The thread's body: commands and events until `Stop`, or until the
/// connection fails, then the close ([`Selections::close`]). A panic in the
/// loop is contained and carried past the close, so the answers owed still
/// go out on the connection; then the reads and writes waiting are answered
/// `Gone`, each delivery contained, before the state drops, and so are the
/// commands still queued, whose reads and writes were never taken. The
/// thread's entry is the boundary its first panic reaches: it is reported
/// last, with what ended the loop, so a report that fails cannot skip the
/// rest (ADR-0281).
pub(super) fn run(mut selections: Selections, commands: Receiver<Command>, bell: &OwnedFd) {
    selections.connection.stream().bound_waits();
    let mut failure = FirstPanic::new();
    let stepped = failure.contain(|| steps(&mut selections, &commands, bell));
    // A broken connection fails the close at once; the loop's own failure
    // is the one reported.
    let closed = failure.contain(|| selections.close());
    failure.contain(|| selections.abandon());
    // The connection closes with the thread, and the selections it owned
    // with it: the owner's turn retires the offers.
    failure.contain(|| selections.notice.end());
    // A read or write answers `Gone` as it drops, here or with the queue.
    while let Ok(command) = commands.try_recv() {
        failure.contain(|| drop(command));
    }
    failure.contain(|| drop(commands));
    let failed = match (stepped, closed) {
        (Some(Err(error)), _) | (_, Some(Err(error))) => Some(error),
        _ => None,
    };
    if let Some(error) = failed {
        report(|| tracing::error!(%error, "the X11 clipboard's connection failed"));
    }
    if let Some(payload) = failure.into_payload() {
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
                // [`run`] closes, answering what this batch decided.
                return Ok(());
            }
        }
        // Whether this step handled an event: then the next follows at
        // once, since a reply read meanwhile may have queued events the
        // wait cannot see, and the bound may have left some.
        let handled = selections.handle_events(MAX_EVENTS_PER_STEP)?;
        selections.expire(Instant::now())?;
        if !handled {
            selections.check_mark(Instant::now())?;
        }
        selections.mark_decided()?;
        selections.connection.flush()?;
        // The step's changes are committed and its requestors told.
        selections.report();
        if handled {
            continue;
        }
        wait(selections, bell)?;
    }
}

/// Wait at most `timeout` for the connection to have something to read.
fn wait_for_connection(connection: &Connection, timeout: Duration) -> Result<(), ReplyOrIdError> {
    // Rounded up, so a wait never wakes just before its deadline.
    let timeout = Timespec::try_from(timeout + Duration::from_millis(1))
        .unwrap_or_else(|_| Timespec::try_from(CLOSE_TIMEOUT).expect("BUG: it fits"));
    let stream = connection.stream().as_fd();
    let mut fds = [PollFd::new(&stream, PollFlags::IN)];
    match poll(&mut fds, Some(&timeout)) {
        Ok(_) | Err(Errno::INTR) => Ok(()),
        Err(error) => Err(x11rb::errors::ConnectionError::IoError(error.into()).into()),
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
    use x11rb::errors::{ConnectionError, ReplyError};
    use x11rb::protocol::ErrorKind;
    use x11rb::x11_utils::X11Error;

    use std::sync::Arc;

    use flui_foundation::panic::FirstPanic;
    use web_time::Instant;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{self, ConnectionExt as _, Property};
    use x11rb::rust_connection::RustConnection;

    use super::super::connection::{LATE, connect};
    use super::{
        Answer, Ask, Decided, DeletionsHeard, Generation, MAX_EVENTS_PER_STEP, MAX_TEXT, NotHeard,
        Notice, Reply, Selections, TRANSFER_TIMEOUT, TextForm, Verdict, WindowSelection, append,
        not_before,
    };

    /// The clipboard thread's state on the run's X server, driven by the case
    /// on its own thread; `None` without an X server.
    fn selections() -> Option<Selections> {
        if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
            eprintln!("skipped: no X server (DISPLAY is not set); CI runs this under xvfb-run");
            return None;
        }
        let (connection, screen) = connect(None).expect("the X server answers");
        let notice = Arc::new(Notice::new(Box::new(|| {})));
        Some(Selections::try_new(connection, screen, notice).expect("the window is made"))
    }

    /// A requestor's window on a connection of its own.
    fn requestor() -> (RustConnection, xproto::Window) {
        let (connection, screen) = RustConnection::connect(None).expect("the X server answers");
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id().expect("an id");
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                xproto::WindowClass::INPUT_ONLY,
                x11rb::COPY_FROM_PARENT,
                &xproto::CreateWindowAux::new(),
            )
            .expect("the request is sent");
        connection
            .get_input_focus()
            .expect("the request is sent")
            .reply()
            .expect("the server answers");
        (connection, window)
    }

    /// A request of the primary selection from `requestor` into property 1.
    fn request(requestor: xproto::Window) -> xproto::SelectionRequestEvent {
        xproto::SelectionRequestEvent {
            response_type: xproto::SELECTION_REQUEST_EVENT,
            sequence: 0,
            time: x11rb::CURRENT_TIME,
            owner: x11rb::NONE,
            requestor,
            selection: xproto::Atom::from(xproto::AtomEnum::PRIMARY),
            target: xproto::Atom::from(xproto::AtomEnum::STRING),
            property: 1,
        }
    }

    /// The property of the next `SelectionNotify` `connection` hears.
    fn told(connection: &RustConnection) -> xproto::Atom {
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(Event::SelectionNotify(notify)) =
                connection.poll_for_event().expect("the connection holds")
            {
                return notify.property;
            }
            assert!(Instant::now() < deadline, "the requestor was told nothing");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// A `tracing` subscriber that panics at every event, as an application's
    /// faulty one does.
    struct PanicsAtEvents;

    impl tracing::Subscriber for PanicsAtEvents {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, _: &tracing::Event<'_>) {
            panic!("a subscriber's own failure");
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// A request the owner thread left past its deadline is refused though
    /// the subscriber that hears of it panics: the refusal is queued before
    /// the report is made.
    #[test]
    fn a_failing_subscriber_loses_no_refusal_of_an_expired_request() {
        let Some(mut selections) = selections() else {
            return;
        };
        let now = Instant::now();
        selections.asks.push(Ask {
            request: request(0x0100_0001),
            property: 1,
            form: TextForm::Latin1,
            offer: Generation::default().next(),
            deadline: now,
        });
        let mut failure = FirstPanic::new();
        tracing::subscriber::with_default(PanicsAtEvents, || {
            failure.contain(|| selections.expire(now + TRANSFER_TIMEOUT));
            failure.contain(|| selections.report());
        });
        assert!(failure.held().is_some(), "the subscriber failed");
        assert_eq!(
            selections.pending.unmarked.len(),
            1,
            "the refusal is queued for the next mark"
        );
    }

    /// A refused transfer's report that panics as the thread closes leaves
    /// the rest of the close done: the requestor is told, and the last mark
    /// comes back.
    #[test]
    fn a_failing_subscriber_as_the_thread_closes_loses_no_answer() {
        let Some(mut selections) = selections() else {
            return;
        };
        let (requestor, window) = requestor();
        let text: Arc<str> = Arc::from("x".repeat(1 << 20));
        let len = text.len();
        selections.pending.marked.push_back(Decided {
            request: request(window),
            property: 1,
            form: TextForm::Latin1,
            verdict: Verdict::Incremental(text, len, DeletionsHeard(())),
        });
        let mut failure = FirstPanic::new();
        tracing::subscriber::with_default(PanicsAtEvents, || {
            failure.contain(|| selections.close());
        });
        assert!(failure.held().is_some(), "the subscriber failed");
        assert_eq!(told(&requestor), x11rb::NONE, "the transfer is refused");
        assert!(!selections.pending.in_flight, "the last mark came back");
    }

    /// Only the mark in flight coming back with its property's new value
    /// counts: another client's deletion of that property does not, and the
    /// answers it covers wait for the mark itself.
    #[test]
    fn only_the_mark_coming_back_counts() {
        let Some(mut selections) = selections() else {
            return;
        };
        let (_requestor, window) = requestor();
        selections.pending.marked.push_back(Decided {
            request: request(window),
            property: 1,
            form: TextForm::Latin1,
            verdict: Verdict::Refused,
        });
        selections.pending.in_flight = true;
        let changed = |state| {
            Event::PropertyNotify(xproto::PropertyNotifyEvent {
                response_type: xproto::PROPERTY_NOTIFY_EVENT,
                sequence: 0,
                window: selections.window,
                atom: selections.atoms.marks[selections.pending.mark],
                time: 0,
                state,
            })
        };
        let deleted = changed(Property::DELETE);
        let replaced = changed(Property::NEW_VALUE);
        selections.event(deleted).expect("the connection holds");
        assert_eq!(selections.pending.marked.len(), 1, "a deletion is no mark");
        selections.event(replaced).expect("the connection holds");
        assert!(selections.pending.marked.is_empty(), "the mark came back");
    }

    /// A mark that does not come back by its deadline gives way to one
    /// bounded round trip, after which the answers it covers are written.
    #[test]
    fn an_overdue_mark_gives_way_to_a_round_trip() {
        let Some(mut selections) = selections() else {
            return;
        };
        let (_requestor, window) = requestor();
        selections.pending.marked.push_back(Decided {
            request: request(window),
            property: 1,
            form: TextForm::Latin1,
            verdict: Verdict::Refused,
        });
        // In flight, though nothing was sent: no mark will come back.
        selections.pending.in_flight = true;
        let now = Instant::now();
        selections.pending.deadline = Some(now);
        selections
            .check_mark(now + TRANSFER_TIMEOUT)
            .expect("the connection holds");
        // The events the server sent before the round trip's reply are
        // handled as a step handles events.
        selections
            .handle_events(MAX_EVENTS_PER_STEP)
            .expect("the connection holds");
        assert!(
            selections.pending.marked.is_empty(),
            "the answers went out behind the round trip"
        );
        assert!(!selections.pending.in_flight);
    }

    /// A requestor whose window is gone is never answered, since the window
    /// that takes its id did not ask; one the server will not let the thread
    /// hear for another reason, or does not answer about in time, is there and
    /// waiting, and is sent a refusal (ICCCM §2.2). The same holds for the
    /// selection of its property deletions an `INCR` transfer needs. Xvfb
    /// refuses these selections only with `BadWindow`, so the other refusals
    /// are fed in at the seam.
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
        let late = WindowSelection::Failed(ReplyError::ConnectionError(ConnectionError::IoError(
            std::io::Error::new(std::io::ErrorKind::TimedOut, LATE),
        )));
        let table = [
            ("heard", WindowSelection::Selected, Ok(()), None),
            ("gone", WindowSelection::Gone, Err(NotHeard::Gone), None),
            (
                "access",
                refused(ErrorKind::Access),
                Err(NotHeard::Refused),
                Some(Reply::Refused),
            ),
            (
                "alloc",
                refused(ErrorKind::Alloc),
                Err(NotHeard::Refused),
                Some(Reply::Refused),
            ),
            ("late", late, Err(NotHeard::Refused), Some(Reply::Refused)),
        ];
        for (case, selection, heard, sent) in table {
            let found = NotHeard::of(selection).expect("no connection error");
            assert_eq!(found, heard, "{case}: heard");
            assert_eq!(
                found.err().map(NotHeard::answer).and_then(Answer::told),
                sent,
                "{case}: what the requestor is sent"
            );
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
