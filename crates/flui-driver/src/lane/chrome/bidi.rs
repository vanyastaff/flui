//! One WebDriver BiDi session: the WebSocket a ChromeDriver on the loopback serves, owned by a
//! thread of its own, which the lane and the page's connection both ask through a [`Session`].
//!
//! - **One owner.** The thread is the socket's only owner: it writes each command with a fresh
//!   id, never reused, and hands each answer to the caller whose id it carries, so a late answer
//!   to a command whose caller gave up is dropped, never taken for a newer one. An error that
//!   carries no id answers nobody.
//! - **Bounded.** A message is at most [`MAX_MESSAGE`] bytes, refused from its frame header before
//!   its payload is read; at most [`MAX_PENDING`] commands wait for answers; the page's lines wait
//!   for its connection at most [`PAGE_ROOM`] at a time and [`PAGE_BYTES`] in all. Past that the
//!   page's connection is broken with why, rather than its lines dropped unseen or the socket left
//!   unread, which would starve the lane's answers. Every command has a deadline.
//! - **One record of the break.** Why the socket broke is recorded once, the first time, and every
//!   later command, and the page's connection, are answered with it.
//! - The page reports its lines through a BiDi channel (`script.message`) whose name is fresh for
//!   each session, so no other script's messages reach the connection.

use std::any::Any;
use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{
    self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flui_foundation::panic::{FirstPanic, discard_payload};
use serde_json::{Map, Value, json};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::error::{Error, Result};

/// How long a command may wait for its answer unless its caller names another bound.
pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest message read from the browser: a screenshot's base64 and its envelope.
pub const MAX_MESSAGE: usize = 64 << 20;

/// How many commands may wait for their answers at once.
pub const MAX_PENDING: usize = 64;

/// How many of the page's lines wait for its connection before the connection is broken.
pub const PAGE_ROOM: usize = 1024;

/// How many bytes of the page's lines wait for its connection before the connection is broken.
pub const PAGE_BYTES: usize = 64 << 20;

/// How long the thread waits for a command while the browser sends nothing.
const IDLE: Duration = Duration::from_millis(2);

/// How long a write may make no progress before the socket is called broken.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the WebSocket's handshake may take.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a command got no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The browser answered with an error.
    Refused {
        /// BiDi's error code (`no such frame`, `invalid argument`).
        error: String,
        /// The browser's message.
        message: String,
    },
    /// No answer came by the deadline: the command may have run.
    Timeout,
    /// [`MAX_PENDING`] commands already wait.
    Busy,
    /// The socket broke, or the session ended: why.
    Gone(String),
}

impl Failure {
    /// The failure as the driver's error, for the command `method`.
    pub(crate) fn into_error(self, method: &str) -> Error {
        match self {
            Self::Refused { error, message } => Error::Lane {
                lane: "chrome".to_owned(),
                unavailable: false,
                message: format!("the browser refused `{method}`: {error}: {message}"),
            },
            Self::Timeout => Error::Timeout {
                effect: None,
                message: format!("the browser did not answer `{method}` in time"),
                retry: flui_protocol::error::Retry::Soon,
            },
            Self::Busy => Error::busy(format!(
                "{MAX_PENDING} commands already wait for the browser's answers"
            )),
            Self::Gone(why) => Error::Lane {
                lane: "chrome".to_owned(),
                unavailable: false,
                message: format!("the browser session ended: {why}"),
            },
        }
    }
}

/// What the page's connection hears, in the order the browser sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PageEvent {
    /// A line the page reported on the session's channel.
    Line(String),
    /// The page reported that its end of the connection closed (`onLine(null)`).
    Ended,
    /// A browsing context closed: the page's, when it names the session's context.
    ContextGone(String),
    /// The session or the page's connection broke: why.
    Broken(String),
}

/// The page's lines, as the session's thread hands them over: taken once, by the page's
/// connection.
pub(crate) struct PageLines {
    pub(crate) receiver: Receiver<PageEvent>,
    /// The bytes of the lines handed over and not yet taken, which the receiver gives back.
    pub(crate) bytes: Arc<AtomicUsize>,
}

