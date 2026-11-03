//! XDND drops against a real X server: a source on a connection of its own
//! drags over a window of the transfer's connection, as another application
//! would, and the window's handler hears what the transfer tells it.
//!
//! CI runs the crate's tests under `xvfb-run`, which sets `DISPLAY`; the
//! live cases skip without it. The model's cases need no server.

use std::cell::Cell;
use std::num::{NonZeroU32, NonZeroU64};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use flui_foundation::geometry::Point;
use flui_foundation::{DataTransferId, DragSessionId, UiRuntimeId};
use flui_platform::data_transfer::{
    DataTransferSource as _, DropFeedback, Mime, RepresentationDescriptor, RepresentationIndex,
    TransferActions, TransferError, TransferFormat, TransferLimits, TransferPayload,
    TransferRequest,
};
use flui_platform::drag::{DragError, DragSessionKey, OsDragEnd, OsDragRequest, ProcessNonce};
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{DispatchEventResult, DragDropEvent, PlatformInput, WindowId};
use flui_testing::window_handler::RecordingWindowHandler;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _,
    CreateWindowAux, EventMask, PropMode, Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent,
    SelectionRequestEvent, WindowClass,
};
use x11rb::protocol::{Event, xinput};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use super::X11Drag;
use super::model::{self, ActionAtoms, Message, MessageAtoms};
use super::source::FINISH_TIMEOUT;
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
/// process per case, the `xdnd-selection` test group does. The connection
/// it holds keeps the server from resetting between cases, as Xvfb does
/// once its last client leaves.
static SERVER: Mutex<Option<RustConnection>> = Mutex::new(None);

/// The server for this case alone; a case that finds no server opens no
/// window and skips.
fn server() -> std::sync::MutexGuard<'static, Option<RustConnection>> {
    let mut server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let present = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
    if present && server.is_none() {
        *server = RustConnection::connect(None)
            .ok()
            .map(|(connection, _)| connection);
    }
    server
}

/// The transfer and one window of its connection a source drags over.
struct Target {
    connection: Arc<X11Connection>,
    drag: Arc<X11Drag>,
    xid: xproto::Window,
    handler: Rc<RecordingWindowHandler>,
    /// How often the transfer asked for the loop to wake.
    wakes: Arc<AtomicUsize>,
    /// The window the session's last visit to a target was known by.
    session: Cell<xproto::Window>,
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
                id: WindowId::try_from_raw(1).expect("a nonzero window id"),
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
            session: Cell::new(NONE),
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
    let _server = server();
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
    let _server = server();
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
    let _server = server();
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
    let _server = server();
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

    // A whole answer past the limit is measured, not held, and refused; its
    // property goes, so the next answer reads whole.
    let past = target.drag.request(
        id,
        RepresentationIndex(0),
        TransferLimits::default().with_max_bytes(100),
    );
    let read = target.resolve(&source, past, |source, asked| {
        source.answer_with(asked, text.as_bytes());
    });
    assert_same!(
        read,
        Err(TransferError::TooLarge {
            actual: 5000,
            limit: 100
        })
    );
    let next = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    let read = target.resolve(&source, next, |source, asked| {
        source.answer_with(asked, b"after");
    });
    assert_same!(read, Ok(TransferPayload::Text("after".to_owned())));
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
    let _server = server();
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
    let _server = server();
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

/// Fails as it is woken.
struct FailsAsWoken;

impl std::task::Wake for FailsAsWoken {
    fn wake(self: Arc<Self>) {
        panic!("the request's waker fails");
    }
}

/// A window whose handler panics as the drag leaves still has every request
/// on the retired offer answered, though the first request's waker panics as
/// it is answered: the handler's panic is the one that goes on.
#[test]
fn requests_are_answered_though_the_handler_and_a_waker_fail() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let drag = Arc::downgrade(&target.drag);
    target.handler.react_to_input(move |input| {
        match &input {
            PlatformInput::DragDrop(DragDropEvent::Exited { .. }) => {
                panic!("the handler fails as the drag leaves")
            }
            PlatformInput::DragDrop(DragDropEvent::Entered { offer, .. }) => {
                if let Some(drag) = drag.upgrade() {
                    drag.update_drop_feedback(
                        offer.id(),
                        DropFeedback {
                            accept: Some(TransferActions::COPY),
                        },
                    );
                }
            }
            _ => {}
        }
        DispatchEventResult::default()
    });
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);
    let id = entered(&target.heard());

    let mut failing = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    let mut other = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    let waker = Waker::from(Arc::new(FailsAsWoken));
    assert!(
        std::pin::Pin::new(&mut failing)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    assert!(
        std::pin::Pin::new(&mut other)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );

    source.send(target.xid, atoms.leave, [source.window, 0, 0, 0, 0]);
    let deadline = Instant::now() + STEP;
    let failure = loop {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            target.deliver_events();
        })) {
            break payload;
        }
        assert!(
            Instant::now() < deadline,
            "the handler never heard the leave"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("the handler fails as the drag leaves")
    );
    flui_foundation::panic::discard_payload(failure);
    for request in [&mut failing, &mut other] {
        assert!(
            std::pin::Pin::new(request)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready(),
            "a request on the retired offer is answered"
        );
    }
}

