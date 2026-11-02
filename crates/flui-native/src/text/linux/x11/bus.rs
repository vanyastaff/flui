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

use std::collections::{HashMap, HashSet, VecDeque};
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
    ANSWER_TIMEOUT, Daemon, Endpoint, IBUS_CAPABILITIES, IBUS_RELEASE, Ticket, X_KEYCODE_OFFSET,
    endpoints, fcitx_capability, ibus_address, ibus_address_file, ibus_content,
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
pub(super) const MAX_HEARD: usize = 4096;

/// The most requests one context holds while the daemon makes it.
const MAX_WAITING: usize = 256;

/// The most calls awaiting the daemon's answer. Past it a key is not asked
/// and is delivered as typed.
const MAX_CALLS: usize = 1024;

/// How long a key's call is awaited: past the owner's own deadline its
/// answer is dropped there anyway.
const KEY_CALL_TIMEOUT: Duration = ANSWER_TIMEOUT.saturating_mul(2);

/// How long the daemon has to make a context before it counts as lost.
pub(super) const CREATE_TIMEOUT: Duration = Duration::from_secs(5);

/// The most context makings given up on whose late answer still destroys
/// what they made. Past it the oldest is forgotten: a daemon that slow
/// leaves its contexts to the connection's end.
const MAX_ABANDONED: usize = 64;

/// The message bus's own name, the sender of what it says about names.
const BUS_NAME: &str = "org.freedesktop.DBus";

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
    FocusIn {
        /// Named again by [`Heard::Fenced`] once the daemon answered it:
        /// what it sent before is the earlier field's.
        fence: u64,
    },
    /// It lost the focus.
    FocusOut,
    /// The client ended the composition: the engine forgets it.
    Reset,
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
    /// The engine asks for the field's state: one attached after the field
    /// last told it heard nothing of it.
    StateAsked(ContextId),
    /// Something the context sent was dropped, the owner behind: nothing
    /// more of it was queued, and the owner retires it.
    Spilled(ContextId),
    /// The daemon answered the [`Request::FocusIn`] of this fence, or will
    /// not: what it sends from here is for the field it names.
    Fenced(ContextId, u64),
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
            | Self::StateAsked(context)
            | Self::Spilled(context)
            | Self::Fenced(context, _)
            | Self::Lost(context) => *context,
        }
    }

    /// The answer to `request` the daemon never gives: a key not taken, a
    /// focus that fences nothing. `None` for a request no answer awaits.
    fn unanswered(context: ContextId, request: &Request) -> Option<Self> {
        match *request {
            Request::Key { ticket, .. } => Some(Self::Answer(context, ticket, false)),
            Request::FocusIn { fence } => Some(Self::Fenced(context, fence)),
            _ => None,
        }
    }
}

/// What the bus task heard, waiting for the owner's turn, and what wakes
/// the owner for it.
pub(crate) struct Inbox {
    heard: Mutex<Heards>,
    wake: Box<dyn Fn() + Send + Sync>,
    /// How many calls the task awaits an answer to, as its last turn left
    /// them.
    #[cfg(test)]
    pub(crate) awaited: std::sync::atomic::AtomicUsize,
}

/// The queue the owner takes from, at most [`MAX_HEARD`] past the entries
/// kept beyond it.
#[derive(Default)]
struct Heards {
    queue: VecDeque<Heard>,
    /// The contexts something was dropped of: until the owner takes their
    /// [`Heard::Spilled`], nothing more of them is queued, so no answer or
    /// edit applies past the gap.
    spilled: HashSet<ContextId>,
}

impl Inbox {
    fn post(&self, heard: Heard) {
        {
            let mut heards = self.heard.lock();
            let Heards { queue, spilled } = &mut *heards;
            let context = heard.context();
            // A fence or a loss is kept past the bound: one comes of each
            // focus and each context the owner asked for, and without it the
            // owner would wait on that context for good.
            let kept = matches!(heard, Heard::Fenced(..) | Heard::Lost(_));
            if !kept && spilled.contains(&context) {
                return;
            }
            if queue.len() >= MAX_HEARD && !kept {
                tracing::debug!(
                    ?heard,
                    "the input method's messages are not taken: its context is retired"
                );
                // An edit or an answer lost leaves the rest of that context's
                // stream wrong: it is retired, as one past a key's deadline.
                // One mark per context, so the bound still holds.
                spilled.insert(context);
                queue.push_back(Heard::Spilled(context));
            } else {
                queue.push_back(heard);
            }
        }
        (self.wake)();
    }

