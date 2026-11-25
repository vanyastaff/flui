//! Test support shared by the command's unit tests.

use std::io;
use std::time::{Duration, Instant};

/// How often a poll is tried again.
const RETRY: Duration = Duration::from_millis(1);

/// The first result of `op` that is not `WouldBlock`, trying it again until `within` has passed:
/// `None` once it has. A poll of a non-blocking stream or listener in a test.
pub(crate) fn ready_within<T>(
    within: Duration,
    mut op: impl FnMut() -> io::Result<T>,
) -> Option<io::Result<T>> {
    let deadline = Instant::now() + within;
    loop {
        match op() {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(RETRY);
            }
            done => return Some(done),
        }
    }
}

/// [`ready_within`] when the answer must come.
///
/// # Panics
///
/// When `op` is still `WouldBlock` after `within`, naming `what` it waited for.
pub(crate) fn until_ready<T>(
    within: Duration,
    what: &str,
    op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    ready_within(within, op).unwrap_or_else(|| panic!("no {what} within {within:?}"))
}
