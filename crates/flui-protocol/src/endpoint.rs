//! A stream of an agent endpoint on this host, polled: the Windows named pipe or Unix domain
//! socket that the app's agent, the driver and the CLI's own endpoints read and write without
//! blocking (ADR-0238). [`PolledStream`] reads and writes it alike on every host: a read is
//! `Ok(0)` only once the peer has gone and `WouldBlock` while nothing has come, and a write that
//! finds no room is `WouldBlock`.
//!
//! A Unix domain socket behaves so already. A non-blocking Windows named pipe does not, in three
//! ways, and each has its rule here:
//!
//! - It answers a read with nothing to read the way it answers a closed pipe (`ERROR_NO_DATA`,
//!   which reads as the end of the stream). An empty write tells them apart: it fails only on a
//!   closed pipe. A pipe found closed is read once more, so what the peer wrote just before it
//!   closed is read before the end.
//! - It takes a write only whole, into its free buffer or a read the peer has pending, so a write
//!   larger than [`PIPE_WRITE_CHUNK`] would never move while the peer polls instead of blocking
//!   in a read. A write offers at most that much.
//! - A write it does not take returns `Ok(0)`, which reads as `WouldBlock`.
//!
//! The stream is any whose shared reference reads and writes (`&S: Read + Write`), so this
//! crate names no OS type. It is for local endpoints only: a TCP stream (an `adb` forward) ends
//! with `Ok(0)` and is read directly.

use std::io::{self, Read, Write};

/// The most one write offers a Windows named pipe: the buffer the endpoints' listeners leave a
/// pipe at (`interprocess`'s default, 512 bytes, in each direction).
pub const PIPE_WRITE_CHUNK: usize = 512;

/// A local endpoint's stream in non-blocking mode, read and written through
/// `&PolledStream`, as the [module](self) says.
#[derive(Debug)]
pub struct PolledStream<S> {
    stream: S,
}

impl<S> PolledStream<S> {
    /// `stream`, which the caller has put in non-blocking mode.
    #[must_use]
    pub const fn new(stream: S) -> Self {
        Self { stream }
    }
}

impl<S> Read for &PolledStream<S>
where
    for<'a> &'a S: Read + Write,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut stream = &self.stream;
        #[cfg(windows)]
        {
            read_pipe(&mut stream, buf)
        }
        #[cfg(not(windows))]
        {
            stream.read(buf)
        }
    }
}

impl<S> Write for &PolledStream<S>
where
    for<'a> &'a S: Read + Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut stream = &self.stream;
        #[cfg(windows)]
        {
            write_pipe(&mut stream, buf)
        }
        #[cfg(not(windows))]
        {
            stream.write(buf)
        }
    }

    /// Nothing is buffered here. A named pipe's flush waits until the peer has read everything,
    /// which a peer that stops reading never lets happen, so it is not asked for.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A non-blocking Windows named pipe's read, as the [module](self) says.
#[cfg(any(windows, test))]
fn read_pipe(pipe: &mut (impl Read + Write), buf: &mut [u8]) -> io::Result<usize> {
    match pipe.read(buf) {
        Ok(0) if !buf.is_empty() => match pipe.write(&[]) {
            Ok(_) => Err(io::Error::from(io::ErrorKind::WouldBlock)),
            // Closed between the read and the probe: what came before the end is read first.
            Err(_) => pipe.read(buf),
        },
        read => read,
    }
}

/// A non-blocking Windows named pipe's write, as the [module](self) says.
#[cfg(any(windows, test))]
fn write_pipe(pipe: &mut impl Write, buf: &[u8]) -> io::Result<usize> {
    let offered = &buf[..buf.len().min(PIPE_WRITE_CHUNK)];
    match pipe.write(offered) {
        Ok(0) if !offered.is_empty() => Err(io::Error::from(io::ErrorKind::WouldBlock)),
        written => written,
    }
}

/// The pipe rule's table, run on every host; `tests/endpoint.rs` drives [`PolledStream`] itself.
#[cfg(test)]
mod tests {
    use std::io;

    use super::{PIPE_WRITE_CHUNK, read_pipe, write_pipe};
    use crate::testing::{PipeAnswer, ScriptedPipe};

    /// What a read or a write came to: a count, or the kind of its error.
    type Outcome = Result<usize, io::ErrorKind>;

