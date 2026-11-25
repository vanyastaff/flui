//! The endpoints this command serves itself, private to the current user: the relay that
//! carries a page's agent ([`super::browser::relay`]) and `flui run`'s run endpoint
//! (`run.reload`, ADR-0401). Both poll their clients through [`PolledStream`], which reads and
//! writes a Windows named pipe as a Unix socket reads and writes.

use std::io;
#[cfg(windows)]
use std::time::Duration;

use flui_protocol::endpoint::PolledStream;
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{Listener, ListenerNonblockingMode, ListenerOptions, Stream};

use super::client::name;

/// What an endpoint still owes one client, written from the front as the client takes it. A
/// write advances an offset rather than moving what is left, so a reply a Windows pipe takes
/// [`flui_protocol::endpoint::PIPE_WRITE_CHUNK`] bytes at a time costs work linear in its
/// length. The written front is let go once it is at least as long as what follows it, so what
/// the buffer holds stays within twice what is owed (plus what is being added), whatever the
/// client's pace, and each byte moves at most once for every byte written before it.
#[derive(Debug, Default)]
pub(crate) struct Unwritten {
    bytes: Vec<u8>,
    /// How much of `bytes` is written.
    sent: usize,
}

impl Unwritten {
    /// How many bytes are still owed.
    pub(crate) fn len(&self) -> usize {
        self.bytes.len() - self.sent
    }

    /// Whether nothing is owed.
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Owes `bytes` after what is owed already, letting go of the written front first once it
    /// is at least as long as what is owed.
    pub(crate) fn extend_from_slice(&mut self, bytes: &[u8]) {
        if self.sent > 0 && self.sent >= self.len() {
            self.bytes.drain(..self.sent);
            self.sent = 0;
        }
        self.bytes.extend_from_slice(bytes);
    }

    /// How many bytes the buffer holds, the written front included.
    #[cfg(test)]
    fn held(&self) -> usize {
        self.bytes.len()
    }

    /// Owes nothing more.
    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
        self.sent = 0;
    }

    /// Writes what `stream` takes: `true` once all of it is written, `false` when the stream
    /// has no room.
    ///
    /// # Errors
    ///
    /// The write failed, or the stream took nothing and said so (`WriteZero`).
    pub(crate) fn write_to(&mut self, mut stream: impl io::Write) -> io::Result<bool> {
        while self.sent < self.bytes.len() {
            match stream.write(&self.bytes[self.sent..]) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(written) => self.sent += written,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        self.clear();
        Ok(true)
    }
}

/// `stream`, a client of an endpoint this command serves or a test's client of one, polled.
///
/// # Errors
///
/// The stream could not be put in non-blocking mode.
pub(crate) fn polled(stream: Stream) -> io::Result<PolledStream<Stream>> {
    stream.set_nonblocking(true)?;
    Ok(PolledStream::new(stream))
}

/// The session's endpoint, private to the current user: a socket in the session's `0700`
/// directory, or a pipe whose DACL grants the current user alone.
#[cfg(unix)]
pub(crate) fn bind(endpoint: &str) -> io::Result<Listener> {
    ListenerOptions::new()
        .name(name(endpoint)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .create_sync()
}

#[cfg(windows)]
pub(crate) fn bind(endpoint: &str) -> io::Result<Listener> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt as _;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;

    let sddl = widestring::U16CString::from_str(format!("D:P(A;;GA;;;{})", current_user_sid()?))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    ListenerOptions::new()
        .name(name(endpoint)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .security_descriptor(descriptor)
        .create_sync()
}

/// The current user's SID, as `whoami` gives it: the user is named by SID rather than as the
/// pipe's owner, which is the Administrators group for an elevated process.
#[cfg(windows)]
fn current_user_sid() -> io::Result<String> {
    let output = crate::proc::output_with_timeout(
        std::process::Command::new("whoami").arg("/user"),
        Duration::from_secs(10),
    )?;
    sid_in_whoami(&String::from_utf8_lossy(&output.stdout))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("`whoami /user` named no SID"))
}

