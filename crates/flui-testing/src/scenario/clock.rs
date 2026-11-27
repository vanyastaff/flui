//! [`VirtualClock`]: a target whose clock the test moves (ADR-0400 §1).

use std::time::Duration;

use flui_protocol::client::CallError;

/// A target whose clock the test moves (ADR-0400 §1).
///
/// It is not a [`Connection`](flui_protocol::client::Connection) bound: a device's clock is the
/// wall's, so a scenario that pumps, settles or freezes a frame compiles only for a target that
/// has one.
pub trait VirtualClock {
    /// Move the clock by `by`, running the frames due on the way.
    ///
    /// # Errors
    ///
    /// [`CallError`] once the target can no longer run frames, as after the end of the test.
    fn advance(&mut self, by: Duration) -> Result<Advanced, CallError>;
}

/// What moving a virtual clock ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Advanced {
    /// The frames presented on the way.
    pub frames: u64,
}