/// A window whose handler panics as it hears the drop it accepted, before
/// concluding it, finishes the drop unaccepted before the panic goes on: the
/// source does not wait for a finish no one will send, and the offer is
/// stale.
#[test]
fn a_drop_whose_handler_fails_finishes_unaccepted() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let drag = Arc::downgrade(&target.drag);
    target.handler.react_to_input(move |input| {
        match &input {
            PlatformInput::DragDrop(DragDropEvent::Dropped { .. }) => {
                panic!("the handler fails at the drop")
            }
            PlatformInput::DragDrop(DragDropEvent::Entered { offer, .. }) => {
                if let Some(drag) = drag.upgrade() {
                    drag.update_drop_feedback(
                        offer.id(),
                        DropFeedback {
                            accept: Some(TransferActions::COPY),
                        },
                    );
                }
            }
            _ => {}
        }
        DispatchEventResult::default()
    });
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);
    let id = entered(&target.heard());

    source.send(target.xid, atoms.drop, [source.window, 0, 11, 0, 0]);
    let deadline = Instant::now() + STEP;
    let mut failed = false;
    let finished = loop {
        failed |= std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            target.deliver_events();
        }))
        .is_err();
        if let Some(Event::ClientMessage(message)) = source.poll()
            && message.type_ == atoms.finished
        {
            break message.data.as_data32();
        }
        assert!(Instant::now() < deadline, "the source heard no finish");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(failed, "the handler's panic went on");
    assert_eq!(finished, model::finished(target.xid, None));
    let mut request = target
        .drag
        .request(id, RepresentationIndex(0), TransferLimits::default());
    assert!(matches!(
        std::pin::Pin::new(&mut request).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(TransferError::StaleOffer(_)))
    ));
}

// ============================================================================
// The drag source
// ============================================================================

/// The session's key.
fn key() -> DragSessionKey {
    DragSessionKey::new(
        ProcessNonce::from_raw(NonZeroU64::new(0xF1A1).expect("non-zero")),
        UiRuntimeId::new_gen(3, NonZeroU32::new(2).expect("non-zero")),
        DragSessionId::from_wire(NonZeroU64::new(7).expect("non-zero")),
    )
}

/// A session offering text and its identity, allowing a copy or a move,
/// telling `ended` how it ended.
fn request(ended: &Rc<Cell<Option<OsDragEnd>>>) -> OsDragRequest {
    let key = key();
    let ended = Rc::clone(ended);
    OsDragRequest {
        key,
        pointer: None,
        representations: vec![
            RepresentationDescriptor {
                format: TransferFormat::Text,
                declared_len: None,
            },
            key.representation(),
        ],
        allowed: TransferActions::COPY | TransferActions::MOVE,
        provider: Box::new(|RepresentationIndex(index)| match index {
            0 => Ok(TransferPayload::Text("dragged \u{2713}".to_owned())),
            _ => Err(TransferError::Cancelled),
        }),
        on_end: Box::new(move |end| ended.set(Some(end))),
    }
}

/// Another application's window taking drops, at (400, 300) on the root
/// window, speaking XDND through `source`'s connection and atoms.
struct Foreign {
    window: xproto::Window,
}

impl Foreign {
    fn open(source: &Source) -> Self {
        Self::open_speaking(source, 5)
    }

