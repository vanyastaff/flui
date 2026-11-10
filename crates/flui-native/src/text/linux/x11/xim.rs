//! The X Input Method client: where no input method answers on D-Bus, the
//! X11 host's text input speaks XIM to the server `XMODIFIERS` names.
//!
//! XIM is a second transport behind the bus's seam: it takes the same
//! commands and posts the same [`Heard`] to the same [`Inbox`], so the
//! owner's key hold, fences and context replacement apply to it unchanged.
//!
//! A key is forwarded to the server as its X event, then a sync: the server
//! answers the sync once it handled every event before it. A key it sends
//! back was not taken and is typed at once; one it did not send back by the
//! sync's answer was taken. A focus is told the same way, its sync's answer
//! fencing what the server sent before. Each context's syncs are answered in
//! order, so one queue per context pairs each answer with its key or focus.

mod link;
mod wire;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::time::Instant;
use x11rb::protocol::xproto::{self, ConnectionExt as _};

use self::link::{Display, Link, Received};
use self::wire::{AttrDecl, AttrValue, FORWARD_SYNCHRONOUS, KEY_PRESS, KEY_RELEASE, Message};
use super::bus::{
    CREATE_TIMEOUT, Command, ContextId, Heard, Inbox, KEY_CALL_TIMEOUT, MAX_WAITING, Request,
};
use crate::text::linux::input_context::Ticket;

/// How long the server has to answer each step of opening the input method.
const OPEN_TIMEOUT: std::time::Duration = link::CONNECT_TIMEOUT;

/// `XNInputStyle`'s preedit and status bits (Xlib's `XIMStyle`).
const PREEDIT_CALLBACKS: u32 = 0x0002;
const PREEDIT_POSITION: u32 = 0x0004;
const PREEDIT_NOTHING: u32 = 0x0008;
const PREEDIT_NONE: u32 = 0x0010;
const STATUS_NOTHING: u32 = 0x0400;
const STATUS_NONE: u32 = 0x0800;

/// The styles this client can serve, best first: the preedit drawn in the
/// field, else where the caret is, else in the server's own window.
const STYLES: [u32; 7] = [
    PREEDIT_CALLBACKS | STATUS_NOTHING,
    PREEDIT_CALLBACKS | STATUS_NONE,
    PREEDIT_POSITION | STATUS_NOTHING,
    PREEDIT_POSITION | STATUS_NONE,
    PREEDIT_NOTHING | STATUS_NOTHING,
    PREEDIT_NOTHING | STATUS_NONE,
    PREEDIT_NONE | STATUS_NONE,
];

/// The X event masks of a key's press and release, as `XIM_SET_EVENT_MASK`
/// names the events the server wants forwarded.
const KEY_PRESS_MASK: u32 = 0x1;
const KEY_RELEASE_MASK: u32 = 0x2;

/// The longest preedit held, in characters.
const MAX_PREEDIT: usize = 4096;

/// `XIM_PREEDIT_CARET`'s direction that names a position.
const CARET_ABSOLUTE: u32 = 10;

/// How the XIM server is found.
#[derive(Clone, Debug)]
pub(crate) struct XimReach {
    /// The X display, `DISPLAY`'s when `None`.
    pub(crate) display: Option<String>,
    /// The server's name, as `XMODIFIERS=@im=` names it.
    pub(crate) name: String,
}

impl XimReach {
    /// The server the session names: `XMODIFIERS`'s `@im=`, unless it names
    /// none.
    pub(super) fn session() -> Option<Self> {
        let modifiers = std::env::var("XMODIFIERS").ok()?;
        let name = server_name(&modifiers)?;
        Some(Self {
            display: None,
            name: name.to_owned(),
        })
    }
}

/// The server `@im=NAME` names in `XMODIFIERS`, `None` for none.
fn server_name(modifiers: &str) -> Option<&str> {
    let (_, after) = modifiers.split_once("@im=")?;
    let name = after.split('@').next()?.trim();
    (!name.is_empty() && name != "none").then_some(name)
}

/// The locale the input method is opened in: `LC_ALL`, `LC_CTYPE` or
/// `LANG`, without its encoding (the client asks for COMPOUND_TEXT).
fn locale() -> Vec<u8> {
    let named = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| String::from("C"));
    let bare = named.split(['.', '@']).next().unwrap_or("C");
    if bare.is_empty() {
        b"C".to_vec()
    } else {
        bare.as_bytes().to_vec()
    }
}