impl PageLines {
    /// The next event, waiting until `deadline`; `Ok(None)` when none came by then.
    pub(crate) fn next(&self, deadline: Instant) -> std::result::Result<Option<PageEvent>, ()> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.receiver.recv_timeout(left) {
            Ok(event) => {
                if let PageEvent::Line(line) = &event {
                    self.bytes.fetch_sub(line.len(), Ordering::AcqRel);
                }
                Ok(Some(event))
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        }
    }
}

/// One command for the session's thread.
struct Command {
    method: String,
    params: Value,
    deadline: Instant,
    reply: SyncSender<std::result::Result<Value, Failure>>,
}

/// One BiDi session, shared by the lane and the page's connection: see the [module docs](self).
/// The session ends when its last handle goes: the socket closes, and then what it was given to
/// own (the ChromeDriver and the page's server) ends.
#[derive(Clone)]
pub(crate) struct Session {
    inner: Arc<Inner>,
}

struct Inner {
    commands: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
    /// The tab the session drives.
    context: String,
    /// The channel the page reports on.
    channel: String,
    /// What ends after the socket closed: the browser the session runs in.
    owned: Option<Box<dyn Any + Send + Sync>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        drop(self.commands.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        drop(self.owned.take());
    }
}

impl Session {
    /// Connect to the BiDi WebSocket at `url`, which must be on the loopback, take the first
    /// top-level browsing context as the session's tab, and subscribe to what the page's
    /// connection hears. `owned` ends after the socket closed.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a URL off the loopback; the connection's or the browser's
    /// failure.
    pub(crate) fn connect(
        url: &str,
        owned: Option<Box<dyn Any + Send + Sync>>,
    ) -> Result<(Self, PageLines)> {
        let address = loopback_address(url)?;
        let broken = |error: &dyn std::fmt::Display| Error::Lane {
            lane: "chrome".to_owned(),
            unavailable: false,
            message: format!("could not open the browser's WebSocket at {url}: {error}"),
        };
        let stream =
            TcpStream::connect_timeout(&address, HANDSHAKE_TIMEOUT).map_err(|e| broken(&e))?;
        stream.set_nodelay(true).map_err(|e| broken(&e))?;
        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| broken(&e))?;
        stream
            .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| broken(&e))?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (ws, _) = tungstenite::client::client_with_config(url, stream, Some(config))
            .map_err(|e| broken(&e))?;
        ws.get_ref().set_nonblocking(true).map_err(|e| broken(&e))?;
        let channel = format!("flui-driver-{}", crate::launch::random_hex(8)?);
        let (commands, inbox) = mpsc::channel::<Command>();
        let (page, receiver) = mpsc::sync_channel::<PageEvent>(PAGE_ROOM);
        let bytes = Arc::new(AtomicUsize::new(0));
        let thread = {
            let channel = channel.clone();
            let bytes = Arc::clone(&bytes);
            thread::Builder::new()
                .name("flui-driver-bidi".into())
                .spawn(move || {
                    // A panic here (a log subscriber's, say) ends the session: the callers waiting
                    // hear it gone when their answer channels close.
                    let mut first = FirstPanic::new();
                    first.contain(|| converse(ws, &inbox, &page, &bytes, &channel));
                    if let Some(payload) = first.into_payload() {
                        tracing::error!("the browser session's thread panicked; the session ended");
                        discard_payload(payload);
                    }
                })?
        };
        let mut inner = Inner {
            commands: Some(commands),
            thread: Some(thread),
            context: String::new(),
            channel,
            owned,
        };
        let tree = call(
            &inner,
            "browsingContext.getTree",
            json!({ "maxDepth": 0 }),
            COMMAND_TIMEOUT,
        )
        .map_err(|failure| failure.into_error("browsingContext.getTree"))?;
        inner.context = tree
            .pointer("/contexts/0/context")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Error::Lane {
                lane: "chrome".to_owned(),
                unavailable: false,
                message: "the browser has no tab to drive".to_owned(),
            })?;
        call(
            &inner,
            "session.subscribe",
            json!({ "events": ["script.message", "browsingContext.contextDestroyed"] }),
            COMMAND_TIMEOUT,
        )
        .map_err(|failure| failure.into_error("session.subscribe"))?;
        Ok((
            Self {
                inner: Arc::new(inner),
            },
            PageLines { receiver, bytes },
        ))
    }

    /// The tab the session drives.
    pub(crate) fn context(&self) -> &str {
        &self.inner.context
    }

    /// The channel the page reports on.
    pub(crate) fn channel(&self) -> &str {
        &self.inner.channel
    }

    /// Send `method` with `params` and wait at most `timeout` for its result.
    pub(crate) fn command(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> std::result::Result<Value, Failure> {
        call(&self.inner, method, params, timeout)
    }

    /// Call `function` in the session's tab with `arguments` (BiDi's local values), on the remote
    /// object `this` when given, awaiting the promise it returns, and answer its result as BiDi
    /// serializes it; `ownership` is BiDi's `resultOwnership` (`root` keeps a handle to it).
    ///
    /// # Errors
    ///
    /// [`Error::Lane`] when the function throws or the browser refuses the call, as
    /// [`Failure::into_error`] otherwise.
    pub(crate) fn call_function(
        &self,
        function: &str,
        this: Option<&str>,
        arguments: Vec<Value>,
        ownership: &str,
    ) -> Result<Value> {
        self.call_function_within(function, this, arguments, ownership, COMMAND_TIMEOUT)
    }

    /// [`Self::call_function`], its answer waited for at most `timeout`.
    pub(crate) fn call_function_within(
        &self,
        function: &str,
        this: Option<&str>,
        arguments: Vec<Value>,
        ownership: &str,
        timeout: Duration,
    ) -> Result<Value> {
        let mut params = json!({
            "functionDeclaration": function,
            "awaitPromise": true,
            "target": { "context": self.context() },
            "arguments": arguments,
            "resultOwnership": ownership,
        });
        if let Some(handle) = this {
            params["this"] = json!({ "handle": handle });
        }
        let mut evaluated = self
            .command("script.callFunction", params, timeout)
            .map_err(|failure| failure.into_error("script.callFunction"))?;
        if evaluated.get("type").and_then(Value::as_str) == Some("success") {
            return Ok(evaluated.get_mut("result").map_or(Value::Null, Value::take));
        }
        let text = evaluated
            .pointer("/exceptionDetails/text")
            .and_then(Value::as_str)
            .unwrap_or("an exception");
        Err(Error::Lane {
            lane: "chrome".to_owned(),
            unavailable: false,
            message: format!("the page threw: {text}"),
        })
    }
}

