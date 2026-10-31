//! XDND on the native X11 host's windows, as targets and as sources (XDND
//! version 5, ADR-0038, ADR-0211).
//!
//! [`X11Drag`] is the platform's one [`DataTransferSource`] and the loop's
//! `LoopDrag`: the loop hands it every event before the input hears it, and
//! a window tells it as it opens and closes. A window that opens is marked
//! `XdndAware`, so a source anywhere on the display (a file manager, a
//! browser, another FLUI application) offers it its drags:
//!
//! - **Entering.** `XdndEnter` names the source's window and the types it
//!   offers, the first three in the message, the rest in its `XdndTypeList`.
//!   The offer is minted at the first `XdndPosition`, which carries the
//!   pointer and the action the source proposes, so the window hears
//!   `Entered` with both.
//! - **Hovering.** Each `XdndPosition` is answered `XdndStatus` from the
//!   feedback the window's target last cached, as the other hosts answer
//!   their OS's hover queries, and asks for every movement (an empty
//!   rectangle), so a change of feedback reaches the source at the next
//!   one.
//! - **Dropping.** `XdndDrop` resolves the action from the cached feedback;
//!   a refusal answers `XdndFinished` at once and the window hears `Exited`.
//!   An accepted drop keeps the offer until the target concludes it, which
//!   answers `XdndFinished` with the action.
//! - **Fetching.** A request converts `XdndSelection` to the
//!   representation's type at the drag's last time, into a window made for
//!   that fetch alone and destroyed as it ends, so an answer a source writes
//!   late reaches no later fetch. It is read on the owner thread as the
//!   loop hands over the `SelectionNotify`, `INCR` a chunk at a time. At
//!   most [`MAX_FETCHES`] run at once, more wait their turn; each is held to
//!   its request's limit and gets [`TRANSFER_TIMEOUT`] between answers,
//!   which the loop wakes for, rung when a request from another thread
//!   starts a fetch.
//!
//! A window is a source too: its drag source runs the runtime's OS session
//! over XDND from the window ([`source`]).
//!
//! No method dispatches with the transfer's state locked, and no completer
//! is completed under it: a window's handler, which may ask for the payload
//! or conclude the drop at once, and a request's waker run with nothing of
//! the transfer held.
//!
//! What the protocol decides with no server call (the messages, the
//! actions, which types offer which representations and what their bytes
//! carry) is [`model`], checked with no X server.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests;

mod source;

pub(crate) use source::X11DragSource;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, RepresentationDescriptor,
    RepresentationIndex, TransferActions, TransferCompleter, TransferError, TransferLimits,
    TransferPayload, TransferRequest,
};
use flui_platform::drag::PlatformDragSource;
use flui_platform::{DragDropEvent, PlatformInput};
use parking_lot::Mutex;
use web_time::Instant;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    Property, PropertyNotifyEvent, SelectionNotifyEvent, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_FROM_PARENT, CURRENT_TIME, NONE};

use crate::drag::offers::OfferTable;
use crate::owner::linux::x11::{InputTarget, LoopDrag, WindowRef, X11Connection};
use crate::owner::slot::OwnerCell;
use model::{Kind, Message, MessageAtoms};

/// How long a fetch waits for the source between answers.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(5);

/// The most fetches converting at once, one property each; more wait.
const MAX_FETCHES: usize = 4;

/// The most bytes an `INCR` fetch reserves on its announcement.
const RESERVED: usize = 4 * 1024 * 1024;

/// The most types read of a source's `XdndTypeList`.
const MAX_TYPES: u32 = 256;

/// The most atom names the transfer keeps, of the types sources offered.
const MAX_NAMES: usize = 1024;

/// The most actions read of a source's `XdndActionList`.
const MAX_ACTIONS: u32 = 16;

/// The protocol, its actions and its types, with no server call: plain
/// functions over atoms and bytes.
pub(crate) mod model {
    use flui_platform::data_transfer::{
        Mime, TransferActions, TransferError, TransferFormat, TransferPayload,
    };
    use x11rb::NONE;
    use x11rb::protocol::xproto::{Atom, Timestamp, Window};

    use crate::clipboard::text::TextForm;
    use crate::drag::offers::{format_of, payload_of_bytes};

    /// The XDND version the windows speak, which `XdndAware` names.
    pub(crate) const VERSION: u32 = 5;

    /// The oldest source answered: version 3 is the first whose drop names
    /// the time `XdndSelection` is converted at, and every toolkit speaks
    /// at least that.
    pub(crate) const OLDEST: u32 = 3;

