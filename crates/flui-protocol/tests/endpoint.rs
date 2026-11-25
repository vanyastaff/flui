//! `PolledStream` through its `Read` and `Write`, over a scripted pipe: the Windows pipe rule
//! where it is built, the stream's own reads and writes elsewhere.

use std::io::{self, Read as _, Write as _};

use flui_protocol::endpoint::{PIPE_WRITE_CHUNK, PolledStream};
use flui_protocol::testing::{PipeAnswer, ScriptedPipe};

/// A quiet pipe reads as nothing yet, by an empty write; a reply larger than the pipe's buffer
/// is offered a chunk at a time; a write the pipe does not take waits. Fails if the dispatch to
/// the pipe rule is lost on Windows.
#[cfg(windows)]
#[test]
fn a_polled_stream_reads_and_writes_a_windows_pipe_by_its_rule() {
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
    assert_eq!(read, Err(io::ErrorKind::WouldBlock), "nothing yet");
    let reply = vec![b'a'; 4 * PIPE_WRITE_CHUNK];
    assert_eq!((&polled).write(&reply).ok(), Some(PIPE_WRITE_CHUNK));
    let refused = (&polled).write(&reply).map_err(|error| error.kind());
    assert_eq!(refused, Err(io::ErrorKind::WouldBlock), "no room");
    (&polled).flush().expect("a flush writes nothing");
    assert_eq!(pipe.offered(), [0, PIPE_WRITE_CHUNK, PIPE_WRITE_CHUNK]);
    assert!(pipe.is_spent());
}

/// Off Windows the stream's own reads and writes go through unchanged: an empty read is the
/// end, with no probe, and a write offers the whole buffer. Fails if the pipe rule reaches a
/// Unix socket, where an empty write to a closed peer need not fail.
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
    (&polled).flush().expect("a flush writes nothing");
    assert_eq!(pipe.offered(), [4 * PIPE_WRITE_CHUNK]);
    assert!(pipe.is_spent());
}
