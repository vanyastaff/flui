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
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: post-frame callbacks run contained so the remaining ones are kept"
        )]
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::debug!(
                total_callbacks = callbacks.len(),
                "draining post-frame callback batch"
            );
            for id in callbacks.by_ref() {
                if let Some(entry) = owner.take_active_post_frame(id) {
                    (entry.callback)(timing);
                }
            }
        }));
        // Keep both a panic tail and entries whose presentation became
        // incomplete reentrantly after the eligible batch was selected.
        owner.restore_post_frame_queue();
        owner.finish_post_frame_batch();
        // The tail behind a failed callback still needs a frame, and the
        // caller is about to resume the panic rather than ask for one.
        if result.is_err() && owner.has_queued_post_frame() {
            super::request_frame_impl_preserving_failure(&self.inner.wake, true);
        }
        result
    }
}