/// The input context attributes this client sets, by the ids the server
/// gave them.
#[derive(Clone, Copy, Debug, Default)]
struct Attributes {
    input_style: Option<u16>,
    client_window: Option<u16>,
    focus_window: Option<u16>,
    preedit: Option<u16>,
    spot: Option<u16>,
}

impl Attributes {
    fn of(declared: &[AttrDecl]) -> Self {
        let id = |name: &[u8]| {
            declared
                .iter()
                .find(|attr| attr.name == name)
                .map(|attr| attr.id)
        };
        Self {
            input_style: id(b"inputStyle"),
            client_window: id(b"clientWindow"),
            focus_window: id(b"focusWindow"),
            preedit: id(b"preeditAttributes"),
            spot: id(b"spotLocation"),
        }
    }
}

/// An input method opened on a server.
pub(super) struct Server {
    link: Link,
    im: u16,
    attributes: Attributes,
    style: u32,
}

/// Open the input method `reach` names: `None` when there is none, or it
/// does not answer in time.
pub(super) async fn open(reach: &XimReach) -> Option<Server> {
    let display = Display::connect(reach.display.as_deref())?;
    let mut link = Link::client(display, &reach.name).await?;
    let opened = tokio::time::timeout(OPEN_TIMEOUT, handshake(&mut link)).await;
    let (im, attributes, style) = opened.ok().flatten()?;
    Some(Server {
        link,
        im,
        attributes,
        style,
    })
}

/// Connect, open the input method in the locale, ask for COMPOUND_TEXT and
/// for the styles it serves.
async fn handshake(link: &mut Link) -> Option<(u16, Attributes, u32)> {
    send(link, &Message::Connect)?;
    expect(link, |message| {
        matches!(message, Message::ConnectReply).then_some(())
    })
    .await?;
    send(link, &Message::Open { locale: locale() })?;
    let (im, im_attrs, ic_attrs) = expect(link, |message| match message {
        Message::OpenReply {
            im,
            im_attrs,
            ic_attrs,
        } => Some((im, im_attrs, ic_attrs)),
        _ => None,
    })
    .await?;
    send(link, &Message::EncodingNegotiation { im })?;
    let index = expect(link, |message| match message {
        Message::EncodingNegotiationReply { im: of, index } if of == im => Some(index),
        _ => None,
    })
    .await?;
    if index != 0 {
        tracing::debug!("the XIM server does not speak COMPOUND_TEXT");
        return None;
    }
    let query = im_attrs
        .iter()
        .find(|attr| attr.name == b"queryInputStyle")
        .map(|attr| attr.id);
    let offered = match query {
        Some(id) => {
            send(link, &Message::GetImValues { im, ids: vec![id] })?;
            let values = expect(link, |message| match message {
                Message::GetImValuesReply { im: of, values } if of == im => Some(values),
                _ => None,
            })
            .await?;
            values
                .iter()
                .find(|value| value.id == id)
                .map(|value| styles_of(&value.value))
                .unwrap_or_default()
        }
        None => Vec::new(),
    };
    let style = STYLES
        .into_iter()
        .find(|style| offered.contains(style))
        .unwrap_or(PREEDIT_NOTHING | STATUS_NOTHING);
    Some((im, Attributes::of(&ic_attrs), style))
}

/// The styles an `XIMStyles` value lists: a count, two unused bytes and a
/// style per four bytes.
fn styles_of(value: &[u8]) -> Vec<u32> {
    let Some((count, rest)) = value.split_first_chunk::<2>() else {
        return Vec::new();
    };
    let count = usize::from(u16::from_ne_bytes(*count));
    rest.get(2..)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .iter()
        .take(count)
        .map(|style| u32::from_ne_bytes(*style))
        .collect()
}

fn send(link: &mut Link, message: &Message) -> Option<()> {
    deliver(link, message).then_some(())
}

/// Send `message`: `false` when the link ends.
fn deliver(link: &mut Link, message: &Message) -> bool {
    let sent = link.send(&message.encode());
    if !sent {
        tracing::debug!("a message to the XIM server was not sent: the link ends");
    }
    sent
}