    /// The message atoms a target reads.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct MessageAtoms {
        pub(crate) enter: Atom,
        pub(crate) position: Atom,
        pub(crate) leave: Atom,
        pub(crate) drop: Atom,
    }

    /// A message a source sends its target.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Message {
        /// A drag entered: the source's version, whether it lists its types
        /// in `XdndTypeList`, and the first three types.
        Enter {
            source: Window,
            version: u32,
            listed: bool,
            types: Vec<Atom>,
        },
        /// The pointer is at `root` (on the root window) at `time`, and the
        /// source proposes `action`.
        Position {
            source: Window,
            root: (i16, i16),
            time: Timestamp,
            action: Atom,
        },
        /// The drag left, or the source gave up.
        Leave { source: Window },
        /// The user dropped at `time`.
        Drop { source: Window, time: Timestamp },
    }

    impl Message {
        /// The message a client message of `kind` carries; `None` for any
        /// other.
        pub(crate) fn read(kind: Atom, data: [u32; 5], atoms: &MessageAtoms) -> Option<Self> {
            let source = data[0];
            if kind == atoms.enter {
                Some(Self::Enter {
                    source,
                    version: data[1] >> 24,
                    listed: data[1] & 1 != 0,
                    types: data[2..]
                        .iter()
                        .copied()
                        .filter(|&atom| atom != NONE)
                        .collect(),
                })
            } else if kind == atoms.position {
                // The root position is two 16-bit coordinates, x high.
                let [x_high, x_low, y_high, y_low] = data[2].to_be_bytes();
                Some(Self::Position {
                    source,
                    root: (
                        i16::from_be_bytes([x_high, x_low]),
                        i16::from_be_bytes([y_high, y_low]),
                    ),
                    time: data[3],
                    action: data[4],
                })
            } else if kind == atoms.leave {
                Some(Self::Leave { source })
            } else if kind == atoms.drop {
                Some(Self::Drop {
                    source,
                    time: data[2],
                })
            } else {
                None
            }
        }
    }

    /// `XdndStatus`'s data: `target` accepts with `action`, or refuses, and
    /// asks for every movement (no rectangle to stay quiet in), so a change
    /// of its answer reaches the source at the next one.
    pub(crate) fn status(target: Window, action: Option<Atom>) -> [u32; 5] {
        const ACCEPTS: u32 = 1;
        const EVERY_MOVEMENT: u32 = 1 << 1;
        let flags = if action.is_some() { ACCEPTS } else { 0 } | EVERY_MOVEMENT;
        [target, flags, 0, 0, action.unwrap_or(NONE)]
    }

    /// `XdndFinished`'s data: `target` took the drop with `action`, or did
    /// not take it.
    pub(crate) fn finished(target: Window, action: Option<Atom>) -> [u32; 5] {
        [
            target,
            u32::from(action.is_some()),
            action.unwrap_or(NONE),
            0,
            0,
        ]
    }

    /// The action atoms.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ActionAtoms {
        pub(crate) copy: Atom,
        pub(crate) move_: Atom,
        pub(crate) link: Atom,
        pub(crate) ask: Atom,
    }

    impl ActionAtoms {
        /// The actions `action` permits: copy, move or link as named;
        /// `XdndActionAsk` and `XdndActionPrivate`, which name none, and an
        /// unknown atom permit nothing ([`Self::is_ask`] tells the source's
        /// list is to be read).
        pub(crate) fn actions_of(&self, action: Atom) -> TransferActions {
            if action == self.copy {
                TransferActions::COPY
            } else if action == self.move_ {
                TransferActions::MOVE
            } else if action == self.link {
                TransferActions::LINK
            } else {
                TransferActions::empty()
            }
        }

        /// Whether `action` asks the user, with the choices in the source's
        /// `XdndActionList`.
        pub(crate) fn is_ask(&self, action: Atom) -> bool {
            action == self.ask
        }

        /// The atom of one resolved action.
        pub(crate) fn atom_of(&self, action: TransferActions) -> Atom {
            if action == TransferActions::MOVE {
                self.move_
            } else if action == TransferActions::LINK {
                self.link
            } else {
                self.copy
            }
        }
    }

    /// What a representation's bytes are read as.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) enum Kind {
        /// Text in one of its forms.
        Text(TextForm),
        /// HTML markup, UTF-8 or UTF-16 behind its byte-order mark.
        Html,
        /// The bytes of a media type, as the source wrote them.
        Bytes(TransferFormat),
    }

    impl Kind {
        /// The format the representation announces.
        pub(crate) fn format(&self) -> TransferFormat {
            match self {
                Self::Text(_) => TransferFormat::Text,
                Self::Html => TransferFormat::Html,
                Self::Bytes(format) => format.clone(),
            }
        }

        /// The payload `bytes` of this kind carry.
        pub(crate) fn payload(&self, mut bytes: Vec<u8>) -> TransferPayload {
            match self {
                Self::Text(form) => TransferPayload::Text(form.decode(bytes)),
                Self::Html => TransferPayload::Html(html(bytes)),
                Self::Bytes(TransferFormat::UriList) => {
                    // A list some sources end with a NUL.
                    if bytes.last() == Some(&0) {
                        bytes.pop();
                    }
                    TransferPayload::Custom {
                        mime: Mime::new("text/uri-list"),
                        bytes: bytes.into(),
                    }
                }
                Self::Bytes(format) => payload_of_bytes(format, bytes),
            }
        }
    }

    /// HTML markup: UTF-16 behind a byte-order mark, as Firefox writes it,
    /// UTF-8 otherwise, with any ill-formed sequence replaced and a
    /// trailing NUL dropped.
    fn html(bytes: Vec<u8>) -> String {
        let utf16 = |pairs: &[u8], unit: fn([u8; 2]) -> u16| {
            let units = pairs.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
            char::decode_utf16(units)
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect::<String>()
        };
        let mut text = match bytes.as_slice() {
            [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
            [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
            _ => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
            },
        };
        if text.ends_with('\0') {
            text.pop();
        }
        text
    }

    /// The representations a drag's `types` (each an atom and its name)
    /// offer, each with the type it is converted to, in the source's order:
    /// text once, in the form a reader prefers, where the first text form
    /// stood; HTML; every other media type as named. A type that names no
    /// media type (`TARGETS`, `COMPOUND_TEXT`, a toolkit's private name) is
    /// left out, as is a second one of a format.
    pub(crate) fn representations(types: &[(Atom, &str)]) -> Vec<(Atom, Kind)> {
        let text = TextForm::preferred(types.iter().map(|(_, name)| *name)).and_then(|form| {
            let (atom, _) = types
                .iter()
                .find(|(_, name)| TextForm::named(name) == Some(form))?;
            Some((*atom, Kind::Text(form)))
        });
        let mut offered: Vec<(Atom, Kind)> = Vec::new();
        for (atom, name) in types {
            let kind = if TextForm::named(name).is_some() {
                match &text {
                    Some(text) => text.clone(),
                    None => continue,
                }
            } else if name.split(';').next().map(str::trim) == Some("text/html") {
                (*atom, Kind::Html)
            } else {
                match format_of(name) {
                    Some(format) => (*atom, Kind::Bytes(format)),
                    None => continue,
                }
            };
            if offered
                .iter()
                .all(|(_, held)| held.format() != kind.1.format())
            {
                offered.push(kind);
            }
        }
        offered
    }

    /// `XdndEnter`'s data from `source`, which speaks [`VERSION`] and offers
    /// `types`: the first three in the message, all of them in
    /// `XdndTypeList` when there are more.
    pub(crate) fn enter(source: Window, types: &[Atom]) -> [u32; 5] {
        let listed = types.len() > 3;
        let mut inline = [NONE; 3];
        for (slot, atom) in inline.iter_mut().zip(types) {
            *slot = *atom;
        }
        [
            source,
            (VERSION << 24) | u32::from(listed),
            inline[0],
            inline[1],
            inline[2],
        ]
    }

    /// `XdndPosition`'s data: the pointer at `root` on the root window at
    /// `time`, proposing `action`.
    pub(crate) fn position(
        source: Window,
        root: (i16, i16),
        time: Timestamp,
        action: Atom,
    ) -> [u32; 5] {
        let [x_high, x_low] = root.0.to_be_bytes();
        let [y_high, y_low] = root.1.to_be_bytes();
        [
            source,
            0,
            u32::from_be_bytes([x_high, x_low, y_high, y_low]),
            time,
            action,
        ]
    }

    /// `XdndLeave`'s data.
    pub(crate) fn leave(source: Window) -> [u32; 5] {
        [source, 0, 0, 0, 0]
    }

    /// `XdndDrop`'s data: the drop at `time`, which the target converts
    /// `XdndSelection` at.
    pub(crate) fn drop_at(source: Window, time: Timestamp) -> [u32; 5] {
        [source, 0, time, 0, 0]
    }

    /// The message atoms a source reads.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ReplyAtoms {
        pub(crate) status: Atom,
        pub(crate) finished: Atom,
    }

    /// A message a target sends its source.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Reply {
        /// `target` accepts the drag with `action`, or refuses it (`None`).
        Status {
            target: Window,
            action: Option<Atom>,
        },
        /// `target` is done with the drop. A version 5 target says whether
        /// it took it (`Some`, with its action) or not (`None`); an older
        /// one says nothing, which reads as taken with no action named
        /// (`Some(NONE)`).
        Finished {
            target: Window,
            action: Option<Atom>,
        },
    }

    impl Reply {
        /// The reply a client message of `kind` carries from a target that
        /// speaks `version`; `None` for any other message.
        pub(crate) fn read(
            kind: Atom,
            data: [u32; 5],
            version: u32,
            atoms: &ReplyAtoms,
        ) -> Option<Self> {
            let target = data[0];
            if kind == atoms.status {
                let accepts = data[1] & 1 != 0;
                Some(Self::Status {
                    target,
                    action: accepts.then_some(data[4]),
                })
            } else if kind == atoms.finished {
                let action = if version < 5 {
                    Some(NONE)
                } else {
                    (data[1] & 1 != 0).then_some(data[2])
                };
                Some(Self::Finished { target, action })
            } else {
                None
            }
        }
    }

    /// The type names a representation of `format` is offered under, the
    /// most precise first: text in UTF-8 under the names every toolkit
    /// reads, every other format under its media type.
    pub(crate) fn names_of(format: &TransferFormat) -> Vec<String> {
        match format {
            TransferFormat::Text => vec![
                "UTF8_STRING".to_owned(),
                "text/plain;charset=utf-8".to_owned(),
                "text/plain".to_owned(),
            ],
            TransferFormat::Html => vec!["text/html".to_owned()],
            TransferFormat::UriList => vec!["text/uri-list".to_owned()],
            TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
                vec![mime.as_str().to_owned()]
            }
        }
    }

    /// The bytes `payload` is written as: text and HTML in UTF-8, any other
    /// payload as it is.
    pub(crate) fn bytes_of(payload: TransferPayload) -> Vec<u8> {
        match payload {
            TransferPayload::Text(text) | TransferPayload::Html(text) => text.into_bytes(),
            TransferPayload::Custom { bytes, .. } => bytes.to_vec(),
        }
    }

    /// The error a payload of `len` bytes past `limit` is refused with.
    pub(crate) fn too_large(len: usize, limit: u64) -> Option<TransferError> {
        let actual = u64::try_from(len).unwrap_or(u64::MAX);
        (actual > limit).then_some(TransferError::TooLarge { actual, limit })
    }
}

