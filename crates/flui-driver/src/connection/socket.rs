//! The conversation over a local socket: a named pipe on Windows, a Unix domain socket elsewhere,
//! or the loopback port `adb forward` carries to an Android app's abstract socket.
//!
//! One thread of the connection's own owns the stream, which it keeps non-blocking: it writes the
//! lines the driver sends, reads what the app sends, and hands complete lines over. An app that
//! stops answering, or stops reading, costs the call's budget instead of a stuck driver.
//!
//! - **The stream goes with the driver.** The thread is the stream's only owner. When the
//!   connection is dropped, by a close, an error the caller returns, or a panic unwinding past it,
//!   the thread sees the driver's channel close within [`IDLE`], shuts the stream down and ends, and
//!   the drop joins it: the app sees the driver leave and lets go of the input it held.
//! - **Bounded.** At most [`INCOMING_ROOM`] lines wait for the caller, each at most [`MAX_REPLY`]
//!   bytes. Past that the thread stops reading until the caller takes one, so the app's own bound
//!   on what a client leaves unread applies, and nothing is dropped unseen.
//! - **One owner of the broken state.** Every line reaches the caller through
//!   [`SocketConnection::receive`], and why the connection broke is recorded in one place
//!   ([`SocketConnection::take`]) the first time it is heard; once broken, every later call fails
//!   at once as the app's process gone, never waiting. The lines that came before the break still
//!   reach the caller, in order: whether the app closed or a write to it failed, the thread hands
//!   over every complete line it read, and those the stream has ready up to [`MAX_REPLY`] bytes
//!   more, before why it ended, which names what was not handed over; a send that finds the
//!   thread ended keeps the lines it reads off the channel to learn why.
//! - A call takes the first line that answers its id: a late answer to an earlier call that timed
//!   out is passed over.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flui_protocol::endpoint::PolledStream;
use flui_protocol::events::Event;
use flui_protocol::framing::LineReader;
use flui_protocol::session::HelloReply;
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use serde_json::Value;

use super::{Call, Events, Heard, MAX_REPLY, Transport};
use crate::error::{Error, Result};

/// How long the hello's answer may take.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a connection to a forwarded port may take to open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the connection's thread waits for a line to send while the app sends nothing: how
/// soon it sees input on a pipe that cannot wake it, and the driver leaving.
const IDLE: Duration = Duration::from_millis(2);

/// How many complete lines wait for the caller before the thread stops reading.
pub(crate) const INCOMING_ROOM: usize = 16;

/// How long a write may make no progress before the connection is called broken: an app that
/// stopped reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

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

/// What the connection's thread hands over: a line without its newline, or why the conversation
/// ended.
type Incoming = std::result::Result<Vec<u8>, String>;