    /// A window that speaks XDND `version`.
    fn open_speaking(source: &Source, version: u32) -> Self {
        let connection = &source.connection;
        let window = connection.generate_id().expect("an X id");
        let root = connection.setup().roots[0].root;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root,
                400,
                300,
                100,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .expect("the target window is asked for");
        let aware = connection
            .intern_atom(false, b"XdndAware")
            .expect("intern")
            .reply()
            .expect("interned")
            .atom;
        connection
            .change_property32(PropMode::REPLACE, window, aware, AtomEnum::ATOM, &[version])
            .expect("the window is aware");
        connection.map_window(window).expect("the window is mapped");
        connection.flush().expect("the requests are sent");
        // Mapped once the server answers a round trip.
        connection
            .get_input_focus()
            .expect("the request is sent")
            .reply()
            .expect("the server answers");
        Self { window }
    }
}

/// A button held through XTest, let go as it drops.
struct Held<'a>(&'a RustConnection);

impl<'a> Held<'a> {
    /// Button 1 held with the pointer away from every window.
    fn press(connection: &'a RustConnection) -> Self {
        Self::press_at(connection, (1, 1))
    }

    /// Button 1 held with the pointer at `root`.
    fn press_at(connection: &'a RustConnection, root: (i16, i16)) -> Self {
        use x11rb::protocol::xtest::ConnectionExt as _;
        let screen = connection.setup().roots[0].root;
        connection
            .xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, screen, root.0, root.1, 0)
            .expect("the motion is sent");
        connection
            .xtest_fake_input(xproto::BUTTON_PRESS_EVENT, 1, 0, NONE, 0, 0, 0)
            .expect("the press is sent");
        connection
            .get_input_focus()
            .expect("the request is sent")
            .reply()
            .expect("the server answers");
        Self(connection)
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        use x11rb::protocol::xtest::ConnectionExt as _;
        drop(
            self.0
                .xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, 1, 0, NONE, 0, 0, 0),
        );
        // Let go of before the next case reads the buttons.
        drop(self.0.get_input_focus().map(x11rb::cookie::Cookie::reply));
    }
}

impl Target {
    /// Start `request`'s session from the window.
    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let source = Arc::clone(&self.drag).drag_source(WindowRef {
            xid: self.xid,
            id: WindowId::try_from_raw(1).expect("a nonzero window id"),
        });
        source.start(request)?;
        self.learn_the_visit();
        Ok(())
    }

    /// Have the transfer hear the server's time now, as the next real event
    /// would bring it: a session the cases' made-up times ended let go of
    /// nothing at them, the selection's last change being later.
    fn hear_the_server_time(&self) {
        let (connection, _) = RustConnection::connect(None).expect("the X server answers");
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
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .expect("the window is asked for");
        connection
            .change_property8(
                PropMode::APPEND,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                &[],
            )
            .expect("the property is touched");
        connection.flush().expect("the requests are sent");
        let time = loop {
            if let Event::PropertyNotify(property) =
                connection.wait_for_event().expect("the connection holds")
            {
                break property.time;
            }
        };
        let press = xinput::ButtonPressEvent {
            event: root,
            time,
            ..xinput::ButtonPressEvent::default()
        };
        self.drag.event(&Event::XinputButtonPress(press));
    }

    /// The window the session's last visit to a target was known by.
    fn session(&self) -> xproto::Window {
        self.session.get()
    }

    /// Keep the window the session's visit is known by, if one is open.
    fn learn_the_visit(&self) {
        if let Some(visit) = self.drag.session_visit() {
            self.session.set(visit);
        }
    }

    /// The pointer moved to `root` at `time`, as the window's implicit grab
    /// reports it.
    fn pointer_to(&self, root: (i32, i32), time: u32) {
        let motion = xinput::MotionEvent {
            event: self.xid,
            root_x: root.0 << 16,
            root_y: root.1 << 16,
            time,
            ..xinput::MotionEvent::default()
        };
        assert!(
            self.drag.event(&Event::XinputMotion(motion)),
            "the session's own"
        );
        self.learn_the_visit();
    }

    /// The held button was let go at `time`.
    fn release(&self, time: u32) {
        self.release_button(1, time);
    }

    /// `button` was let go at `time`.
    fn release_button(&self, button: u32, time: u32) {
        let release = xinput::ButtonReleaseEvent {
            event: self.xid,
            detail: button,
            time,
            ..xinput::ButtonReleaseEvent::default()
        };
        assert!(
            self.drag.event(&Event::XinputButtonRelease(release)),
            "the session's own"
        );
    }
}

