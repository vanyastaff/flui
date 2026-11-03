//! One connection: the hello, then requests run in order on a worker of the
//! connection's own while this thread reads the next ones, writes the
//! answers and streams the events the connection subscribed to.

use std::io::{self, Read as _, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use flui_protocol::framing::{LineReader, MAX_LINE};
use flui_protocol::version::PROTOCOL_VERSION;
use flui_protocol::{
    Capabilities, ErrorCode, ErrorObject, Greeting, Hello, HelloReply, InputPath, MethodInfo,
    Request, RequestId, ServedBy, ServerLine,
};
use interprocess::local_socket::Stream;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use super::dispatch::{self, Context, Subscriptions};
use super::endpoint::{POLL, Shared};

/// What the server writes: a result is the method's output as a JSON value.
type Line = ServerLine<Value>;

/// How long a reply may take to leave before the connection is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// The most one write offers the stream. A non-blocking named pipe takes a
/// write whole or not at all, so a write larger than the pipe's free buffer
/// would never go through; small writes go through as the client reads.
const WRITE_CHUNK: usize = 512;

/// How long a connection being closed may stay quiet before it is closed.
const LINGER_IDLE: Duration = Duration::from_millis(250);

/// One request, queued for the connection's worker.
struct Job {
    id: RequestId,
    method: &'static MethodInfo,
    params: Value,
}

/// How the conversation ended.
enum End {
    /// The driver closed the connection, or broke it.
    Peer,
    /// The agent is stopping.
    Stopped,
}

pub(super) fn serve(stream: Stream, shared: &Arc<Shared>) {
    let mut connection = Connection {
        stream,
        lines: LineReader::new(MAX_LINE),
        progress: Instant::now(),
    };
    if !handshake(&mut connection, shared) {
        return;
    }
    // Under `EXIT_ON_DISCONNECT` the first connection past its hello is the
    // driver; only its going ends the app, not an inspector's beside it.
    let disconnect = shared
        .disconnect
        .as_ref()
        .filter(|_| !shared.driver_claimed.swap(true, Ordering::SeqCst));
    // A conversation that panics (a subscriber at one of its traces) broke the
    // connection as surely as the driver closing it: the driver is gone.
    let end = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        converse(&mut connection, shared)
    }))
    .unwrap_or_else(|payload| {
        flui_foundation::panic::discard_payload(payload);
        End::Peer
    });
    if let (End::Peer, Some(disconnect)) = (end, disconnect) {
        disconnect.fire();
    }
}

/// Read the hello and answer it: `true` once the driver is admitted.
fn handshake(connection: &mut Connection, shared: &Shared) -> bool {
    // A timeout too long to reach (`Duration::MAX`) is no deadline at all.
    let deadline = Instant::now().checked_add(shared.limits.handshake_timeout);
    let hello = loop {
        match connection.next_line(shared) {
            Next::Line(line) => break line,
            Next::Pending if deadline.is_none_or(|deadline| Instant::now() < deadline) => {
                connection.pause();
            }
            Next::Pending | Next::TooLong | Next::Closed => return false,
        }
    };
    let Some(hello) = parse_hello(&hello) else {
        super::trace(|| tracing::debug!("agent connection refused: no hello"));
        return false;
    };
    if !same_token(hello.token.as_bytes(), shared.token.as_bytes()) {
        super::trace(|| tracing::debug!("agent connection refused: wrong token"));
        return false;
    }
    // An app serves every older client its schema still reads (ADR-0238 §2), and answers
    // with its own version: the capabilities say what it answers.
    if !PROTOCOL_VERSION.serves(hello.protocol) {
        let error = ErrorObject::version(PROTOCOL_VERSION, hello.protocol);
        if connection.write_line(&Line::refusal(error), shared).is_ok() {
            connection.linger(shared);
        }
        return false;
    }
    let mut capabilities = Capabilities::default();
    capabilities.paths = vec![InputPath::Semantic];
    capabilities.methods = dispatch::SERVED
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let welcome = HelloReply::new(PROTOCOL_VERSION)
        .with_app(shared.app.clone())
        .with_capabilities(capabilities);
    connection.write_line(&Line::hello(welcome), shared).is_ok()
}