/// The SID in `whoami /user`'s table, whose last row is `machine\user  S-1-5-21-…`. The headings
/// are in the system's language and a user's name may hold spaces, so the SID is the last word
/// that starts as one does.
#[cfg(any(windows, test))]
pub(crate) fn sid_in_whoami(table: &str) -> Option<&str> {
    table
        .split_whitespace()
        .rev()
        .find(|word| word.starts_with("S-1-"))
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read as _, Write as _};
    use std::time::Duration;

    use flui_protocol::endpoint::{PIPE_WRITE_CHUNK, PolledStream};
    use interprocess::local_socket::traits::{Listener as _, Stream as _};
    use interprocess::local_socket::{Listener, Stream};

    use super::{Unwritten, bind, polled};
    use crate::agent::client::name;
    use crate::testing::until_ready;

    /// How long a test waits for the other end.
    const WAIT: Duration = Duration::from_secs(10);

    /// Takes at most a pipe's chunk per write, and nothing once `room` is spent.
    struct Chunked {
        room: usize,
        taken: Vec<u8>,
    }

    impl io::Write for Chunked {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let count = buf.len().min(PIPE_WRITE_CHUNK).min(self.room);
            if count == 0 {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            self.room -= count;
            self.taken.extend_from_slice(&buf[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A client that takes most of each reply but never catches up is owed its bytes in order,
    /// and the buffer holds at most twice what is owed after each reply is added. Fails if the
    /// written front is kept until the client catches up, which a client that stays behind never
    /// does, so the buffer grows for the life of the connection.
    #[test]
    fn what_a_client_that_stays_behind_is_owed_stays_bounded_and_in_order() {
        const REPLY: usize = 2 * PIPE_WRITE_CHUNK;
        let mut owed = Unwritten::default();
        let mut client = Chunked {
            room: 0,
            taken: Vec::new(),
        };
        let mut sent = Vec::new();
        for round in 0..256_usize {
            let reply = (0..REPLY)
                .map(|i| u8::try_from((round * 7 + i) % 251).expect("BUG: below 251"))
                .collect::<Vec<u8>>();
            owed.extend_from_slice(&reply);
            sent.extend_from_slice(&reply);
            assert!(
                owed.held() <= 2 * owed.len(),
                "round {round}: {} bytes held for {} owed",
                owed.held(),
                owed.len()
            );
            // The client takes all of each reply but a few bytes.
            client.room = REPLY - 24;
            assert!(
                !owed.write_to(&mut client).expect("a write"),
                "round {round}"
            );
        }
        client.room = usize::MAX;
        assert!(
            owed.write_to(&mut client).expect("a write"),
            "all of it is written"
        );
        assert!(owed.is_empty());
        assert_eq!(client.taken, sent, "written whole and in order");
    }

    /// A write that takes nothing is `WriteZero`, an interrupted one is made again, and a failed
    /// one is handed on with what was written kept. Fails if one of the three answers is taken
    /// for another.
    #[test]
    fn what_is_owed_is_written_as_the_stream_answers() {
        use flui_protocol::testing::{PipeAnswer, ScriptedPipe};

        let line = b"0123456789";
        let written_as = |answers: Vec<PipeAnswer>| {
            let mut owed = Unwritten::default();
            owed.extend_from_slice(line);
            let pipe = ScriptedPipe::new([], answers);
            let outcome = owed.write_to(&pipe).map_err(|error| error.kind());
            assert!(pipe.is_spent(), "every scripted write is made");
            (outcome, owed.len())
        };
        assert_eq!(
            written_as(vec![PipeAnswer::Bytes(0)]),
            (Err(io::ErrorKind::WriteZero), line.len()),
            "a write that takes nothing"
        );
        assert_eq!(
            written_as(vec![PipeAnswer::Interrupted, PipeAnswer::Bytes(line.len())]),
            (Ok(true), 0),
            "an interrupted write, made again"
        );
        assert_eq!(
            written_as(vec![PipeAnswer::Bytes(4), PipeAnswer::Closed]),
            (Err(io::ErrorKind::BrokenPipe), line.len() - 4),
            "a failed write after a partial one"
        );
        assert_eq!(
            written_as(vec![PipeAnswer::Bytes(4), PipeAnswer::WouldBlock]),
            (Ok(false), line.len() - 4),
            "no room after a partial write"
        );
    }

    /// A private endpoint, its listener, and one client connected to it with the server's end
    /// accepted.
    fn connected() -> (tempfile::TempDir, Listener, Stream, PolledStream<Stream>) {
        let (dir, address) = crate::agent::private_endpoint("polled").expect("an endpoint");
        let listener = bind(&address).expect("bind");
        let client = Stream::connect(name(&address).expect("a name")).expect("connect");
        let served = until_ready(WAIT, "accepted client", || listener.accept()).expect("accept");
        (dir, listener, client, polled(served).expect("non-blocking"))
    }

    /// A connection the peer keeps open but sends nothing on reads as waiting, what the peer
    /// sends reads as itself, and the peer gone reads as the end. Fails on Windows if an empty
    /// read of an open pipe is taken for its end.
    #[test]
    fn a_quiet_connection_waits_and_a_closed_one_ends() {
        let (_dir, _listener, client, served) = connected();
        let mut chunk = [0_u8; 64];
        let read = (&served).read(&mut chunk).map_err(|error| error.kind());
        assert_eq!(read, Err(io::ErrorKind::WouldBlock), "nothing has come yet");

        (&client).write_all(b"hello\n").expect("the client writes");
        let read = until_ready(WAIT, "line", || (&served).read(&mut chunk)).expect("a read");
        assert_eq!(&chunk[..read], b"hello\n");

        drop(client);
        let end = until_ready(WAIT, "end", || (&served).read(&mut chunk));
        assert_eq!(
            end.map_err(|error| error.kind()),
            Ok(0),
            "the end reads as the end"
        );
    }

    /// A reply many times the pipe's buffer reaches a client that polls instead of blocking in
    /// a read, and starts reading only after the write found no room. Fails on Windows if a
    /// write offers more than the pipe takes whole, which never moves while the client polls,
    /// or hands on a write that took nothing.
    #[test]
    fn a_reply_larger_than_the_buffer_reaches_a_client_that_polls() {
        let (_dir, _listener, client, served) = connected();
        let client = polled(client).expect("non-blocking");
        // Past what any host buffers for an unread socket or pipe.
        let reply = (0..8192 * PIPE_WRITE_CHUNK)
            .map(|i| u8::try_from(i % 251).expect("BUG: below 251"))
            .collect::<Vec<u8>>();

        // Nobody reads yet: the writes fill the buffer until one finds no room.
        let mut written = 0;
        loop {
            match (&served).write(&reply[written..]) {
                Ok(0) => panic!("a write took nothing and said so"),
                Ok(count) => written += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("the write failed: {error}"),
            }
            assert!(written < reply.len(), "the whole reply fit an unread pipe");
        }

        // Then the client polls a little at a time, and each turn offers the rest again.
        let mut read = Vec::with_capacity(reply.len());
        let mut chunk = [0_u8; 1024];
        while read.len() < reply.len() {
            if written < reply.len() {
                match (&served).write(&reply[written..]) {
                    Ok(0) => panic!("a write took nothing and said so"),
                    Ok(count) => written += count,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("the write failed: {error}"),
                }
            }
            let count = until_ready(WAIT, "reply bytes", || (&client).read(&mut chunk))
                .expect("the client reads");
            assert!(count > 0, "the reply ended after {} bytes", read.len());
            read.extend_from_slice(&chunk[..count]);
        }
        assert_eq!(read, reply, "the client reads the reply whole and in order");
    }
}
