//! The clipboard thread's connection to the X server, over a stream whose
//! every wait for the server ends at a deadline.
//!
//! The thread waits on a reply in a few places (whether it still owns a
//! selection, a property it reads, whether a requestor's window can be
//! heard). A server another client holds with a grab reads none of the
//! thread's requests until it lets go, and a link that died answers nothing:
//! a plain wait would hold the thread, and the application's exit that joins
//! it, for as long. Here, once the thread runs ([`ReplyStream::bound_waits`]),
//! a wait for something to read gives up at [`REPLY_TIMEOUT`], or sooner at
//! the deadline the thread's close sets ([`ReplyStream::end_by`]), and the
//! request it waited on counts as failed ([`reply_within`]). The setup and
//! the atoms the clipboard starts with are waited on as any connection's,
//! on the owner thread as it starts the clipboard. A write the server stops
//! reading still blocks once the socket's buffer is full; the thread's writes
//! are small, and its close writes only once the server has answered it.

use std::cell::Cell;
use std::io::{self, IoSlice};
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use web_time::Instant;
use x11rb::connection::{DiscardMode, RequestConnection as _, RequestKind};
use x11rb::cookie::Cookie;
use x11rb::errors::{ConnectError, ConnectionError, DisplayParsingError, ReplyError};
use x11rb::reexports::x11rb_protocol::parse_display::parse_display;
use x11rb::reexports::x11rb_protocol::xauth::get_auth;
use x11rb::rust_connection::{DefaultStream, PollMode, RustConnection, Stream};
use x11rb::utils::RawFdContainer;
use x11rb::x11_utils::TryParse;

/// How long the thread waits for one reply in normal operation; past it the
/// request counts as failed.
pub(super) const REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// The clipboard thread's connection.
pub(super) type Connection = RustConnection<ReplyStream>;

/// What a wait that gave up returns, which [`is_late`] tells from a broken
/// connection.
pub(super) const LATE: &str = "the X server did not answer in time";

/// The socket to the server, whose waits for something to read end at
/// [`REPLY_TIMEOUT`] or at the close's deadline, whichever comes first.
#[derive(Debug)]
pub(super) struct ReplyStream {
    socket: DefaultStream,
    /// Whether waits are bounded: from the thread's start, after the
    /// connection's setup and the atoms the clipboard starts with, which the
    /// owner thread waits on as any connection's are.
    bounded: Cell<bool>,
    /// The close's deadline, once the thread closes.
    closing_by: Cell<Option<Instant>>,
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

    /// How long a wait that starts now may last; `None` while unbounded.
    fn patience(&self) -> Option<Duration> {
        if !self.bounded.get() {
            return None;
        }
        let reply = REPLY_TIMEOUT;
        Some(match self.closing_by.get() {
            Some(deadline) => reply.min(deadline.saturating_duration_since(Instant::now())),
            None => reply,
        })
    }
}

impl AsFd for ReplyStream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }
}

impl Stream for ReplyStream {
    /// A wait to read (a reply) ends at the deadline. A wait to write, which
    /// x11rb makes as `ReadAndWritable` while it reads what the server sends
    /// meanwhile, is the socket's own: a write that failed there would leave
    /// part of a request sent, and the connection unusable.
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        let Some(patience) = self
            .patience()
            .filter(|_| matches!(mode, PollMode::Readable))
        else {
            return self.socket.poll(mode);
        };
        let timeout = Timespec::try_from(patience)
            .unwrap_or_else(|_| Timespec::try_from(REPLY_TIMEOUT).expect("BUG: it fits"));
        let fd = self.socket.as_fd();
        loop {
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::TimedOut, LATE)),
                Ok(_) => return Ok(()),
                Err(Errno::INTR) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn read(&self, buf: &mut [u8], fd_storage: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.socket.read(buf, fd_storage)
    }

    fn write(&self, buf: &[u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.socket.write(buf, fds)
    }

    fn write_vectored(
        &self,
        bufs: &[IoSlice<'_>],
        fds: &mut Vec<RawFdContainer>,
    ) -> io::Result<usize> {
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

/// Whether `error` is a wait that gave up at its deadline, not a broken
/// connection.
pub(super) fn is_late(error: &ConnectionError) -> bool {
    matches!(error, ConnectionError::IoError(error)
        if error.kind() == io::ErrorKind::TimedOut
            && error.get_ref().is_some_and(|inner| inner.to_string() == LATE))
}

/// The reply to `cookie`; `None` when it did not come in time, the request
/// failed, and its reply, should it come later, is discarded. The one way
/// the clipboard thread waits for a reply.
pub(super) fn reply_within<R: TryParse>(
    connection: &Connection,
    cookie: Cookie<'_, Connection, R>,
) -> Result<Option<R>, ReplyError> {
    let sequence = cookie.sequence_number();
    match cookie.reply() {
        Ok(reply) => Ok(Some(reply)),
        Err(ReplyError::ConnectionError(error)) if is_late(&error) => {
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