fn parse_hello(line: &[u8]) -> Option<Hello> {
    serde_json::from_slice::<Greeting>(line)
        .ok()
        .map(|greeting| greeting.hello)
}

/// Compares two tokens in time that depends on their lengths only.
fn same_token(presented: &[u8], expected: &[u8]) -> bool {
    let mut difference = presented.len() ^ expected.len();
    for index in 0..presented.len().max(expected.len()) {
        let a = presented.get(index).copied().unwrap_or(0);
        let b = expected.get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
    }
    std::hint::black_box(difference) == 0
}

/// The requests a connection holds: queued, running, or answered and not
/// yet written. Counted on the connection's thread alone.
struct Queue {
    jobs: mpsc::Sender<Job>,
    held: usize,
    cap: usize,
}

/// Marks the connection ended when dropped, on every exit of the
/// conversation, so its worker stops waiting and takes no further request.
struct Closing(Arc<AtomicBool>);

impl Drop for Closing {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Serve requests until the driver goes or the agent stops.
fn converse(connection: &mut Connection, shared: &Arc<Shared>) -> End {
    let subscriptions = Arc::new(Mutex::new(Subscriptions::default()));
    let (outbox, answers) = mpsc::channel::<Line>();
    let closing = Closing(Arc::new(AtomicBool::new(false)));
    let Some(jobs) = start_worker(shared, &subscriptions, &closing.0, outbox) else {
        return End::Stopped;
    };
    let mut queue = Queue {
        jobs,
        held: 0,
        cap: shared.limits.queue.max(1),
    };
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return End::Stopped;
        }
        let mut busy = false;
        match connection.next_line(shared) {
            Next::Line(line) => {
                busy = true;
                if let Some(refusal) = admit(&line, &mut queue)
                    && connection.write_line(&refusal, shared).is_err()
                {
                    return End::Peer;
                }
            }
            Next::TooLong => {
                let error = ErrorObject::new(
                    ErrorCode::InvalidArgument,
                    format!("a request line is longer than {MAX_LINE} bytes"),
                );
                if connection.write_line(&Line::refusal(error), shared).is_ok() {
                    connection.linger(shared);
                }
                return End::Peer;
            }
            Next::Pending => {}
            Next::Closed => return End::Peer,
        }
        loop {
            match answers.try_recv() {
                Ok(answer) => {
                    busy = true;
                    queue.held = queue.held.saturating_sub(1);
                    if connection.write_line(&answer, shared).is_err() {
                        return End::Peer;
                    }
                }
                Err(TryRecvError::Empty) => break,
                // The worker is gone (it panicked): nothing the connection
                // queued would be answered, so the connection ends and the
                // driver sees it closed rather than waiting.
                Err(TryRecvError::Disconnected) => return End::Peer,
            }
        }
        // Traced only once the subscriptions are let go: a subscriber is user code.
        let drained = subscriptions.lock().drain(&shared.registry);
        for (window, code) in drained.refused {
            super::trace(|| {
                tracing::warn!(window = %window, code = %code, "a subscription cannot reach a window");
            });
        }
        let events = drained.events;
        for event in events {
            busy = true;
            // An event that cannot be written is skipped, not the connection.
            let Some(bytes) = encode(&Line::Event(event)) else {
                continue;
            };
            if connection.write_bytes(&bytes, shared).is_err() {
                return End::Peer;
            }
        }
        if !busy {
            connection.pause();
        }
    }
}

