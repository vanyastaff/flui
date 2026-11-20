//! How long a lane waits, in the slices it can watch its guard between: the pause of a sequence
//! and the echo of the lane's own first event.

use std::time::Duration;

/// How long a pause waits between two looks at the guard: a pause is waited out whole, never
/// cut short, and the person's input during it ends the run before its next event.
pub const WATCH_SLICE: Duration = Duration::from_millis(50);

/// How long a lane waits for its guard to see an event it sent. A guard that stopped seeing the
/// lane's own input (a Windows hook the OS removed for answering too slowly, an X server that
/// stopped delivering) can no longer tell the person's input from the lane's.
pub const ECHO_WITHIN: Duration = Duration::from_millis(500);

/// `pause` as the slices it is waited out in, each at most [`WATCH_SLICE`], adding up to it.
pub fn slices(pause: Duration) -> impl Iterator<Item = Duration> {
    let mut left = pause;
    std::iter::from_fn(move || {
        if left.is_zero() {
            return None;
        }
        let slice = left.min(WATCH_SLICE);
        left -= slice;
        Some(slice)
    })
}
