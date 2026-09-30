//! One connection: the hello, then newline-delimited requests, each
//! answered in order.

use std::io::{self, Read as _, Write as _};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use flui_protocol::{
    ActionRequest, ElementId, ErrorCode, PROTOCOL_VERSION, ReadQuery, Retry, WindowId,
};
use flui_sdk::view::dev_agent::{AgentAnswer, AgentFault, HandleKind};
use interprocess::local_socket::Stream;
use serde_json::{Value, json};

use super::MAX_LINE;
use super::endpoint::{POLL, Shared};
use super::registry::Lookup;

/// How long a reply may take to leave before the connection is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// The most one write offers the stream. A non-blocking named pipe takes a
/// write whole or not at all, so a write larger than the pipe's free buffer
/// would never go through; small writes go through as the client reads.
const WRITE_CHUNK: usize = 512;

/// How long a connection being closed may stay quiet before it is closed.
const LINGER_IDLE: Duration = Duration::from_millis(250);

/// The longest single wait on an answer before the stop flag is looked at.
const WAIT_SLICE: Duration = Duration::from_millis(50);

pub(super) fn serve(stream: Stream, shared: &Shared) {
    let mut connection = Connection {
        stream,
        buffer: Vec::new(),
    };
    let deadline = Instant::now() + shared.handshake_timeout;
    let Next::Line(hello) = connection.next_line(Some(deadline), shared) else {
        return;
    };
    if !is_hello(&hello, &shared.token) {
        tracing::debug!("agent connection refused: no valid hello");
        return;
    }
    let welcome = json!({ "hello": { "protocol": PROTOCOL_VERSION.to_string() } });
    if connection.write_line(&welcome).is_err() {
        return;
    }
    loop {
        match connection.next_line(None, shared) {
            Next::Line(line) => {
                let reply = handle(&line, shared);
                if connection.write_line(&reply).is_err() {
                    return;
                }
            }
            Next::TooLong => {
                let fault = AgentFault::new(
                    ErrorCode::InvalidArgument,
                    Retry::Never,
                    format!("a request line is longer than {MAX_LINE} bytes"),
                );
                if connection
                    .write_line(&error_reply(Value::Null, &fault, None))
                    .is_ok()
                {
                    connection.linger(shared);
                }
                return;
            }
            Next::Closed | Next::Stopped | Next::TimedOut => return,
        }
    }
}