/// The next message `pick` takes; `None` when the server refuses or goes.
async fn expect<T>(link: &mut Link, mut pick: impl FnMut(Message) -> Option<T>) -> Option<T> {
    loop {
        let bytes = match link.receive().await {
            Received::Message(bytes) => bytes,
            Received::Other => continue,
            Received::Gone => return None,
        };
        let message = Message::decode(&bytes).ok()?;
        if let Message::Error { code, .. } = message {
            tracing::debug!(code, "the XIM server refused to open");
            return None;
        }
        if let Some(picked) = pick(message) {
            return Some(picked);
        }
    }
}

/// One entry of a context's queue of syncs, answered in order.
#[derive(Clone, Copy, Debug)]
enum Entry {
    /// A key forwarded; `forwarded_back` once the server sent it back, not
    /// taken.
    Key {
        ticket: Ticket,
        keycode: u8,
        release: bool,
        forwarded_back: bool,
    },
    /// A focus told.
    Fence(u64),
}

/// One context as the session holds it.
struct Context {
    /// The client and focus window.
    window: xproto::Window,
    /// The server's id of it, once made.
    ic: Option<u16>,
    /// What was asked of it before it was made.
    waiting: Vec<Request>,
    /// The syncs sent and not answered, each with its deadline.
    syncs: VecDeque<(Instant, Entry)>,
    /// The preedit as the server last drew it, and its caret, in characters.
    preedit: Vec<char>,
    caret: usize,
    /// The events the server wants forwarded.
    forward: u32,
}

/// The session's state on an opened input method.
struct Session {
    im: u16,
    attributes: Attributes,
    style: u32,
    root: xproto::Window,
    inbox: Arc<Inbox>,
    contexts: HashMap<ContextId, Context>,
    ics: HashMap<u16, ContextId>,
    /// The contexts asked for, in the order the server answers them, each
    /// with its deadline; one closed meanwhile is destroyed once made.
    creating: VecDeque<(Instant, ContextId)>,
    /// The events the server wants forwarded, for contexts made from here.
    forward: u32,
    serial: u16,
}

/// Serve the owner's commands on `server` until the owner goes (`true`) or
/// the server does (`false`); every context is lost with the server.
pub(super) async fn serve(
    server: Server,
    commands: &mut mpsc::Receiver<Command>,
    lifecycle: &mut mpsc::UnboundedReceiver<Command>,
    inbox: &Arc<Inbox>,
) -> bool {
    let Server {
        mut link,
        im,
        attributes,
        style,
    } = server;
    let mut session = Session {
        im,
        attributes,
        style,
        root: link.root(),
        inbox: Arc::clone(inbox),
        contexts: HashMap::new(),
        ics: HashMap::new(),
        creating: VecDeque::new(),
        forward: KEY_PRESS_MASK | KEY_RELEASE_MASK,
        serial: 0,
    };
    let owner_went = loop {
        let due = session.next_due();
        let live = tokio::select! {
            biased;
            Some(command) = lifecycle.recv() => session.command(&mut link, command),
            () = tokio::time::sleep_until(due.unwrap_or_else(Instant::now)), if due.is_some() => {
                session.expire(Instant::now())
            }
            command = commands.recv() => {
                let Some(command) = command else {
                    break true;
                };
                session.command(&mut link, command)
            }
            received = link.receive() => match received {
                Received::Message(bytes) => session.heard(&mut link, &bytes),
                Received::Other => true,
                Received::Gone => false,
            },
        };
        if !live {
            break false;
        }
    };
    if !owner_went {
        tracing::debug!("the XIM server went: its contexts are lost");
    }
    let contexts: Vec<ContextId> = session.contexts.keys().copied().collect();
    for context in contexts {
        session.lost(context);
    }
    owner_went
}

