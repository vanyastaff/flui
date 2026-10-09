//! Scheduler identity, and the single owner-frame slot an [OwnerFrame](crate::OwnerFrame)
//! claims.

use std::rc::Rc;
use std::sync::atomic::Ordering;

use super::UpdateScheduler;

impl UpdateScheduler {
    /// Whether `self` and `other` are clones of the **same** scheduler — the same
    /// callback queues, the same frame.
    ///
    /// `UpdateScheduler` is `Arc`-backed, so this is pointer identity on the shared
    /// inner state. It exists because a capability handed to a widget must be
    /// provably pointed at the UI runtime's own scheduler, not some other one.
    #[must_use]
    pub fn is_same_instance(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Claim this scheduler's single owner-frame slot; `false` if a live
    /// [`OwnerFrame`](crate::OwnerFrame) already holds it.
    pub(crate) fn claim_owner_frame(&self) -> bool {
        self.inner
            .owner_frame_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Release the slot [`claim_owner_frame`](Self::claim_owner_frame) took.
    pub(crate) fn release_owner_frame(&self) {
        self.inner
            .owner_frame_claimed
            .store(false, Ordering::Release);
    }

    /// A stable, opaque identity for tracing/diagnostics only.
    ///
    /// Never used for equality — [`is_same_instance`](Self::is_same_instance)
    /// owns that comparison. This exists so a scheduler-mismatch trace (e.g.
    /// driving an [`OwnerFrame`](crate::OwnerFrame) with the wrong
    /// `UpdateScheduler`) can name both sides without printing the whole
    /// internal state the `Debug` impl shows.
    pub(crate) fn debug_ptr(&self) -> usize {
        Rc::as_ptr(&self.inner) as usize
    }
}