/// The atoms the transfer speaks.
#[derive(Debug, Clone, Copy)]
struct Atoms {
    aware: xproto::Atom,
    status: xproto::Atom,
    finished: xproto::Atom,
    selection: xproto::Atom,
    type_list: xproto::Atom,
    action_list: xproto::Atom,
    incr: xproto::Atom,
    messages: MessageAtoms,
    actions: model::ActionAtoms,
    /// The property a fetch's requestor window is converted into.
    property: xproto::Atom,
    /// `TARGETS`, which a target may convert the selection to first.
    targets: xproto::Atom,
}

impl Atoms {
    fn intern(connection: &X11Connection) -> Result<Self, crate::PlatformError> {
        const NAMES: [&[u8]; 17] = [
            b"XdndAware",
            b"XdndEnter",
            b"XdndPosition",
            b"XdndStatus",
            b"XdndLeave",
            b"XdndDrop",
            b"XdndFinished",
            b"XdndSelection",
            b"XdndTypeList",
            b"XdndActionList",
            b"XdndActionCopy",
            b"XdndActionMove",
            b"XdndActionLink",
            b"XdndActionAsk",
            b"INCR",
            b"FLUI_XDND",
            b"TARGETS",
        ];
        let failed = |error: &dyn std::fmt::Display| crate::PlatformError::Init {
            message: format!("cannot intern the XDND atoms: {error}"),
        };
        let xcb = connection.xcb();
        let cookies = NAMES
            .iter()
            .map(|name| xcb.intern_atom(false, name))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| failed(&error))?;
        let mut atoms = [0; NAMES.len()];
        for (atom, cookie) in atoms.iter_mut().zip(cookies) {
            *atom = cookie.reply().map_err(|error| failed(&error))?.atom;
        }
        let [
            aware,
            enter,
            position,
            status,
            leave,
            drop,
            finished,
            selection,
            type_list,
            action_list,
            copy,
            move_,
            link,
            ask,
            incr,
            property,
            targets,
        ] = atoms;
        Ok(Self {
            aware,
            status,
            finished,
            selection,
            type_list,
            action_list,
            incr,
            messages: MessageAtoms {
                enter,
                position,
                leave,
                drop,
            },
            actions: model::ActionAtoms {
                copy,
                move_,
                link,
                ask,
            },
            property,
            targets,
        })
    }
}

