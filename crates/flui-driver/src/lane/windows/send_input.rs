//! What a `SendInput` call answers: how many of the events it was handed Windows inserted, read
//! as all, some or none. Built on every host, so its table runs on Linux CI; the call itself is in
//! the `cfg(windows)` binding.

use crate::error::Error;
use crate::lane::SendFailure;
use crate::lane::windows::safety::LANE;

/// What a `SendInput` call that inserted `inserted` of its `total` events answers: one that
/// inserted them all succeeds, and one that inserted only some fails as partly sent, since the
/// events it did insert reached the desktop.
///
/// # Errors
///
/// [`Error::Lane`] naming `what` when Windows took fewer than `total`, partly sent when it took
/// any.
pub fn outcome(inserted: u32, total: usize, what: &str) -> Result<(), SendFailure> {
    if usize::try_from(inserted).ok() == Some(total) {
        return Ok(());
    }
    let error = Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: format!(
            "Windows took {inserted} of the {total} events of {what} (blocked: the app may run \
             elevated, or the desktop changed)"
        ),
    };
    Err(if inserted > 0 {
        SendFailure::partly_sent(error)
    } else {
        SendFailure::from(error)
    })
}
