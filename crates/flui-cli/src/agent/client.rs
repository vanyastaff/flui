//! A connection to an app's driver agent (ADR-0239).
//!
//! The conversation is `flui-protocol`'s: newline-delimited JSON, a
//! `{"hello":{"token","protocol","client"}}` line first, answered by the
//! app's hello (or a `version` error when it speaks another protocol), then
//! `{"id","method","params"}` requests answered by `{"id","result"}` or
//! `{"id","error"}`. One request is in flight at a time, and this client
//! subscribes to no events.
//!
//! Reads happen on a thread of the connection's own, so a peer that stops
//! answering costs a bounded wait ([`REPLY_TIMEOUT`]) instead of a stuck
//! `flui mcp`; the thread ends when the peer closes or the connection drops.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use flui_protocol::session::{Hello, HelloReply};
use flui_protocol::version::PROTOCOL_VERSION;
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{Name, Stream};
use serde_json::{Value, json};

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
    requests: mpsc::Sender<String>,
    replies: mpsc::Receiver<Result<String, Failure>>,
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

        let (requests, inbox) = mpsc::channel::<String>();
        let (outbox, replies) = mpsc::channel();
        thread::Builder::new()
            .name("flui-agent-client".into())
            .spawn(move || pump(stream, &inbox, &outbox))
            .map_err(|e| format!("could not start the connection thread: {e}"))?;

        let mut connection = Self {
            requests,
            replies,
            next_id: 1,
        };
        let hello = Hello::new(credentials.token.clone(), PROTOCOL_VERSION).with_client(CLIENT);
        let reply = connection
            .exchange(&json!({ "hello": hello }).to_string())
            .map_err(|e| match e {
                // The server closes unread on a wrong token.
                Failure::Transport(why) | Failure::Unsent(why) => format!(
                    "the app refused the connection ({why}); the session is stale or belongs to another run"
                ),
                Failure::Wire(error) => error.to_string(),
            })?;
        if let Some(error) = reply.get("error") {
            // A `version` refusal names what the app speaks.
            return Err(format!(
                "the app refused protocol {PROTOCOL_VERSION}: {}",
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("no reason given")
            ));
        }
        reply
            .get("hello")
            .cloned()
            .and_then(|hello| serde_json::from_value::<HelloReply>(hello).ok())
            .ok_or("the peer did not answer with a hello; it is not a flui agent")?;
        Ok(connection)
    }

    /// Calls `method` with `params` and returns its result.
    pub(crate) fn request(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({ "id": id, "method": method, "params": params });
        let mut reply = self.exchange(&line.to_string())?;
        if reply.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(Failure::Transport(format!(
                "the app answered a different request than {id}"
            )));
        }
        if let Some(error) = reply.get_mut("error") {
            return Err(Failure::Wire(error.take()));
        }
        Ok(reply.get_mut("result").map_or(Value::Null, Value::take))
    }

    fn exchange(&mut self, line: &str) -> Result<Value, Failure> {
        self.requests
            .send(line.to_owned())
            .map_err(|_| Failure::Unsent("the connection closed".into()))?;
        match self.replies.recv_timeout(REPLY_TIMEOUT) {
            Ok(Ok(reply)) => serde_json::from_str(&reply)
                .map_err(|e| Failure::Transport(format!("the app sent a malformed reply: {e}"))),
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

/// Writes each request line and reads its reply line, until either side
/// goes away.
fn pump(
    stream: Stream,
    inbox: &mpsc::Receiver<String>,
    outbox: &mpsc::Sender<Result<String, Failure>>,
) {
    let mut reader = BufReader::new(stream);
    while let Ok(line) = inbox.recv() {
        let result = write_line(&mut reader, &line)
            .map_err(Failure::Unsent)
            .and_then(|()| read_line(&mut reader).map_err(Failure::Transport));
        let failed = result.is_err();
        if outbox.send(result).is_err() || failed {
            return;
        }
    }
}

fn write_line(reader: &mut BufReader<Stream>, line: &str) -> Result<(), String> {
    let stream = reader.get_mut();
    stream
        .write_all(line.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|e| format!("could not send: {e}"))
}

fn read_line(reader: &mut BufReader<Stream>) -> Result<String, String> {
    let mut reply = String::new();
    // `take` bounds a peer that never ends its line.
    let read = reader
        .by_ref()
        .take(MAX_REPLY as u64)
        .read_line(&mut reply)
        .map_err(|e| format!("could not read the reply: {e}"))?;
    if read == 0 {
        return Err("the app closed the connection".into());
    }
    if !reply.ends_with('\n') {
        return Err("the reply is longer than the client reads".into());
    }
    Ok(reply)
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