/// Where a drag over a window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Entered, and no position told yet: no offer is minted.
    Entered,
    /// The window heard `Entered` for this offer.
    Over(DataTransferId),
    /// The target concluded the offer while the drag hovered: the drag is
    /// refused from then on, and the window hears nothing more of it.
    Concluded,
}

/// The drag over one of the windows: XDND runs one at a time on a display.
#[derive(Debug)]
struct Hover {
    source: xproto::Window,
    window: WindowRef,
    /// The type each representation is converted to, by index.
    types: Arc<[(xproto::Atom, Kind)]>,
    phase: Phase,
    /// The actions the source's `XdndActionList` names, read once, for an
    /// `XdndActionAsk`.
    listed: Option<TransferActions>,
}

/// A live offer, hovering or dropped.
#[derive(Debug)]
struct Offer {
    source: xproto::Window,
    /// The window the drag is over or was dropped on, which answers the
    /// source.
    window: xproto::Window,
    types: Arc<[(xproto::Atom, Kind)]>,
    feedback: DropFeedback,
    /// The actions the source permitted at the last position.
    allowed: TransferActions,
    /// The last position the window was told.
    position: Option<Point<f64>>,
    /// The time `XdndSelection` is converted at: the last position's, then
    /// the drop's.
    time: xproto::Timestamp,
    /// The action the drop took, once dropped.
    dropped: Option<TransferActions>,
}

/// A request for one representation.
#[derive(Debug)]
struct Wanted {
    id: DataTransferId,
    target: xproto::Atom,
    kind: Kind,
    time: xproto::Timestamp,
    limit: u64,
    completer: TransferCompleter,
}

/// A fetch converting into a requestor window of its own, destroyed as the
/// fetch ends: a source that answers late, or goes on writing chunks, writes
/// to no window, never into a later fetch.
#[derive(Debug)]
struct Fetch {
    requestor: xproto::Window,
    wanted: Wanted,
    /// The bytes of an `INCR` transfer, once announced.
    incremental: Option<Vec<u8>>,
    deadline: Instant,
}

/// What a fetch's read of its property came to.
enum Read {
    /// The whole payload, or an `INCR` transfer's last (empty) chunk.
    Done(Vec<u8>),
    /// The owner goes `INCR`, announcing this many bytes.
    Incremental(usize),
    /// One `INCR` chunk.
    Chunk(Vec<u8>),
}

/// What the transfer's lock guards.
#[derive(Debug, Default)]
struct State {
    table: OfferTable,
    windows: HashMap<xproto::Window, InputTarget>,
    hover: Option<Hover>,
    offers: HashMap<DataTransferId, Offer>,
    /// The fetches converting, at most [`MAX_FETCHES`].
    fetches: Vec<Fetch>,
    /// Requests waiting for a fetch to end.
    waiting: VecDeque<Wanted>,
    /// The name the server gave each atom a source offered that has one, at
    /// most [`MAX_NAMES`]: an atom the server cannot name is asked again.
    names: HashMap<xproto::Atom, String>,
}

/// What a step leaves to do once the lock is let go: completers to resolve
/// and an event for a window, and whether a fetch started.
#[derive(Default)]
struct After {
    completions: Vec<(TransferCompleter, Result<TransferPayload, TransferError>)>,
    started: bool,
    dispatch: Option<(InputTarget, DragDropEvent)>,
}

impl After {
    fn gone(&mut self, completer: TransferCompleter) {
        self.completions
            .push((completer, Err(TransferError::SourceGone)));
    }

    /// `fetch` ended with `outcome`: its requestor goes.
    fn end(
        &mut self,
        wire: &Wire<'_>,
        fetch: Fetch,
        outcome: Result<TransferPayload, TransferError>,
    ) {
        wire.close(fetch.requestor);
        self.completions.push((fetch.wanted.completer, outcome));
    }
}

/// The requests that make and end a fetch's requestor, sent with the
/// transfer's lock held: a fetch is made and ended on the wire in the order
/// its state changes, whatever thread changes it, so no window is made for a
/// fetch already ended. They are queued, not flushed; nothing waits for the
/// server under the lock.
struct Wire<'a> {
    connection: &'a X11Connection,
    selection: xproto::Atom,
    property: xproto::Atom,
}

impl Wire<'_> {
    /// Make `requestor` and ask the source to convert into it.
    fn open(&self, requestor: xproto::Window, target: xproto::Atom, time: xproto::Timestamp) {
        let xcb = self.connection.xcb();
        let started = xcb
            .create_window(
                0,
                requestor,
                self.connection.root(),
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map(drop)
            .and_then(|()| {
                xcb.convert_selection(requestor, self.selection, target, self.property, time)
                    .map(drop)
            });
        if let Err(error) = started {
            // The fetch times out.
            tracing::debug!(%error, "cannot ask the XDND source for its data");
        }
    }

    /// Destroy `requestor`: what a source writes to it late goes nowhere.
    fn close(&self, requestor: xproto::Window) {
        if let Err(error) = self.connection.xcb().destroy_window(requestor) {
            tracing::debug!(%error, "cannot destroy an XDND requestor");
        }
    }
}

impl State {
    /// Retire `id`: its fetches and waiting requests resolve as
    /// [`TransferError::SourceGone`], and what waits for another offer
    /// starts in the fetches it leaves.
    fn retire(&mut self, id: DataTransferId, wire: &Wire<'_>, after: &mut After) -> Option<Offer> {
        let _ = self.table.retire(id);
        let (stale, kept): (Vec<Fetch>, Vec<Fetch>) = std::mem::take(&mut self.fetches)
            .into_iter()
            .partition(|fetch| fetch.wanted.id == id);
        self.fetches = kept;
        for fetch in stale {
            after.end(wire, fetch, Err(TransferError::SourceGone));
        }
        let (stale, kept): (VecDeque<Wanted>, VecDeque<Wanted>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|wanted| wanted.id == id);
        self.waiting = kept;
        for wanted in stale {
            after.gone(wanted.completer);
        }
        self.start_waiting(wire, after);
        self.offers.remove(&id)
    }

