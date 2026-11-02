//! The input method's bus, served on the background executor: one task that
//! holds the connection to IBus or Fcitx5, makes and destroys each window's
//! input context, sends what the owner asks, and posts what it hears back to
//! the owner in the order the daemon sent it.
//!
//! The answers to keys and the daemon's signals arrive on one stream, so a
//! string the input method committed for a key reaches the owner before the
//! answer that the key was taken, as the daemon sent them. A call is sent
//! without waiting for its answer, so a key the input method is slow on
//! holds no request behind it; its answer is matched by the call's serial.
//!
//! Nothing here runs on the owner thread or reaches application code: what
//! is heard waits in [`Inbox`] until the owner's turn takes it.

use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use zbus::message::Type as MessageType;
use zbus::{Connection, Message, MessageStream};
use zbus::{OwnedObjectPath, Variant as Value};

use crate::text::linux::input_context::{
    Daemon, Endpoint, IBUS_CAPABILITIES, IBUS_RELEASE, Ticket, X_KEYCODE_OFFSET, endpoints,
    fcitx_capability, ibus_address, ibus_address_file, ibus_content,
};
use crate::text::linux::text_input::ContentType;

/// How long a daemon has to answer the connection and the question whether
/// it runs.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// The most requests waiting for the bus task. Past it a key is not asked
/// and is delivered as typed; a field's state is stated again by its next
/// change.
const MAX_COMMANDS: usize = 1024;

/// The most things heard and not yet taken by the owner. Past it what the
/// daemon sends is dropped, its keys' answers among it: those keys run out
/// of time and are delivered as typed.
const MAX_HEARD: usize = 4096;

/// The most requests one context holds while the daemon makes it.
const MAX_WAITING: usize = 256;

/// Names one window's input context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ContextId(u64);

impl ContextId {
    /// The context after `self`.
    pub(crate) fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("BUG: a process opens fewer than 2^64 windows"),
        )
    }
}

/// What the owner asks of a window's input context.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    /// The window's field took the focus.
    FocusIn,
    /// It lost the focus.
    FocusOut,
    /// Does the input method take this key? Its answer names `ticket`.
    Key {
        ticket: Ticket,
        keysym: u32,
        keycode: u8,
        state: u16,
        release: bool,
        time: u32,
    },
    /// The text around the caret, the cursor and anchor in characters.
    Surrounding {
        text: String,
        cursor: u32,
        anchor: u32,
    },
    /// The field's purpose and hints.
    Content(ContentType),
    /// Where the caret is on the screen, in pixels of the root window.
    Cursor {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
}

/// What the bus task hears for a context, in the order the daemon sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    /// The daemon made the context: it may be asked about keys.
    Ready(ContextId),
    /// Whether the input method took the key asked about under the ticket.
    Answer(ContextId, Ticket, bool),
    /// The input method committed `text`.
    Commit(ContextId, String),
    /// The preedit is `text` with the caret at byte `caret` of it; empty
    /// text clears it.
    Preedit(ContextId, String, i32),
    /// Delete `chars` characters from `offset` characters past the cursor.
    Delete(ContextId, i32, u32),
    /// The context is gone (the daemon refused it, or the bus went): what
    /// waits on it is answered as not taken.
    Lost(ContextId),
}

impl Heard {
    /// The context it is of.
    pub(crate) fn context(&self) -> ContextId {
        match self {
            Self::Ready(context)
            | Self::Answer(context, ..)
            | Self::Commit(context, _)
            | Self::Preedit(context, ..)
            | Self::Delete(context, ..)
            | Self::Lost(context) => *context,
        }
    }
}

/// What the bus task heard, waiting for the owner's turn, and what wakes
/// the owner for it.
pub(crate) struct Inbox {
    heard: Mutex<VecDeque<Heard>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Inbox {
    fn post(&self, heard: Heard) {
        {
            let mut queue = self.heard.lock();
            if queue.len() >= MAX_HEARD {
                tracing::debug!(?heard, "the input method's messages are not taken: dropped");
                return;
            }
            queue.push_back(heard);
        }
        (self.wake)();
    }

    /// The first thing heard and not taken yet.
    pub(crate) fn take(&self) -> Option<Heard> {
        self.heard.lock().pop_front()
    }
}

/// How the bus task finds the input method.
#[derive(Clone, Debug)]
pub(crate) enum Reach {
    /// As the session names it: the environment, IBus's address file and
    /// the session bus.
    Session,
    /// `daemon` at bus `address`, its well-known name owned there: a
    /// stand-in on a test's private bus.
    #[cfg(test)]
    Address { daemon: Daemon, address: String },
}

enum Command {
    Open(ContextId),
    Send(ContextId, Request),
    Close(ContextId),
}

/// The owner's end of the bus task.
pub(crate) struct Bus {
    commands: mpsc::Sender<Command>,
    inbox: Arc<Inbox>,
    task: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bus").finish_non_exhaustive()
    }
}

