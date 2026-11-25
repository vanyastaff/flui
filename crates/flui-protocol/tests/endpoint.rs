//! `PolledStream` through its `Read` and `Write`, over a scripted pipe: the Windows pipe rule
//! where it is built, the stream's own reads and writes elsewhere. On every host a flush asks
//! nothing of the stream, since a pipe's flush waits until its peer has read everything.

use std::io::{Read as _, Write as _};

use flui_protocol::endpoint::{PIPE_WRITE_CHUNK, PolledStream};
use flui_protocol::testing::{PipeAnswer, ScriptedPipe};

/// A quiet pipe reads as nothing yet, by an empty write; a reply larger than the pipe's buffer
/// is offered a chunk at a time; a write the pipe does not take waits; a flush reaches no pipe.
/// Fails if the dispatch to the pipe rule is lost on Windows, or a flush reaches the pipe.
#[cfg(windows)]
#[test]
fn a_polled_stream_reads_and_writes_a_windows_pipe_by_its_rule() {
    use std::io::ErrorKind;

    let pipe = ScriptedPipe::new(
        [PipeAnswer::Bytes(0)],
        [
            PipeAnswer::Bytes(0),
            PipeAnswer::Bytes(PIPE_WRITE_CHUNK),
            PipeAnswer::Bytes(0),
        ],
    );
    let polled = PolledStream::new(pipe.clone());
    let read = (&polled)
        .read(&mut [0_u8; 64])
        .map_err(|error| error.kind());
    assert_eq!(read, Err(ErrorKind::WouldBlock), "nothing yet");
    let reply = vec![b'a'; 4 * PIPE_WRITE_CHUNK];
    assert_eq!((&polled).write(&reply).ok(), Some(PIPE_WRITE_CHUNK));
    let refused = (&polled).write(&reply).map_err(|error| error.kind());
    assert_eq!(refused, Err(ErrorKind::WouldBlock), "no room");
    (&polled).flush().expect("a flush succeeds");
    assert_eq!(pipe.flushes(), 0, "a flush reaches no pipe");
    assert_eq!(pipe.offered(), [0, PIPE_WRITE_CHUNK, PIPE_WRITE_CHUNK]);
    assert!(pipe.is_spent());
}

/// Off Windows the stream's own reads and writes go through unchanged: an empty read is the
/// end, with no probe, and a write offers the whole buffer; a flush reaches no socket. Fails if
/// the pipe rule reaches a Unix socket, where an empty write to a closed peer need not fail, or
/// a flush reaches the socket.
#[cfg(not(windows))]
#[test]
fn a_polled_stream_passes_a_unix_socket_through() {
    let pipe = ScriptedPipe::new(
        [PipeAnswer::Bytes(0)],
        [PipeAnswer::Bytes(4 * PIPE_WRITE_CHUNK)],
    );
    let polled = PolledStream::new(pipe.clone());
    let read = (&polled)
        .read(&mut [0_u8; 64])
        .map_err(|error| error.kind());
    assert_eq!(read, Ok(0), "the end, unprobed");
    let reply = vec![b'a'; 4 * PIPE_WRITE_CHUNK];
    assert_eq!((&polled).write(&reply).ok(), Some(4 * PIPE_WRITE_CHUNK));
    (&polled).flush().expect("a flush succeeds");
    assert_eq!(pipe.flushes(), 0, "a flush reaches no socket");
    assert_eq!(pipe.offered(), [4 * PIPE_WRITE_CHUNK]);
    assert!(pipe.is_spent());
}