    /// End the drag over a window: its offer, if minted and not dropped, is
    /// retired, and the window hears `Exited`.
    fn end_hover(&mut self, wire: &Wire<'_>, after: &mut After) {
        let Some(hover) = self.hover.take() else {
            return;
        };
        let Phase::Over(id) = hover.phase else {
            return;
        };
        self.retire(id, wire, after);
        if let Some(target) = self.windows.get(&hover.window.xid) {
            after.dispatch = Some((target.clone(), DragDropEvent::Exited { id }));
        }
    }

    /// Start the waiting requests the fetches have room for, each with a
    /// requestor of its own.
    fn start_waiting(&mut self, wire: &Wire<'_>, after: &mut After) {
        while self.fetches.len() < MAX_FETCHES {
            let Some(wanted) = self.waiting.pop_front() else {
                return;
            };
            if wanted.completer.is_cancelled() {
                after.gone(wanted.completer);
                continue;
            }
            // An id is the client's to number: no round trip but when its
            // range runs out.
            let requestor = match wire.connection.xcb().generate_id() {
                Ok(requestor) => requestor,
                Err(error) => {
                    tracing::debug!(%error, "no X id for an XDND requestor");
                    after.gone(wanted.completer);
                    continue;
                }
            };
            wire.open(requestor, wanted.target, wanted.time);
            after.started = true;
            self.fetches.push(Fetch {
                requestor,
                wanted,
                incremental: None,
                deadline: Instant::now() + TRANSFER_TIMEOUT,
            });
        }
    }

    /// The fetch converting into `requestor`.
    fn fetch(&mut self, requestor: xproto::Window) -> Option<&mut Fetch> {
        self.fetches
            .iter_mut()
            .find(|fetch| fetch.requestor == requestor)
    }

    /// Take the fetch converting into `requestor` out.
    fn take_fetch(&mut self, requestor: xproto::Window) -> Option<Fetch> {
        let at = self
            .fetches
            .iter()
            .position(|fetch| fetch.requestor == requestor)?;
        Some(self.fetches.swap_remove(at))
    }
}

/// The X11 host's drag and drop: its windows as XDND targets, and as the
/// sources of the sessions they start.
pub(crate) struct X11Drag {
    connection: Arc<X11Connection>,
    atoms: Atoms,
    /// Wakes the owner's loop, which reads the deadline of a fetch a request
    /// from another thread started.
    wake: Box<dyn Fn() + Send + Sync>,
    state: Mutex<State>,
    /// The session a window started, on the thread that made the transfer.
    session: OwnerCell<Option<source::Session>>,
    /// The windows whose press a session took, for the loop to have their
    /// input end it.
    taken: OwnerCell<Vec<xproto::Window>>,
    /// The server time of the last pointer or key event the loop heard.
    last_time: AtomicU32,
}

impl std::fmt::Debug for X11Drag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Drag").finish_non_exhaustive()
    }
}

impl X11Drag {
    /// The transfer on `connection`, which calls `wake` to have the owner's
    /// loop read its deadlines again.
    ///
    /// # Errors
    /// [`PlatformError::Init`](crate::PlatformError::Init) when its atoms
    /// cannot be interned.
    pub(crate) fn try_new(
        connection: &Arc<X11Connection>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>, crate::PlatformError> {
        Ok(Arc::new(Self {
            connection: Arc::clone(connection),
            atoms: Atoms::intern(connection)?,
            wake,
            state: Mutex::new(State::default()),
            session: OwnerCell::new(),
            taken: OwnerCell::new(),
            last_time: AtomicU32::new(0),
        }))
    }

    /// Send `data` of `kind` to `window`.
    fn send(&self, window: xproto::Window, kind: xproto::Atom, data: [u32; 5]) {
        let event = ClientMessageEvent::new(32, window, kind, data);
        if let Err(error) =
            self.connection
                .xcb()
                .send_event(false, window, EventMask::NO_EVENT, event)
        {
            tracing::debug!(%error, "cannot send an XDND message");
        }
    }

    /// The requests of the fetches' requestors.
    fn wire(&self) -> Wire<'_> {
        Wire {
            connection: &self.connection,
            selection: self.atoms.selection,
            property: self.atoms.property,
        }
    }

    /// Do what a step left, with no lock held, and flush what it sent.
    fn finish(&self, after: After) {
        if let Err(error) = self.connection.xcb().flush() {
            tracing::debug!(%error, "cannot send the XDND transfer's requests");
        }
        if after.started {
            // A fetch has a deadline the loop may not be waiting for.
            (self.wake)();
        }
        if let Some((target, event)) = after.dispatch {
            target
                .delivery
                .dispatch_input(PlatformInput::DragDrop(event));
        }
        for (completer, result) in after.completions {
            completer.complete(result);
        }
    }

    /// The names of `atoms` the server has not named to the transfer yet,
    /// asked in one batch.
    fn learn_names(&self, atoms: &[xproto::Atom]) {
        let unknown: Vec<xproto::Atom> = {
            let state = self.state.lock();
            atoms
                .iter()
                .copied()
                .filter(|atom| !state.names.contains_key(atom))
                .collect()
        };
        if unknown.is_empty() {
            return;
        }
        let xcb = self.connection.xcb();
        let cookies: Vec<_> = unknown
            .iter()
            .map(|&atom| xcb.get_atom_name(atom).ok())
            .collect();
        let named: Vec<(xproto::Atom, String)> = unknown
            .into_iter()
            .zip(cookies)
            .filter_map(|(atom, cookie)| {
                let reply = cookie?.reply().ok()?;
                Some((atom, String::from_utf8(reply.name).ok()?))
            })
            .collect();
        let mut state = self.state.lock();
        if state.names.len().saturating_add(named.len()) > MAX_NAMES {
            // Names are learned again as sources offer them.
            state.names.clear();
        }
        state.names.extend(named);
    }