impl Bus {
    /// Start the bus task on `runtime`, finding the input method by
    /// `reach`; `wake` wakes the owner when something was heard.
    pub(crate) fn start(
        runtime: &tokio::runtime::Handle,
        reach: Reach,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(MAX_COMMANDS);
        let inbox = Arc::new(Inbox {
            heard: Mutex::new(VecDeque::new()),
            wake,
        });
        let task = runtime.spawn(serve(reach, receiver, Arc::clone(&inbox)));
        Self {
            commands,
            inbox,
            task,
        }
    }

    /// What the task heard.
    pub(crate) fn inbox(&self) -> &Inbox {
        &self.inbox
    }

    /// Make context `context`; it is [`Heard::Ready`] once the daemon made
    /// it, else [`Heard::Lost`].
    pub(crate) fn open(&self, context: ContextId) {
        if self.commands.try_send(Command::Open(context)).is_err() {
            self.inbox.post(Heard::Lost(context));
        }
    }

    /// Send `request` on `context`: `false` when the task takes no more
    /// now, and the request is not sent.
    pub(crate) fn send(&self, context: ContextId, request: Request) -> bool {
        self.commands
            .try_send(Command::Send(context, request))
            .is_ok()
    }

    /// Destroy `context`; nothing more is heard of it.
    pub(crate) fn close(&self, context: ContextId) {
        if self.commands.try_send(Command::Close(context)).is_err() {
            tracing::debug!("an input context goes undestroyed: the bus task is behind");
        }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// How one daemon is reached on a connection.
#[derive(Clone, Copy, Debug)]
struct Names {
    daemon: Daemon,
    destination: &'static str,
    /// Where `CreateInputContext` is asked.
    path: &'static str,
    interface: &'static str,
}

impl Names {
    fn of(endpoint: Endpoint) -> Self {
        match (endpoint.daemon, endpoint.portal) {
            (Daemon::IBus, false) => Self {
                daemon: Daemon::IBus,
                destination: "org.freedesktop.IBus",
                path: "/org/freedesktop/IBus",
                interface: "org.freedesktop.IBus",
            },
            (Daemon::IBus, true) => Self {
                daemon: Daemon::IBus,
                destination: "org.freedesktop.portal.IBus",
                path: "/org/freedesktop/IBus",
                interface: "org.freedesktop.IBus.Portal",
            },
            (Daemon::Fcitx5, portal) => Self {
                daemon: Daemon::Fcitx5,
                destination: if portal {
                    "org.freedesktop.portal.Fcitx"
                } else {
                    "org.fcitx.Fcitx5"
                },
                path: "/org/freedesktop/portal/inputmethod",
                interface: "org.fcitx.Fcitx.InputMethod1",
            },
        }
    }

    /// The input context's interface.
    fn context_interface(self) -> &'static str {
        match self.daemon {
            Daemon::IBus => "org.freedesktop.IBus.InputContext",
            Daemon::Fcitx5 => "org.fcitx.Fcitx.InputContext1",
        }
    }
}

/// A connection to the daemon `names` reaches.
struct Link {
    connection: Connection,
    names: Names,
}

/// Connect to the first endpoint whose daemon runs.
async fn connect(reach: Reach) -> Option<Link> {
    match reach {
        #[cfg(test)]
        Reach::Address { daemon, address } => {
            let names = Names::of(Endpoint {
                daemon,
                portal: false,
            });
            let connection =
                connect_to(zbus::connection::Builder::address(address.as_str())).await?;
            runs(&connection, names.destination)
                .await
                .then_some(Link { connection, names })
        }
        Reach::Session => {
            let var = |name: &str| std::env::var(name).ok();
            let sandboxed = std::path::Path::new("/.flatpak-info").exists();
            for endpoint in endpoints(var, sandboxed) {
                let names = Names::of(endpoint);
                let connection = if endpoint.daemon == Daemon::IBus && !endpoint.portal {
                    let Some(address) = ibus_session_address() else {
                        continue;
                    };
                    connect_to(zbus::connection::Builder::address(address.as_str())).await
                } else {
                    connect_to(zbus::connection::Builder::session()).await
                };
                let Some(connection) = connection else {
                    continue;
                };
                if runs(&connection, names.destination).await {
                    return Some(Link { connection, names });
                }
            }
            None
        }
    }
}

/// IBus's bus address in this session, as `ibus_get_address` reads it.
fn ibus_session_address() -> Option<String> {
    let var = |name: &str| std::env::var(name).ok();
    if let Some(address) = var("IBUS_ADDRESS") {
        return Some(address);
    }
    let machine_id = std::fs::read_to_string("/var/lib/dbus/machine-id")
        .or_else(|_| std::fs::read_to_string("/etc/machine-id"))
        .ok()?;
    let config = var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| var("HOME").map(|home| std::path::Path::new(&home).join(".config")))?;
    let file = ibus_address_file(var, &machine_id, &config);
    ibus_address(&std::fs::read_to_string(file).ok()?)
}

