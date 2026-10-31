//! XDND drops against a real X server: a source on a connection of its own
//! drags over a window of the transfer's connection, as another application
//! would, and the window's handler hears what the transfer tells it.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`; the
//! live cases skip without it. The model's cases need no server.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use flui_foundation::DataTransferId;
use flui_foundation::geometry::Point;
use flui_platform::data_transfer::{
    DataTransferSource as _, DropFeedback, Mime, RepresentationIndex, TransferActions,
    TransferError, TransferFormat, TransferLimits, TransferPayload, TransferRequest,
};
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{DispatchEventResult, DragDropEvent, PlatformInput, WindowId};
use flui_testing::window_handler::RecordingWindowHandler;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _,
    CreateWindowAux, EventMask, PropMode, Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent,
    SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use super::X11Drag;
use super::model::{self, ActionAtoms, Kind, Message, MessageAtoms};
use crate::clipboard::text::TextForm;
use crate::owner::linux::x11::{InputTarget, LoopDrag as _, WindowRef, WindowScale, X11Connection};
use crate::owner::window_delivery::WindowDelivery;

// ============================================================================
// The model
// ============================================================================

/// The platform's transfer values compare by what they print: they hold no
/// equality of their own.
macro_rules! assert_same {
    ($actual:expr, $expected:expr $(, $($context:tt)*)?) => {{
        let (actual, expected) = printed(&$actual, &$expected);
        assert_eq!(actual, expected $(, $($context)*)?)
    }};
}

/// What `actual` and `expected`, of one type, print.
fn printed<T: std::fmt::Debug>(actual: &T, expected: &T) -> (String, String) {
    (format!("{actual:?}"), format!("{expected:?}"))
}

const MESSAGES: MessageAtoms = MessageAtoms {
    enter: 101,
    position: 102,
    leave: 103,
    drop: 104,
};

const ACTIONS: ActionAtoms = ActionAtoms {
    copy: 201,
    move_: 202,
    link: 203,
    ask: 204,
};

/// Each message reads what the source wrote, and nothing else is one.
#[test]
fn messages_read_what_the_source_wrote() {
    let table: [(&str, u32, [u32; 5], Option<Message>); 6] = [
        (
            "enter, version 5, three types inline",
            101,
            [7, 5 << 24, 11, 12, 0],
            Some(Message::Enter {
                source: 7,
                version: 5,
                listed: false,
                types: vec![11, 12],
            }),
        ),
        (
            "enter listing its types",
            101,
            [7, (4 << 24) | 1, 11, 12, 13],
            Some(Message::Enter {
                source: 7,
                version: 4,
                listed: true,
                types: vec![11, 12, 13],
            }),
        ),
        (
            "position, negative root coordinates",
            102,
            [7, 0, 0xFFFE_0010, 99, 201],
            Some(Message::Position {
                source: 7,
                root: (-2, 16),
                time: 99,
                action: 201,
            }),
        ),
        (
            "leave",
            103,
            [7, 0, 0, 0, 0],
            Some(Message::Leave { source: 7 }),
        ),
        (
            "drop",
            104,
            [7, 0, 55, 0, 0],
            Some(Message::Drop {
                source: 7,
                time: 55,
            }),
        ),
        ("another client message", 999, [7, 0, 0, 0, 0], None),
    ];
    for (case, kind, data, expected) in table {
        assert_eq!(Message::read(kind, data, &MESSAGES), expected, "{case}");
    }
}

/// A status asks for every movement either way and names the action only
/// when it accepts; a finish names the action only when the drop was taken.
#[test]
fn status_and_finish_carry_the_answer() {
    assert_eq!(model::status(9, Some(201)), [9, 0b11, 0, 0, 201]);
    assert_eq!(model::status(9, None), [9, 0b10, 0, 0, NONE]);
    assert_eq!(model::finished(9, Some(202)), [9, 1, 202, 0, 0]);
    assert_eq!(model::finished(9, None), [9, 0, NONE, 0, 0]);
}