    /// The atoms of a source's property `name`, at most `most`.
    fn atoms_of(&self, source: xproto::Window, name: xproto::Atom, most: u32) -> Vec<xproto::Atom> {
        self.connection
            .xcb()
            .get_property(false, source, name, AtomEnum::ATOM, 0, most)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(Iterator::collect))
            .unwrap_or_default()
    }

    /// The position `root` on the root window, in `target`'s logical
    /// pixels; `None` when the server cannot place it.
    fn position_in(&self, target: &InputTarget, root: (i16, i16)) -> Option<Point<f64>> {
        let reply = self
            .connection
            .xcb()
            .translate_coordinates(self.connection.root(), target.window.xid, root.0, root.1)
            .ok()?
            .reply()
            .ok()?;
        let scale = target.scale.get();
        Some(Point::new(
            f64::from(reply.dst_x) / scale,
            f64::from(reply.dst_y) / scale,
        ))
    }

    /// A drag entered `window`; one that never left is left first.
    fn enter(
        &self,
        window: xproto::Window,
        source: xproto::Window,
        version: u32,
        listed: bool,
        types: Vec<xproto::Atom>,
    ) {
        let mut after = After::default();
        let entered = {
            let mut state = self.state.lock();
            state.end_hover(&self.wire(), &mut after);
            match state.windows.get(&window) {
                Some(target) if version >= model::OLDEST => {
                    let target = target.window;
                    state.hover = Some(Hover {
                        source,
                        window: target,
                        types: Arc::from([]),
                        phase: Phase::Entered,
                        listed: None,
                    });
                    true
                }
                Some(_) => {
                    tracing::debug!(
                        version,
                        "an XDND source older than version 3 is not answered"
                    );
                    false
                }
                None => false,
            }
        };
        self.finish(after);
        if !entered {
            return;
        }
        let types = if listed {
            self.atoms_of(source, self.atoms.type_list, MAX_TYPES)
        } else {
            types
        };
        self.learn_names(&types);
        let mut state = self.state.lock();
        let named: Vec<(xproto::Atom, &str)> = types
            .iter()
            .filter_map(|atom| Some((*atom, state.names.get(atom)?.as_str())))
            .collect();
        let offered: Arc<[(xproto::Atom, Kind)]> = model::representations(&named).into();
        if let Some(hover) = state.hover.as_mut().filter(|hover| hover.source == source) {
            hover.types = offered;
        }
    }

    /// The pointer moved over `window`: the window hears it, then the
    /// source is answered from the feedback cached after.
    fn position(
        &self,
        window: xproto::Window,
        source: xproto::Window,
        root: (i16, i16),
        time: xproto::Timestamp,
        action: xproto::Atom,
    ) {
        let ask = self.atoms.actions.is_ask(action);
        let entered = {
            let state = self.state.lock();
            match (&state.hover, state.windows.get(&window)) {
                (Some(hover), Some(target))
                    if hover.source == source && hover.window.xid == window =>
                {
                    Some((target.clone(), ask && hover.listed.is_none()))
                }
                _ => None,
            }
        };
        let Some((target, read_list)) = entered else {
            // A position from a source that did not enter: refused.
            self.refuse(source, window);
            return;
        };
        let listed = read_list.then(|| {
            self.atoms_of(source, self.atoms.action_list, MAX_ACTIONS)
                .into_iter()
                .fold(TransferActions::empty(), |actions, atom| {
                    actions | self.atoms.actions.actions_of(atom)
                })
        });
        let position = self.position_in(&target, root);
        let mut after = After::default();
        let over = {
            let mut state = self.state.lock();
            let State {
                table,
                hover,
                offers,
                ..
            } = &mut *state;
            match hover.as_mut().filter(|hover| hover.source == source) {
                Some(hover) => {
                    if let Some(listed) = listed {
                        hover.listed = Some(listed);
                    }
                    let allowed = if ask {
                        hover.listed.unwrap_or_default()
                    } else {
                        self.atoms.actions.actions_of(action)
                    };
                    match hover.phase {
                        Phase::Entered => {
                            let representations: Arc<[RepresentationDescriptor]> = hover
                                .types
                                .iter()
                                .map(|(_, kind)| RepresentationDescriptor {
                                    format: kind.format(),
                                    declared_len: None,
                                })
                                .collect();
                            let id = table.mint(Arc::clone(&representations));
                            offers.insert(
                                id,
                                Offer {
                                    source,
                                    window,
                                    types: Arc::clone(&hover.types),
                                    feedback: DropFeedback::default(),
                                    allowed,
                                    position,
                                    time,
                                    dropped: None,
                                },
                            );
                            hover.phase = Phase::Over(id);
                            after.dispatch = Some((
                                target,
                                DragDropEvent::Entered {
                                    offer: DataTransferOffer::new(id, representations),
                                    allowed,
                                    position,
                                },
                            ));
                            Some(id)
                        }
                        Phase::Over(id) => {
                            if let Some(offer) = offers.get_mut(&id) {
                                offer.allowed = allowed;
                                offer.time = time;
                                // A point the window cannot place is no
                                // movement: it keeps the position it was
                                // told last.
                                if let Some(position) = position {
                                    offer.position = Some(position);
                                    after.dispatch = Some((
                                        target,
                                        DragDropEvent::Moved {
                                            id,
                                            allowed,
                                            position,
                                        },
                                    ));
                                }
                            }
                            Some(id)
                        }
                        Phase::Concluded => None,
                    }
                }
                None => None,
            }
        };
        self.finish(after);
        // The window may have concluded the offer, or closed, from inside its
        // callback: the answer is what is cached now.
        let answer = over.and_then(|id| {
            let state = self.state.lock();
            let hovering = state
                .hover
                .as_ref()
                .is_some_and(|hover| hover.phase == Phase::Over(id));
            let offer = state.offers.get(&id).filter(|_| hovering)?;
            offer.feedback.resolve(offer.allowed)
        });
        let action = answer.map(|action| self.atoms.actions.atom_of(action));
        self.send(source, self.atoms.status, model::status(window, action));
        self.flush();
    }

    /// Refuse a drag over `window`.
    fn refuse(&self, source: xproto::Window, window: xproto::Window) {
        self.send(source, self.atoms.status, model::status(window, None));
        self.flush();
    }

    /// Whether a fetch converts into `window`.
    fn fetching(&self, window: xproto::Window) -> bool {
        self.state
            .lock()
            .fetches
            .iter()
            .any(|fetch| fetch.requestor == window)
    }

    fn flush(&self) {
        if let Err(error) = self.connection.xcb().flush() {
            tracing::debug!(%error, "cannot send an XDND message");
        }
    }

    /// The drag left, or its source gave up.
    fn leave(&self, source: xproto::Window) {
        let mut after = After::default();
        {
            let mut state = self.state.lock();
            if state
                .hover
                .as_ref()
                .is_some_and(|hover| hover.source == source)
            {
                state.end_hover(&self.wire(), &mut after);
            }
        }
        self.finish(after);
    }

    /// The user dropped: the window hears `Dropped` with the action its
    /// cached feedback resolves, or `Exited` when it refuses, which the
    /// source hears at once.
    fn drop_on(&self, source: xproto::Window, time: xproto::Timestamp) {
        let mut after = After::default();
        let refused = {
            let mut state = self.state.lock();
            let Some(hover) = state.hover.as_ref().filter(|hover| hover.source == source) else {
                return;
            };
            let window = hover.window.xid;
            let accepted = match hover.phase {
                Phase::Over(id) => state
                    .offers
                    .get(&id)
                    .and_then(|offer| offer.feedback.resolve(offer.allowed))
                    .zip(state.windows.get(&window).cloned())
                    .map(|(action, target)| (id, action, target)),
                Phase::Entered | Phase::Concluded => None,
            };
            if let Some((id, action, target)) = accepted {
                state.hover = None;
                let offer = state
                    .offers
                    .get_mut(&id)
                    .expect("BUG: a hovering offer is live");
                offer.dropped = Some(action);
                // A source that names no time leaves the last position's.
                if time != CURRENT_TIME {
                    offer.time = time;
                }
                after.dispatch = Some((
                    target,
                    DragDropEvent::Dropped {
                        id,
                        action,
                        position: offer.position,
                    },
                ));
                None
            } else {
                state.end_hover(&self.wire(), &mut after);
                Some(window)
            }
        };
        if let Some(window) = refused {
            self.send(source, self.atoms.finished, model::finished(window, None));
        }
        self.finish(after);
    }

    /// The fetch converting into `requestor` read `read`, or failed.
    fn fetched(&self, requestor: xproto::Window, read: Result<Read, TransferError>) {
        let mut after = After::default();
        {
            let mut state = self.state.lock();
            let Some(fetch) = state.fetch(requestor) else {
                return;
            };
            let limit = fetch.wanted.limit;
            let outcome = match read {
                Err(error) => Some(Err(error)),
                Ok(Read::Incremental(announced)) => {
                    if let Some(error) = model::too_large(announced, limit) {
                        Some(Err(error))
                    } else {
                        // What a source announces is reserved only up to a
                        // bound: the chunks prove the rest.
                        fetch.incremental = Some(Vec::with_capacity(announced.min(RESERVED)));
                        fetch.deadline = Instant::now() + TRANSFER_TIMEOUT;
                        None
                    }
                }
                Ok(Read::Chunk(chunk)) => {
                    let bytes = fetch
                        .incremental
                        .as_mut()
                        .expect("BUG: a chunk is read only of an INCR fetch");
                    if let Some(error) =
                        model::too_large(bytes.len().saturating_add(chunk.len()), limit)
                    {
                        Some(Err(error))
                    } else {
                        bytes.extend_from_slice(&chunk);
                        fetch.deadline = Instant::now() + TRANSFER_TIMEOUT;
                        None
                    }
                }
                Ok(Read::Done(bytes)) => {
                    let bytes = fetch.incremental.take().unwrap_or(bytes);
                    let payload = fetch.wanted.kind.payload(bytes);
                    let len = usize::try_from(payload.byte_len()).unwrap_or(usize::MAX);
                    Some(model::too_large(len, limit).map_or(Ok(payload), Err))
                }
            };
            if let Some(outcome) = outcome {
                let fetch = state
                    .take_fetch(requestor)
                    .expect("BUG: the fetch was found under this lock");
                after.end(&self.wire(), fetch, outcome);
                state.start_waiting(&self.wire(), &mut after);
            }
        }
        self.finish(after);
    }

    /// The source answered a conversion.
    fn selection_notify(&self, event: &SelectionNotifyEvent) {
        let limit = {
            let mut state = self.state.lock();
            let Some(fetch) = state.fetch(event.requestor) else {
                return;
            };
            if fetch.incremental.is_some() || fetch.wanted.target != event.target {
                return;
            }
            fetch.wanted.limit
        };
        let read = if event.property == NONE {
            // The source refuses a type it offered.
            Err(TransferError::SourceGone)
        } else {
            self.read_property(event.requestor, None, limit)
        };
        self.fetched(event.requestor, read);
    }

    /// The source wrote an `INCR` chunk.
    fn property_notify(&self, event: &PropertyNotifyEvent) {
        if event.state != Property::NEW_VALUE || event.atom != self.atoms.property {
            return;
        }
        let found = {
            let mut state = self.state.lock();
            state.fetch(event.window).and_then(|fetch| {
                let held = fetch.incremental.as_ref()?.len();
                Some((held, fetch.wanted.limit))
            })
        };
        let Some((held, limit)) = found else {
            return;
        };
        let read = self.read_property(event.window, Some(held), limit);
        self.fetched(event.window, read);
    }

    /// Read and delete the property of `requestor`, refusing it when it
    /// would take the fetch past `limit` bytes before it is read: the first
    /// answer of a conversion, or an `INCR` chunk after the `held` bytes.
    fn read_property(
        &self,
        requestor: xproto::Window,
        held: Option<usize>,
        limit: u64,
    ) -> Result<Read, TransferError> {
        let first = held.is_none();
        let xcb = self.connection.xcb();
        let property = self.atoms.property;
        let failed = |error: &dyn std::fmt::Display| {
            tracing::debug!(%error, "cannot read the XDND source's data");
            TransferError::SourceGone
        };
        // Its size first, read without its value or deleting it.
        let size = xcb
            .get_property(false, requestor, property, AtomEnum::ANY, 0, 0)
            .map_err(|error| failed(&error))?
            .reply()
            .map_err(|error| failed(&error))?;
        if first && size.type_ == self.atoms.incr {
            // The announcement is one word, whatever else the source wrote;
            // deleting it asks for the first chunk.
            let reply = xcb
                .get_property(true, requestor, property, AtomEnum::ANY, 0, 1)
                .map_err(|error| failed(&error))?
                .reply()
                .map_err(|error| failed(&error))?;
            let announced = reply
                .value32()
                .and_then(|mut words| words.next())
                .ok_or(TransferError::SourceGone)?;
            return Ok(Read::Incremental(
                usize::try_from(announced).unwrap_or(usize::MAX),
            ));
        }
        let len = usize::try_from(size.bytes_after)
            .unwrap_or(usize::MAX)
            .saturating_add(held.unwrap_or_default());
        if let Some(error) = model::too_large(len, limit) {
            drop(xcb.delete_property(requestor, property));
            return Err(error);
        }
        let bytes = xcb
            .get_property(
                true,
                requestor,
                property,
                AtomEnum::ANY,
                0,
                size.bytes_after.div_ceil(4),
            )
            .map_err(|error| failed(&error))?
            .reply()
            .map_err(|error| failed(&error))?
            .value;
        Ok(if first || bytes.is_empty() {
            Read::Done(bytes)
        } else {
            Read::Chunk(bytes)
        })
    }
}

