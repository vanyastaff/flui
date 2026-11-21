//! The `eventfd` that wakes the owner thread from `poll`.

use std::os::fd::OwnedFd;

use rustix::event::{EventfdFlags, eventfd};
use rustix::io::Errno;

/// What wakes the owner thread from `poll`: an `eventfd` any thread
/// writes.
pub(crate) struct LoopWaker(pub(super) OwnedFd);

impl LoopWaker {
    pub(super) fn try_new() -> rustix::io::Result<Self> {
        eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map(Self)
    }

    /// Wake the owner thread. A counter already at its limit has a wake
    /// pending, which is all a write would add.
    pub(crate) fn wake(&self) {
        match rustix::io::write(&self.0, &1_u64.to_ne_bytes()) {
            Ok(_) | Err(Errno::AGAIN) => {}
            Err(error) => tracing::error!(%error, "the Wayland loop could not be woken"),
        }
    }

    /// Clear the wakes that were counted.
    pub(super) fn drain(&self) {
        let mut count = [0_u8; 8];
        // Empty already (`AGAIN`) is what a drain wants.
        let _ = rustix::io::read(&self.0, &mut count);
    }
}