/// The next client message of `kind` `foreign` hears, turning `target`.
fn heard_by(source: &Source, target: &Target, kind: xproto::Atom) -> [u32; 5] {
    source.expect_message(target, kind)
}

/// A session over another application's window that accepts it enters,
/// moves, drops, serves its text, and ends dropped with the target's
/// action once the target finishes.
#[test]
fn a_session_drops_on_a_target_that_accepts() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    target.start(request(&ended)).expect("the session starts");

    target.pointer_to((450, 350), 100);
    let enter = heard_by(&other, &target, atoms.enter);
    assert_eq!(enter[0], target.session());
    assert_eq!(enter[1] >> 24, 5);
    assert!(
        enter[2..].contains(&atoms.utf8),
        "offers UTF8_STRING: {enter:?}"
    );
    let position = heard_by(&other, &target, atoms.position);
    assert_eq!(
        position,
        model::position(target.session(), (450, 350), 100, atoms.copy)
    );

    // A movement while the position waits is held, and goes out once
    // answered.
    target.pointer_to((460, 350), 101);
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, Some(atoms.copy)),
    );
    let position = heard_by(&other, &target, atoms.position);
    assert_eq!(
        position,
        model::position(target.session(), (460, 350), 101, atoms.copy)
    );
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, Some(atoms.copy)),
    );
    target.deliver_events();

    target.release(102);
    let dropped = heard_by(&other, &target, atoms.drop);
    assert_eq!(dropped, model::drop_at(target.session(), 102));

    // The target converts the selection at the drop's time.
    other
        .connection
        .convert_selection(
            foreign.window,
            atoms.selection,
            atoms.utf8,
            atoms.utf8,
            102_u32,
        )
        .expect("the conversion is asked");
    other.connection.flush().expect("the conversion is asked");
    let deadline = Instant::now() + STEP;
    let notify = loop {
        target.deliver_events();
        if let Some(Event::SelectionNotify(notify)) = other.poll() {
            break notify;
        }
        assert!(Instant::now() < deadline, "the source never answered");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(notify.property, atoms.utf8, "the conversion is answered");
    let text = other
        .connection
        .get_property(true, foreign.window, atoms.utf8, AtomEnum::ANY, 0, 1024)
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .value;
    assert_eq!(String::from_utf8(text).expect("UTF-8"), "dragged \u{2713}");

    assert!(ended.get().is_none(), "the session waits for the finish");
    other.send(
        target.session(),
        atoms.finished,
        model::finished(foreign.window, Some(atoms.copy)),
    );
    let deadline = Instant::now() + STEP;
    while ended.get().is_none() {
        assert!(Instant::now() < deadline, "the session never ended");
        target.deliver_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(ended.get(), Some(OsDragEnd::Dropped(TransferActions::COPY)));
}

/// A release over a target that refuses leaves it and ends cancelled; a
/// second session then starts.
#[test]
fn a_session_released_over_a_refusing_target_is_cancelled() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    target.start(request(&ended)).expect("the session starts");
    assert!(
        target.start(request(&Rc::new(Cell::new(None)))).is_err(),
        "one session at a time"
    );

    target.pointer_to((450, 350), 100);
    heard_by(&other, &target, atoms.position);
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, None),
    );
    target.deliver_events();
    target.release(101);
    let left = heard_by(&other, &target, atoms.leave);
    assert_eq!(left, model::leave(target.session()));
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));

    target.hear_the_server_time();
    let again = Rc::new(Cell::new(None));
    target
        .start(request(&again))
        .expect("a second session starts");
}

/// A session with no button held would never hear its release: it does not
/// start.
#[test]
fn a_session_with_no_button_held_does_not_start() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let ended = Rc::new(Cell::new(None));
    assert!(matches!(
        target.start(request(&ended)),
        Err(DragError::Unsupported(_))
    ));
}

/// Start a session over `foreign` whose target accepted a copy at (450, 350),
/// the position answered.
fn accepted(target: &Target, other: &Source, foreign: &Foreign, request: OsDragRequest) {
    let atoms = &other.atoms;
    target.start(request).expect("the session starts");
    target.pointer_to((450, 350), 100);
    heard_by(other, target, atoms.position);
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, Some(atoms.copy)),
    );
    target.deliver_events();
}