/// Whether `line` is `{"hello":{"token":…}}` with the right token.
fn is_hello(line: &[u8], token: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    value
        .get("hello")
        .and_then(|hello| hello.get("token"))
        .and_then(Value::as_str)
        .is_some_and(|presented| same_token(presented.as_bytes(), token.as_bytes()))
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

enum Next {
    Line(Vec<u8>),
    TooLong,
    Closed,
    Stopped,
    TimedOut,
}

struct Connection {
    stream: Stream,
    buffer: Vec<u8>,
}

impl Connection {
    /// The next line, without its newline. The stream is non-blocking, so
    /// an idle connection looks at the stop flag and the deadline every
    /// [`POLL`].
    fn next_line(&mut self, deadline: Option<Instant>, shared: &Shared) -> Next {
        let mut chunk = [0_u8; 8192];
        let mut progress = Instant::now();
        loop {
            if let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                if end > MAX_LINE {
                    return Next::TooLong;
                }
                let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Next::Line(line);
            }
            if self.buffer.len() > MAX_LINE {
                return Next::TooLong;
            }
            if shared.stop.load(Ordering::SeqCst) {
                return Next::Stopped;
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Next::TimedOut;
            }
            match read_some(&self.stream, &mut chunk) {
                Ok(0) => return Next::Closed,
                Ok(read) => {
                    self.buffer.extend_from_slice(&chunk[..read]);
                    progress = Instant::now();
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => pause(progress),
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
        let mut progress = Instant::now();
        while Instant::now() < deadline
            && progress.elapsed() < LINGER_IDLE
            && !shared.stop.load(Ordering::SeqCst)
        {
            match read_some(&self.stream, &mut chunk) {
                Ok(0) => return,
                Ok(_) => progress = Instant::now(),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => pause(progress),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    }

    fn write_line(&mut self, value: &Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        bytes.push(b'\n');
        let deadline = Instant::now() + WRITE_TIMEOUT;
        let mut written = 0;
        let mut progress = Instant::now();
        while written < bytes.len() {
            let end = bytes.len().min(written + WRITE_CHUNK);
            match (&self.stream).write(&bytes[written..end]) {
                Ok(0) => {}
                Ok(count) => {
                    written += count;
                    progress = Instant::now();
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            pause(progress);
        }
        // No flush: on Windows it waits until the client has read everything,
        // which a client that stops reading never lets happen. What is written
        // is delivered before the connection closes.
        Ok(())
    }
}

/// Waits before the next attempt at a non-blocking stream: briefly while
/// data is flowing (a named pipe's buffer is 512 bytes, so a long line moves
/// in many small steps), [`POLL`] once the connection has been idle a while.
fn pause(progress: Instant) {
    if progress.elapsed() < Duration::from_millis(50) {
        std::thread::sleep(Duration::from_micros(200));
    } else {
        std::thread::sleep(POLL);
    }
}

/// A non-blocking read: `Ok(0)` only once the peer has closed, `WouldBlock`
/// while no data has come.
///
/// A non-blocking named pipe answers an empty read the way it answers a
/// closed one (`ERROR_NO_DATA`, which reads as end of file), so on Windows an
/// empty read is told apart by an empty write, which fails only on a closed
/// pipe.
pub(super) fn read_some(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
    match (&*stream).read(buffer) {
        Ok(0) if cfg!(windows) => match (&*stream).write(&[]) {
            Ok(_) => Err(io::ErrorKind::WouldBlock.into()),
            Err(_) => Ok(0),
        },
        other => other,
    }
}

/// Answers one request line.
fn handle(line: &[u8], shared: &Shared) -> Value {
    let Ok(request) = serde_json::from_slice::<Value>(line) else {
        let fault = invalid("the request is not a JSON object");
        return error_reply(Value::Null, &fault, None);
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let started = Instant::now();
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    let outcome = match op {
        "windows" => Ok(json!({ "windows": shared.registry.list() })),
        "read" => read(&request, shared),
        "act" => act(&request, shared),
        _ => Err((invalid("unknown op"), None)),
    };
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match outcome {
        Ok(result) => {
            tracing::debug!(op, elapsed_ms, "agent request served");
            json!({ "id": id, "result": result })
        }
        Err((fault, handle)) => {
            tracing::debug!(
                op,
                code = fault.code().name(),
                handle = handle.as_deref(),
                elapsed_ms,
                "agent request failed"
            );
            error_reply(id, &fault, handle)
        }
    }
}

/// A fault, and the handle it names.
type Failure = (AgentFault, Option<String>);

fn read(request: &Value, shared: &Shared) -> Result<Value, Failure> {
    let window = window(request, shared)?;
    let query: ReadQuery = match request.get("query") {
        None | Some(Value::Null) => ReadQuery::new(),
        Some(query) => serde_json::from_value(query.clone())
            .map_err(|_| (invalid("the query is malformed"), None))?,
    };
    let root = query.root;
    let fail = |fault: AgentFault| name_handle(fault, window.id(), root);
    let answer = window.read(query).map_err(fail)?;
    let tree = match wait(answer, shared) {
        Some(Ok(tree)) => tree,
        Some(Err(fault)) => return Err(fail(fault)),
        None => {
            return Err(fail(AgentFault::new(
                ErrorCode::Timeout,
                Retry::Soon,
                "the window did not answer in time",
            )));
        }
    };
    serde_json::to_value(tree).map_err(|_| {
        (
            AgentFault::new(
                ErrorCode::Platform,
                Retry::Never,
                "the tree did not serialize",
            ),
            None,
        )
    })
}

fn act(request: &Value, shared: &Shared) -> Result<Value, Failure> {
    let window = window(request, shared)?;
    let action: ActionRequest = request
        .get("request")
        .cloned()
        .ok_or_else(|| (invalid("an act names its request"), None))
        .and_then(|action| {
            serde_json::from_value(action).map_err(|_| (invalid("the request is malformed"), None))
        })?;
    let element = action.element;
    let fail = |fault: AgentFault| name_handle(fault, window.id(), Some(element));
    let answer = window.act(action).map_err(fail)?;
    match wait(answer, shared) {
        Some(Ok(())) => Ok(json!({})),
        Some(Err(fault)) => Err(fail(fault)),
        None => Err(fail(
            AgentFault::new(
                ErrorCode::Timeout,
                Retry::Never,
                "the window did not answer in time; the action is still queued",
            )
            .with_may_have_run(true),
        )),
    }
}

/// The open window a request names.
fn window(
    request: &Value,
    shared: &Shared,
) -> Result<flui_sdk::view::dev_agent::AgentWindow, Failure> {
    let id: WindowId = request
        .get("window")
        .and_then(Value::as_str)
        .ok_or_else(|| (invalid("the request names no window"), None))?
        .parse()
        .map_err(|_| (invalid("the window handle is malformed"), None))?;
    match shared.registry.lookup(id) {
        Lookup::Open(window) => Ok(window),
        Lookup::Closed => Err((AgentFault::window_gone(), Some(id.to_string()))),
        Lookup::Unknown => Err((
            AgentFault::new(
                ErrorCode::UnknownHandle,
                Retry::Never,
                format!("window {id} was never issued"),
            )
            .with_kind(HandleKind::Window),
            Some(id.to_string()),
        )),
    }
}

/// Waits for the owner's answer, up to the reply timeout, looking at the
/// stop flag between slices. `None` on a timeout or a stop.
fn wait<T>(mut answer: AgentAnswer<T>, shared: &Shared) -> Option<Result<T, AgentFault>> {
    let deadline = Instant::now() + shared.reply_timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if let Some(result) = answer.recv_timeout(left.min(WAIT_SLICE)) {
            return Some(result);
        }
        if left <= WAIT_SLICE || shared.stop.load(Ordering::SeqCst) {
            return None;
        }
    }
}

/// Pairs `fault` with the handle its kind names.
fn name_handle(fault: AgentFault, window: WindowId, element: Option<ElementId>) -> Failure {
    let handle = match fault.kind() {
        Some(HandleKind::Window) => Some(window.to_string()),
        Some(HandleKind::Element) => element.map(|element| element.to_string()),
        _ => None,
    };
    (fault, handle)
}

fn invalid(message: &str) -> AgentFault {
    AgentFault::new(ErrorCode::InvalidArgument, Retry::Never, message)
}

/// ADR-0080's error object.
fn error_reply(id: Value, fault: &AgentFault, handle: Option<String>) -> Value {
    let mut error = json!({
        "code": fault.code().name(),
        "message": fault.message(),
        "retry": fault.retry().to_string(),
    });
    if let Some(kind) = fault.kind() {
        error["kind"] = json!(kind.name());
        if let Some(handle) = handle {
            error["handle"] = json!(handle);
        }
    }
    if fault.may_have_run() {
        error["effect"] = json!({
            "kind": "may_have_run",
            "detail": "the action may have reached the application",
        });
    }
    json!({ "id": id, "error": error })
}
