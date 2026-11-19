//! The conversation over a local socket: a named pipe on Windows, a Unix domain socket elsewhere,
//! or the loopback port `adb forward` carries to an Android app's abstract socket.
//!
//! A thread of the connection's own reads every line the app sends and another writes, so an app
//! that stops answering, or stops reading, costs the call's budget instead of a stuck driver. A
//! call takes the first line that answers its id: a late answer to an earlier call that timed out
//! is passed over. Once the connection breaks, every later call fails at once as the app's process
//! gone, never waiting.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use flui_protocol::events::Event;
use flui_protocol::framing::LineReader;
use flui_protocol::session::HelloReply;
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use serde_json::Value;

use super::{Call, Connection, Events, Heard, MAX_REPLY};
use crate::error::{Error, Result};

/// How long the hello's answer may take.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a connection to a forwarded port may take to open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the app's agent listens.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Endpoint {
    /// A pipe name on Windows, a socket path elsewhere, `@name` for a Linux abstract socket.
    Local(String),
    /// A loopback port carried to an Android app's abstract socket.
    Forwarded(SocketAddr),
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(address) => f.write_str(address),
            Self::Forwarded(address) => write!(f, "{address} (adb forward)"),
        }
    }
}

/// What the reader thread hands over: a line without its newline, or why the input ended.
type Incoming = std::result::Result<Vec<u8>, String>;

/// A conversation over a socket.
pub(crate) struct SocketConnection {
    outgoing: mpsc::Sender<Vec<u8>>,
    incoming: mpsc::Receiver<Incoming>,
    hello: HelloReply,
    next_id: u64,
    events: Events,
    /// Why the connection broke, once it did.
    broken: Option<String>,
}

