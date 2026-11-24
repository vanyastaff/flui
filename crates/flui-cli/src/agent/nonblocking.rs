//! A local-socket stream in non-blocking mode that reads and writes alike on every host: a read
//! is `Ok(0)` only once the peer has gone and `WouldBlock` while nothing has come, and a write
//! that finds no room is `WouldBlock`. The endpoints this command serves poll their clients
//! through it ([`super::browser::relay`], [`super::supervisor`]).
//!
//! A Unix domain socket behaves so already. A non-blocking Windows named pipe does not: it
//! answers a read with nothing to read the way it answers a closed pipe (`ERROR_NO_DATA`, which
//! reads as the end of the stream), and a write that finds the pipe's buffer full with `Ok(0)`.
//! So on Windows an empty read is told apart by an empty write, which fails only on a closed
//! pipe, and a write that took nothing is `WouldBlock`.

use std::io::{self, Read, Write};

use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;

/// A local-socket stream, non-blocking, read and written through `&NonblockingStream`.
#[derive(Debug)]
pub(crate) struct NonblockingStream(Stream);

impl NonblockingStream {
    /// Puts `stream` in non-blocking mode.
    ///
    /// # Errors
    ///
    /// The mode could not be set.
    pub(crate) fn try_new(stream: Stream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self(stream))
    }
}

impl Read for &NonblockingStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match (&self.0).read(buf) {
            #[cfg(windows)]
            Ok(0) if !buf.is_empty() => match (&self.0).write(&[]) {
                Ok(_) => Err(io::ErrorKind::WouldBlock.into()),
                Err(_) => Ok(0),
            },
            read => read,
        }
    }
}

impl Write for &NonblockingStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match (&self.0).write(buf) {
            #[cfg(windows)]
            Ok(0) if !buf.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            written => written,
        }
    }

    /// Nothing is buffered here. A named pipe's flush waits until the peer has read everything,
    /// which a peer that stops reading never lets happen, so it is not asked for.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read as _, Write as _};
    use std::time::{Duration, Instant};

    use interprocess::local_socket::traits::{Listener as _, Stream as _};
    use interprocess::local_socket::{Listener, Stream};

    use super::NonblockingStream;
    use crate::agent::client::name;
    use crate::agent::listener::bind;

    /// How long a test waits for the other end.
    const WAIT: Duration = Duration::from_secs(10);

    /// A private endpoint, its listener, and one client connected to it with the server's end
    /// accepted.
    fn connected() -> (tempfile::TempDir, Listener, Stream, NonblockingStream) {
        let (dir, address) = crate::agent::private_endpoint("nonblocking").expect("an endpoint");
        let listener = bind(&address).expect("bind");
        let client = Stream::connect(name(&address).expect("a name")).expect("connect");
        let deadline = Instant::now() + WAIT;
        let served = loop {
            match listener.accept() {
                Ok(stream) => break NonblockingStream::try_new(stream).expect("non-blocking"),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "the client is accepted");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        (dir, listener, client, served)
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
        let deadline = Instant::now() + WAIT;
        let read = loop {
            match (&served).read(&mut chunk) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "the line arrives");
                    std::thread::sleep(Duration::from_millis(5));
                }
                read => break read.expect("a read"),
            }
        };
        assert_eq!(&chunk[..read], b"hello\n");

        drop(client);
        let deadline = Instant::now() + WAIT;
        loop {
            match (&served).read(&mut chunk) {
                Ok(0) => break,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "the end arrives");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("the end reads as an error: {error}"),
                Ok(read) => panic!("{read} bytes from a client that sent nothing more"),
            }
        }
    }

    /// A reply larger than the connection's buffer goes through whole as the peer reads it: a
    /// write that finds the buffer full waits instead of reading as a write of nothing. Fails on
    /// Windows if a full named pipe's `Ok(0)` is handed on, which the endpoints take for a peer
    /// that will never read.
    #[test]
    fn a_reply_larger_than_the_buffer_goes_through_as_the_peer_reads() {
        let (_dir, _listener, client, served) = connected();
        let reply = (0..1 << 20).map(|i| (i % 251) as u8).collect::<Vec<u8>>();
        let expected = reply.clone();
        let reader = std::thread::spawn(move || {
            let mut read = Vec::with_capacity(expected.len());
            let mut chunk = [0_u8; 4 << 10];
            while read.len() < expected.len() {
                match (&client).read(&mut chunk)? {
                    0 => break,
                    count => read.extend_from_slice(&chunk[..count]),
                }
            }
            io::Result::Ok(read == expected)
        });
        let deadline = Instant::now() + WAIT;
        let mut written = 0;
        while written < reply.len() {
            match (&served).write(&reply[written..]) {
                Ok(0) => panic!("a write took nothing and said so"),
                Ok(count) => written += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "{written} of {} bytes written",
                        reply.len()
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("the write failed: {error}"),
            }
        }
        assert!(
            reader
                .join()
                .expect("the reader")
                .expect("the client reads"),
            "the client reads the reply whole and in order"
        );
    }
}
