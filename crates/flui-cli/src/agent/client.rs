//! A connection to an app's driver agent (ADR-0239).
//!
//! The conversation is `flui-protocol`'s (`flui_protocol::envelope`): newline-delimited JSON, a
//! [`Greeting`] first, answered by the app's hello (or a `version` refusal when the app does not
//! serve this client's protocol), then [`Request`]s, each answered by a [`ServerLine`] carrying
//! its id. One request is in flight at a time, and this client subscribes to no events.
//!
//! A thread of the connection's own reads every line the app sends and another writes, so a
//! peer that stops answering, or stops reading, costs a bounded wait ([`REPLY_TIMEOUT`])
//! instead of a stuck `flui mcp`. A request takes the first line that answers its id: an
//! event, or a late answer to an earlier request that timed out, is passed over.

use std::io::{Read as _, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use flui_protocol::framing::LineReader;
use flui_protocol::session::Hello;
use flui_protocol::version::PROTOCOL_VERSION;
use flui_protocol::{ErrorCode, Greeting, Request, RequestId, ServerLine};
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{Name, RecvHalf, SendHalf, Stream};
use serde_json::Value;

use super::Credentials;

/// How the CLI names itself in its hello.
const CLIENT: &str = concat!("flui-cli/", env!("CARGO_PKG_VERSION"));

/// How long one request waits for its reply. The app's own reply timeout is
/// 5 s, so this only fires for a peer that is not an agent server.
pub(crate) const REPLY_TIMEOUT: Duration = Duration::from_secs(15);

/// The longest reply line read, in bytes. The server caps requests at 1 MiB
/// and bounds reads with `max_nodes`; a reply past this is not one of its.
const MAX_REPLY: usize = 16 << 20;

/// Why a request produced no result.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The app answered with ADR-0080's error object.
    Wire(Value),
    /// The endpoint could not be reached, or the conversation broke. The
    /// connection is unusable afterwards.
    Transport(String),
    /// The request never reached the app whole: the connection was already
    /// broken when it was written. The connection is unusable afterwards.
    Unsent(String),
}

/// One authenticated connection to an app's endpoint.
pub(crate) struct Connection {
    /// Lines for the writing thread.
    outgoing: mpsc::Sender<String>,
    /// Every line the app sent, and the failure that ended either thread.
    incoming: mpsc::Receiver<Result<String, Failure>>,
    next_id: u64,
}

impl Connection {
    /// Connects and authenticates, retrying until `wait` has passed: the app
    /// may still be building, or restarting after a change.
    pub(crate) fn connect(credentials: &Credentials, wait: Duration) -> Result<Self, String> {
        let deadline = Instant::now() + wait;
        loop {
            match Self::connect_once(credentials) {
                Ok(connection) => return Ok(connection),
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => thread::sleep(Duration::from_millis(200)),
            }
        }
    }

    fn connect_once(credentials: &Credentials) -> Result<Self, String> {
        let name = name(&credentials.endpoint)
            .map_err(|e| format!("bad endpoint address {}: {e}", credentials.endpoint))?;
        let stream = Stream::connect(name)
            .map_err(|e| format!("could not connect to {}: {e}", credentials.endpoint))?;
        let (receiving, sending) = stream.split();

        let (outgoing, to_write) = mpsc::channel::<String>();
        let (lines, incoming) = mpsc::channel();
        let failures = lines.clone();
        thread::Builder::new()
            .name("flui-agent-client-write".into())
            .spawn(move || send(sending, &to_write, &failures))
            .map_err(|e| format!("could not start the connection's writer: {e}"))?;
        thread::Builder::new()
            .name("flui-agent-client-read".into())
            .spawn(move || receive(receiving, &lines))
            .map_err(|e| format!("could not start the connection's reader: {e}"))?;

        let connection = Self {
            outgoing,
            incoming,
            next_id: 1,
        };
        let hello = Hello::new(credentials.token.clone(), PROTOCOL_VERSION).with_client(CLIENT);
        let refused = |why: String| {
            // The server closes unread on a wrong token.
            format!(
                "the app refused the connection ({why}); the session is stale or belongs to another run"
            )
        };
        let greeting = serde_json::to_string(&Greeting::new(hello))
            .map_err(|e| format!("the hello did not serialize: {e}"))?;
        connection.send(greeting).map_err(|e| match e {
            Failure::Transport(why) | Failure::Unsent(why) => refused(why),
            Failure::Wire(error) => error.to_string(),
        })?;
        match connection.next(Instant::now() + REPLY_TIMEOUT) {
            Ok(ServerLine::Hello { .. }) => Ok(connection),
            // A `version` refusal names what the app speaks; a `not_driver` one, that another
            // client drives the app (ADR-0288 §8).
            Ok(ServerLine::Failed { id: None, error }) => Err(match error.code {
                ErrorCode::Version => format!(
                    "the app refused protocol {PROTOCOL_VERSION}: {}",
                    error.message
                ),
                ErrorCode::NotDriver => format!(
                    "another client drives the app; close it and try again ({})",
                    error.message
                ),
                _ => format!("the app refused the connection: {error}"),
            }),
            Ok(_) => Err("the peer did not answer with a hello; it is not a flui agent".into()),
            Err(Failure::Transport(why) | Failure::Unsent(why)) => Err(refused(why)),
            Err(Failure::Wire(error)) => Err(error.to_string()),
        }
    }