/// The named actions permit themselves, ask and private permit nothing on
/// their own, and each resolved action has its atom.
#[test]
fn actions_map_both_ways() {
    let table = [
        (201, TransferActions::COPY),
        (202, TransferActions::MOVE),
        (203, TransferActions::LINK),
        (204, TransferActions::empty()),
        (999, TransferActions::empty()),
    ];
    for (atom, actions) in table {
        assert_eq!(ACTIONS.actions_of(atom), actions, "atom {atom}");
    }
    assert!(ACTIONS.is_ask(204));
    for action in [
        TransferActions::COPY,
        TransferActions::MOVE,
        TransferActions::LINK,
    ] {
        assert_eq!(ACTIONS.actions_of(ACTIONS.atom_of(action)), action);
    }
}

/// Text is offered once, in the form a reader prefers, where the first text
/// form stood; names that are no media type are left out.
#[test]
fn types_offer_each_format_once() {
    let types = [
        (1, "TARGETS"),
        (2, "text/plain"),
        (3, "text/uri-list"),
        (4, "UTF8_STRING"),
        (5, "text/html"),
        (6, "COMPOUND_TEXT"),
        (7, "image/png"),
        (8, "text/plain;charset=utf-8"),
        (9, "text/x-moz-url"),
    ];
    let offered = model::representations(&types);
    assert_eq!(
        offered,
        vec![
            (8, Kind::Text(TextForm::PlainUtf8)),
            (3, Kind::Bytes(TransferFormat::UriList)),
            (5, Kind::Html),
            (
                7,
                Kind::Bytes(TransferFormat::Image {
                    mime: Mime::new("image/png")
                })
            ),
            (
                9,
                Kind::Bytes(TransferFormat::Custom {
                    mime: Mime::new("text/x-moz-url")
                })
            ),
        ]
    );
    assert_eq!(model::representations(&[(1, "TARGETS")]), Vec::new());
}

/// Each kind's bytes make its payload: text decoded, HTML in UTF-8 or
/// behind a UTF-16 mark, a URI list as its bytes without a trailing NUL;
/// each measured exactly before it is made.
#[test]
fn bytes_make_their_payload() {
    let table: [(&str, Kind, Vec<u8>, TransferPayload); 7] = [
        (
            "ill-formed utf-8 text",
            Kind::Text(TextForm::Utf8String),
            vec![b'a', 0xFF, 0xFE, b'b'],
            TransferPayload::Text("a\u{FFFD}\u{FFFD}b".to_owned()),
        ),
        (
            "utf-16le html with a lone surrogate and a trailing NUL",
            Kind::Html,
            vec![0xFF, 0xFE, b'x', 0, 0x00, 0xD8, 0, 0],
            TransferPayload::Html("x\u{FFFD}".to_owned()),
        ),
        (
            "latin-1 text",
            Kind::Text(TextForm::Latin1),
            vec![b'a', 0xE9],
            TransferPayload::Text("aé".to_owned()),
        ),
        (
            "utf-8 html with a trailing NUL",
            Kind::Html,
            b"<b>x</b>\0".to_vec(),
            TransferPayload::Html("<b>x</b>".to_owned()),
        ),
        (
            "utf-16le html",
            Kind::Html,
            vec![0xFF, 0xFE, b'<', 0, b'p', 0, b'>', 0],
            TransferPayload::Html("<p>".to_owned()),
        ),
        (
            "utf-16be html",
            Kind::Html,
            vec![0xFE, 0xFF, 0, b'<', 0, b'p', 0, b'>'],
            TransferPayload::Html("<p>".to_owned()),
        ),
        (
            "a uri list ended by a NUL",
            Kind::Bytes(TransferFormat::UriList),
            b"file:///a\r\n\0".to_vec(),
            TransferPayload::Custom {
                mime: Mime::new("text/uri-list"),
                bytes: Arc::from(&b"file:///a\r\n"[..]),
            },
        ),
    ];
    for (case, kind, bytes, expected) in table {
        let len = u64::try_from(kind.decoded_len(&bytes)).expect("a small payload");
        assert_eq!(len, expected.byte_len(), "{case}: measured as made");
        assert_same!(kind.payload(bytes), expected, "{case}");
    }
}