/// A session takes its window's press: the loop has the window's input end
/// it, since its release now goes to the session.
#[test]
fn a_session_takes_its_windows_press() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let _held = Held::press(&other.connection);
    assert_eq!(target.drag.pointers_taken(), Vec::<xproto::Window>::new());
    target
        .start(request(&Rc::new(Cell::new(None))))
        .expect("the session starts");
    assert_eq!(target.drag.pointers_taken(), [target.xid]);
    assert_eq!(
        target.drag.pointers_taken(),
        Vec::<xproto::Window>::new(),
        "taken once"
    );
}

/// Only the release of the button the drag holds drops: a wheel's notch or
/// another button let go meanwhile goes by.
#[test]
fn a_session_drops_at_its_own_buttons_release() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&ended));

    target.release_button(4, 101);
    target.release_button(3, 102);
    target.release(103);
    let dropped = heard_by(&other, &target, other.atoms.drop);
    assert_eq!(dropped, model::drop_at(target.session(), 103));
    assert_eq!(ended.get(), None);
}

/// A release while a position waits, after the pointer moved on, waits for
/// the target's answer to where it was let go: an earlier spot's acceptance
/// does not drop there.
#[test]
fn a_release_waits_for_the_answer_where_it_let_go() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    target.start(request(&ended)).expect("the session starts");

    target.pointer_to((450, 350), 100);
    heard_by(&other, &target, atoms.position);
    target.pointer_to((470, 360), 101);
    target.release(102);
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, Some(atoms.copy)),
    );
    let position = heard_by(&other, &target, atoms.position);
    assert_eq!(
        position,
        model::position(target.session(), (470, 360), 101, atoms.copy)
    );
    assert_eq!(ended.get(), None, "the drop waits for that answer");
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, None),
    );
    let left = heard_by(&other, &target, atoms.leave);
    assert_eq!(left, model::leave(target.session()));
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
}

/// A target that finishes a drop with an action the session did not allow
/// does not make it the outcome: the session ends cancelled.
#[test]
fn a_finish_with_an_action_not_allowed_is_cancelled() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&ended));
    target.release(101);
    heard_by(&other, &target, atoms.drop);
    // The request allows a copy or a move.
    let link = other
        .connection
        .intern_atom(false, b"XdndActionLink")
        .expect("intern")
        .reply()
        .expect("interned")
        .atom;
    other.send(
        target.session(),
        atoms.finished,
        model::finished(foreign.window, Some(link)),
    );
    let deadline = Instant::now() + STEP;
    while ended.get().is_none() {
        assert!(Instant::now() < deadline, "the session never ended");
        target.deliver_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
}

/// A session whose `XdndSelection` another client takes leaves its target and
/// ends cancelled, and lets go of nothing: the selection stays the other
/// client's.
#[test]
fn a_session_whose_selection_is_taken_ends_and_keeps_off_it() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&ended));

    other
        .connection
        .set_selection_owner(foreign.window, atoms.selection, CURRENT_TIME)
        .expect("the request is sent");
    other.connection.flush().expect("the request is sent");
    let left = heard_by(&other, &target, atoms.leave);
    assert_eq!(left, model::leave(target.session()));
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
    let owner = other
        .connection
        .get_selection_owner(atoms.selection)
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .owner;
    assert_eq!(owner, foreign.window, "the other client keeps it");
}

/// A session whose claim on `XdndSelection` is older than another client's
/// does not start: the server ignored the claim, and the other client would
/// answer the drop.
#[test]
fn a_session_the_selection_refuses_does_not_start() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    // The other client takes the selection at the server's time now.
    let other = Source::new();
    let _held = Held::press(&other.connection);
    // The last time the transfer heard is older than that.
    let press = xinput::ButtonPressEvent {
        event: target.xid,
        detail: 1,
        time: 1,
        ..xinput::ButtonPressEvent::default()
    };
    target.drag.event(&Event::XinputButtonPress(press));
    let ended = Rc::new(Cell::new(None));
    assert!(matches!(
        target.start(request(&ended)),
        Err(DragError::Unsupported(_))
    ));
    assert_eq!(target.drag.pointers_taken(), Vec::<xproto::Window>::new());
    let owner = other
        .connection
        .get_selection_owner(other.atoms.selection)
        .expect("the request is sent")
        .reply()
        .expect("the server answers")
        .owner;
    assert_eq!(owner, other.window, "the other client keeps it");
}