/// The connection's worker: runs each queued request in order and posts
/// its answer to `outbox`. It ends when the connection drops its queue or
/// has ended.
fn start_worker(
    shared: &Arc<Shared>,
    subscriptions: &Arc<Mutex<Subscriptions>>,
    closed: &Arc<AtomicBool>,
    outbox: mpsc::Sender<Line>,
) -> Option<mpsc::Sender<Job>> {
    let (jobs, queue) = mpsc::channel::<Job>();
    let shared = Arc::clone(shared);
    let subscriptions = Arc::clone(subscriptions);
    let closed = Arc::clone(closed);
    std::thread::Builder::new()
        .name("flui-agent-worker".into())
        .spawn(move || work(&queue, &shared, &subscriptions, &closed, &outbox))
        .ok()?;
    Some(jobs)
}

fn work(
    queue: &Receiver<Job>,
    shared: &Shared,
    subscriptions: &Mutex<Subscriptions>,
    closed: &AtomicBool,
    outbox: &mpsc::Sender<Line>,
) {
    let cx = Context {
        shared,
        subscriptions,
        closed,
    };
    while let Ok(Job { id, method, params }) = queue.recv() {
        // Nobody reads an answer once the connection ended.
        if closed.load(Ordering::SeqCst) {
            return;
        }
        let started = Instant::now();
        let outcome = dispatch::run(method, params, &cx);
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let failed = outcome.as_ref().err().map(|error| error.code);
        let answer = Line::answer(id, outcome);
        // The answer is posted before it is traced.
        let posted = outbox.send(answer).is_ok();
        super::trace(|| match failed {
            None => tracing::debug!(method = method.name, elapsed_ms, "agent request served"),
            Some(code) => tracing::debug!(
                method = method.name,
                code = code.name(),
                elapsed_ms,
                "agent request failed"
            ),
        });
        if !posted {
            return;
        }
    }
}

/// Queue the request on `line`, or the line to refuse it with at once.
fn admit(line: &[u8], queue: &mut Queue) -> Option<Line> {
    let Ok(request) = serde_json::from_slice::<Request<Value>>(line) else {
        return Some(malformed(line));
    };
    let Request {
        id,
        method: name,
        params,
        ..
    } = request;
    let refuse = |error: ErrorObject| Some(Line::answer(id.clone(), Err(error)));
    let Some(method) = flui_protocol::method::find(&name) else {
        // Only a known method's name is echoed or traced: an unknown one is
        // the client's text, which may carry anything.
        return refuse(ErrorObject::new(
            ErrorCode::InvalidArgument,
            "no such method",
        ));
    };
    if method.served_by != ServedBy::Agent || !dispatch::SERVED.contains(&method.name) {
        return refuse(dispatch::not_supported(method));
    }
    if queue.held >= queue.cap {
        return refuse(
            ErrorObject::new(
                ErrorCode::Busy,
                "the connection holds as many requests as it takes",
            )
            .with_retry(flui_protocol::Retry::Soon),
        );
    }
    let params = if params.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        params
    };
    let job = Job {
        id: id.clone(),
        method,
        params,
    };
    if queue.jobs.send(job).is_err() {
        return refuse(ErrorObject::new(
            ErrorCode::ShuttingDown,
            "the connection's worker has stopped",
        ));
    }
    queue.held += 1;
    None
}

/// The refusal of a line that is not a request: it answers the line's id when the line
/// carries one, so a client that sent a broken request is not left waiting for it.
fn malformed(line: &[u8]) -> Line {
    use serde::Deserialize as _;

    let invalid = |message| ErrorObject::new(ErrorCode::InvalidArgument, message);
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Line::refusal(invalid("the request is not JSON"));
    };
    match value.get("id").map(RequestId::deserialize) {
        Some(Ok(id)) => Line::answer(id, Err(invalid("a request names its method"))),
        _ => Line::refusal(invalid("a request carries an id: a number or a string")),
    }
}

enum Next {
    Line(Vec<u8>),
    /// No whole line yet.
    Pending,
    TooLong,
    Closed,
}

struct Connection {
    stream: Stream,
    lines: LineReader,
    /// When bytes last moved, for [`Self::pause`].
    progress: Instant,
}

