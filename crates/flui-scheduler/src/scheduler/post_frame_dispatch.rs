//! Completed-frame callbacks and panic-tail recovery in the owner's queue.

use super::UpdateScheduler;
use crate::{FrameTiming, OwnerFrame};

impl UpdateScheduler {
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
                let cancelled = self.inner.callbacks.cancelled.borrow().contains(&id);
                if let Some(entry) = owner.take_active_post_frame(id)
                    && !cancelled
                {
                    (entry.callback)(timing);
                }
            }
        }));
        if result.is_err() {
            // The uninvoked tail retains its IDs ahead of reentrant admissions.
            owner.restore_post_frame_queue();
        } else {
            self.inner.callbacks.cancelled.borrow_mut().clear();
        }
        result
    }
}
