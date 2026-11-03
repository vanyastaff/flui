//! A transfer's bytes through the pipe its reader and writer share, bounded
//! in time and size: what the threads of a read and a write run, for the
//! selection and a drag alike, and how many writes run at once.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use web_time::Instant;

/// Why a read ended without the writer's whole answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipeError {
    /// The writer was still writing at the deadline.
    Timeout,
    /// The writer wrote past the limit: at least `read` bytes.
    TooLarge { read: usize },
    /// The read failed.
    Failed,
}

/// The writes running now, at most the number they were made with.
#[derive(Debug)]
pub(crate) struct Writers {
    running: Arc<AtomicUsize>,
    most: usize,
}

/// One write's place among [`Writers`], given back when dropped.
#[derive(Debug)]
pub(crate) struct Permit(Arc<AtomicUsize>);

impl Writers {
    /// At most `most` writes at once.
    pub(crate) fn new(most: usize) -> Self {
        Self {
            running: Arc::default(),
            most,
        }
    }

    /// A place for one more write; `None` while the most run.
    pub(crate) fn permit(&self) -> Option<Permit> {
        self.running
            .try_update(Ordering::AcqRel, Ordering::Acquire, |running| {
                (running < self.most).then_some(running + 1)
            })
            .ok()
            .map(|_| Permit(Arc::clone(&self.running)))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Wait until `fd` is ready for `flags` or `deadline` passes; `false` on the
/// deadline or a failed poll.
fn ready(fd: &OwnedFd, flags: PollFlags, deadline: Instant) -> bool {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        let timeout = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: left.subsec_nanos().into(),
        };
        let mut descriptors = [PollFd::new(fd, flags)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => return false,
            Ok(_) => return true,
            Err(Errno::INTR) => {}
            Err(_) => return false,
        }
    }
}

/// Everything the writer writes to `reader` until it closes its end, at
/// most `limit` bytes: [`PipeError::Timeout`] when it is still writing at
/// `deadline`, [`PipeError::TooLarge`] past `limit`, [`PipeError::Failed`]
/// when the read fails.
pub(crate) fn read(reader: OwnedFd, deadline: Instant, limit: usize) -> Result<Vec<u8>, PipeError> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        if !ready(&reader, PollFlags::IN, deadline) {
            return Err(PipeError::Timeout);
        }
        match rustix::io::read(&reader, &mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(count) => {
                let read = bytes.len().saturating_add(count);
                if read > limit {
                    return Err(PipeError::TooLarge { read });
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
            Err(Errno::INTR | Errno::AGAIN) => {}
            Err(error) => {
                tracing::debug!(%error, "a Wayland transfer's read failed");
                return Err(PipeError::Failed);
            }
        }
    }
}

/// Write `bytes` to `writer` and close it; stop at `deadline` or when the
/// reader closed its end.
pub(crate) fn write(writer: OwnedFd, bytes: &[u8], deadline: Instant) {
    // A full pipe answers `AGAIN` instead of holding the thread past its
    // deadline.
    if let Err(error) = rustix::fs::fcntl_setfl(&writer, rustix::fs::OFlags::NONBLOCK) {
        tracing::debug!(%error, "a Wayland transfer's write could not be made non-blocking");
        return;
    }
    let mut left = bytes;
    while !left.is_empty() {
        if !ready(&writer, PollFlags::OUT, deadline) {
            tracing::debug!("a Wayland transfer's write timed out");
            return;
        }
        // A reader gone answers `PIPE`: Rust's runtime ignores `SIGPIPE`, so
        // the write fails instead of ending the process.
        match rustix::io::write(&writer, left) {
            Ok(count) => left = &left[count..],
            Err(Errno::INTR | Errno::AGAIN) => {}
            Err(error) => {
                tracing::debug!(%error, "a Wayland transfer's write ended early");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// At most the writes a [`Writers`] was made with run at once; each that
    /// ends gives its place to the next.
    #[test]
    fn writes_past_the_limit_wait_for_a_place() {
        let writers = Writers::new(4);
        let mut held: Vec<Permit> = (0..4)
            .map(|_| writers.permit().expect("a place below the limit"))
            .collect();
        assert!(writers.permit().is_none(), "the limit is full");
        drop(held.pop().expect("a held place"));
        assert!(writers.permit().is_some(), "an ended write frees its place");
    }

    fn pipe() -> (OwnedFd, OwnedFd) {
        rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).expect("a pipe")
    }

    /// What one end writes the other reads whole, past the pipe's own buffer,
    /// once the writer closes its end; past the limit it is refused.
    #[test]
    fn a_transfer_crosses_the_pipe_whole_up_to_its_limit() {
        let text = "selection ".repeat(100_000).into_bytes();
        let deadline = Instant::now() + Duration::from_secs(5);
        for (limit, expected) in [
            (text.len(), Ok(text.clone())),
            (
                text.len() - 1,
                Err(PipeError::TooLarge { read: text.len() }),
            ),
        ] {
            let (reader, writer) = pipe();
            let sent = text.clone();
            let writing = std::thread::spawn(move || write(writer, &sent, deadline));
            match (read(reader, deadline, limit), expected) {
                (Err(PipeError::TooLarge { read }), Err(PipeError::TooLarge { .. })) => {
                    assert!(read > limit, "{read} bytes read past {limit}");
                }
                (read, expected) => assert_eq!(read, expected, "limit {limit}"),
            }
            writing.join().expect("the writer ends");
        }
    }

    /// A writer that never closes its end answers nothing once the deadline
    /// passes; a reader gone ends the write at once.
    #[test]
    fn a_transfer_ends_at_its_deadline_or_its_peer() {
        let (reader, writer) = pipe();
        let started = Instant::now();
        assert_eq!(
            read(reader, started + Duration::from_millis(50), usize::MAX),
            Err(PipeError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(writer);

        let (reader, writer) = pipe();
        drop(reader);
        let started = Instant::now();
        write(writer, b"nobody reads", started + Duration::from_secs(5));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a closed reader ends the write before its deadline"
        );
    }
}