fn call(
    inner: &Inner,
    method: &str,
    params: Value,
    timeout: Duration,
) -> std::result::Result<Value, Failure> {
    let (reply, answer) = mpsc::sync_channel(1);
    let now = Instant::now();
    let deadline = now.checked_add(timeout).unwrap_or(now + COMMAND_TIMEOUT);
    let sent = inner.commands.as_ref().is_some_and(|commands| {
        commands
            .send(Command {
                method: method.to_owned(),
                params,
                deadline,
                reply,
            })
            .is_ok()
    });
    if !sent {
        return Err(Failure::Gone("the session's thread has ended".to_owned()));
    }
    match answer.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => Err(Failure::Timeout),
        Err(RecvTimeoutError::Disconnected) => {
            Err(Failure::Gone("the session's thread has ended".to_owned()))
        }
    }
}

/// A command waiting for its answer.
struct Pending {
    deadline: Instant,
    reply: SyncSender<std::result::Result<Value, Failure>>,
}

/// The socket and what its thread keeps: see the [module docs](self).
struct Conversation<'a> {
    ws: WebSocket<TcpStream>,
    pending: BTreeMap<u64, Pending>,
    next_id: u64,
    /// Since when written data waits for the socket to take it.
    unflushed: Option<Instant>,
    page: Forward<'a>,
    channel: &'a str,
}

/// What a turn of the thread came to.
enum Turn {
    /// It did something: turn again at once.
    Worked,
    /// Nothing came: wait a little for a command.
    Idle,
    /// The last handle went: the session ends.
    Ended,
}