impl Connection {
    /// The next whole line, without its newline, reading what the stream
    /// has now and never waiting for more.
    fn next_line(&mut self, shared: &Shared) -> Next {
        let mut chunk = [0_u8; 8192];
        loop {
            match self.lines.next_line() {
                Ok(Some(line)) => return Next::Line(line),
                Ok(None) => {}
                Err(_) => return Next::TooLong,
            }
            if shared.stop.load(Ordering::SeqCst) {
                return Next::Pending;
            }
            match read_some(&self.stream, &mut chunk) {
                Ok(0) => return Next::Closed,
                Ok(read) => {
                    self.lines.push(&chunk[..read]);
                    self.progress = Instant::now();
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Next::Pending,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Next::Closed,
            }
        }
    }

    /// Before closing on a client still writing: discard what it sends until
    /// it stops (or a bound passes), so the reply already written is not lost
    /// with the unread input when the connection closes.
    fn linger(&mut self, shared: &Shared) {
        let deadline = Instant::now() + WRITE_TIMEOUT;
        let mut chunk = [0_u8; 8192];
        self.progress = Instant::now();
        while Instant::now() < deadline
            && self.progress.elapsed() < LINGER_IDLE
            && !shared.stop.load(Ordering::SeqCst)
        {
            match read_some(&self.stream, &mut chunk) {
                Ok(0) => return,
                Ok(_) => self.progress = Instant::now(),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => self.pause(),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }

    /// Write `line` as one line.
    fn write_line(&mut self, line: &impl Serialize, shared: &Shared) -> io::Result<()> {
        let bytes = encode(line).ok_or_else(|| io::Error::other("a line did not serialize"))?;
        self.write_bytes(&bytes, shared)
    }

    /// Write `bytes`, a whole line with its newline. Gives up when the agent
    /// stops, so a client that stops reading never holds up a detach, and
    /// after [`WRITE_TIMEOUT`].
    fn write_bytes(&mut self, bytes: &[u8], shared: &Shared) -> io::Result<()> {
        let deadline = Instant::now() + WRITE_TIMEOUT;
        let mut written = 0;
        while written < bytes.len() {
            let end = bytes.len().min(written + WRITE_CHUNK);
            match (&self.stream).write(&bytes[written..end]) {
                Ok(0) => {}
                Ok(count) => {
                    written += count;
                    self.progress = Instant::now();
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if shared.stop.load(Ordering::SeqCst) {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.pause();
        }
        // No flush: on Windows it waits until the client has read everything,
        // which a client that stops reading never lets happen. What is written
        // is delivered before the connection closes.
        Ok(())
    }

    /// Waits before the next attempt at the non-blocking stream: briefly
    /// while data is flowing (a named pipe's buffer is 512 bytes, so a long
    /// line moves in many small steps), [`POLL`] once the connection has been
    /// idle a while.
    fn pause(&self) {
        if self.progress.elapsed() < Duration::from_millis(50) {
            std::thread::sleep(Duration::from_micros(200));
        } else {
            std::thread::sleep(POLL);
        }
    }
}

/// A non-blocking read: `Ok(0)` only once the peer has closed, `WouldBlock`
/// while no data has come.
///
/// A non-blocking named pipe answers an empty read the way it answers a
/// closed one (`ERROR_NO_DATA`, which reads as end of file), so on Windows an
/// empty read is told apart by an empty write, which fails only on a closed
/// pipe.
fn read_some(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
    match (&*stream).read(buffer) {
        Ok(0) if cfg!(windows) => match (&*stream).write(&[]) {
            Ok(_) => Err(io::ErrorKind::WouldBlock.into()),
            Err(_) => Ok(0),
        },
        other => other,
    }
}

/// `line` as the bytes of one line, its newline included; `None` when it does not serialize.
fn encode(line: &impl Serialize) -> Option<Vec<u8>> {
    let mut bytes = serde_json::to_vec(line).ok()?;
    bytes.push(b'\n');
    Some(bytes)
}