    /// How many things heard wait for the owner.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.heard.lock().queue.len()
    }

    /// The first thing heard and not taken yet.
    pub(crate) fn take(&self) -> Option<Heard> {
        let mut heards = self.heard.lock();
        let heard = heards.queue.pop_front()?;
        if let Heard::Spilled(context) = heard {
            heards.spilled.remove(&context);
        }
        Some(heard)
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
    /// The contexts to make and destroy, apart from the bounded commands and
    /// taken before them: a window's context is never dropped while the task
    /// is behind, and is made before anything is sent on it. One of each per
    /// window and per replacement, so bounded by what the owner does.
    lifecycle: mpsc::UnboundedSender<Command>,
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
        let (lifecycle, lifecycles) = mpsc::unbounded_channel();
        let inbox = Arc::new(Inbox {
            heard: Mutex::new(Heards::default()),
            #[cfg(test)]
            awaited: std::sync::atomic::AtomicUsize::new(0),
            wake,
        });
        let task = runtime.spawn(serve(reach, receiver, lifecycles, Arc::clone(&inbox)));
        Self {
            commands,
            lifecycle,
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
        // Fails only once the task ended, and its connection with it.
        if self.lifecycle.send(Command::Open(context)).is_err() {
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
        // Fails only once the task ended, and its connection with it.
        if self.lifecycle.send(Command::Close(context)).is_err() {
            tracing::debug!("an input context closes with its bus already gone");
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
    Focus(ContextId, u64),
}

impl Awaited {
    /// What the owner hears of a call answered with an error, unsent or
    /// given up on; `None` for a context's making, which is handled apart.
    fn unanswered(&self) -> Option<Heard> {
        match *self {
            Self::Create(_) => None,
            Self::Key(context, ticket) => Some(Heard::Answer(context, ticket, false)),
            Self::Focus(context, fence) => Some(Heard::Fenced(context, fence)),
        }
    }

    /// The context it waits for an answer about.
    fn context(&self) -> ContextId {
        match *self {
            Self::Create(context) | Self::Key(context, _) | Self::Focus(context, _) => context,
        }
    }
}

/// The bus task's state on a live connection.
struct Session {
    link: Link,
    inbox: Arc<Inbox>,
    contexts: HashMap<ContextId, Context>,
    paths: HashMap<OwnedObjectPath, ContextId>,
    /// The calls awaiting an answer, each until its deadline, at most
    /// [`MAX_CALLS`].
    calls: HashMap<NonZeroU32, (tokio::time::Instant, Awaited)>,
    /// The calls making a context that were given up on, newest last, at most
    /// [`MAX_ABANDONED`]: a context made late after all is destroyed.
    abandoned: VecDeque<NonZeroU32>,
}

async fn serve(
    reach: Reach,
    mut commands: mpsc::Receiver<Command>,
    mut lifecycle: mpsc::UnboundedReceiver<Command>,
    inbox: Arc<Inbox>,
) {
    let Some(link) = connect(reach).await else {
        tracing::info!("no input method answers on D-Bus: X11 windows take keys as typed (gap 5)");
        // Every context is lost at once, so no key waits on it.
        refuse(&mut commands, &mut lifecycle, &inbox).await;
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
    // A daemon that stops leaves a session bus running, so the stream does
    // not end: its name losing its owner says it went.
    let owner = format!(
        "type='signal',sender='{BUS_NAME}',interface='{BUS_NAME}',member='NameOwnerChanged',arg0='{}'",
        link.names.destination
    );
    if let Err(error) = link
        .connection
        .call_method(
            Some(BUS_NAME),
            "/org/freedesktop/DBus",
            Some(BUS_NAME),
            "AddMatch",
            &(owner.as_str(),),
        )
        .await
    {
        tracing::debug!(%error, "the input method's going is not watched");
    }
    let mut stream = MessageStream::from(&link.connection);
    let mut session = Session {
        link,
        inbox,
        contexts: HashMap::new(),
        paths: HashMap::new(),
        calls: HashMap::new(),
        abandoned: VecDeque::new(),
    };
    loop {
        let due = session.next_due();
        tokio::select! {
            // A context is made or destroyed before any command is taken:
            // the owner sends a context's making before anything on it, so a
            // command sent on it finds it made, and one sent on a context
            // destroyed is answered as not taken.
            biased;
            Some(command) = lifecycle.recv() => {
                session.command(command).await;
            }
            () = tokio::time::sleep_until(due.unwrap_or_else(tokio::time::Instant::now)), if due.is_some() => {
                session.expire(tokio::time::Instant::now());
            }
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
        #[cfg(test)]
        session
            .inbox
            .awaited
            .store(session.calls.len(), std::sync::atomic::Ordering::Relaxed);
    }
    tracing::debug!("the input method's bus went: its contexts are lost");
    for context in session.contexts.keys() {
        session.inbox.post(Heard::Lost(*context));
    }
    refuse(&mut commands, &mut lifecycle, &session.inbox).await;
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
                    if let Some(heard) = Heard::unanswered(context, &request) {
                        self.inbox.post(heard);
                    }
                    return;
                };
                let Some(path) = held.path.clone() else {
                    if held.waiting.len() < MAX_WAITING {
                        held.waiting.push(request);
                    } else if let Some(heard) = Heard::unanswered(context, &request) {
                        self.inbox.post(heard);
                    }
                    return;
                };
                self.request(context, &path, request).await;
            }
            Command::Close(context) => {
                let Some(held) = self.contexts.remove(&context) else {
                    return;
                };
                // Its answers have no one to reach; the call that makes it
                // stays, so a context made after all is destroyed.
                self.calls.retain(|_, (_, awaited)| {
                    matches!(awaited, Awaited::Create(_)) || awaited.context() != context
                });
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
        let unanswered = awaited.as_ref().and_then(Awaited::unanswered);
        if !self.send(call, awaited).await
            && let Some(heard) = unanswered
        {
            self.inbox.post(heard);
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
            (_, Request::FocusIn { fence }) => (
                on("FocusIn").build(&()),
                Some(Awaited::Focus(context, fence)),
            ),
            (_, Request::FocusOut) => (on("FocusOut").build(&()), None),
            (_, Request::Reset) => (on("Reset").build(&()), None),
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
            if self.calls.len() >= MAX_CALLS {
                tracing::debug!("too many calls await the input method: one is not sent");
                return false;
            }
            let timeout = match awaited {
                Awaited::Create(_) => CREATE_TIMEOUT,
                Awaited::Key(..) | Awaited::Focus(..) => KEY_CALL_TIMEOUT,
            };
            self.calls.insert(
                call.primary_header().serial_num(),
                (tokio::time::Instant::now() + timeout, awaited),
            );
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

    /// When the first call awaiting an answer runs out of time.
    fn next_due(&self) -> Option<tokio::time::Instant> {
        self.calls.values().map(|(due, _)| *due).min()
    }

    /// Give up on the calls unanswered by `now`. A key's late answer would
    /// be dropped anyway, the owner having delivered the key at its own
    /// deadline; a focus fences no longer, so the field hears the daemon
    /// again; a context the daemon did not make in time is lost, so no key
    /// waits on it, and destroyed should the daemon make it late.
    fn expire(&mut self, now: tokio::time::Instant) {
        let mut overdue = Vec::new();
        let abandoned = &mut self.abandoned;
        self.calls.retain(|serial, (due, awaited)| {
            let live = *due > now;
            if !live {
                overdue.push(awaited.unanswered().ok_or(awaited.context()));
                if matches!(awaited, Awaited::Create(_)) {
                    if abandoned.len() == MAX_ABANDONED {
                        abandoned.pop_front();
                    }
                    abandoned.push_back(*serial);
                }
            }
            live
        });
        for overdue in overdue {
            match overdue {
                Ok(heard) => self.inbox.post(heard),
                Err(context) => {
                    if self.contexts.contains_key(&context) {
                        tracing::debug!("the input method did not make an input context in time");
                        self.lost(context);
                    }
                }
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
                if let Some(heard) = Heard::unanswered(context, &request) {
                    self.inbox.post(heard);
                }
            }
        }
        self.inbox.post(Heard::Lost(context));
    }

    /// The bus says who owns a name: when the daemon's name loses the owner
    /// that made the contexts, every context is lost with it.
    fn owner_changed(&mut self, message: &Message) {
        let body = message.body();
        let Ok((name, old, _new)) = body.deserialize::<(&str, &str, &str)>() else {
            return;
        };
        if name != self.link.names.destination || old.is_empty() {
            return;
        }
        tracing::debug!("the input method left the bus: its contexts are lost");
        let contexts: Vec<ContextId> = self.contexts.keys().copied().collect();
        for context in contexts {
            self.lost(context);
        }
    }

    /// The path of the context an answer to `CreateInputContext` made, if it
    /// made one.
    fn made_path(&self, message: &Message) -> Option<OwnedObjectPath> {
        if message.message_type() == MessageType::Error {
            return None;
        }
        let body = message.body();
        match self.link.names.daemon {
            Daemon::IBus => body.deserialize::<OwnedObjectPath>().ok(),
            Daemon::Fcitx5 => body
                .deserialize::<(OwnedObjectPath, Vec<u8>)>()
                .ok()
                .map(|(path, _)| path),
        }
    }

    /// A message from the bus: an answer to a call, or a signal of a context.
    async fn heard(&mut self, message: &Message) {
        let header = message.header();
        match message.message_type() {
            MessageType::MethodReturn | MessageType::Error => {
                let Some(serial) = header.reply_serial() else {
                    return;
                };
                let Some((_, awaited)) = self.calls.remove(&serial) else {
                    // A context made after it was given up on is destroyed.
                    if let Some(index) = self.abandoned.iter().position(|late| *late == serial) {
                        self.abandoned.remove(index);
                        if let Some(path) = self.made_path(message) {
                            let call = destroy(self.link.names, &path);
                            self.send(call, None).await;
                        }
                    }
                    return;
                };
                let failed = message.message_type() == MessageType::Error;
                match awaited {
                    Awaited::Key(context, ticket) => {
                        let taken =
                            !failed && message.body().deserialize::<bool>().unwrap_or(false);
                        self.inbox.post(Heard::Answer(context, ticket, taken));
                    }
                    // Refused or not, the daemon handled it in its turn: what
                    // it sent before came before this answer.
                    Awaited::Focus(context, fence) => {
                        self.inbox.post(Heard::Fenced(context, fence));
                    }
                    Awaited::Create(context) => {
                        if let Some(path) = self.made_path(message) {
                            self.made(context, path).await;
                        } else {
                            tracing::debug!("the input method refused an input context");
                            self.lost(context);
                        }
                    }
                }
            }
            MessageType::Signal => {
                if header.sender().is_some_and(|sender| sender == BUS_NAME) {
                    if header
                        .member()
                        .is_some_and(|member| member == "NameOwnerChanged")
                    {
                        self.owner_changed(message);
                    }
                    return;
                }
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
            (Daemon::IBus, "RequireSurroundingText") => Some(Heard::StateAsked(context)),
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
async fn refuse(
    commands: &mut mpsc::Receiver<Command>,
    lifecycle: &mut mpsc::UnboundedReceiver<Command>,
    inbox: &Inbox,
) {
    loop {
        let command = tokio::select! {
            biased;
            Some(command) = lifecycle.recv() => Some(command),
            command = commands.recv() => command,
        };
        let Some(command) = command else {
            return;
        };
        match command {
            Command::Open(context) => inbox.post(Heard::Lost(context)),
            Command::Send(context, request) => {
                if let Some(heard) = Heard::unanswered(context, &request) {
                    inbox.post(heard);
                }
            }
            Command::Close(_) => {}
        }
    }
}