/// The session's thread: see the [module docs](self).
fn converse(
    ws: WebSocket<TcpStream>,
    inbox: &Receiver<Command>,
    page: &SyncSender<PageEvent>,
    bytes: &AtomicUsize,
    channel: &str,
) {
    let mut conversation = Conversation {
        ws,
        pending: BTreeMap::new(),
        next_id: 1,
        unflushed: None,
        page: Forward {
            sender: page,
            bytes,
            failure: None,
            told: false,
        },
        channel,
    };
    let why = loop {
        match conversation.turn(inbox) {
            Ok(Turn::Worked) => {}
            Ok(Turn::Idle) => match inbox.recv_timeout(IDLE) {
                Ok(command) => {
                    if let Err(why) = conversation.write(command) {
                        break why;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return conversation.close(),
            },
            Ok(Turn::Ended) => return conversation.close(),
            Err(why) => break why,
        }
    };
    // Broken: every caller waiting, every later one and the page's connection hear why.
    for (_, waiting) in std::mem::take(&mut conversation.pending) {
        let _ = waiting.reply.try_send(Err(Failure::Gone(why.clone())));
    }
    conversation.page.fail(why.clone());
    loop {
        conversation.page.retry();
        match inbox.recv_timeout(Duration::from_millis(20)) {
            Ok(command) => {
                let _ = command.reply.try_send(Err(Failure::Gone(why.clone())));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

impl Conversation<'_> {
    /// One turn: write what the callers handed over, flush, read one message, and let go of the
    /// commands whose callers gave up.
    fn turn(&mut self, inbox: &Receiver<Command>) -> std::result::Result<Turn, String> {
        let mut worked = false;
        loop {
            match inbox.try_recv() {
                Ok(command) => {
                    worked = true;
                    self.write(command)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(Turn::Ended),
            }
        }
        self.flush()?;
        match self.ws.read() {
            Ok(Message::Text(text)) => {
                worked = true;
                self.heard(text.as_str())?;
            }
            Ok(Message::Close(_)) => return Err("the browser closed its WebSocket".to_owned()),
            // Pings are answered as the socket reads; nothing else carries a message.
            Ok(_) => worked = true,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(format!("the browser's WebSocket broke: {error}")),
        }
        self.page.retry();
        // A command past its deadline waits no more: its caller is told it timed out (it may be
        // giving up at this instant itself), and its answer, if it comes, is dropped.
        let now = Instant::now();
        self.pending.retain(|_, waiting| {
            let waits = waiting.deadline > now;
            if !waits {
                let _ = waiting.reply.try_send(Err(Failure::Timeout));
            }
            waits
        });
        Ok(if worked { Turn::Worked } else { Turn::Idle })
    }

    /// Write `command` with a fresh id and keep its caller waiting.
    fn write(&mut self, command: Command) -> std::result::Result<(), String> {
        if self.pending.len() >= MAX_PENDING {
            let _ = command.reply.try_send(Err(Failure::Busy));
            return Ok(());
        }
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| "the session ran out of command ids".to_owned())?;
        let mut message = Map::new();
        message.insert("id".into(), Value::from(id));
        message.insert("method".into(), Value::from(command.method));
        message.insert("params".into(), command.params);
        match self
            .ws
            .send(Message::text(Value::Object(message).to_string()))
        {
            Ok(()) => {}
            // Queued in the socket's buffer: the next flush sends it.
            Err(tungstenite::Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                self.unflushed.get_or_insert_with(Instant::now);
            }
            Err(error) => return Err(format!("could not write to the browser: {error}")),
        }
        self.pending.insert(
            id,
            Pending {
                deadline: command.deadline,
                reply: command.reply,
            },
        );
        Ok(())
    }

    /// Send what waits in the socket's buffer.
    fn flush(&mut self) -> std::result::Result<(), String> {
        let Some(since) = self.unflushed else {
            return Ok(());
        };
        match self.ws.flush() {
            Ok(()) => self.unflushed = None,
            Err(tungstenite::Error::Io(error)) if error.kind() == io::ErrorKind::WouldBlock => {
                if since.elapsed() >= WRITE_TIMEOUT {
                    return Err("the browser stopped reading what the driver sends".to_owned());
                }
            }
            Err(error) => return Err(format!("could not write to the browser: {error}")),
        }
        Ok(())
    }

    /// What one message from the browser means: an answer to the command whose id it carries, or
    /// an event.
    fn heard(&mut self, text: &str) -> std::result::Result<(), String> {
        let message: Value = serde_json::from_str(text)
            .map_err(|error| format!("the browser sent a message that is not JSON: {error}"))?;
        if let Some(id) = message.get("id") {
            // An error with no id answers no command it could be matched to.
            if let Some(waiting) = id.as_u64().and_then(|id| self.pending.remove(&id)) {
                let _ = waiting.reply.try_send(outcome(message));
            }
            return Ok(());
        }
        let params = message.get("params").unwrap_or(&Value::Null);
        let event = match message.get("method").and_then(Value::as_str) {
            Some("script.message")
                if params.get("channel").and_then(Value::as_str) == Some(self.channel) =>
            {
                match params.pointer("/data/type").and_then(Value::as_str) {
                    Some("string") => match params.pointer("/data/value").and_then(Value::as_str) {
                        Some(line) => PageEvent::Line(line.to_owned()),
                        None => return Ok(()),
                    },
                    Some("null") => PageEvent::Ended,
                    _ => return Ok(()),
                }
            }
            Some("browsingContext.contextDestroyed") => {
                match params.get("context").and_then(Value::as_str) {
                    Some(context) => PageEvent::ContextGone(context.to_owned()),
                    None => return Ok(()),
                }
            }
            _ => return Ok(()),
        };
        self.page.push(event);
        Ok(())
    }

    /// Close the socket as the session ends.
    fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

/// An answer as its command's outcome.
fn outcome(mut message: Value) -> std::result::Result<Value, Failure> {
    match message.get("type").and_then(Value::as_str) {
        Some("success") => Ok(message.get_mut("result").map_or(Value::Null, Value::take)),
        _ => Err(Failure::Refused {
            error: message
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_owned(),
            message: message
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        }),
    }
}

/// The page's lines on their way to its connection, within [`PAGE_ROOM`] and [`PAGE_BYTES`].
struct Forward<'a> {
    sender: &'a SyncSender<PageEvent>,
    bytes: &'a AtomicUsize,
    /// Why the page's connection broke, once it did: nothing more is handed over.
    failure: Option<String>,
    /// Whether the connection has been told why.
    told: bool,
}

impl Forward<'_> {
    fn push(&mut self, event: PageEvent) {
        if self.failure.is_some() {
            return;
        }
        let size = match &event {
            PageEvent::Line(line) => line.len(),
            _ => 0,
        };
        if self.bytes.load(Ordering::Acquire).saturating_add(size) > PAGE_BYTES {
            return self.fail(format!(
                "the page sent more than {PAGE_BYTES} bytes of lines the driver did not read"
            ));
        }
        self.bytes.fetch_add(size, Ordering::AcqRel);
        match self.sender.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.bytes.fetch_sub(size, Ordering::AcqRel);
                self.fail(format!(
                    "the page sent more than {PAGE_ROOM} lines the driver did not read"
                ));
            }
            // The page's connection went: nobody reads its lines.
            Err(TrySendError::Disconnected(_)) => {
                self.bytes.fetch_sub(size, Ordering::AcqRel);
            }
        }
    }

    /// Break the page's connection with `why`, the first failure kept.
    fn fail(&mut self, why: String) {
        if self.failure.is_none() {
            self.failure = Some(why);
        }
        self.retry();
    }

    /// Tell the connection why it broke, once there is room.
    fn retry(&mut self) {
        if self.told {
            return;
        }
        let Some(why) = &self.failure else {
            return;
        };
        match self.sender.try_send(PageEvent::Broken(why.clone())) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => self.told = true,
            Err(TrySendError::Full(_)) => {}
        }
    }
}

/// The loopback address `url` names.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for any other host, or a scheme other than `ws`.
pub(crate) fn loopback_address(url: &str) -> Result<SocketAddr> {
    let refused = || {
        Error::invalid_argument(format!(
            "`{url}` is not a WebSocket on this host's loopback"
        ))
    };
    let rest = url.strip_prefix("ws://").ok_or_else(refused)?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host != "localhost" && !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return Err(refused());
    }
    authority
        .to_socket_addrs()
        .map_err(|_| refused())?
        .find(|address| address.ip().is_loopback())
        .ok_or_else(refused)
}
