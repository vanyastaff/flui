//! The clipboard thread's connection to the X server, over a stream whose
//! every wait for the server ends at a deadline.
//!
//! The thread waits on the server in two ways: for a reply (whether it
//! still owns a selection, a property it reads, whether a requestor's window
//! took a selection), and for room to write once the socket's buffer is
//! full. A server another client holds with a grab reads none of the
//! thread's requests and answers none until it lets go, and a link that died
//! does neither: a plain wait would hold the thread, and the application's
//! exit that joins it, for as long. Here, once the thread runs
//! ([`ReplyStream::bound_waits`]), each wait ends at a deadline that packets
//! arriving meanwhile (another client's events, a flood of them included)
//! never move:
//!
//! - a reply's, [`REPLY_TIMEOUT`] from the request ([`reply_within`], the
//!   one way the thread waits for a reply or for a request's error). Past it
//!   the request counts as failed, its answer discarded by sequence, and the
//!   server as held: no further reply is waited for in the thread's step
//!   until an event read shows the server carried out the late request
//!   ([`ReplyStream::heard`]). A step whose batch holds `Stop` waits for no
//!   reply at all ([`ReplyStream::stopping`]). Each step, and the close,
//!   begins with the server counted as answering
//!   ([`ReplyStream::begin_step`]);
//! - a write's, [`REPLY_TIMEOUT`] from the last write that sent bytes: a
//!   slow link or a pause of the server's delays a write without failing it,
//!   and only a server that takes nothing for that long stalls it. A stalled
//!   write may have sent part of a request, so the connection is broken from
//!   then on, every use failing at once;
//! - both end sooner at the close's deadline ([`ReplyStream::end_by`]).
//!
//! A connection the thread breaks, or loses, ends the clipboard for the rest
//! of the process: the thread closes, its handles answer `Gone`, and nothing
//! connects again, as with any failure of the connection (`docs/gaps.md`).
//!
//! The setup and the atoms the clipboard starts with are waited on as any
//! connection's, on the owner thread as it starts the clipboard.

use std::cell::Cell;
use std::io::{self, IoSlice};
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use web_time::Instant;
use x11rb::connection::{DiscardMode, RequestConnection as _, RequestKind, SequenceNumber};
use x11rb::cookie::{Cookie, VoidCookie};
use x11rb::errors::{
    ConnectError, ConnectionError, DisplayParsingError, ReplyError, ReplyOrIdError,
};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::reexports::x11rb_protocol::parse_display::parse_display;
use x11rb::reexports::x11rb_protocol::xauth::get_auth;
use x11rb::rust_connection::{DefaultStream, PollMode, RustConnection, Stream};
use x11rb::utils::RawFdContainer;
use x11rb::x11_utils::{TryParse, X11Error};

/// How long the thread waits for one reply, or for a write to send any
/// bytes; past it the request counts as failed, or the connection as broken.
pub(super) const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// The clipboard thread's connection.
pub(super) type Connection = RustConnection<ReplyStream>;

/// A wait for a reply that passed its deadline, which [`is_late`] tells
/// from a broken connection.
#[derive(Debug, thiserror::Error)]
#[error("the X server did not answer in time")]
struct Late;

/// A connection broken by a write that sent nothing for [`REPLY_TIMEOUT`].
#[derive(Debug, thiserror::Error)]
#[error("a write to the X server sent nothing in time")]
struct Broken;

/// What one step of the thread knows of the server: the one record of it,
/// replaced as each step and the close begin ([`ReplyStream::begin_step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The server answers: a reply is waited for.
    Answering,
    /// The reply to the request this names did not come in time: no reply
    /// is waited for until an event read shows the server carried that
    /// request out.
    Held(SequenceNumber),
    /// The step's batch holds `Stop`: no reply is waited for.
    Stopping,
}