impl LoopDrag for X11Drag {
    fn drag_source(self: Arc<Self>, window: WindowRef) -> Arc<dyn PlatformDragSource> {
        Arc::new(X11DragSource::new(self, window))
    }

    fn window_opened(&self, target: InputTarget) {
        let xid = target.window.xid;
        if let Err(error) = self.connection.xcb().change_property32(
            PropMode::REPLACE,
            xid,
            self.atoms.aware,
            AtomEnum::ATOM,
            &[model::VERSION],
        ) {
            tracing::debug!(%error, "cannot mark an X11 window as an XDND target");
        }
        let replaced = self.state.lock().windows.insert(xid, target);
        // The entry a window with the same X id left goes with no lock held.
        drop(replaced);
    }

    fn window_closed(&self, window: WindowRef) {
        self.session_window_closed(window.xid);
        let mut after = After::default();
        let removed = {
            let mut state = self.state.lock();
            if state
                .windows
                .get(&window.xid)
                .is_none_or(|target| target.window.id != window.id)
            {
                return;
            }
            let removed = state.windows.remove(&window.xid);
            // The drag over it ends with nothing to tell; a drop the window
            // took stays until its target concludes it.
            if state
                .hover
                .as_ref()
                .is_some_and(|hover| hover.window.id == window.id)
            {
                state.end_hover(&self.wire(), &mut after);
            }
            removed
        };
        drop(removed);
        self.finish(after);
    }