impl Session {
    /// Carry out one command; `false` when the server can no longer be
    /// reached.
    fn command(&mut self, link: &mut Link, command: Command) -> bool {
        match command {
            Command::Open(context, window) => {
                self.contexts.insert(
                    context,
                    Context {
                        window,
                        ic: None,
                        waiting: Vec::new(),
                        syncs: VecDeque::new(),
                        preedit: Vec::new(),
                        caret: 0,
                        forward: self.forward,
                    },
                );
                let values = self.create_values(window);
                self.creating
                    .push_back((Instant::now() + CREATE_TIMEOUT, context));
                deliver(
                    link,
                    &Message::CreateIc {
                        im: self.im,
                        values,
                    },
                )
            }
            Command::Send(context, request) => {
                let Some(held) = self.contexts.get_mut(&context) else {
                    if let Some(heard) = Heard::unanswered(context, &request) {
                        self.inbox.post(heard);
                    }
                    return true;
                };
                let Some(ic) = held.ic else {
                    if held.waiting.len() < MAX_WAITING {
                        held.waiting.push(request);
                    } else {
                        // As on the bus: what the field told the context
                        // before it was made is no longer whole.
                        if let Some(answer @ Heard::Answer(..)) =
                            Heard::unanswered(context, &request)
                        {
                            self.inbox.post(answer);
                        }
                        self.inbox.post(Heard::Spilled(context));
                    }
                    return true;
                };
                self.request(link, context, ic, request)
            }
            Command::Close(context) => {
                let Some(held) = self.contexts.remove(&context) else {
                    return true;
                };
                // Its answers have no one to reach; one still being made is
                // destroyed once it is.
                let Some(ic) = held.ic else {
                    return true;
                };
                self.ics.remove(&ic);
                deliver(link, &Message::DestroyIc { im: self.im, ic })
            }
        }
    }

    /// The values a context is made with: the style, and the window as its
    /// client and its focus.
    fn create_values(&self, window: xproto::Window) -> Vec<AttrValue> {
        let attributes = self.attributes;
        [
            (attributes.input_style, self.style),
            (attributes.client_window, window),
            (attributes.focus_window, window),
        ]
        .into_iter()
        .filter_map(|(id, value)| {
            Some(AttrValue {
                id: id?,
                value: value.to_ne_bytes().to_vec(),
            })
        })
        .collect()
    }

    fn request(&mut self, link: &mut Link, context: ContextId, ic: u16, request: Request) -> bool {
        let im = self.im;
        match request {
            Request::FocusIn { fence } => {
                if !self.push(context, Entry::Fence(fence)) {
                    return true;
                }
                deliver(link, &Message::SetIcFocus { im, ic })
                    && deliver(link, &Message::Sync { im, ic })
            }
            Request::FocusOut => deliver(link, &Message::UnsetIcFocus { im, ic }),
            Request::Reset => {
                if let Some(held) = self.contexts.get_mut(&context) {
                    held.preedit.clear();
                    held.caret = 0;
                }
                deliver(link, &Message::ResetIc { im, ic })
            }
            Request::Key {
                ticket,
                keycode,
                state,
                release,
                time,
                ..
            } => {
                let Some(held) = self.contexts.get(&context) else {
                    return true;
                };
                let mask = if release {
                    KEY_RELEASE_MASK
                } else {
                    KEY_PRESS_MASK
                };
                if held.forward & mask == 0 {
                    // The server wants none of these: typed as it is.
                    self.inbox.post(Heard::Answer(context, ticket, false));
                    return true;
                }
                let window = held.window;
                let entry = Entry::Key {
                    ticket,
                    keycode,
                    release,
                    forwarded_back: false,
                };
                if !self.push(context, entry) {
                    return true;
                }
                self.serial = self.serial.wrapping_add(1);
                let event = key_event(
                    self.serial,
                    keycode,
                    state,
                    release,
                    time,
                    self.root,
                    window,
                );
                deliver(
                    link,
                    &Message::ForwardEvent {
                        im,
                        ic,
                        flag: 0,
                        serial: self.serial,
                        event,
                    },
                ) && deliver(link, &Message::Sync { im, ic })
            }
            Request::Cursor {
                x,
                y,
                width: _,
                height,
            } => {
                let attributes = self.attributes;
                let (Some(nested), Some(spot)) = (attributes.preedit, attributes.spot) else {
                    return true;
                };
                let Some(window) = self.contexts.get(&context).map(|held| held.window) else {
                    return true;
                };
                // The spot is the caret's baseline, in the window's own
                // coordinates.
                let Some((x, y)) = to_window(link, self.root, window, x, y.saturating_add(height))
                else {
                    return true;
                };
                let mut point = Vec::with_capacity(4);
                point.extend_from_slice(&x.to_ne_bytes());
                point.extend_from_slice(&y.to_ne_bytes());
                let values = vec![AttrValue {
                    id: nested,
                    value: wire::nested(&[AttrValue {
                        id: spot,
                        value: point,
                    }]),
                }];
                deliver(link, &Message::SetIcValues { im, ic, values })
            }
            // XIM has no surrounding text and no purpose
            // (docs/gaps.md#native-linux-host-wayland-and-x11).
            Request::Surrounding { .. } | Request::Content(_) => true,
        }
    }