/// Fails as it drops.
struct FailsOnDrop;

impl Drop for FailsOnDrop {
    fn drop(&mut self) {
        panic!("a provider's capture failed");
    }
}

/// A provider whose capture fails as the session ends is contained: the end
/// hook still hears how the session ended.
#[test]
fn an_end_hook_hears_the_end_though_the_provider_fails_as_it_drops() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    let mut failing = request(&ended);
    let capture = FailsOnDrop;
    failing.provider = Box::new(move |_| {
        let _held = &capture;
        Err(TransferError::Cancelled)
    });
    target.start(failing).expect("the session starts");
    target.pointer_to((450, 350), 100);
    heard_by(&other, &target, atoms.position);
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, None),
    );
    target.deliver_events();
    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| target.release(101)));
    assert!(released.is_ok(), "the provider's failure is contained");
    heard_by(&other, &target, atoms.leave);
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
}

/// A target speaking an older XDND hears the session in its own version, as
/// XDND asks, or it would ignore it.
#[test]
fn a_session_enters_an_older_target_in_its_version() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let _foreign = Foreign::open_speaking(&other, 4);
    let _held = Held::press(&other.connection);
    target
        .start(request(&Rc::new(Cell::new(None))))
        .expect("the session starts");
    target.pointer_to((450, 350), 100);
    let enter = heard_by(&other, &target, other.atoms.enter);
    assert_eq!(enter[1] >> 24, 4);
}

/// A session that starts with the pointer already over a target enters it
/// at once, so a release in place drops there.
#[test]
fn a_session_started_over_a_target_enters_it_at_once() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press_at(&other.connection, (450, 350));
    let ended = Rc::new(Cell::new(None));
    target.start(request(&ended)).expect("the session starts");
    heard_by(&other, &target, atoms.enter);
    let position = heard_by(&other, &target, atoms.position);
    assert_eq!(
        position[2],
        model::position(target.session(), (450, 350), 0, atoms.copy)[2],
        "where the pointer is"
    );
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, Some(atoms.copy)),
    );
    target.deliver_events();
    target.release(101);
    heard_by(&other, &target, atoms.drop);
}

/// A window unmapped mid-drag loses the grab its release would come by: the
/// session leaves its target and ends cancelled, and the next one starts.
#[test]
fn a_session_whose_window_unmaps_ends_cancelled() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&ended));
    let unmap = xproto::UnmapNotifyEvent {
        event: target.xid,
        window: target.xid,
        ..xproto::UnmapNotifyEvent::default()
    };
    assert!(
        !target.drag.event(&Event::UnmapNotify(unmap)),
        "the window hears its unmap too"
    );
    heard_by(&other, &target, other.atoms.leave);
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
    target.hear_the_server_time();
    target
        .start(request(&Rc::new(Cell::new(None))))
        .expect("the next session starts");
}

/// A drag source kept past its window's close starts nothing, though a
/// button is held: no event of the closed window would end the session, and
/// it would hold the platform's one drag.
#[test]
fn a_closed_windows_drag_source_starts_no_session() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let _held = Held::press(&other.connection);
    let source = Arc::clone(&target.drag).drag_source(WindowRef {
        xid: target.xid,
        id: WindowId::try_from_raw(1).expect("a nonzero window id"),
    });
    target.drag.window_closed(WindowRef {
        xid: target.xid,
        id: WindowId::try_from_raw(1).expect("a nonzero window id"),
    });
    let ended = Rc::new(Cell::new(None));
    assert!(
        source.start(request(&ended)).is_err(),
        "a closed window's source is refused"
    );
    assert!(
        target.drag.session.with(|session| session.is_none()) == Some(true),
        "no session holds the drag"
    );
    assert_eq!(ended.get(), None, "a refused start has no end");
}

