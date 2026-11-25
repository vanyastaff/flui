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

    use super::{bind, polled};
    use crate::agent::client::name;
    use crate::testing::until_ready;

    /// How long a test waits for the other end.
    const WAIT: Duration = Duration::from_secs(10);

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