    /// Calls `method` with `params` and returns its result.
    pub(crate) fn request(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        let id = RequestId::Number(self.next_id);
        self.next_id += 1;
        let line = serde_json::to_string(&Request::new(id.clone(), method, params))
            .map_err(|e| Failure::Unsent(format!("the request did not serialize: {e}")))?;
        self.send(line)?;
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            match self.next(deadline)? {
                ServerLine::Answered {
                    id: answered,
                    result,
                } if answered == id => {
                    return Ok(result);
                }
                ServerLine::Failed {
                    id: Some(answered),
                    error,
                } if answered == id => return Err(wire(&error)),
                // The app could not read the line as a request: this one, as only one is out.
                ServerLine::Failed { id: None, error } => return Err(wire(&error)),
                ServerLine::Hello { .. } => {
                    return Err(Failure::Transport("the app said hello again".into()));
                }
                // An event, the late answer to an earlier request that timed out, or a line
                // of a newer app's that this client does not know.
                _ => {}
            }
        }
    }

    fn send(&self, line: String) -> Result<(), Failure> {
        self.outgoing
            .send(line)
            .map_err(|_| Failure::Unsent("the connection closed".into()))
    }

    /// The next line the app sent, waiting until `deadline` for it.
    fn next(&self, deadline: Instant) -> Result<ServerLine<Value>, Failure> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.incoming.recv_timeout(left) {
            Ok(Ok(line)) => serde_json::from_str::<ServerLine<Value>>(&line)
                .map_err(|e| Failure::Transport(format!("the app sent a malformed line: {e}"))),
            Ok(Err(failure)) => Err(failure),
            Err(RecvTimeoutError::Timeout) => Err(Failure::Transport(format!(
                "no reply within {} s",
                REPLY_TIMEOUT.as_secs()
            ))),
            Err(RecvTimeoutError::Disconnected) => {
                Err(Failure::Transport("the connection closed".into()))
            }
        }
    }
}

/// The app's error object as the agent hands it on.
fn wire(error: &flui_protocol::ErrorObject) -> Failure {
    match serde_json::to_value(error) {
        Ok(error) => Failure::Wire(error),
        Err(e) => Failure::Transport(format!("the app's error did not serialize: {e}")),
    }
}

/// Writes each line handed over until the connection drops it or a write fails; a failure
/// is reported once, on the lines' channel.
fn send(
    mut stream: SendHalf,
    lines: &mpsc::Receiver<String>,
    failures: &mpsc::Sender<Result<String, Failure>>,
) {
    while let Ok(line) = lines.recv() {
        if let Err(failure) = write_line(&mut stream, &line) {
            let _ = failures.send(Err(failure));
            return;
        }
    }
}

/// Reads every line the app sends until it closes, a read fails or a line runs past
/// [`MAX_REPLY`]; the end is reported once, on the lines' channel.
fn receive(mut stream: RecvHalf, lines: &mpsc::Sender<Result<String, Failure>>) {
    let mut reader = LineReader::new(MAX_REPLY);
    let mut chunk = vec![0_u8; 64 << 10];
    loop {
        loop {
            let line = match reader.next_line() {
                Ok(Some(line)) => String::from_utf8(line).map_err(|_| {
                    Failure::Transport("the app sent a line that is not UTF-8".into())
                }),
                Ok(None) => break,
                Err(_) => Err(Failure::Transport(
                    "the reply is longer than the client reads".into(),
                )),
            };
            let failed = line.is_err();
            if lines.send(line).is_err() || failed {
                return;
            }
        }
        let read = match stream.read(&mut chunk) {
            Ok(0) => Err("the app closed the connection".to_owned()),
            Ok(read) => Ok(read),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => Err(format!("could not read the reply: {e}")),
        };
        match read {
            Ok(read) => reader.push(&chunk[..read]),
            Err(why) => {
                let _ = lines.send(Err(Failure::Transport(why)));
                return;
            }
        }
    }
}

/// Sends `line` and its newline. A failure before the newline is written
/// leaves the request unsent (the app dispatches whole lines only); one in
/// the flush after it may have delivered it.
fn write_line(stream: &mut impl Write, line: &str) -> Result<(), Failure> {
    stream
        .write_all(line.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .map_err(|e| Failure::Unsent(format!("could not send: {e}")))?;
    stream
        .flush()
        .map_err(|e| Failure::Transport(format!("the request may not have arrived: {e}")))
}

/// The endpoint's name for `address`, as the app binds it.
pub(crate) fn name(address: &str) -> std::io::Result<Name<'_>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
        address
            .strip_prefix(r"\\.\pipe\")
            .unwrap_or(address)
            .to_ns_name::<GenericNamespaced>()
    }
    #[cfg(unix)]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName as _};
        address.to_fs_name::<GenericFilePath>()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{Failure, write_line};

    /// Takes `accept` bytes, then fails; fails its flush when `flush_fails`.
    struct Pipe {
        accept: usize,
        flush_fails: bool,
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.accept == 0 {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            let taken = buf.len().min(self.accept);
            self.accept -= taken;
            Ok(taken)
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.flush_fails {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_request_is_unsent_only_while_its_newline_is_not_written() {
        let line = r#"{"id":1}"#;
        for (accept, flush_fails, unsent) in [
            (0, false, true),
            // The whole request but its newline: the app never dispatches it.
            (line.len(), false, true),
            // Written whole, then the flush failed: it may have arrived.
            (line.len() + 1, true, false),
        ] {
            let mut pipe = Pipe {
                accept,
                flush_fails,
            };
            let failure = write_line(&mut pipe, line).expect_err("the pipe fails");
            assert_eq!(
                matches!(failure, Failure::Unsent(_)),
                unsent,
                "{accept} bytes taken, flush fails: {flush_fails}: {failure:?}"
            );
        }
    }
}