    /// A read case: the buffer's length, the pipe's answers to the reads and to the empty-write
    /// probes, and what the read comes to.
    struct ReadCase {
        case: &'static str,
        buf: usize,
        reads: &'static [PipeAnswer],
        probes: &'static [PipeAnswer],
        read: Outcome,
    }

    /// A read, from the pipe's answers to the read and to each probe after it: nothing has come
    /// while the pipe is open, the end once it is closed, and the bytes the peer wrote just
    /// before it closed; each probe is a write of nothing, and a read into no room probes not
    /// at all. Fails if an empty read is handed on as the end, a pipe found closed drops what
    /// it still holds, a probe writes bytes, or a read into no room probes.
    #[test]
    fn a_pipe_read_tells_nothing_yet_from_the_end() {
        use PipeAnswer::{Bytes, Closed};

        let cases = [
            ReadCase {
                case: "bytes",
                buf: 64,
                reads: &[Bytes(5)],
                probes: &[],
                read: Ok(5),
            },
            ReadCase {
                case: "nothing yet",
                buf: 64,
                reads: &[Bytes(0)],
                probes: &[Bytes(0)],
                read: Err(io::ErrorKind::WouldBlock),
            },
            ReadCase {
                case: "the end",
                buf: 64,
                reads: &[Bytes(0), Bytes(0)],
                probes: &[Closed],
                read: Ok(0),
            },
            ReadCase {
                case: "a line, then the end, between the read and the probe",
                buf: 64,
                reads: &[Bytes(0), Bytes(7)],
                probes: &[Closed],
                read: Ok(7),
            },
            ReadCase {
                case: "a failed read",
                buf: 64,
                reads: &[Closed],
                probes: &[],
                read: Err(io::ErrorKind::BrokenPipe),
            },
            ReadCase {
                case: "no room to read into",
                buf: 0,
                reads: &[Bytes(0)],
                probes: &[],
                read: Ok(0),
            },
        ];
        for ReadCase {
            case,
            buf,
            reads,
            probes,
            read,
        } in cases
        {
            let pipe = ScriptedPipe::new(reads.iter().copied(), probes.iter().copied());
            let outcome = read_pipe(&mut &pipe, &mut vec![0_u8; buf]).map_err(|error| error.kind());
            assert_eq!(outcome, read, "{case}");
            assert!(pipe.is_spent(), "{case}: every scripted call is made");
            assert_eq!(
                pipe.offered(),
                vec![0; probes.len()],
                "{case}: each probe writes nothing"
            );
        }
    }

    /// A write case: how much it offers the pipe, the pipe's answer, what the write comes to and
    /// how much reached the pipe.
    struct WriteCase {
        case: &'static str,
        len: usize,
        answer: PipeAnswer,
        written: Outcome,
        offered: usize,
    }

    /// A write offers the pipe at most its buffer, and one the pipe does not take waits. Fails
    /// if a write offers more than [`PIPE_WRITE_CHUNK`], or hands on a write of nothing.
    #[test]
    fn a_pipe_write_offers_at_most_its_buffer() {
        use PipeAnswer::{Bytes, Closed};

        let cases = [
            WriteCase {
                case: "small, taken",
                len: 100,
                answer: Bytes(100),
                written: Ok(100),
                offered: 100,
            },
            WriteCase {
                case: "large, taken",
                len: 4 * PIPE_WRITE_CHUNK,
                answer: Bytes(PIPE_WRITE_CHUNK),
                written: Ok(PIPE_WRITE_CHUNK),
                offered: PIPE_WRITE_CHUNK,
            },
            WriteCase {
                case: "not taken",
                len: 4 * PIPE_WRITE_CHUNK,
                answer: Bytes(0),
                written: Err(io::ErrorKind::WouldBlock),
                offered: PIPE_WRITE_CHUNK,
            },
            WriteCase {
                case: "empty",
                len: 0,
                answer: Bytes(0),
                written: Ok(0),
                offered: 0,
            },
            WriteCase {
                case: "closed",
                len: 100,
                answer: Closed,
                written: Err(io::ErrorKind::BrokenPipe),
                offered: 100,
            },
        ];
        for WriteCase {
            case,
            len,
            answer,
            written,
            offered,
        } in cases
        {
            let pipe = ScriptedPipe::new([], [answer]);
            let outcome = write_pipe(&mut &pipe, &vec![b'a'; len]).map_err(|error| error.kind());
            assert_eq!(outcome, written, "{case}");
            assert_eq!(pipe.offered(), [offered], "{case}: what the write offered");
        }
    }
}