/// What a wait for a reply came to.
#[derive(Debug)]
pub(super) enum Replied<R> {
    /// The server's reply.
    Reply(R),
    /// The server did not answer in time, now or earlier in the step: the
    /// request counts as failed, though the server may carry it out later.
    Late,
    /// The thread is stopping, so no reply was waited for; the request may
    /// still be carried out.
    Stopping,
}

/// The socket to the server, whose waits end at their deadlines.
#[derive(Debug)]
pub(super) struct ReplyStream {
    socket: DefaultStream,
    /// Whether waits are bounded: from the thread's start.
    bounded: Cell<bool>,
    /// The close's deadline, once the thread closes.
    closing_by: Cell<Option<Instant>>,
    /// The deadline of the reply wait under way ([`reply_within`]).
    reply_by: Cell<Option<Instant>>,
    /// The deadline of the write under way: [`REPLY_TIMEOUT`] from its first
    /// wait since the last write that sent bytes, which clears it.
    write_by: Cell<Option<Instant>>,
    step: Cell<Step>,
    /// Whether a write stalled.
    broken: Cell<bool>,
}

impl ReplyStream {
    /// `socket`, its waits unbounded until the thread runs.
    fn over(socket: DefaultStream) -> Self {
        Self {
            socket,
            bounded: Cell::new(false),
            closing_by: Cell::new(None),
            reply_by: Cell::new(None),
            write_by: Cell::new(None),
            step: Cell::new(Step::Answering),
            broken: Cell::new(false),
        }
    }

    /// Bound every wait from now on: the clipboard's thread runs.
    pub(super) fn bound_waits(&self) {
        self.bounded.set(true);
    }

    /// End every wait by `deadline` from now on: the thread closes.
    pub(super) fn end_by(&self, deadline: Instant) {
        self.bounded.set(true);
        self.closing_by.set(Some(deadline));
    }

    /// A step of the thread, or its close, begins: the server counts as
    /// answering until a reply is late, whatever the last step found.
    pub(super) fn begin_step(&self) {
        self.step.set(Step::Answering);
    }

    /// The step's batch holds `Stop`: no reply is waited for in the rest of
    /// it, each wait answering [`Replied::Stopping`].
    pub(super) fn stopping(&self) {
        self.step.set(Step::Stopping);
    }

    /// An event read carries `sequence`, the last request the server had
    /// carried out as it sent it: a server held since an earlier request
    /// answers again.
    pub(super) fn heard(&self, sequence: SequenceNumber) {
        if let Step::Held(late) = self.step.get()
            && sequence >= late
        {
            self.step.set(Step::Answering);
        }
    }

    /// `deadline`, or the close's if that comes first.
    fn ending(&self, deadline: Instant) -> Instant {
        self.closing_by
            .get()
            .map_or(deadline, |closing| closing.min(deadline))
    }

    /// The wait in `mode` passed its deadline: a reply is late, a write
    /// stalled and the connection broken.
    fn expired(&self, mode: PollMode) -> io::Error {
        if matches!(mode, PollMode::Readable) {
            io::Error::new(io::ErrorKind::TimedOut, Late)
        } else {
            self.broken.set(true);
            Self::broken_error()
        }
    }

    /// A write's outcome: one that sent bytes gives the next wait to write
    /// a deadline of its own.
    fn wrote(&self, written: io::Result<usize>) -> io::Result<usize> {
        if written.as_ref().is_ok_and(|&count| count > 0) {
            self.write_by.set(None);
        }
        written
    }

    fn broken_error() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, Broken)
    }
}

impl AsFd for ReplyStream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }
}