/// Without the keyboard, which the unmapped window cannot grab, a session
/// whose window loses the focus leaves its target and ends cancelled:
/// Escape would follow the focus away. A focus change a grab makes is no
/// loss, and the window hears the loss too.
#[test]
fn a_session_without_the_keyboard_ends_as_its_window_loses_the_focus() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&ended));
    let focus = |mode| xproto::FocusOutEvent {
        detail: xproto::NotifyDetail::NONLINEAR,
        event: target.xid,
        mode,
        ..xproto::FocusOutEvent::default()
    };
    assert!(
        !target
            .drag
            .event(&Event::FocusOut(focus(xproto::NotifyMode::GRAB)))
    );
    assert_eq!(ended.get(), None, "a grab's focus change is no loss");
    assert!(
        !target
            .drag
            .event(&Event::FocusOut(focus(xproto::NotifyMode::NORMAL))),
        "the window hears its focus loss too"
    );
    heard_by(&other, &target, other.atoms.leave);
    assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
}

/// A status for a visit the pointer left, coming after the pointer entered
/// the same target again and was let go there, does not decide the drop:
/// each visit is known by a window of its own, so the target's answer to
/// the new visit does.
#[test]
fn a_status_for_an_earlier_visit_does_not_decide_the_drop() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let ended = Rc::new(Cell::new(None));
    target.start(request(&ended)).expect("the session starts");
    target.pointer_to((450, 350), 100);
    heard_by(&other, &target, atoms.position);
    let earlier = target.session();
    // The pointer leaves before the target answers, and comes back.
    target.pointer_to((5, 5), 101);
    heard_by(&other, &target, atoms.leave);
    target.pointer_to((460, 350), 102);
    heard_by(&other, &target, atoms.position);
    target.release(103);
    // The answer to the earlier visit refuses; the one to this visit
    // accepts.
    other.send(earlier, atoms.status, model::status(foreign.window, None));
    other.send(
        target.session(),
        atoms.status,
        model::status(foreign.window, Some(atoms.copy)),
    );
    let dropped = heard_by(&other, &target, atoms.drop);
    assert_eq!(dropped, model::drop_at(target.session(), 103));
    assert_eq!(ended.get(), None, "the drop waits for its finish");
}

/// A target's late finish for a session that ended as its drop went
/// unanswered does not end the next session from the window, dropped on
/// the same target: each session is known by a window of its own.
#[test]
fn a_late_finish_does_not_end_the_next_session() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    let first = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&first));
    target.release(101);
    heard_by(&other, &target, atoms.drop);
    let expired = target.session();
    target
        .drag
        .expire(Instant::now() + FINISH_TIMEOUT + Duration::from_secs(1));
    assert_eq!(first.get(), Some(OsDragEnd::Cancelled), "the drop expired");

    target.hear_the_server_time();
    let next = Rc::new(Cell::new(None));
    accepted(&target, &other, &foreign, request(&next));
    target.release(102);
    heard_by(&other, &target, atoms.drop);
    other.send(
        expired,
        atoms.finished,
        model::finished(foreign.window, Some(atoms.copy)),
    );
    // A round trip on each connection: the server has sent the finish, and
    // the transfer's connection holds whatever it was sent.
    other
        .connection
        .get_input_focus()
        .expect("the request is sent")
        .reply()
        .expect("the server answers");
    target
        .connection
        .xcb()
        .get_input_focus()
        .expect("the request is sent")
        .reply()
        .expect("the server answers");
    target.deliver_events();
    assert_eq!(next.get(), None, "the late finish is not the next drop's");
    other.send(
        target.session(),
        atoms.finished,
        model::finished(foreign.window, Some(atoms.copy)),
    );
    let deadline = Instant::now() + STEP;
    while next.get().is_none() {
        assert!(Instant::now() < deadline, "the session never ended");
        target.deliver_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(next.get(), Some(OsDragEnd::Dropped(TransferActions::COPY)));
}

