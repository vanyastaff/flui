//! One connection: a thread of its own drives the conversation's [`Session`] and writes its
//! lines, parked until there is something to do. An owner's answer, an event and the
//! session's deadline wake it.
//!
//! Unix: a reader thread blocks on the socket and hands each read over, waking the
//! connection's thread, so nothing polls; closing the socket (`shutdown`) ends its blocking
//! reads and writes, and a write blocked past [`WRITE_TIMEOUT`] drops the connection.
//!
//! Windows: a pipe takes no timeout, and a blocking read on it cannot be woken without
//! discarding what the client has not read yet. The connection's thread reads the pipe without
//! blocking instead, looking for input every 10 ms while nothing else wakes it, and
//! writes it a chunk at a time, giving up after [`WRITE_TIMEOUT`] or when the agent stops.

use std::sync::Arc;
use std::task::{Wake, Waker};
use std::thread::Thread;
use std::time::Duration;

use flui_runtime::agent::Session;
use interprocess::local_socket::Stream;
use web_time::Instant;

use super::endpoint::{Done, Transport};

/// How long a write may make no progress before the connection is dropped: a client that
/// stopped reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// The most one read takes.
const CHUNK: usize = 8192;

/// Wakes the connection's thread.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Serve `session` over `stream` until the conversation closes, the client goes, or the agent
/// stops. Runs on the connection's own thread; dropping the session at the end ends the
/// conversation.
pub(super) fn serve(stream: Stream, session: Session, transport: &Arc<Transport>, done: &Done) {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    // A conversation that panics (a subscriber at one of its traces, an owner's wake) broke the
    // connection as surely as the client closing it: the session is dropped as for a client
    // that left.
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a panicking conversation drops its session like a client leaving"
    )]
    let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        platform::serve(stream, session, transport, done, &waker);
    }));
    if let Err(payload) = served {
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Park until woken, or until `deadline` passes, or at most `bound`.
fn park(deadline: Option<Instant>, bound: Option<Duration>) {
    let until_deadline =
        deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
    match (until_deadline, bound) {
        (Some(left), Some(bound)) => std::thread::park_timeout(left.min(bound)),
        (Some(wait), None) | (None, Some(wait)) => std::thread::park_timeout(wait),
        (None, None) => std::thread::park(),
    }
}

#[cfg(unix)]
mod platform {
    use std::io::{self, Read as _, Write as _};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
    use std::task::Waker;

    use flui_runtime::agent::{Session, Status};
    use interprocess::local_socket::Stream;
    use web_time::Instant;

    use super::super::endpoint::{Done, Transport};
    use super::{CHUNK, WRITE_TIMEOUT};

    /// How many reads wait for the connection's thread before the reader stops reading: what a
    /// client sends past them waits in the socket.
    const INPUT_CHUNKS: usize = 4;

    /// A handle that closes the connection from another thread.
    pub(in super::super) type Closer = UnixStream;

    /// Close the connection `closer` belongs to: its reads end and its writes fail, on every
    /// thread. What was written stays readable: a Unix socket has no reset to lose it to.
    pub(in super::super) fn close(closer: &Closer) {
        let _ = closer.shutdown(std::net::Shutdown::Both);
    }

    /// What the reader hands over.
    enum Input {
        Bytes(Vec<u8>),
        /// The socket's input ended: the client closed it, or it broke.
        End,
    }

    pub(super) fn serve(
        stream: Stream,
        session: Session,
        transport: &Arc<Transport>,
        done: &Done,
        waker: &Waker,
    ) {
        let Ok((reader, writer, closer)) = halves(stream) else {
            return;
        };
        // Closes the socket when the conversation ends, which ends the reader too.
        let Some(_open) = transport.open(closer) else {
            return;
        };
        let (input, inbox) = mpsc::sync_channel::<Input>(INPUT_CHUNKS);
        let reading = waker.clone();
        let reader_done = done.clone();
        let spawned = std::thread::Builder::new()
            .name("flui-agent-read".into())
            .spawn(move || {
                let _done = reader_done;
                read(reader, &input, &reading);
            });
        if spawned.is_err() {
            return;
        }
        converse(session, &inbox, writer, waker);
    }

    /// Drive `session` until it closes or a write fails.
    fn converse(
        mut session: Session,
        inbox: &Receiver<Input>,
        mut writer: UnixStream,
        waker: &Waker,
    ) {
        loop {
            let now = Instant::now();
            let mut received = false;
            match inbox.try_recv() {
                Ok(Input::Bytes(bytes)) => {
                    session.receive(&bytes, now);
                    received = true;
                }
                Ok(Input::End) | Err(TryRecvError::Disconnected) => session.peer_closed(),
                Err(TryRecvError::Empty) => {}
            }
            session.poll(now, waker);
            while let Some(line) = session.transmit() {
                if writer.write_all(&line).is_err() {
                    return;
                }
            }
            if session.status() == Status::Closed {
                return;
            }
            // A thread's wake is one token however many reads woke it: take every read before
            // parking.
            if !received {
                super::park(session.next_deadline(), None);
            }
        }
    }

    /// Read until the socket ends or the connection's thread is gone, handing each read over.
    fn read(mut reader: UnixStream, input: &SyncSender<Input>, waker: &Waker) {
        let mut chunk = vec![0_u8; CHUNK];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if input.send(Input::Bytes(chunk[..read].to_vec())).is_err() {
                        return;
                    }
                    waker.wake_by_ref();
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = input.send(Input::End);
        waker.wake_by_ref();
    }

    /// The socket's reading and writing ends, blocking, and a handle that closes it.
    fn halves(stream: Stream) -> io::Result<(UnixStream, UnixStream, Closer)> {
        let Stream::UdSocket(stream) = stream;
        let writer = stream.inner().try_clone()?;
        drop(stream);
        writer.set_nonblocking(false)?;
        writer.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let reader = writer.try_clone()?;
        let closer = writer.try_clone()?;
        Ok((reader, writer, closer))
    }
}

#[cfg(windows)]
mod platform {
    use std::io::{self, Read as _, Write as _};
    use std::sync::Arc;
    use std::task::Waker;
    use std::time::Duration;

    use flui_runtime::agent::{Session, Status};
    use interprocess::local_socket::Stream;
    use interprocess::local_socket::traits::Stream as _;
    use web_time::Instant;

    use super::super::endpoint::{Done, Transport};
    use super::{CHUNK, WRITE_TIMEOUT};

    /// How often the connection's thread looks for input while nothing else wakes it.
    const PIPE_POLL: Duration = Duration::from_millis(10);

    /// How long a write that made no progress waits before it tries again.
    const WRITE_RETRY: Duration = Duration::from_millis(1);

    /// The most one write offers the pipe. A non-blocking named pipe takes a write whole or not
    /// at all, so a write larger than the pipe's free buffer would never go through; small
    /// writes go through as the client reads.
    const WRITE_CHUNK: usize = 512;

    /// What wakes a connection's thread from another: its thread, unparked.
    pub(in super::super) type Closer = std::thread::Thread;

    /// Wake the connection `closer` belongs to, which then sees the agent stopping.
    pub(in super::super) fn close(closer: &Closer) {
        closer.unpark();
    }

    pub(super) fn serve(
        stream: Stream,
        mut session: Session,
        transport: &Arc<Transport>,
        _done: &Done,
        waker: &Waker,
    ) {
        if stream.set_nonblocking(true).is_err() {
            return;
        }
        let Some(_open) = transport.open(std::thread::current()) else {
            return;
        };
        let mut chunk = vec![0_u8; CHUNK];
        loop {
            let now = Instant::now();
            let mut received = false;
            loop {
                match read_some(&stream, &mut chunk) {
                    Ok(0) => {
                        session.peer_closed();
                        break;
                    }
                    Ok(read) => {
                        session.receive(&chunk[..read], now);
                        received = true;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        session.peer_closed();
                        break;
                    }
                }
            }
            session.poll(now, waker);
            while let Some(line) = session.transmit() {
                if write_bytes(&stream, &line, transport).is_err() {
                    return;
                }
            }
            if session.status() == Status::Closed {
                return;
            }
            if !received {
                super::park(session.next_deadline(), Some(PIPE_POLL));
            }
        }
    }

    /// Write `bytes`, a whole line with its newline. Gives up when the agent stops, so a
    /// client that stops reading never holds up a detach, and after [`WRITE_TIMEOUT`] without
    /// progress.
    fn write_bytes(stream: &Stream, bytes: &[u8], transport: &Transport) -> io::Result<()> {
        let mut progress = Instant::now();
        let mut written = 0;
        while written < bytes.len() {
            let end = bytes.len().min(written + WRITE_CHUNK);
            match (&*stream).write(&bytes[written..end]) {
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
            if transport.is_stopping() {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            if progress.elapsed() >= WRITE_TIMEOUT {
                return Err(io::ErrorKind::TimedOut.into());
            }
            std::thread::park_timeout(WRITE_RETRY);
        }
        // No flush: it waits until the client has read everything, which a client that stops
        // reading never lets happen. What is written is delivered before the pipe closes.
        Ok(())
    }

    /// A non-blocking read: `Ok(0)` only once the peer has closed, `WouldBlock` while no data
    /// has come.
    ///
    /// A non-blocking named pipe answers an empty read the way it answers a closed one
    /// (`ERROR_NO_DATA`, which reads as end of file), so an empty read is told apart by an empty
    /// write, which fails only on a closed pipe.
    fn read_some(stream: &Stream, buffer: &mut [u8]) -> io::Result<usize> {
        match (&*stream).read(buffer) {
            Ok(0) => match (&*stream).write(&[]) {
                Ok(_) => Err(io::ErrorKind::WouldBlock.into()),
                Err(_) => Ok(0),
            },
            other => other,
        }
    }
}

pub(super) use platform::{Closer, close};
