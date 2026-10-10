//! Post-frame admission, eligible delivery and panic-tail recovery in the owner's queue.

use super::UpdateScheduler;
use crate::{FrameTiming, OwnerFrame, PostFrameCallback};

impl UpdateScheduler {
    /// Add a post-frame callback.
    ///
    /// Fires once after the current/next host frame completes, independently
    /// of presentation-scoped geometry completion.
    ///
    /// Post-frame callbacks are called exactly once and cannot be
    /// cancelled before they fire. Returns `()` — no cancellation handle.
    pub fn add_post_frame_callback(&self, callback: PostFrameCallback) {
        let _ = crate::PostFrameHandle::new(self).schedule(callback);
    }

    pub(super) fn dispatch_post_frame_callbacks(
        &self,
        owner: &OwnerFrame,
        timing: &FrameTiming,
    ) -> std::thread::Result<()> {
        let Ok(mut callbacks) = owner.take_post_frame_queue_for(self) else {
            return Ok(());
        };
        callbacks.sort_unstable();
        let mut callbacks = callbacks.into_iter();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::debug!(
                total_callbacks = callbacks.len(),
                "draining post-frame callback batch"
            );
            for id in callbacks.by_ref() {
                if let Some(entry) = owner.take_active_post_frame(id) {
                    let cancelled = self.inner.callbacks.cancelled.borrow_mut().remove(&id);
                    if !cancelled {
                        (entry.callback)(timing);
                    }
                }
            }
        }));
        // Keep both a panic tail and entries whose presentation became
        // incomplete reentrantly after the eligible batch was selected.
        // Their cancellation records survive until an entry is consumed.
        owner.restore_post_frame_queue();
        owner.finish_post_frame_batch();
        self.retain_pending_post_frame_cancellations();
        result
    }

    pub(super) fn retain_pending_post_frame_cancellations(&self) {
        let lane = self.inner.callbacks.post_frame.borrow().lane();
        self.inner
            .callbacks
            .cancelled
            .borrow_mut()
            .retain(|id| lane.as_ref().is_some_and(|lane| lane.contains(*id)));
    }
}