async fn connect_to(builder: zbus::connection::Builder<'_>) -> Option<Connection> {
    let built = tokio::time::timeout(
        CONNECT_TIMEOUT,
        builder.method_timeout(CONNECT_TIMEOUT).build(),
    )
    .await;
    match built {
        Ok(Ok(connection)) => Some(connection),
        Ok(Err(error)) => {
            tracing::debug!(%error, "an input method's bus did not answer");
            None
        }
        Err(_) => {
            tracing::debug!("an input method's bus timed out");
            None
        }
    }
}

/// Whether `name` has an owner on `connection`'s bus.
async fn runs(connection: &Connection, name: &str) -> bool {
    let asked = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(name,),
        )
        .await;
    asked
        .ok()
        .and_then(|reply| reply.body().deserialize::<bool>().ok())
        .unwrap_or(false)
}

/// One context as the task holds it.
#[derive(Default)]
struct Context {
    /// Its object, once the daemon made it.
    path: Option<OwnedObjectPath>,
    /// What was asked of it before it was made.
    waiting: Vec<Request>,
    /// IBus's preedit as last updated, which `Show` and `Hide` show again
    /// and hide.
    preedit: (String, i32),
}

/// What a call sent waits to hear.
enum Awaited {
    Create(ContextId),
    Key(ContextId, Ticket),
}

/// The bus task's state on a live connection.
struct Session {
    link: Link,
    inbox: Arc<Inbox>,
    contexts: HashMap<ContextId, Context>,
    paths: HashMap<OwnedObjectPath, ContextId>,
    calls: HashMap<NonZeroU32, Awaited>,
}

async fn serve(reach: Reach, mut commands: mpsc::Receiver<Command>, inbox: Arc<Inbox>) {
    let Some(link) = connect(reach).await else {
        tracing::info!("no input method answers on D-Bus: X11 windows take keys as typed (gap 5)");
        // Every context is lost at once, so no key waits on it.
        refuse(&mut commands, &inbox).await;
        return;
    };
    // The daemon's signals reach this connection alone; a bus that
    // broadcasts them (a portal's) is asked for them.
    let rule = format!("type='signal',sender='{}'", link.names.destination);
    if let Err(error) = link
        .connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "AddMatch",
            &(rule.as_str(),),
        )
        .await
    {
        tracing::debug!(%error, "the input method's signals are not asked for");
    }
    let mut stream = MessageStream::from(&link.connection);
    let mut session = Session {
        link,
        inbox,
        contexts: HashMap::new(),
        paths: HashMap::new(),
        calls: HashMap::new(),
    };
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    return;
                };
                session.command(command).await;
            }
            message = stream.next() => {
                match message {
                    Some(Ok(message)) => session.heard(&message).await,
                    Some(Err(error)) => {
                        tracing::debug!(%error, "an unreadable message from the input method");
                    }
                    None => break,
                }
            }
        }
    }
    tracing::debug!("the input method's bus went: its contexts are lost");
    for context in session.contexts.keys() {
        session.inbox.post(Heard::Lost(*context));
    }
    refuse(&mut commands, &session.inbox).await;
}