    /// Queue `entry` for the context's next sync: `false`, and the context
    /// retired with a key answered as not taken, when the server is so far
    /// behind that its queue is full.
    fn push(&mut self, context: ContextId, entry: Entry) -> bool {
        let Some(held) = self.contexts.get_mut(&context) else {
            return false;
        };
        if held.syncs.len() >= MAX_WAITING {
            tracing::debug!("the XIM server is behind on a context: it is retired");
            if let Entry::Key { ticket, .. } = entry {
                self.inbox.post(Heard::Answer(context, ticket, false));
            }
            self.inbox.post(Heard::Spilled(context));
            return false;
        }
        held.syncs
            .push_back((Instant::now() + KEY_CALL_TIMEOUT, entry));
        true
    }

    /// Handle a message from the server; `false` when the link ends.
    fn heard(&mut self, link: &mut Link, bytes: &[u8]) -> bool {
        let message = match Message::decode(bytes) {
            Ok(message) => message,
            Err(error) => {
                // A message misread leaves the rest of the stream unknown.
                tracing::debug!(%error, "an unreadable message from the XIM server: the link ends");
                return false;
            }
        };
        let im = self.im;
        match message {
            Message::CreateIcReply { ic, .. } => {
                let Some((_, context)) = self.creating.pop_front() else {
                    return true;
                };
                let Some(held) = self.contexts.get_mut(&context) else {
                    // Closed while it was made.
                    return deliver(link, &Message::DestroyIc { im, ic });
                };
                held.ic = Some(ic);
                let waiting = std::mem::take(&mut held.waiting);
                self.ics.insert(ic, context);
                self.inbox.post(Heard::Ready(context));
                for request in waiting {
                    if !self.request(link, context, ic, request) {
                        return false;
                    }
                }
                true
            }
            Message::SetEventMask { ic, forward, .. } => {
                if let Some(held) = self.context_of(ic) {
                    held.forward = forward;
                } else if ic == 0 {
                    self.forward = forward;
                }
                true
            }
            Message::ForwardEvent {
                ic, flag, event, ..
            } => {
                if let Some(context) = self.ics.get(&ic).copied() {
                    self.forwarded_back(context, &event);
                }
                flag & FORWARD_SYNCHRONOUS == 0 || deliver(link, &Message::SyncReply { im, ic })
            }
            Message::SyncReply { ic, .. } => {
                if let Some(context) = self.ics.get(&ic).copied() {
                    self.synced(context);
                }
                true
            }
            Message::Sync { ic, .. } => deliver(link, &Message::SyncReply { im, ic }),
            Message::Commit {
                ic,
                synchronous,
                keysym,
                text,
                ..
            } => {
                if let Some(context) = self.ics.get(&ic).copied() {
                    let committed = match text {
                        Some(text) => wire::text_of(&text),
                        None => keysym.and_then(keysym_char).map(String::from),
                    };
                    match committed {
                        Some(text) if !text.is_empty() => {
                            self.inbox.post(Heard::Commit(context, text));
                        }
                        Some(_) => {}
                        None => {
                            tracing::debug!("an unreadable commit from the XIM server");
                            self.inbox.post(Heard::Spilled(context));
                        }
                    }
                }
                !synchronous || deliver(link, &Message::SyncReply { im, ic })
            }
            Message::PreeditStart { ic, .. } => {
                deliver(link, &Message::PreeditStartReply { im, ic, limit: -1 })
            }
            Message::PreeditDraw {
                im: _,
                ic,
                caret,
                first,
                length,
                status,
                text,
            } => {
                self.preedit_draw(ic, caret, first, length, status, &text);
                true
            }
            Message::PreeditCaret {
                ic,
                position,
                direction,
                ..
            } => {
                let position = self.preedit_caret(ic, position, direction);
                deliver(link, &Message::PreeditCaretReply { im, ic, position })
            }
            Message::PreeditDone { ic, .. } => {
                if let Some(context) = self.ics.get(&ic).copied() {
                    if let Some(held) = self.contexts.get_mut(&context) {
                        held.preedit.clear();
                        held.caret = 0;
                    }
                    self.inbox.post(Heard::Preedit(context, String::new(), 0));
                }
                true
            }
            Message::Error { ic, flag, code, .. } => {
                tracing::debug!(code, "the XIM server refused a request");
                if flag & wire::ERROR_IC_VALID != 0 {
                    if let Some(context) = self.ics.get(&ic).copied() {
                        self.lost(context);
                    }
                } else if let Some((_, context)) = self.creating.pop_front() {
                    // An error on no context answers the oldest making.
                    if self.contexts.contains_key(&context) {
                        self.lost(context);
                    }
                }
                true
            }
            Message::Disconnect | Message::Close { .. } => false,
            _ => true,
        }
    }