/// Convert `XdndSelection` to UTF-8 text into `requestor` at `time` through
/// `connection`, and the property the answer names.
fn convert(
    target: &Target,
    connection: &RustConnection,
    atoms: &SourceAtoms,
    requestor: xproto::Window,
    time: u32,
) -> xproto::Atom {
    connection
        .convert_selection(requestor, atoms.selection, atoms.utf8, atoms.utf8, time)
        .expect("the conversion is asked");
    connection.flush().expect("the conversion is asked");
    let deadline = Instant::now() + STEP;
    loop {
        target.deliver_events();
        if let Some(Event::SelectionNotify(notify)) =
            connection.poll_for_event().expect("the connection holds")
            && notify.requestor == requestor
        {
            return notify.property;
        }
        assert!(Instant::now() < deadline, "the source never answered");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The session's data is the drop's: a target hovered over, or another
/// client after the drop, converts nothing; the target's client after its
/// drop does.
#[test]
fn only_the_dropped_on_client_reads_the_session_data() {
    let _server = server();
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let other = Source::new();
    let foreign = Foreign::open(&other);
    let atoms = &other.atoms;
    let _held = Held::press(&other.connection);
    accepted(
        &target,
        &other,
        &foreign,
        request(&Rc::new(Cell::new(None))),
    );

    assert_eq!(
        convert(&target, &other.connection, atoms, foreign.window, 100),
        NONE,
        "a hovered target reads nothing"
    );
    target.release(101);
    heard_by(&other, &target, atoms.drop);
    // A client of its own, which owns no selection.
    let (bystander, _) = RustConnection::connect(None).expect("the X server answers");
    let window = bystander.generate_id().expect("an X id");
    bystander
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            bystander.setup().roots[0].root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .expect("the window is asked for");
    assert_eq!(
        convert(&target, &bystander, atoms, window, 101),
        NONE,
        "another client reads nothing"
    );
    assert_eq!(
        convert(&target, &other.connection, atoms, foreign.window, 101),
        atoms.utf8,
        "the target reads the drop"
    );
}

/// A drag that enters again while the window still hears the last one is
/// offered with its types, though the window's handler panics as it hears
/// the last one end: the failure goes on once the new drag is set up.
#[test]
fn a_drag_entered_over_a_failing_exit_offers_its_types() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    target.handler.react_to_input(|input| {
        if let PlatformInput::DragDrop(DragDropEvent::Exited { .. }) = input {
            panic!("the handler fails as the drag exits");
        }
        DispatchEventResult::default()
    });
    let source = Source::new();
    let atoms = &source.atoms;
    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (20, 30), 10, atoms.copy);
    source.expect_message(&target, atoms.status);

    source.enter(target.xid, &[atoms.utf8]);
    source.position(target.xid, (21, 30), 11, atoms.copy);
    let deadline = Instant::now() + STEP;
    let mut failed = false;
    loop {
        failed |= std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            target.deliver_events();
        }))
        .is_err();
        if let Some(Event::ClientMessage(message)) = source.poll()
            && message.type_ == atoms.status
        {
            break;
        }
        assert!(Instant::now() < deadline, "the source heard no status");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(failed, "the handler's panic went on");
    let offers: Vec<usize> = target
        .heard()
        .iter()
        .filter_map(|event| match event {
            DragDropEvent::Entered { offer, .. } => Some(offer.representations().len()),
            _ => None,
        })
        .collect();
    assert_eq!(offers, [1, 1], "each drag offers its text");
}

/// Records whether the transfer's lock was held as a diagnostic was
/// emitted: a subscriber that reached the transfer then would deadlock.
struct LockedAtDiagnostic {
    drag: Arc<X11Drag>,
    events: Arc<AtomicUsize>,
    locked: Arc<AtomicUsize>,
}

impl tracing::Subscriber for LockedAtDiagnostic {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {
        self.events.fetch_add(1, Ordering::SeqCst);
        if self.drag.state.try_lock().is_none() {
            self.locked.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// The diagnostic for a source too old to answer is emitted with the
/// transfer's lock let go: a subscriber is user code and may reach the
/// transfer.
#[test]
fn a_diagnostic_is_emitted_with_the_lock_let_go() {
    let _server = SERVER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(target) = Target::open(1.0) else {
        return;
    };
    let source = Source::new();
    let atoms = &source.atoms;
    source.send(
        target.xid,
        atoms.enter,
        [source.window, 2 << 24, atoms.utf8, NONE, NONE],
    );
    let events = Arc::new(AtomicUsize::new(0));
    let locked = Arc::new(AtomicUsize::new(0));
    let subscriber = LockedAtDiagnostic {
        drag: Arc::clone(&target.drag),
        events: Arc::clone(&events),
        locked: Arc::clone(&locked),
    };
    let deadline = Instant::now() + STEP;
    tracing::subscriber::with_default(subscriber, || {
        while events.load(Ordering::SeqCst) == 0 {
            target.deliver_events();
            assert!(Instant::now() < deadline, "no diagnostic was emitted");
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    assert_eq!(locked.load(Ordering::SeqCst), 0, "emitted under the lock");
}