/// A payload past the limit is refused, one at it is not.
#[test]
fn a_payload_past_its_limit_is_too_large() {
    assert_same!(model::too_large(10, 10), None);
    assert_same!(
        model::too_large(11, 10),
        Some(TransferError::TooLarge {
            actual: 11,
            limit: 10
        })
    );
}

// ============================================================================
// Live
// ============================================================================

/// How long a step may take before the case fails.
const STEP: Duration = Duration::from_secs(10);

/// How long a source waits for the requestor to take a chunk before it
/// takes the transfer as refused.
const QUIET: Duration = Duration::from_secs(1);

/// `XdndSelection` is the server's, one for every client: the cases take
/// turns at it. The lock keeps one process's cases apart; under nextest, a
/// process per case, the `xdnd-selection` test group does.
static SERVER: Mutex<()> = Mutex::new(());

/// The transfer and one window of its connection a source drags over.
struct Target {
    connection: Arc<X11Connection>,
    drag: Arc<X11Drag>,
    xid: xproto::Window,
    handler: Rc<RecordingWindowHandler>,
    /// How often the transfer asked for the loop to wake.
    wakes: Arc<AtomicUsize>,
}

impl Target {
    /// A window at (10, 20) on the root window, at `scale`.
    fn open(scale: f64) -> Option<Self> {
        let present = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
        if !present {
            eprintln!("skipped: no X server (DISPLAY is not set); CI runs this under xvfb-run");
            return None;
        }
        let connection = X11Connection::connect().expect("the X server answers");
        let wakes = Arc::new(AtomicUsize::new(0));
        let rung = Arc::clone(&wakes);
        let wake = Box::new(move || {
            rung.fetch_add(1, Ordering::SeqCst);
        });
        let drag = X11Drag::try_new(&connection, wake).expect("the transfer is made");
        let xcb = connection.xcb();
        let xid = xcb.generate_id().expect("an X id");
        xcb.create_window(
            COPY_DEPTH_FROM_PARENT,
            xid,
            connection.root(),
            10,
            20,
            200,
            100,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .expect("the window is asked for");
        let handler = RecordingWindowHandler::new();
        drag.window_opened(InputTarget {
            window: WindowRef {
                xid,
                id: WindowId(1),
            },
            delivery: Arc::new(WindowDelivery::new(
                Rc::clone(&handler) as _,
                InputCapabilities::UNAVAILABLE,
            )),
            scale: WindowScale::new(scale),
        });
        xcb.flush().expect("the requests are sent");
        Some(Self {
            connection,
            drag,
            xid,
            handler,
            wakes,
        })
    }

    /// Hand the transfer every event its connection has.
    fn deliver_events(&self) {
        while let Some(event) = self
            .connection
            .xcb()
            .poll_for_event()
            .expect("the connection holds")
        {
            self.drag.event(&event);
        }
        self.drag.expire(web_time::Instant::now());
    }

    /// The drag events the window heard.
    fn heard(&self) -> Vec<DragDropEvent> {
        self.handler
            .inputs()
            .into_iter()
            .filter_map(|input| match input {
                PlatformInput::DragDrop(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    /// Have the window cache `accept` as its answer to every offer it hears
    /// of.
    fn answer(&self, accept: Option<TransferActions>) {
        let drag = Arc::downgrade(&self.drag);
        self.handler.react_to_input(move |input| {
            let id = match &input {
                PlatformInput::DragDrop(DragDropEvent::Entered { offer, .. }) => Some(offer.id()),
                PlatformInput::DragDrop(DragDropEvent::Moved { id, .. }) => Some(*id),
                _ => None,
            };
            if let (Some(id), Some(drag)) = (id, drag.upgrade()) {
                drag.update_drop_feedback(id, DropFeedback { accept });
            }
            DispatchEventResult::default()
        });
    }

    /// Poll `request` to its end, turning the transfer and the source.
    fn resolve(
        &self,
        source: &Source,
        mut request: TransferRequest,
        answer: impl Fn(&Source, &SelectionRequestEvent),
    ) -> Result<TransferPayload, TransferError> {
        let deadline = Instant::now() + STEP;
        let mut context = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(result) = std::pin::Pin::new(&mut request).poll(&mut context) {
                return result;
            }
            assert!(Instant::now() < deadline, "the request was never resolved");
            self.deliver_events();
            if let Some(Event::SelectionRequest(asked)) = source.poll() {
                answer(source, &asked);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Another application dragging: its window, which owns `XdndSelection`,
/// and the atoms it speaks.
struct Source {
    connection: RustConnection,
    window: xproto::Window,
    atoms: SourceAtoms,
}

struct SourceAtoms {
    enter: xproto::Atom,
    position: xproto::Atom,
    status: xproto::Atom,
    leave: xproto::Atom,
    drop: xproto::Atom,
    finished: xproto::Atom,
    selection: xproto::Atom,
    type_list: xproto::Atom,
    copy: xproto::Atom,
    incr: xproto::Atom,
    uri_list: xproto::Atom,
    utf8: xproto::Atom,
    html: xproto::Atom,
    png: xproto::Atom,
}

impl Source {
    fn new() -> Self {
        let (connection, _) = RustConnection::connect(None).expect("the X server answers");
        let names: [&[u8]; 14] = [
            b"XdndEnter",
            b"XdndPosition",
            b"XdndStatus",
            b"XdndLeave",
            b"XdndDrop",
            b"XdndFinished",
            b"XdndSelection",
            b"XdndTypeList",
            b"XdndActionCopy",
            b"INCR",
            b"text/uri-list",
            b"UTF8_STRING",
            b"text/html",
            b"image/png",
        ];
        let cookies: Vec<_> = names
            .iter()
            .map(|name| connection.intern_atom(false, name).expect("intern"))
            .collect();
        let atoms: Vec<xproto::Atom> = cookies
            .into_iter()
            .map(|cookie| cookie.reply().expect("interned").atom)
            .collect();
        let [
            enter,
            position,
            status,
            leave,
            drop,
            finished,
            selection,
            type_list,
            copy,
            incr,
            uri_list,
            utf8,
            html,
            png,
        ] = <[xproto::Atom; 14]>::try_from(atoms).expect("one atom a name");
        let window = connection.generate_id().expect("an X id");
        let root = connection.setup().roots[0].root;
        connection
            .create_window(
                0,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .expect("the source window is asked for");
        connection
            .set_selection_owner(window, selection, CURRENT_TIME)
            .expect("the selection is taken");
        connection.flush().expect("the requests are sent");
        Self {
            connection,
            window,
            atoms: SourceAtoms {
                enter,
                position,
                status,
                leave,
                drop,
                finished,
                selection,
                type_list,
                copy,
                incr,
                uri_list,
                utf8,
                html,
                png,
            },
        }
    }

    fn send(&self, target: xproto::Window, kind: xproto::Atom, data: [u32; 5]) {
        let event = ClientMessageEvent::new(32, target, kind, data);
        self.connection
            .send_event(false, target, EventMask::NO_EVENT, event)
            .expect("the message is sent");
        self.connection.flush().expect("the message is sent");
    }

    /// Enter `target`, offering `types`: inline when three fit, listed
    /// otherwise.
    fn enter(&self, target: xproto::Window, types: &[xproto::Atom]) {
        let listed = types.len() > 3;
        if listed {
            self.connection
                .change_property32(
                    PropMode::REPLACE,
                    self.window,
                    self.atoms.type_list,
                    AtomEnum::ATOM,
                    types,
                )
                .expect("the type list is written");
        }
        let mut inline = [NONE; 3];
        for (slot, atom) in inline.iter_mut().zip(types) {
            *slot = *atom;
        }
        self.send(
            target,
            self.atoms.enter,
            [
                self.window,
                (5 << 24) | u32::from(listed),
                inline[0],
                inline[1],
                inline[2],
            ],
        );
    }

    fn position(&self, target: xproto::Window, root: (u16, u16), time: u32, action: u32) {
        self.send(
            target,
            self.atoms.position,
            [
                self.window,
                0,
                (u32::from(root.0) << 16) | u32::from(root.1),
                time,
                action,
            ],
        );
    }

    /// The next event the source has, if any.
    fn poll(&self) -> Option<Event> {
        self.connection
            .poll_for_event()
            .expect("the connection holds")
    }

    /// The next client message of `kind` the source hears, turning `target`
    /// meanwhile.
    fn expect_message(&self, target: &Target, kind: xproto::Atom) -> [u32; 5] {
        let deadline = Instant::now() + STEP;
        loop {
            target.deliver_events();
            if let Some(Event::ClientMessage(message)) = self.poll()
                && message.type_ == kind
            {
                return message.data.as_data32();
            }
            assert!(
                Instant::now() < deadline,
                "the source heard no message {kind}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Answer `request` with `bytes` of its target, at once.
    fn answer_with(&self, request: &SelectionRequestEvent, bytes: &[u8]) {
        self.connection
            .change_property8(
                PropMode::REPLACE,
                request.requestor,
                request.property,
                request.target,
                bytes,
            )
            .expect("the property is written");
        self.notify(request, request.property);
    }

    fn notify(&self, request: &SelectionRequestEvent, property: xproto::Atom) {
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property,
        };
        self.connection
            .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
            .expect("the answer is sent");
        self.connection.flush().expect("the answer is sent");
    }

    /// Announce `len` bytes `INCR` in answer to `request`, and write no
    /// chunk.
    fn announce(&self, request: &SelectionRequestEvent, len: u32) {
        self.connection
            .change_property32(
                PropMode::REPLACE,
                request.requestor,
                request.property,
                self.atoms.incr,
                &[len],
            )
            .expect("the announcement is written");
        self.notify(request, request.property);
    }

    /// Answer `request` `INCR`, announcing `announced` bytes and writing
    /// `bytes` `chunk` bytes at a time, each once the requestor deleted the
    /// last; a requestor that stops deleting for [`QUIET`] refused the rest.
    fn answer_incrementally(
        &self,
        target: &Target,
        request: &SelectionRequestEvent,
        announced: u32,
        bytes: &[u8],
        chunk: usize,
    ) {
        self.connection
            .change_window_attributes(
                request.requestor,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .expect("the requestor's properties are watched");
        self.connection
            .change_property32(
                PropMode::REPLACE,
                request.requestor,
                request.property,
                self.atoms.incr,
                &[announced],
            )
            .expect("the announcement is written");
        self.notify(request, request.property);
        let mut chunks = bytes.chunks(chunk).chain(std::iter::once(&[][..]));
        let mut deadline = Instant::now() + QUIET;
        loop {
            target.deliver_events();
            match self.poll() {
                Some(Event::PropertyNotify(property))
                    if property.atom == request.property && property.state == Property::DELETE =>
                {
                    let Some(next) = chunks.next() else {
                        return;
                    };
                    self.connection
                        .change_property8(
                            PropMode::REPLACE,
                            request.requestor,
                            request.property,
                            request.target,
                            next,
                        )
                        .expect("the chunk is written");
                    self.connection.flush().expect("the chunk is written");
                    deadline = Instant::now() + QUIET;
                    if next.is_empty() {
                        return;
                    }
                }
                _ => std::thread::sleep(Duration::from_millis(2)),
            }
            if Instant::now() >= deadline {
                // The requestor stopped deleting: it refused the transfer.
                return;
            }
        }
    }
}

/// The id the window's `Entered` named.
fn entered(heard: &[DragDropEvent]) -> DataTransferId {
    match heard.first() {
        Some(DragDropEvent::Entered { offer, .. }) => offer.id(),
        other => panic!("the window first heard {other:?}, not Entered"),
    }
}

/// A drag the window accepts is offered with its types and position,
/// answered from the feedback the window cached as it heard it, dropped,
/// fetched, and finished with the action once concluded; the concluded
/// offer is stale.
#[test]
fn an_accepted_drop_is_fetched_and_finished() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(2.0) else {
        return;
    };
    target.answer(Some(TransferActions::COPY));
    let source = Source::new();
    let atoms = &source.atoms;
    // Four types: the source lists them in `XdndTypeList`.
    source.enter(
        target.xid,
        &[atoms.uri_list, atoms.utf8, atoms.html, atoms.png],
    );
    source.position(target.xid, (50, 60), 10, atoms.copy);
    let status = source.expect_message(&target, atoms.status);
    assert_eq!(status, model::status(target.xid, Some(atoms.copy)));

    let heard = target.heard();
    let id = entered(&heard);
    match &heard[0] {
        DragDropEvent::Entered {
            offer,
            allowed,
            position,
        } => {
            let formats: Vec<TransferFormat> = offer
                .representations()
                .iter()
                .map(|representation| representation.format.clone())
                .collect();
            assert_eq!(
                formats,
                vec![
                    TransferFormat::UriList,
                    TransferFormat::Text,
                    TransferFormat::Html,
                    TransferFormat::Image {
                        mime: Mime::new("image/png")
                    },
                ]
            );
            assert_eq!(*allowed, TransferActions::COPY);
            // (50, 60) on the root is (40, 40) in the window, at scale 2.
            assert_eq!(*position, Some(Point::new(20.0, 20.0)));
        }
        other => panic!("not Entered: {other:?}"),
    }

    source.position(target.xid, (60, 60), 11, atoms.copy);
    source.expect_message(&target, atoms.status);
    source.send(target.xid, atoms.drop, [source.window, 0, 12, 0, 0]);
    let deadline = Instant::now() + STEP;
    while !target
        .heard()
        .iter()
        .any(|event| matches!(event, DragDropEvent::Dropped { .. }))
    {
        assert!(Instant::now() < deadline, "the window heard no drop");
        target.deliver_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_same!(
        target.heard().last(),
        Some(&DragDropEvent::Dropped {
            id,
            action: TransferActions::COPY,
            position: Some(Point::new(25.0, 20.0)),
        })
    );

    let request = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    let fetched = target.resolve(&source, request, |source, asked| {
        assert_eq!(asked.selection, source.atoms.selection);
        assert_eq!(asked.target, source.atoms.uri_list);
        assert_eq!(asked.time, 12, "converted at the drop's time");
        source.answer_with(asked, b"file:///tmp/a%20b\r\n");
    });
    assert_same!(
        fetched,
        Ok(TransferPayload::Custom {
            mime: Mime::new("text/uri-list"),
            bytes: Arc::from(&b"file:///tmp/a%20b\r\n"[..]),
        })
    );

    target.drag.conclude_drop(id);
    let finished = source.expect_message(&target, atoms.finished);
    assert_eq!(finished, model::finished(target.xid, Some(atoms.copy)));
    let stale = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    assert_same!(
        target.resolve(&source, stale, |_, _| {}),
        Err(TransferError::StaleOffer(id))
    );
}

/// A window that caches no acceptance is answered refused at every
/// position; its drop finishes unaccepted at once and the window hears it
/// leave, not drop.
#[test]
fn a_refused_drop_finishes_unaccepted_and_exits() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    let status = source.expect_message(&target, atoms.status);
    assert_eq!(status, model::status(target.xid, None));
    source.send(target.xid, atoms.drop, [source.window, 0, 11, 0, 0]);
    let finished = source.expect_message(&target, atoms.finished);
    assert_eq!(finished, model::finished(target.xid, None));
    let heard = target.heard();
    let id = entered(&heard);
    assert_same!(heard.last(), Some(&DragDropEvent::Exited { id }));
}

/// A drag that leaves retires its offer: the window hears it leave, and a
/// request for it is stale.
#[test]
fn a_drag_that_leaves_retires_its_offer() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    target.answer(Some(TransferActions::COPY));
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);
    source.send(target.xid, atoms.leave, [source.window, 0, 0, 0, 0]);
    let deadline = Instant::now() + STEP;
    while target.heard().len() < 2 {
        assert!(
            Instant::now() < deadline,
            "the window never heard the drag leave"
        );
        target.deliver_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    let id = entered(&target.heard());
    assert_same!(target.heard().last(), Some(&DragDropEvent::Exited { id }));
    let request = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    assert_same!(
        target.resolve(&source, request, |_, _| {}),
        Err(TransferError::StaleOffer(id))
    );
}

/// An `INCR` answer is read a chunk at a time into the whole payload; one
/// announced past the request's limit is refused before any chunk is read,
/// and one whose chunks outgrow it at the chunk that does.
#[test]
fn an_incr_answer_is_read_in_chunks_and_held_to_its_limit() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    target.answer(Some(TransferActions::COPY));
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);
    let id = entered(&target.heard());
    let text = "xdnd ".repeat(1000);

    let whole = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    let read = target.resolve(&source, whole, |source, asked| {
        source.answer_incrementally(&target, asked, 5000, text.as_bytes(), 700);
    });
    assert_same!(read, Ok(TransferPayload::Text(text.clone())));

    let limited = target.drag.request(
        id,
        RepresentationIndex(0),
        TransferLimits::default().with_max_bytes(100),
    );
    let read = target.resolve(&source, limited, |source, asked| {
        source.announce(asked, 5000);
    });
    assert_same!(
        read,
        Err(TransferError::TooLarge {
            actual: 5000,
            limit: 100
        })
    );

    // A source that announces less than it sends is held to the limit by
    // the chunks themselves.
    let lying = target.drag.request(
        id,
        RepresentationIndex(0),
        TransferLimits::default().with_max_bytes(1000),
    );
    let read = target.resolve(&source, lying, |source, asked| {
        source.answer_incrementally(&target, asked, 50, text.as_bytes(), 700);
    });
    assert_same!(
        read,
        Err(TransferError::TooLarge {
            actual: 1400,
            limit: 1000
        })
    );
}

/// The source's conversion requests, turning `target` until `count` came.
fn conversions(target: &Target, source: &Source, count: usize) -> Vec<SelectionRequestEvent> {
    let deadline = Instant::now() + STEP;
    let mut asked = Vec::new();
    while asked.len() < count {
        assert!(
            Instant::now() < deadline,
            "the source was asked {} times, not {count}",
            asked.len()
        );
        target.deliver_events();
        match source.poll() {
            Some(Event::SelectionRequest(request)) => asked.push(request),
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    asked
}

/// Requests past the fetches the transfer runs at once wait, and start as
/// a retired offer frees its fetches; each fetch converts into a requestor
/// of its own, so the answer a source writes late for a retired fetch
/// reaches no later one. A request made off the loop wakes it.
#[test]
fn a_retired_offer_frees_its_fetches_for_the_requests_waiting() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    target.answer(Some(TransferActions::COPY));
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);
    source.send(target.xid, atoms.drop, [source.window, 0, 11, 0, 0]);
    let deadline = Instant::now() + STEP;
    while !target
        .heard()
        .iter()
        .any(|event| matches!(event, DragDropEvent::Dropped { .. }))
    {
        assert!(Instant::now() < deadline, "the window heard no drop");
        target.deliver_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    let dropped = entered(&target.heard());
    // The dropped offer stays until concluded; a second drag over the
    // window is a second offer.
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 12, atoms.copy);
    source.expect_message(&target, atoms.status);
    let hovering = match target.heard().last() {
        Some(DragDropEvent::Entered { offer, .. }) => offer.id(),
        other => panic!("the second drag was not offered: {other:?}"),
    };

    let woken = target.wakes.load(Ordering::SeqCst);
    let held: Vec<TransferRequest> = (0..super::MAX_FETCHES)
        .map(|_| {
            target
                .drag
                .request(dropped, RepresentationIndex(0), TransferLimits::default())
        })
        .collect();
    assert!(
        target.wakes.load(Ordering::SeqCst) > woken,
        "a fetch started off the loop wakes it"
    );
    let unanswered = conversions(&target, &source, super::MAX_FETCHES);
    let waiting = target
        .drag
        .request(hovering, RepresentationIndex(0), TransferLimits::default());
    target.drag.conclude_drop(dropped);
    for request in held {
        let mut context = Context::from_waker(Waker::noop());
        let mut request = request;
        assert_same!(
            std::pin::Pin::new(&mut request).poll(&mut context),
            Poll::Ready(Err::<TransferPayload, _>(TransferError::SourceGone))
        );
    }

    let read = target.resolve(&source, waiting, |source, asked| {
        assert!(
            unanswered
                .iter()
                .all(|late| late.requestor != asked.requestor),
            "a fetch converts into a requestor of its own"
        );
        // Answers the retired fetches' requestors no longer exist to take.
        for late in &unanswered {
            source.answer_with(late, b"late");
        }
        source.answer_with(asked, b"fresh");
    });
    assert_same!(read, Ok(TransferPayload::Text("fresh".to_owned())));
}

/// A type the server cannot name, or names past the longest media type, is
/// not offered, and is kept as refused, so a source repeating it costs no
/// second lookup; the type the server names as a media type still offers.
#[test]
fn a_type_unnamed_or_named_past_a_media_type_is_refused_once() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    target.answer(Some(TransferActions::COPY));
    let source = Source::new();
    let atoms = &source.atoms;
    let long = format!("text/{}", "x".repeat(super::MAX_NAME_LEN));
    let long = source
        .connection
        .intern_atom(false, long.as_bytes())
        .expect("intern")
        .reply()
        .expect("interned")
        .atom;
    let dropped = [0x0FFF_FFF0, long, 0x0FFF_FFF1];
    // The long name listed again and again is one lookup.
    source.enter(
        target.xid,
        &[
            dropped[0], atoms.utf8, dropped[1], dropped[1], dropped[1], dropped[2],
        ],
    );
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);
    let names = &target.drag.state.lock().names;
    assert!(
        dropped.iter().all(|atom| names.get(atom) == Some(&None)),
        "not refused: {names:?}"
    );
    assert!(matches!(names.get(&atoms.utf8), Some(Some(_))));
    let heard = target.heard();
    let Some(DragDropEvent::Entered { offer, .. }) = heard.first() else {
        panic!("not entered: {heard:?}");
    };
    assert_eq!(offer.representations().len(), 1, "only the text offers");
}

/// A window whose handler panics as it hears of a drag still answers the
/// source, with a refusal, before the panic goes on: at the enter and at a
/// later movement.
#[test]
fn a_failing_drag_handler_still_answers_the_source() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    target.handler.react_to_input(|input| {
        if let PlatformInput::DragDrop(event) = input {
            panic!("the handler fails at {event:?}");
        }
        DispatchEventResult::default()
    });
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    for (time, x) in [(10, 50), (11, 60)] {
        source.position(target.xid, (x, 60), time, atoms.copy);
        let deadline = Instant::now() + STEP;
        let mut failed = false;
        let status = loop {
            failed |= std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                target.deliver_events();
            }))
            .is_err();
            if let Some(Event::ClientMessage(message)) = source.poll()
                && message.type_ == atoms.status
            {
                assert!(failed, "the handler's panic went on");
                break message.data.as_data32();
            }
            assert!(Instant::now() < deadline, "the source heard no status");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(status, model::status(target.xid, None));
    }
    let heard = target.heard();
    assert!(
        matches!(
            heard.as_slice(),
            [DragDropEvent::Entered { .. }, DragDropEvent::Moved { .. }]
        ),
        "the window heard {heard:?}"
    );
}