    fn context_of(&mut self, ic: u16) -> Option<&mut Context> {
        let context = self.ics.get(&ic)?;
        self.contexts.get_mut(context)
    }

    /// The server sent a key back: the oldest of that key it was forwarded,
    /// not yet sent back, was not taken.
    fn forwarded_back(&mut self, context: ContextId, event: &wire::WireEvent) {
        let release = match event[0] & 0x7f {
            KEY_PRESS => false,
            KEY_RELEASE => true,
            _ => return,
        };
        let keycode = event[1];
        let Some(held) = self.contexts.get_mut(&context) else {
            return;
        };
        let found = held.syncs.iter_mut().find_map(|(_, entry)| match entry {
            Entry::Key {
                ticket,
                keycode: of,
                release: was,
                forwarded_back,
            } if !*forwarded_back && *of == keycode && *was == release => {
                *forwarded_back = true;
                Some(*ticket)
            }
            _ => None,
        });
        if let Some(ticket) = found {
            self.inbox.post(Heard::Answer(context, ticket, false));
        }
    }

    /// The server answered a context's oldest sync.
    fn synced(&mut self, context: ContextId) {
        let Some((_, entry)) = self
            .contexts
            .get_mut(&context)
            .and_then(|held| held.syncs.pop_front())
        else {
            return;
        };
        match entry {
            Entry::Key {
                ticket,
                forwarded_back: false,
                ..
            } => self.inbox.post(Heard::Answer(context, ticket, true)),
            Entry::Key { .. } => {}
            Entry::Fence(fence) => self.inbox.post(Heard::Fenced(context, fence)),
        }
    }

    fn preedit_draw(
        &mut self,
        ic: u16,
        caret: i32,
        first: i32,
        length: i32,
        status: u32,
        text: &[u8],
    ) {
        let Some(context) = self.ics.get(&ic).copied() else {
            return;
        };
        let drawn = if status & wire::PREEDIT_NO_STRING != 0 {
            Some(String::new())
        } else {
            wire::text_of(text)
        };
        let Some(drawn) = drawn else {
            tracing::debug!("an unreadable preedit from the XIM server");
            self.inbox.post(Heard::Spilled(context));
            return;
        };
        let Some(held) = self.contexts.get_mut(&context) else {
            return;
        };
        let len = held.preedit.len();
        let first = clamp_index(first, len);
        let end = first.saturating_add(clamp_index(length, len)).min(len);
        held.preedit.splice(first..end, drawn.chars());
        if held.preedit.len() > MAX_PREEDIT {
            // A server that only ever inserts would grow it for good.
            tracing::debug!("a preedit past the longest held: its context is retired");
            held.preedit.clear();
            held.caret = 0;
            self.inbox.post(Heard::Spilled(context));
            return;
        }
        held.caret = clamp_index(caret, held.preedit.len());
        let (text, caret) = preedit_of(held);
        self.inbox.post(Heard::Preedit(context, text, caret));
    }

    /// Move the caret to `position` when the server names one; the caret
    /// where it is now.
    fn preedit_caret(&mut self, ic: u16, position: i32, direction: u32) -> i32 {
        let Some(context) = self.ics.get(&ic).copied() else {
            return position;
        };
        let Some(held) = self.contexts.get_mut(&context) else {
            return position;
        };
        if direction == CARET_ABSOLUTE {
            held.caret = clamp_index(position, held.preedit.len());
            let (text, caret) = preedit_of(held);
            self.inbox.post(Heard::Preedit(context, text, caret));
        }
        i32::try_from(held.caret).unwrap_or(i32::MAX)
    }

    fn next_due(&self) -> Option<Instant> {
        let syncs = self
            .contexts
            .values()
            .filter_map(|held| held.syncs.front().map(|(due, _)| *due));
        let creating = self.creating.front().map(|(due, _)| *due);
        syncs.chain(creating).min()
    }