impl SocketConnection {
    /// Connect to `endpoint` and present `token`, retrying a refused connection until `wait` has
    /// passed or `alive` says the app it waits for is gone: a launched app may still be starting.
    pub(crate) fn connect(
        endpoint: &Endpoint,
        token: &str,
        wait: Duration,
        alive: &mut dyn FnMut() -> bool,
    ) -> Result<Self> {
        // A wait past what an `Instant` names waits for as long as the app lives.
        let deadline = Instant::now().checked_add(wait);
        let (reading, writing) = loop {
            match open(endpoint) {
                Ok(halves) => break halves,
                Err(error)
                    if deadline.is_some_and(|deadline| Instant::now() >= deadline) || !alive() =>
                {
                    return Err(Error::Io(io::Error::new(
                        error.kind(),
                        format!("could not connect to the app's agent at {endpoint}: {error}"),
                    )));
                }
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        };
        let (outgoing, to_write) = mpsc::channel::<Vec<u8>>();
        let (lines, incoming) = mpsc::channel::<Incoming>();
        let failures = lines.clone();
        thread::Builder::new()
            .name("flui-driver-write".into())
            .spawn(move || write_lines(writing, &to_write, &failures))?;
        thread::Builder::new()
            .name("flui-driver-read".into())
            .spawn(move || read_lines(reading, &lines))?;
        let mut connection = Self {
            outgoing,
            incoming,
            hello: HelloReply::new(flui_protocol::version::PROTOCOL_VERSION),
            next_id: 1,
            events: Events::default(),
            broken: None,
        };
        connection.greet(token)?;
        Ok(connection)
    }

    fn greet(&mut self, token: &str) -> Result<()> {
        self.send(super::hello_line(token)?)?;
        let deadline = Instant::now() + HELLO_TIMEOUT;
        loop {
            let Some(line) = self.next_line(deadline).map_err(|why| Error::Protocol {
                message: format!(
                    "the app closed the connection at the hello ({why}): the token is wrong, or the \
                     endpoint is not a FLUI agent"
                ),
            })?
            else {
                return Err(Error::Protocol {
                    message: "the app did not answer the hello".into(),
                });
            };
            match super::hear(&line, None)? {
                Heard::Hello(hello) => {
                    self.hello = *hello;
                    return Ok(());
                }
                Heard::Answer(Err(refused)) => return Err(refused),
                Heard::Event(event) => self.events.push(event),
                Heard::Answer(Ok(_)) | Heard::Other => {}
            }
        }
    }

    fn send(&mut self, line: Vec<u8>) -> Result<()> {
        if let Some(why) = &self.broken {
            return Err(Error::process_gone(why.clone(), None));
        }
        self.outgoing.send(line).map_err(|_| {
            let why = "the connection to the app closed".to_owned();
            self.broken = Some(why.clone());
            Error::process_gone(why, None)
        })
    }

    /// The next line, waiting until `deadline`: `Ok(None)` when none came by then, `Err` with why
    /// once the connection broke.
    fn next_line(&mut self, deadline: Instant) -> std::result::Result<Option<Vec<u8>>, String> {
        if let Some(why) = &self.broken {
            return Err(why.clone());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        match self.incoming.recv_timeout(left) {
            Ok(Ok(line)) => Ok(Some(line)),
            Ok(Err(why)) => {
                self.broken = Some(why.clone());
                Err(why)
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                let why = "the connection to the app closed".to_owned();
                self.broken = Some(why.clone());
                Err(why)
            }
        }
    }
}

impl Connection for SocketConnection {
    fn hello(&self) -> &HelloReply {
        &self.hello
    }

    fn call(&mut self, call: Call) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(super::request_line(id, &call)?)?;
        // The agent holds a request at most a day (a wait's longest timeout) past its reply bound.
        let deadline = Instant::now() + call.budget.min(super::MAX_BUDGET);
        loop {
            let line = match self.next_line(deadline) {
                Ok(Some(line)) => line,
                Ok(None) => return Err(super::unanswered(&call)),
                Err(why) => return Err(Error::process_gone(why, None)),
            };
            match super::hear(&line, Some(id))? {
                Heard::Answer(answer) => return answer,
                Heard::Event(event) => self.events.push(event),
                Heard::Hello(_) => {
                    return Err(Error::Protocol {
                        message: "the app said hello again".into(),
                    });
                }
                Heard::Other => {}
            }
        }
    }

    fn take_events(&mut self) -> (Vec<Event>, u64) {
        // Lines already read go to the queue first, so nothing the app sent before is left out.
        while let Ok(Ok(line)) = self.incoming.try_recv() {
            if let Ok(Heard::Event(event)) = super::hear(&line, None) {
                self.events.push(event);
            }
        }
        self.events.take()
    }
}

/// Writes each line handed over until the connection drops it or a write fails; a failure is
/// reported once, on the lines' channel.
fn write_lines(
    mut stream: Box<dyn Write + Send>,
    lines: &mpsc::Receiver<Vec<u8>>,
    failures: &mpsc::Sender<Incoming>,
) {
    while let Ok(line) = lines.recv() {
        if let Err(error) = stream.write_all(&line).and_then(|()| stream.flush()) {
            let _ = failures.send(Err(format!("could not write to the app: {error}")));
            return;
        }
    }
}

/// Reads every line the app sends until it closes, a read fails or a line runs past
/// [`MAX_REPLY`]; the end is reported once, on the lines' channel.
fn read_lines(mut stream: Box<dyn Read + Send>, lines: &mpsc::Sender<Incoming>) {
    let mut reader = LineReader::new(MAX_REPLY);
    let mut chunk = vec![0_u8; 64 << 10];
    loop {
        loop {
            match reader.next_line() {
                Ok(Some(line)) => {
                    if lines.send(Ok(line)).is_err() {
                        return;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    let _ = lines.send(Err(format!(
                        "the app sent a line longer than {MAX_REPLY} bytes"
                    )));
                    return;
                }
            }
        }
        match stream.read(&mut chunk) {
            Ok(0) => {
                let _ = lines.send(Err("the app closed the connection".to_owned()));
                return;
            }
            Ok(read) => reader.push(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                let _ = lines.send(Err(format!("the connection to the app broke: {error}")));
                return;
            }
        }
    }
}

/// The halves of a connection to `endpoint`, as its reader and its writer own them.
fn open(endpoint: &Endpoint) -> io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
    match endpoint {
        Endpoint::Local(address) => {
            let (reading, writing) = Stream::connect(local_name(address)?)?.split();
            Ok((Box::new(reading), Box::new(writing)))
        }
        Endpoint::Forwarded(address) => {
            let stream = TcpStream::connect_timeout(address, CONNECT_TIMEOUT)?;
            // One line is one request: send it whole, now.
            stream.set_nodelay(true)?;
            Ok((Box::new(stream.try_clone()?), Box::new(stream)))
        }
    }
}

/// The endpoint's name for `address`, as the app binds it.
pub(crate) fn local_name(address: &str) -> io::Result<interprocess::local_socket::Name<'_>> {
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
        #[cfg(target_os = "linux")]
        if let Some(name) = address.strip_prefix('@') {
            use interprocess::local_socket::ToNsName as _;
            use interprocess::os::unix::local_socket::AbstractNsUdSocket;
            return name.to_ns_name::<AbstractNsUdSocket>();
        }
        use interprocess::local_socket::{GenericFilePath, ToFsName as _};
        address.to_fs_name::<GenericFilePath>()
    }
}