/// A conversation over a socket.
pub(crate) struct SocketConnection {
    /// Lines for the connection's thread; dropping it is the driver leaving.
    outgoing: Option<mpsc::Sender<Vec<u8>>>,
    incoming: mpsc::Receiver<Incoming>,
    io: Option<JoinHandle<()>>,
    hello: HelloReply,
    next_id: u64,
    events: Events,
    /// Why the connection broke, once it did.
    broken: Option<String>,
    /// Lines the thread handed over before it ended, read off its channel to learn why: the caller
    /// takes them before the break.
    held: VecDeque<Vec<u8>>,
    opened: Instant,
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
        let stream = loop {
            match Io::open(endpoint) {
                Ok(stream) => break stream,
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
        let (outgoing, to_send) = mpsc::channel::<Vec<u8>>();
        let (lines, incoming) = mpsc::sync_channel::<Incoming>(INCOMING_ROOM);
        let io = thread::Builder::new()
            .name("flui-driver-connection".into())
            .spawn(move || converse(&stream, &to_send, &lines))?;
        let mut connection = Self {
            outgoing: Some(outgoing),
            incoming,
            io: Some(io),
            hello: HelloReply::new(flui_protocol::version::PROTOCOL_VERSION),
            next_id: 1,
            events: Events::default(),
            broken: None,
            held: VecDeque::new(),
            opened: Instant::now(),
        };
        connection.greet(token)?;
        Ok(connection)
    }

    fn greet(&mut self, token: &str) -> Result<()> {
        self.send(super::hello_line(token)?)?;
        let deadline = Instant::now() + HELLO_TIMEOUT;
        loop {
            let Some(line) = self.receive(Some(deadline)).map_err(|why| Error::Protocol {
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
        let sent = self
            .outgoing
            .as_ref()
            .is_some_and(|outgoing| outgoing.send(line).is_ok());
        if sent {
            return Ok(());
        }
        // The thread ended: why is on its channel, after the lines it handed over before. Those
        // lines are kept for the caller, and the first failure is the one reported.
        let why = loop {
            match self.take(None) {
                Ok(Some(line)) => self.held.push_back(line),
                Err(why) => break why,
                // The thread ended, so its channel is closed: `take` answers the end.
                Ok(None) => break "the connection to the app closed".to_owned(),
            }
        };
        Err(Error::process_gone(why, None))
    }

    /// The next line, waiting until `deadline` for it (not at all with `None`): `Ok(None)` when
    /// none came by then, `Err` with why once the connection broke and every line before the break
    /// was taken.
    fn receive(
        &mut self,
        deadline: Option<Instant>,
    ) -> std::result::Result<Option<Vec<u8>>, String> {
        if let Some(line) = self.held.pop_front() {
            return Ok(Some(line));
        }
        self.take(deadline)
    }

    /// The next line off the thread's channel, as [`Self::receive`] answers it: the one place the
    /// broken state is recorded, the first failure kept.
    fn take(&mut self, deadline: Option<Instant>) -> std::result::Result<Option<Vec<u8>>, String> {
        if let Some(why) = &self.broken {
            return Err(why.clone());
        }
        let heard = match deadline {
            Some(deadline) => {
                let left = deadline.saturating_duration_since(Instant::now());
                self.incoming
                    .recv_timeout(left)
                    .map_err(|error| match error {
                        RecvTimeoutError::Timeout => None,
                        RecvTimeoutError::Disconnected => Some(()),
                    })
            }
            None => self.incoming.try_recv().map_err(|error| match error {
                TryRecvError::Empty => None,
                TryRecvError::Disconnected => Some(()),
            }),
        };
        let why = match heard {
            Ok(Ok(line)) => return Ok(Some(line)),
            Err(None) => return Ok(None),
            Ok(Err(why)) => why,
            Err(Some(())) => "the connection to the app closed".to_owned(),
        };
        self.broken = Some(why.clone());
        Err(why)
    }
}

impl Transport for SocketConnection {
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
            let line = match self.receive(Some(deadline)) {
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
        while let Ok(Some(line)) = self.receive(None) {
            if let Ok(Heard::Event(event)) = super::hear(&line, None) {
                self.events.push(event);
            }
        }
        self.events.take()
    }

    fn clock(&self) -> Duration {
        self.opened.elapsed()
    }
}

impl Drop for SocketConnection {
    fn drop(&mut self) {
        // The driver leaves: the thread sees its channel close, shuts the stream down and ends.
        drop(self.outgoing.take());
        if let Some(io) = self.io.take() {
            // Take what the thread still hands over, so one waiting for room is let go.
            while !io.is_finished() {
                let _ = self.incoming.recv_timeout(IDLE);
            }
            let _ = io.join();
        }
    }
}

/// What one read found.
enum Received {
    Bytes(usize),
    Nothing,
    End,
}

/// The stream, as the connection's thread owns it, non-blocking. A local endpoint is polled by
/// the rule its app reads and writes it by ([`PolledStream`]); a TCP stream ends with `Ok(0)`.
enum Io {
    Local(PolledStream<Stream>),
    Tcp(TcpStream),
}

impl Io {
    fn open(endpoint: &Endpoint) -> io::Result<Self> {
        match endpoint {
            Endpoint::Local(address) => {
                let stream = Stream::connect(local_name(address)?)?;
                stream.set_nonblocking(true)?;
                Ok(Self::Local(PolledStream::new(stream)))
            }
            Endpoint::Forwarded(address) => {
                let stream = TcpStream::connect_timeout(address, CONNECT_TIMEOUT)?;
                // One line is one request: send it whole, now.
                stream.set_nodelay(true)?;
                stream.set_nonblocking(true)?;
                Ok(Self::Tcp(stream))
            }
        }
    }

    fn read(&self, buffer: &mut [u8]) -> io::Result<Received> {
        let read = match self {
            Self::Local(stream) => (&*stream).read(buffer),
            Self::Tcp(stream) => (&*stream).read(buffer),
        };
        match read {
            Ok(0) => Ok(Received::End),
            Ok(count) => Ok(Received::Bytes(count)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(Received::Nothing)
            }
            Err(error) => Err(error),
        }
    }

    /// Write `line` whole, as the stream takes it ([`write_whole`]).
    fn write_line(&self, line: &[u8]) -> std::result::Result<(), String> {
        match self {
            Self::Local(stream) => write_whole(stream, line),
            Self::Tcp(stream) => write_whole(stream, line),
        }
    }

    /// Close the stream for both directions. A local stream closes when its one owner drops it,
    /// right after this.
    fn shutdown(&self) {
        if let Self::Tcp(stream) = self {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// Write `line` whole to `stream`, a non-blocking stream, giving up after [`WRITE_TIMEOUT`]
/// without progress.
///
/// No room is `WouldBlock` on both kinds of stream the driver writes (a [`PolledStream`] maps a
/// pipe's write that took nothing to it), so a write of something that takes nothing is a
/// stream that will take no more: an error, not a wait.
fn write_whole(mut stream: impl Write, line: &[u8]) -> std::result::Result<(), String> {
    let mut progress = Instant::now();
    let mut written = 0;
    while written < line.len() {
        match stream.write(&line[written..]) {
            Ok(0) => return Err("could not write to the app: it took nothing".to_owned()),
            Ok(count) => {
                written += count;
                progress = Instant::now();
                continue;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(format!("could not write to the app: {error}")),
        }
        if progress.elapsed() >= WRITE_TIMEOUT {
            return Err("the app stopped reading what the driver sends".to_owned());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

/// The connection's thread: send what the driver hands over, hand over what the app sends, until
/// the driver leaves or the conversation ends. See the [module docs](self).
fn converse(stream: &Io, to_send: &mpsc::Receiver<Vec<u8>>, lines: &SyncSender<Incoming>) {
    let mut reader = LineReader::new(MAX_REPLY);
    let mut chunk = vec![0_u8; 64 << 10];
    // A complete line the caller had no room for yet.
    let mut waiting: Option<Vec<u8>> = None;
    let mut ended: Option<String> = None;
    loop {
        // Hand over what is complete, while the caller has room.
        loop {
            let line = match waiting.take() {
                Some(line) => line,
                None => match reader.next_line() {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    // The reader's refusal ends the conversation, and `finish` names it.
                    Err(_) => return finish(stream, lines, None, &mut reader, &mut chunk, None),
                },
            };
            match lines.try_send(Ok(line)) {
                Ok(()) => {}
                Err(TrySendError::Full(Ok(line))) => {
                    waiting = Some(line);
                    break;
                }
                Err(_) => return stream.shutdown(),
            }
        }
        if let Some(why) = ended.take() {
            return finish(stream, lines, waiting, &mut reader, &mut chunk, Some(why));
        }
        // Send what the driver handed over; its channel closing is the driver leaving.
        loop {
            match to_send.try_recv() {
                Ok(line) => {
                    if let Err(why) = stream.write_line(&line) {
                        return finish(stream, lines, waiting, &mut reader, &mut chunk, Some(why));
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return stream.shutdown(),
            }
        }
        // Read while the caller has room.
        let mut read = false;
        if waiting.is_none() && ended.is_none() {
            match stream.read(&mut chunk) {
                Ok(Received::Bytes(count)) => {
                    reader.push(&chunk[..count]);
                    read = true;
                }
                Ok(Received::Nothing) => {}
                Ok(Received::End) => ended = Some("the app closed the connection".to_owned()),
                Err(error) => ended = Some(format!("the connection to the app broke: {error}")),
            }
        }
        if read || ended.is_some() {
            continue;
        }
        match to_send.recv_timeout(IDLE) {
            Ok(line) => {
                if let Err(why) = stream.write_line(&line) {
                    return finish(stream, lines, waiting, &mut reader, &mut chunk, Some(why));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return stream.shutdown(),
        }
    }
}

/// The end of the conversation, for `why`, its first failure, or for the line too long that
/// `reader` refuses when `why` is `None`.
///
/// Handed over in order, each as the caller makes room: the line `waiting` for room, the complete
/// lines `reader` holds, and the complete lines among what the stream has ready, of which at most
/// [`MAX_REPLY`] bytes are read, and one more to learn whether the app sent anything past them.
/// Then why it ended, naming what was not handed over: a line too long and what follows it, an
/// unfinished last line, or what the app sent past the bound when it sent any. A
/// driver that leaves meanwhile takes what is left in its drop, so no hand-over waits for ever.
fn finish(
    stream: &Io,
    lines: &SyncSender<Incoming>,
    waiting: Option<Vec<u8>>,
    reader: &mut LineReader,
    chunk: &mut [u8],
    why: Option<String>,
) {
    let mut pending = waiting;
    let mut residue = MAX_REPLY;
    let lost = loop {
        let line = match pending.take() {
            Some(line) => line,
            None => match reader.next_line() {
                Ok(Some(line)) => line,
                Ok(None) => {
                    // Once the bound is spent, one byte more tells whether the app sent anything
                    // past it.
                    let room = residue.min(chunk.len()).max(1);
                    match stream.read(&mut chunk[..room]) {
                        Ok(Received::Bytes(_)) if residue == 0 => {
                            break Some(format!(
                                "the driver read {MAX_REPLY} bytes more, and what the app sent \
                                 after them is not handed over"
                            ));
                        }
                        Ok(Received::Bytes(count)) => {
                            reader.push(&chunk[..count]);
                            residue -= count;
                            continue;
                        }
                        // Nothing ready, the end or a break: only an unfinished line is left.
                        Ok(Received::Nothing | Received::End) | Err(_) => {
                            break match reader.buffered() {
                                0 => None,
                                left => Some(format!(
                                    "an unfinished line of {left} bytes is not handed over"
                                )),
                            };
                        }
                    }
                }
                Err(_) => {
                    break Some(format!(
                        "the app sent a line longer than {MAX_REPLY} bytes, which is not handed \
                         over, nor what follows it"
                    ));
                }
            },
        };
        if lines.send(Ok(line)).is_err() {
            return stream.shutdown();
        }
    };
    let why = match (why, lost) {
        (Some(why), Some(lost)) => format!("{why}; {lost}"),
        (Some(why), None) => why,
        (None, lost) => lost.unwrap_or_else(|| "the connection to the app closed".to_owned()),
    };
    let _ = lines.send(Err(why));
    stream.shutdown();
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

#[cfg(test)]
mod tests {
    use flui_protocol::testing::{PipeAnswer, ScriptedPipe};

    use super::write_whole;

    /// A line goes whole across partial and interrupted writes and a write that found no room; a
    /// write of something that takes nothing, or that fails, ends it at once. Fails if a write
    /// that takes nothing is waited on, which would retry it without end.
    #[test]
    fn a_line_is_written_whole_or_ends_at_a_stream_that_takes_nothing() {
        let line = b"{\"id\":1}\n";
        let written = |answers: Vec<PipeAnswer>| {
            let pipe = ScriptedPipe::new([], answers);
            let outcome = write_whole(&pipe, line);
            assert!(pipe.is_spent(), "every scripted write is made");
            outcome
        };
        assert_eq!(
            written(vec![
                PipeAnswer::Bytes(3),
                PipeAnswer::WouldBlock,
                PipeAnswer::Interrupted,
                PipeAnswer::Bytes(line.len()),
            ]),
            Ok(())
        );
        let took_nothing = written(vec![PipeAnswer::Bytes(3), PipeAnswer::Bytes(0)]);
        assert!(
            took_nothing
                .as_ref()
                .is_err_and(|why| why.contains("took nothing")),
            "{took_nothing:?}"
        );
        let failed = written(vec![PipeAnswer::Closed]);
        assert!(
            failed
                .as_ref()
                .is_err_and(|why| why.contains("could not write")),
            "{failed:?}"
        );
    }
}