    /// Retire what is overdue: a context whose oldest sync went unanswered,
    /// and the server when it made no context in time. `false` when the
    /// link ends.
    fn expire(&mut self, now: Instant) -> bool {
        if self.creating.front().is_some_and(|(due, _)| *due <= now) {
            // Its answers come in order: one never given leaves every later
            // making unmatched.
            tracing::debug!("the XIM server made no input context in time: the link ends");
            return false;
        }
        let overdue: Vec<ContextId> = self
            .contexts
            .iter()
            .filter(|(_, held)| held.syncs.front().is_some_and(|(due, _)| *due <= now))
            .map(|(context, _)| *context)
            .collect();
        for context in overdue {
            tracing::debug!("the XIM server did not answer a context in time: it is retired");
            let Some(held) = self.contexts.get_mut(&context) else {
                continue;
            };
            let syncs = std::mem::take(&mut held.syncs);
            for (_, entry) in syncs {
                if let Entry::Key {
                    ticket,
                    forwarded_back: false,
                    ..
                } = entry
                {
                    self.inbox.post(Heard::Answer(context, ticket, false));
                }
            }
            self.inbox.post(Heard::Spilled(context));
        }
        true
    }

    /// `context` is gone: what waits on it is answered.
    fn lost(&mut self, context: ContextId) {
        if let Some(held) = self.contexts.remove(&context) {
            if let Some(ic) = held.ic {
                self.ics.remove(&ic);
            }
            for request in held.waiting {
                if let Some(heard) = Heard::unanswered(context, &request) {
                    self.inbox.post(heard);
                }
            }
            for (_, entry) in held.syncs {
                match entry {
                    Entry::Key {
                        ticket,
                        forwarded_back: false,
                        ..
                    } => self.inbox.post(Heard::Answer(context, ticket, false)),
                    Entry::Key { .. } => {}
                    Entry::Fence(fence) => self.inbox.post(Heard::Fenced(context, fence)),
                }
            }
        }
        self.inbox.post(Heard::Lost(context));
    }
}

/// A preedit's text and its caret as a byte offset into it.
fn preedit_of(held: &Context) -> (String, i32) {
    let text: String = held.preedit.iter().collect();
    let caret: usize = held.preedit[..held.caret.min(held.preedit.len())]
        .iter()
        .map(|c| c.len_utf8())
        .sum();
    (text, i32::try_from(caret).unwrap_or(i32::MAX))
}

/// A server's index into a preedit of `len` characters, within it.
fn clamp_index(index: i32, len: usize) -> usize {
    usize::try_from(index).unwrap_or(0).min(len)
}

/// The character a keysym commits: Latin-1's own, or a Unicode keysym's.
fn keysym_char(keysym: u32) -> Option<char> {
    match keysym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(keysym),
        0x0100_0100..=0x0110_ffff => char::from_u32(keysym - 0x0100_0000),
        _ => None,
    }
}

/// A key as the X protocol's event, in this client's byte order.
fn key_event(
    serial: u16,
    keycode: u8,
    state: u16,
    release: bool,
    time: u32,
    root: xproto::Window,
    window: xproto::Window,
) -> wire::WireEvent {
    let mut event = [0u8; 32];
    event[0] = if release { KEY_RELEASE } else { KEY_PRESS };
    event[1] = keycode;
    event[2..4].copy_from_slice(&serial.to_ne_bytes());
    event[4..8].copy_from_slice(&time.to_ne_bytes());
    event[8..12].copy_from_slice(&root.to_ne_bytes());
    event[12..16].copy_from_slice(&window.to_ne_bytes());
    // No child; the pointer's place is not known here, and the server
    // reads none of it for a key.
    event[28..30].copy_from_slice(&state.to_ne_bytes());
    // Same screen.
    event[30] = 1;
    event
}

/// `x`, `y` of the root window in `window`'s coordinates.
fn to_window(
    link: &Link,
    root: xproto::Window,
    window: xproto::Window,
    x: i32,
    y: i32,
) -> Option<(i16, i16)> {
    let narrow = |value: i32| i16::try_from(value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)));
    let reply = link
        .conn()
        .translate_coordinates(root, window, narrow(x).ok()?, narrow(y).ok()?)
        .ok()?
        .reply()
        .ok()?;
    Some((reply.dst_x, reply.dst_y))
}

#[cfg(test)]
#[path = "xim_tests.rs"]
mod tests;