impl Session {
    async fn command(&mut self, command: Command) {
        match command {
            Command::Open(context) => {
                self.contexts.insert(context, Context::default());
                let names = self.link.names;
                let call = match names.daemon {
                    Daemon::IBus => message(
                        names.destination,
                        names.path,
                        names.interface,
                        "CreateInputContext",
                    )
                    .build(&("FLUI",)),
                    Daemon::Fcitx5 => {
                        let args: Vec<(&str, &str)> = vec![("program", "flui")];
                        message(
                            names.destination,
                            names.path,
                            names.interface,
                            "CreateInputContext",
                        )
                        .build(&(args,))
                    }
                };
                if !self.send(call, Some(Awaited::Create(context))).await {
                    self.lost(context);
                }
            }
            Command::Send(context, request) => {
                let Some(held) = self.contexts.get_mut(&context) else {
                    // Closed, or never opened: a key's answer is that it was
                    // not taken.
                    if let Request::Key { ticket, .. } = request {
                        self.inbox.post(Heard::Answer(context, ticket, false));
                    }
                    return;
                };
                let Some(path) = held.path.clone() else {
                    if held.waiting.len() < MAX_WAITING {
                        held.waiting.push(request);
                    } else if let Request::Key { ticket, .. } = request {
                        self.inbox.post(Heard::Answer(context, ticket, false));
                    }
                    return;
                };
                self.request(context, &path, request).await;
            }
            Command::Close(context) => {
                let Some(held) = self.contexts.remove(&context) else {
                    return;
                };
                let Some(path) = held.path else {
                    return;
                };
                self.paths.remove(&path);
                let call = destroy(self.link.names, &path);
                self.send(call, None).await;
            }
        }
    }

    /// Send `request` on the context at `path`.
    async fn request(&mut self, context: ContextId, path: &OwnedObjectPath, request: Request) {
        let (call, awaited) = self.build(context, path, request);
        let key = match &awaited {
            Some(Awaited::Key(context, ticket)) => Some((*context, *ticket)),
            _ => None,
        };
        if !self.send(call, awaited).await
            && let Some((context, ticket)) = key
        {
            self.inbox.post(Heard::Answer(context, ticket, false));
        }
    }

