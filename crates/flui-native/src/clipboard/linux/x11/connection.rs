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
//! ([`ReplyStream::bound_waits`]), each wait has one absolute deadline,
//! which packets arriving meanwhile (another client's events) do not move:
//!
//! - a reply's, [`REPLY_TIMEOUT`] from the request ([`reply_within`], the
//!   one way the thread waits for a reply or for a request's error), past
//!   which the request counts as failed, its answer discarded by sequence,
//!   and the server as held for the rest of the thread's step
//!   ([`ReplyStream::begin_step`]): no further reply is waited for;
//! - the writes', [`REPLY_TIMEOUT`] from the first that waits in the step,
//!   shared by all of them: a write that cannot finish by it may have sent
//!   part of a request, so the connection is broken from then on, every use
//!   failing at once, and the thread closes as on any connection failure;
//! - both end sooner at the close's deadline ([`ReplyStream::end_by`]).
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
use x11rb::connection::{DiscardMode, RequestConnection as _, RequestKind};
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

/// How long the thread waits for one reply, or for its step's writes to
/// find room; past it the request counts as failed, or the connection as
/// broken.
pub(super) const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// The clipboard thread's connection.
pub(super) type Connection = RustConnection<ReplyStream>;

/// A wait for a reply that passed its deadline, which [`is_late`] tells
/// from a broken connection.
#[derive(Debug)]
struct Late;

impl std::fmt::Display for Late {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the X server did not answer in time")
    }
}

impl std::error::Error for Late {}

/// A connection broken by a write that did not finish by its deadline.
#[derive(Debug)]
struct Broken;

impl std::fmt::Display for Broken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a write to the X server did not finish in time")
    }
}

impl std::error::Error for Broken {}

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
    /// The deadline the step's writes share, set by the first that waits.
    write_by: Cell<Option<Instant>>,
    /// Whether a reply was late this step: no further one is waited for.
    held: Cell<bool>,
    /// Whether a write did not finish by its deadline.
    broken: Cell<bool>,
}

impl ReplyStream {
    /// Bound every wait from now on: the clipboard's thread runs.
    pub(super) fn bound_waits(&self) {
        self.bounded.set(true);
    }

    /// End every wait by `deadline` from now on: the thread closes.
    pub(super) fn end_by(&self, deadline: Instant) {
        self.bounded.set(true);
        self.closing_by.set(Some(deadline));
    }

    /// A step of the thread begins: its writes get a deadline of their own,
    /// and the server counts as answering until a reply is late.
    pub(super) fn begin_step(&self) {
        self.write_by.set(None);
        self.held.set(false);
    }

    /// Treat the server as held for the rest of the step: no reply is
    /// waited for, each request answering as if the server did not.
    pub(super) fn hold(&self) {
        self.held.set(true);
    }

    /// `deadline`, or the close's if that comes first.
    fn ending(&self, deadline: Instant) -> Instant {
        self.closing_by
            .get()
            .map_or(deadline, |closing| closing.min(deadline))
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
    /// A wait to read is a reply's (x11rb reads only to answer one); a wait
    /// to write, `Writable` or `ReadAndWritable` as x11rb reads what arrives
    /// meanwhile, shares the step's write deadline and breaks the
    /// connection when it passes.
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
                let reply_by = self.reply_by.get().unwrap_or_else(|| now + REPLY_TIMEOUT);
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
            let timeout = Timespec::try_from(left)
                .unwrap_or_else(|_| Timespec::try_from(REPLY_TIMEOUT).expect("BUG: it fits"));
            let mut fds = [PollFd::new(&fd, flags)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) if matches!(mode, PollMode::Readable) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, Late));
                }
                Ok(0) => {
                    self.broken.set(true);
                    return Err(Self::broken_error());
                }
                Ok(_) => return Ok(()),
                Err(Errno::INTR) => {}
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
        self.socket.write(buf, fds)
    }

    fn write_vectored(
        &self,
        bufs: &[IoSlice<'_>],
        fds: &mut Vec<RawFdContainer>,
    ) -> io::Result<usize> {
        if self.broken.get() {
            return Err(Self::broken_error());
        }
        self.socket.write_vectored(bufs, fds)
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
                let stream = ReplyStream {
                    socket,
                    bounded: Cell::new(false),
                    closing_by: Cell::new(None),
                    reply_by: Cell::new(None),
                    write_by: Cell::new(None),
                    held: Cell::new(false),
                    broken: Cell::new(false),
                };
                let connection =
                    RustConnection::connect_to_stream_with_auth_info(stream, screen, name, data)?;
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
/// the close's deadline): `None` when it did not come by then, or the server
/// counts as held this step. The request then failed, its reply, should it
/// come later, is discarded by sequence, and the server counts as held for
/// the rest of the step.
pub(super) fn reply_within<R: TryParse>(
    connection: &Connection,
    cookie: Cookie<'_, Connection, R>,
) -> Result<Option<R>, ReplyError> {
    let stream = connection.stream();
    if stream.held.get() {
        cookie.discard_reply_and_errors();
        return Ok(None);
    }
    let sequence = cookie.sequence_number();
    stream.reply_by.set(Some(Instant::now() + REPLY_TIMEOUT));
    let reply = cookie.reply();
    stream.reply_by.set(None);
    match reply {
        Ok(reply) => Ok(Some(reply)),
        Err(ReplyError::ConnectionError(error)) if is_late(&error) => {
            stream.held.set(true);
            connection.discard_reply(
                sequence,
                RequestKind::HasResponse,
                DiscardMode::DiscardReplyAndError,
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Whether the request `cookie` names, which has no reply, took:
/// `Some(Ok(()))`, `Some(Err(error))` with the server's refusal, or `None`
/// when a round trip behind it did not come back in time ([`reply_within`]).
/// The request may then still be carried out later, and its refusal is
/// discarded.
pub(super) fn checked(
    connection: &Connection,
    cookie: VoidCookie<'_, Connection>,
) -> Result<Option<Result<(), X11Error>>, ReplyOrIdError> {
    let sync = connection.get_input_focus()?;
    if reply_within(connection, sync)?.is_none() {
        cookie.ignore_error();
        return Ok(None);
    }
    // The round trip came back behind the request, so its refusal, if any,
    // is read: the check waits on nothing, or at most a reply's deadline.
    match cookie.check() {
        Ok(()) => Ok(Some(Ok(()))),
        Err(ReplyError::X11Error(error)) => Ok(Some(Err(error))),
        Err(ReplyError::ConnectionError(error)) if is_late(&error) => {
            connection.stream().held.set(true);
            Ok(None)
        }
        Err(ReplyError::ConnectionError(error)) => Err(error.into()),
    }
}