    fn event(&self, event: &Event) -> bool {
        if self.session_event(event) {
            return true;
        }
        match event {
            Event::ClientMessage(message) if message.format == 32 => {
                let Some(read) = Message::read(
                    message.type_,
                    message.data.as_data32(),
                    &self.atoms.messages,
                ) else {
                    return false;
                };
                if !self.state.lock().windows.contains_key(&message.window) {
                    return false;
                }
                let window = message.window;
                match read {
                    Message::Enter {
                        source,
                        version,
                        listed,
                        types,
                    } => self.enter(window, source, version, listed, types),
                    Message::Position {
                        source,
                        root,
                        time,
                        action,
                    } => self.position(window, source, root, time, action),
                    Message::Leave { source } => self.leave(source),
                    Message::Drop { source, time } => self.drop_on(source, time),
                }
                true
            }
            Event::SelectionNotify(notify) if self.fetching(notify.requestor) => {
                self.selection_notify(notify);
                true
            }
            Event::PropertyNotify(property) if self.fetching(property.window) => {
                self.property_notify(property);
                true
            }
            _ => false,
        }
    }

    fn deadline(&self) -> Option<Instant> {
        let fetches = self
            .state
            .lock()
            .fetches
            .iter()
            .map(|fetch| fetch.deadline)
            .min();
        fetches.into_iter().chain(self.session_deadline()).min()
    }

    fn pointers_taken(&self) -> Vec<xproto::Window> {
        self.taken_pointers()
    }

    fn expire(&self, now: Instant) {
        self.session_expire(now);
        let mut after = After::default();
        {
            let mut state = self.state.lock();
            let (ended, live): (Vec<Fetch>, Vec<Fetch>) = std::mem::take(&mut state.fetches)
                .into_iter()
                .partition(|fetch| fetch.deadline <= now || fetch.wanted.completer.is_cancelled());
            state.fetches = live;
            if !ended.is_empty() {
                for fetch in ended {
                    after.end(&self.wire(), fetch, Err(TransferError::SourceGone));
                }
                state.start_waiting(&self.wire(), &mut after);
            }
        }
        self.finish(after);
    }
}
impl DataTransferSource for X11Drag {
    /// The clipboard is the clipboard feature's.
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let mut after = After::default();
        let request = {
            let mut state = self.state.lock();
            let Some(offer) = state.offers.get(&id) else {
                return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
            };
            let Some((target, kind)) = offer.types.get(usize::from(representation.0)).cloned()
            else {
                return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                    id,
                    index: representation,
                }));
            };
            let time = offer.time;
            let (request, completer) = TransferRequest::pending();
            state.waiting.push_back(Wanted {
                id,
                target,
                kind,
                time,
                limit: limits.max_bytes,
                completer,
            });
            state.start_waiting(&self.wire(), &mut after);
            request
        };
        self.finish(after);
        request
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        if let Some(offer) = self
            .state
            .lock()
            .offers
            .get_mut(&id)
            .filter(|offer| offer.dropped.is_none())
        {
            offer.feedback = feedback;
        }
    }

    fn conclude_drop(&self, id: DataTransferId) {
        let mut after = After::default();
        let finished = {
            let mut state = self.state.lock();
            if let Some(hover) = state
                .hover
                .as_mut()
                .filter(|hover| hover.phase == Phase::Over(id))
            {
                hover.phase = Phase::Concluded;
            }
            state
                .retire(id, &self.wire(), &mut after)
                .and_then(|offer| {
                    let action = offer.dropped?;
                    Some((offer.source, offer.window, action))
                })
        };
        if let Some((source, window, action)) = finished {
            let action = self.atoms.actions.atom_of(action);
            self.send(
                source,
                self.atoms.finished,
                model::finished(window, Some(action)),
            );
        }
        self.finish(after);
    }
}