    /// `request` on the context at `path` as a call, and what its answer is
    /// awaited for.
    fn build(
        &self,
        context: ContextId,
        path: &OwnedObjectPath,
        request: Request,
    ) -> (zbus::Result<Message>, Option<Awaited>) {
        let names = self.link.names;
        let on = |member: &'static str| {
            message(
                names.destination,
                path.as_str(),
                names.context_interface(),
                member,
            )
        };
        match (names.daemon, request) {
            (_, Request::FocusIn) => (on("FocusIn").build(&()), None),
            (_, Request::FocusOut) => (on("FocusOut").build(&()), None),
            (
                Daemon::IBus,
                Request::Key {
                    ticket,
                    keysym,
                    keycode,
                    state,
                    release,
                    ..
                },
            ) => {
                let state = u32::from(state) | if release { IBUS_RELEASE } else { 0 };
                let keycode = u32::from(keycode).saturating_sub(X_KEYCODE_OFFSET);
                (
                    on("ProcessKeyEvent").build(&(keysym, keycode, state)),
                    Some(Awaited::Key(context, ticket)),
                )
            }
            (
                Daemon::Fcitx5,
                Request::Key {
                    ticket,
                    keysym,
                    keycode,
                    state,
                    release,
                    time,
                },
            ) => (
                on("ProcessKeyEvent").build(&(
                    keysym,
                    u32::from(keycode),
                    u32::from(state),
                    release,
                    time,
                )),
                Some(Awaited::Key(context, ticket)),
            ),
            (
                Daemon::IBus,
                Request::Surrounding {
                    text,
                    cursor,
                    anchor,
                },
            ) => (
                on("SetSurroundingText").build(&(ibus_text(&text), cursor, anchor)),
                None,
            ),
            (
                Daemon::Fcitx5,
                Request::Surrounding {
                    text,
                    cursor,
                    anchor,
                },
            ) => (
                on("SetSurroundingText").build(&(text.as_str(), cursor, anchor)),
                None,
            ),
            (Daemon::IBus, Request::Content(content)) => (
                message(
                    names.destination,
                    path.as_str(),
                    "org.freedesktop.DBus.Properties",
                    "Set",
                )
                .build(&(
                    names.context_interface(),
                    "ContentType",
                    Value::from(ibus_content(content)),
                )),
                None,
            ),
            (Daemon::Fcitx5, Request::Content(content)) => (
                on("SetCapability").build(&(fcitx_capability(content),)),
                None,
            ),
            (
                Daemon::IBus,
                Request::Cursor {
                    x,
                    y,
                    width,
                    height,
                },
            ) => (on("SetCursorLocation").build(&(x, y, width, height)), None),
            (
                Daemon::Fcitx5,
                Request::Cursor {
                    x,
                    y,
                    width,
                    height,
                },
            ) => (on("SetCursorRect").build(&(x, y, width, height)), None),
        }
    }

    /// Send `call`, keeping what its answer is awaited for: `false` when it
    /// could not be built or sent.
    async fn send(&mut self, call: zbus::Result<Message>, awaited: Option<Awaited>) -> bool {
        let call = match call {
            Ok(call) => call,
            Err(error) => {
                tracing::debug!(%error, "a request to the input method could not be built");
                return false;
            }
        };
        if let Some(awaited) = awaited {
            self.calls
                .insert(call.primary_header().serial_num(), awaited);
        }
        match self.link.connection.send(&call).await {
            Ok(()) => true,
            Err(error) => {
                self.calls.remove(&call.primary_header().serial_num());
                tracing::debug!(%error, "a request to the input method was not sent");
                false
            }
        }
    }

    /// `context` is gone: what waits on it is answered.
    fn lost(&mut self, context: ContextId) {
        if let Some(held) = self.contexts.remove(&context) {
            if let Some(path) = held.path {
                self.paths.remove(&path);
            }
            for request in held.waiting {
                if let Request::Key { ticket, .. } = request {
                    self.inbox.post(Heard::Answer(context, ticket, false));
                }
            }
        }
        self.inbox.post(Heard::Lost(context));
    }

    /// A message from the bus: an answer to a call, or a signal of a context.
    async fn heard(&mut self, message: &Message) {
        let header = message.header();
        match message.message_type() {
            MessageType::MethodReturn | MessageType::Error => {
                let Some(awaited) = header
                    .reply_serial()
                    .and_then(|serial| self.calls.remove(&serial))
                else {
                    return;
                };
                let failed = message.message_type() == MessageType::Error;
                match awaited {
                    Awaited::Key(context, ticket) => {
                        let taken =
                            !failed && message.body().deserialize::<bool>().unwrap_or(false);
                        self.inbox.post(Heard::Answer(context, ticket, taken));
                    }
                    Awaited::Create(context) => {
                        let path = if failed {
                            None
                        } else {
                            match self.link.names.daemon {
                                Daemon::IBus => {
                                    message.body().deserialize::<OwnedObjectPath>().ok()
                                }
                                Daemon::Fcitx5 => message
                                    .body()
                                    .deserialize::<(OwnedObjectPath, Vec<u8>)>()
                                    .ok()
                                    .map(|(path, _)| path),
                            }
                        };
                        if let Some(path) = path {
                            self.made(context, path).await;
                        } else {
                            tracing::debug!("the input method refused an input context");
                            self.lost(context);
                        }
                    }
                }
            }
            MessageType::Signal => {
                let Some(context) = header
                    .path()
                    .and_then(|path| self.paths.get(&OwnedObjectPath::from(path.to_owned())))
                    .copied()
                else {
                    return;
                };
                let Some(member) = header.member() else {
                    return;
                };
                if let Some(heard) = self.signal(context, member.as_str(), message) {
                    self.inbox.post(heard);
                }
            }
            MessageType::MethodCall => {}
        }
    }

    /// What a signal of `context` tells the owner.
    fn signal(&mut self, context: ContextId, member: &str, message: &Message) -> Option<Heard> {
        let body = message.body();
        match (self.link.names.daemon, member) {
            (Daemon::IBus, "CommitText") => {
                let (text,): (Value<'_>,) = body.deserialize().ok()?;
                Some(Heard::Commit(context, ibus_text_of(&text)?))
            }
            (Daemon::IBus, "UpdatePreeditText" | "UpdatePreeditTextWithMode") => {
                let (text, cursor, visible): (Value<'_>, u32, bool) =
                    body.deserialize_ignore_signature().ok()?;
                let text = ibus_text_of(&text)?;
                let caret = crate::text::linux::input_context::preedit_caret(&text, cursor);
                let held = self.contexts.get_mut(&context)?;
                held.preedit = (text.clone(), caret);
                Some(if visible {
                    Heard::Preedit(context, text, caret)
                } else {
                    Heard::Preedit(context, String::new(), 0)
                })
            }
            (Daemon::IBus, "ShowPreeditText") => {
                let (text, caret) = self.contexts.get(&context)?.preedit.clone();
                Some(Heard::Preedit(context, text, caret))
            }
            (Daemon::IBus, "HidePreeditText") => Some(Heard::Preedit(context, String::new(), 0)),
            (Daemon::Fcitx5, "CommitString") => {
                let text: String = body.deserialize().ok()?;
                Some(Heard::Commit(context, text))
            }
            (Daemon::Fcitx5, "UpdateFormattedPreedit") => {
                let (parts, caret): (Vec<(String, i32)>, i32) = body.deserialize().ok()?;
                let text: String = parts.into_iter().map(|(part, _)| part).collect();
                Some(Heard::Preedit(context, text, caret))
            }
            (_, "DeleteSurroundingText") => {
                let (offset, chars): (i32, u32) = body.deserialize().ok()?;
                Some(Heard::Delete(context, offset, chars))
            }
            (_, "ForwardKeyEvent" | "ForwardKey") => {
                tracing::debug!(
                    "the input method forwarded a key, which X11 windows do not take yet (gap 5)"
                );
                None
            }
            _ => None,
        }
    }

    /// The daemon made `context` at `path`: its capabilities are stated and
    /// what waited for it is sent, in order before anything newer, and the
    /// owner may ask it about keys. One closed while it was made goes.
    async fn made(&mut self, context: ContextId, path: OwnedObjectPath) {
        let names = self.link.names;
        let Some(held) = self.contexts.get_mut(&context) else {
            let call = destroy(names, &path);
            self.send(call, None).await;
            return;
        };
        held.path = Some(path.clone());
        let waiting = std::mem::take(&mut held.waiting);
        self.paths.insert(path.clone(), context);
        let capabilities = match names.daemon {
            Daemon::IBus => message(
                names.destination,
                path.as_str(),
                names.context_interface(),
                "SetCapabilities",
            )
            .build(&(IBUS_CAPABILITIES,)),
            Daemon::Fcitx5 => message(
                names.destination,
                path.as_str(),
                names.context_interface(),
                "SetCapability",
            )
            .build(&(fcitx_capability(ContentType {
                hint: 0,
                purpose: ContentType::NORMAL,
            }),)),
        };
        self.send(capabilities, None).await;
        self.inbox.post(Heard::Ready(context));
        for request in waiting {
            self.request(context, &path, request).await;
        }
    }
}

/// The call that destroys the context at `path`.
fn destroy(names: Names, path: &OwnedObjectPath) -> zbus::Result<Message> {
    match names.daemon {
        Daemon::IBus => message(
            names.destination,
            path.as_str(),
            "org.freedesktop.IBus.Service",
            "Destroy",
        )
        .build(&()),
        Daemon::Fcitx5 => message(
            names.destination,
            path.as_str(),
            names.context_interface(),
            "DestroyIC",
        )
        .build(&()),
    }
}

/// A method call to `member` of `interface` at `path` of `destination`.
fn message<'a>(
    destination: &'static str,
    path: &'a str,
    interface: &'static str,
    member: &'static str,
) -> zbus::message::Builder<'a> {
    Message::method_call(path, member)
        .destination(destination)
        .interface(interface)
}

