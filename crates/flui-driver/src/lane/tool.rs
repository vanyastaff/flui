//! A host tool a lane drives (`adb`, `xcrun`), run to its end with a deadline and a bound on what
//! it may print.
//!
//! A tool that outlives its deadline is killed and answers [`Failure::Timeout`]; one that prints
//! more than its bound is killed and answers [`Failure::OverLimit`], and nothing past the bound is
//! kept. Its error output is kept up to [`MAX_STDERR`] bytes. What each failure means to the lane
//! (a timeout's `may_have_run` effect, a tool that is not installed) is the lane's to say.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The most bytes of a tool's error output kept: enough for its message.
pub(crate) const MAX_STDERR: usize = 64 * 1024;

/// How long a tool's output readers may take to finish once it ended: a tool that left a child
/// holding its pipes is not waited on past this.
const READERS_WITHIN: Duration = Duration::from_secs(1);

/// How often a running tool is looked at for its end, its deadline and its bound.
const POLL: Duration = Duration::from_millis(5);

/// What a tool that ran to its end answered.
#[derive(Debug)]
pub(crate) struct Finished {
    /// Its exit code; `None` when it ended without one (a signal).
    pub(crate) exit: Option<i32>,
    /// What it printed, at most the bound it ran with.
    pub(crate) stdout: Vec<u8>,
    /// What it printed as errors, at most [`MAX_STDERR`] bytes, bytes that are not UTF-8 replaced.
    pub(crate) stderr: String,
}

/// Why a tool did not run to its end.
#[derive(Debug)]
pub(crate) enum Failure {
    /// It could not be started: not installed, not on the `PATH`.
    Start(io::Error),
    /// Its process could not be watched, or a reader thread not started.
    Io(io::Error),
    /// It outlived its deadline and was killed: it may have done what it was asked.
    Timeout,
    /// It printed more than its bound and was killed.
    OverLimit,
    /// It ended and left its output open past [`READERS_WITHIN`].
    OutputLeftOpen,
}

/// Run `process` to its end within `timeout`, keeping at most `limit` bytes of its output; its
/// input is closed. `name` names the reader threads (`flui-driver-<name>-out`).
pub(crate) fn run(
    mut process: Command,
    limit: usize,
    timeout: Duration,
    name: &str,
) -> Result<Finished, Failure> {
    process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().map_err(Failure::Start)?;
    let stdout = child
        .stdout
        .take()
        .expect("BUG: the tool's output was piped");
    let stderr = child
        .stderr
        .take()
        .expect("BUG: the tool's error output was piped");
    let (out_done, out) = mpsc::channel();
    let (err_done, err) = mpsc::channel();
    let started = thread::Builder::new()
        .name(format!("flui-driver-{name}-out"))
        .spawn(move || {
            let _ = out_done.send(read_bounded(stdout, limit));
        })
        .and_then(|_| {
            thread::Builder::new()
                .name(format!("flui-driver-{name}-err"))
                .spawn(move || {
                    let _ = err_done.send(read_bounded(stderr, MAX_STDERR));
                })
        });
    if let Err(error) = started {
        end(&mut child);
        return Err(Failure::Io(error));
    }
    let deadline = Instant::now().checked_add(timeout);
    let mut early: Option<Bounded> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                end(&mut child);
                return Err(Failure::Io(error));
            }
        }
        // Output past the bound ends the tool at once.
        match out.try_recv() {
            Ok(read) if read.over => {
                early = Some(read);
                end(&mut child);
                break None;
            }
            Ok(read) => early = Some(read),
            Err(_) => {}
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            end(&mut child);
            return Err(Failure::Timeout);
        }
        thread::sleep(POLL);
    };
    let read = match early {
        Some(read) => read,
        None => out
            .recv_timeout(READERS_WITHIN)
            .map_err(|_| Failure::OutputLeftOpen)?,
    };
    if read.over {
        return Err(Failure::OverLimit);
    }
    let stderr = err.recv_timeout(READERS_WITHIN).map_or_else(
        |_| String::new(),
        |read| String::from_utf8_lossy(&read.bytes).into_owned(),
    );
    Ok(Finished {
        exit: status.and_then(|status| status.code()),
        stdout: read.bytes,
        stderr,
    })
}

/// Kill `child` and reap it.
pub(crate) fn end(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// What a reader read: at most its bound, and whether there was more.
struct Bounded {
    bytes: Vec<u8>,
    over: bool,
}

/// Read `from` to its end, keeping at most `limit` bytes; past them, stop reading.
fn read_bounded(mut from: impl Read, limit: usize) -> Bounded {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) | Err(_) => return Bounded { bytes, over: false },
            Ok(read) => {
                if bytes.len() + read > limit {
                    return Bounded { bytes, over: true };
                }
                bytes.extend_from_slice(&buffer[..read]);
            }
        }
    }
}