impl Stream for ReplyStream {
    /// A wait to read is a reply's (x11rb reads only to answer one) and ends
    /// at `reply_by`; a wait to write, `Writable` or `ReadAndWritable` as
    /// x11rb reads what arrives meanwhile, ends at `write_by` and breaks the
    /// connection when it passes.
    /// Packets that arrive end the poll, as x11rb reads them, but never move
    /// the deadline: a poll past it fails without looking.
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        if self.broken.get() {
            return Err(Self::broken_error());
        }
        if !self.bounded.get() {
            return self.socket.poll(mode);
        }
        let now = Instant::now();
        let (deadline, flags) = match mode {
            PollMode::Readable => {
                let reply_by = self.reply_by.get().unwrap_or(now + REPLY_TIMEOUT);
                (self.ending(reply_by), PollFlags::IN)
            }
            PollMode::Writable | PollMode::ReadAndWritable => {
                let write_by = self.write_by.get().unwrap_or(now + REPLY_TIMEOUT);
                self.write_by.set(Some(write_by));
                let flags = if matches!(mode, PollMode::Writable) {
                    PollFlags::OUT
                } else {
                    PollFlags::IN | PollFlags::OUT
                };
                (self.ending(write_by), flags)
            }
        };
        let fd = self.socket.as_fd();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(self.expired(mode));
            }
            let timeout = Timespec::try_from(left)
                .unwrap_or_else(|_| Timespec::try_from(REPLY_TIMEOUT).expect("BUG: it fits"));
            let mut fds = [PollFd::new(&fd, flags)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) | Err(Errno::INTR) => {}
                Ok(_) => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn read(&self, buf: &mut [u8], fd_storage: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        if self.broken.get() {
            return Err(Self::broken_error());
        }
        self.socket.read(buf, fd_storage)
    }

    fn write(&self, buf: &[u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        if self.broken.get() {
            return Err(Self::broken_error());
        }
        self.wrote(self.socket.write(buf, fds))
    }

    fn write_vectored(
        &self,
        bufs: &[IoSlice<'_>],
        fds: &mut Vec<RawFdContainer>,
    ) -> io::Result<usize> {
        if self.broken.get() {
            return Err(Self::broken_error());
        }
        self.wrote(self.socket.write_vectored(bufs, fds))
    }
}

/// Connect to the X server `display` names, `DISPLAY` when `None`, as
/// [`RustConnection::connect`] does, over a [`ReplyStream`]: each address
/// the display names in turn, with the authorization `XAUTHORITY` holds
/// for it.
pub(super) fn connect(display: Option<&str>) -> Result<(Connection, usize), ConnectError> {
    let parsed = parse_display(display)?;
    let screen = usize::from(parsed.screen);
    let mut failure = None;
    for address in parsed.connect_instruction() {
        match DefaultStream::connect(&address) {
            Ok((socket, (family, peer))) => {
                // A server that takes no authorization is tried without.
                let (name, data) = get_auth(family, &peer, parsed.display)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let connection = RustConnection::connect_to_stream_with_auth_info(
                    ReplyStream::over(socket),
                    screen,
                    name,
                    data,
                )?;
                return Ok((connection, screen));
            }
            Err(error) => failure = Some(error),
        }
    }
    Err(match failure {
        Some(error) => ConnectError::IoError(error),
        None => DisplayParsingError::Unknown.into(),
    })
}

/// Whether `error` is a reply that came too late, not a broken connection.
fn is_late(error: &ConnectionError) -> bool {
    matches!(error, ConnectionError::IoError(error)
        if error.get_ref().is_some_and(<dyn std::error::Error + Send + Sync>::is::<Late>))
}

/// The reply to `cookie`, waited for until [`REPLY_TIMEOUT`] from now (or
/// the close's deadline). A reply that does not come by then, or a server
/// held since a reply earlier in the step was late, answers
/// [`Replied::Late`]: the request failed, and its reply, should it come
/// later, is discarded by sequence. A step that stops answers
/// [`Replied::Stopping`] without waiting.
pub(super) fn reply_within<R: TryParse>(
    connection: &Connection,
    cookie: Cookie<'_, Connection, R>,
) -> Result<Replied<R>, ReplyError> {
    let stream = connection.stream();
    let not_waited = match stream.step.get() {
        Step::Answering => None,
        Step::Held(_) => Some(Replied::Late),
        Step::Stopping => Some(Replied::Stopping),
    };
    if let Some(replied) = not_waited {
        cookie.discard_reply_and_errors();
        return Ok(replied);
    }
    let sequence = cookie.sequence_number();
    stream.reply_by.set(Some(Instant::now() + REPLY_TIMEOUT));
    let reply = cookie.reply();
    stream.reply_by.set(None);
    match reply {
        Ok(reply) => Ok(Replied::Reply(reply)),
        Err(ReplyError::ConnectionError(error)) if is_late(&error) => {
            stream.step.set(Step::Held(sequence));
            connection.discard_reply(
                sequence,
                RequestKind::HasResponse,
                DiscardMode::DiscardReplyAndError,
            );
            Ok(Replied::Late)
        }
        Err(error) => Err(error),
    }
}

/// Whether the request `cookie` names, which has no reply, took: a reply of
/// `Ok(())`, or of the server's refusal, once a round trip behind it came
/// back ([`reply_within`]). When the round trip does not come back the
/// request may still be carried out later, and its refusal is discarded.
pub(super) fn checked(
    connection: &Connection,
    cookie: VoidCookie<'_, Connection>,
) -> Result<Replied<Result<(), X11Error>>, ReplyOrIdError> {
    let sync = connection.get_input_focus()?;
    match reply_within(connection, sync)? {
        Replied::Reply(_) => {}
        Replied::Late => {
            cookie.ignore_error();
            return Ok(Replied::Late);
        }
        Replied::Stopping => {
            cookie.ignore_error();
            return Ok(Replied::Stopping);
        }
    }
    // The round trip came back behind the request, so its refusal, if any,
    // is already read: the check reads nothing more, and waits on nothing.
    match cookie.check() {
        Ok(()) => Ok(Replied::Reply(Ok(()))),
        Err(ReplyError::X11Error(error)) => Ok(Replied::Reply(Err(error))),
        Err(ReplyError::ConnectionError(error)) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write as _};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    use web_time::Instant;
    use x11rb::rust_connection::{DefaultStream, PollMode, Stream as _};

    use super::{REPLY_TIMEOUT, ReplyStream};

    /// A wait ends at its deadline though the socket stays readable: what
    /// arrives, or what the reader has not taken yet, ends each poll so that
    /// x11rb reads it, but never moves the deadline, so a flood of events
    /// keeps neither a reply's wait nor a stalled write's going past it.
    /// The socket's peer sends one byte nobody reads and reads nothing, so
    /// ours stays readable and, once its buffer is full, never writable.
    #[test]
    fn a_socket_that_stays_readable_does_not_move_a_deadline() {
        let rows = [
            (PollMode::Readable, io::ErrorKind::TimedOut),
            (PollMode::ReadAndWritable, io::ErrorKind::BrokenPipe),
        ];
        for (mode, failure) in rows {
            let (ours, mut theirs) = UnixStream::pair().expect("a socket pair");
            theirs.write_all(&[0]).expect("the byte is sent");
            let (socket, _) =
                DefaultStream::from_unix_stream(ours).expect("the socket is a stream");
            let stream = ReplyStream::over(socket);
            if matches!(mode, PollMode::Readable) {
                // As `reply_within` sets it for the reply it waits for.
                stream.reply_by.set(Some(Instant::now() + REPLY_TIMEOUT));
            } else {
                let chunk = [0_u8; 4096];
                while stream.write(&chunk, &mut Vec::new()).is_ok() {}
            }
            stream.bound_waits();
            let started = Instant::now();
            let ended = loop {
                match stream.poll(mode) {
                    Ok(()) => assert!(
                        started.elapsed() < 3 * REPLY_TIMEOUT,
                        "{mode:?}: the wait goes on past its deadline"
                    ),
                    Err(error) => break error,
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            let took = started.elapsed();
            assert_eq!(ended.kind(), failure, "{mode:?}: how the wait ends");
            assert!(
                took < REPLY_TIMEOUT + Duration::from_secs(1),
                "{mode:?}: the wait ended after {took:?}"
            );
        }
    }
}