/// `text` as an `IBusText`: the serialized object IBus passes text as,
/// `("IBusText", {}, text, ("IBusAttrList", {}, []))`.
fn ibus_text(text: &str) -> Value<'static> {
    let attributes = Value::from((
        "IBusAttrList",
        HashMap::<String, Value<'static>>::new(),
        Vec::<Value<'static>>::new(),
    ));
    Value::from((
        "IBusText",
        HashMap::<String, Value<'static>>::new(),
        text.to_owned(),
        attributes,
    ))
}

/// The text of an `IBusText` (its third field).
pub(crate) fn ibus_text_of(value: &Value<'_>) -> Option<String> {
    match value {
        Value::Variant(inner) => ibus_text_of(inner),
        Value::Structure(structure) => match structure.fields().get(2)? {
            Value::Str(text) => Some(text.as_str().to_owned()),
            _ => None,
        },
        Value::Str(text) => Some(text.as_str().to_owned()),
        _ => None,
    }
}

/// With no input method on the bus: every context is lost as it is opened,
/// and every key asked is answered as not taken, so nothing waits on a
/// daemon that is not there.
async fn refuse(commands: &mut mpsc::Receiver<Command>, inbox: &Inbox) {
    while let Some(command) = commands.recv().await {
        match command {
            Command::Open(context) => inbox.post(Heard::Lost(context)),
            Command::Send(context, Request::Key { ticket, .. }) => {
                inbox.post(Heard::Answer(context, ticket, false));
            }
            Command::Send(..) | Command::Close(_) => {}
        }
    }
}
