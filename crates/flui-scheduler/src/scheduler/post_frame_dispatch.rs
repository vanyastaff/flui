//! Completed-frame callback snapshots and panic-tail recovery.

use super::{CancellablePostFrameCallback, UpdateScheduler};
use crate::post_frame::LocalPostFrameEntry;
use crate::{CallbackId, FrameTiming, LocalPostFrameLane};

/// Preserve queue provenance so an uninvoked panic tail can return to its owner.
enum PendingPostFrame {
    Shared(CancellablePostFrameCallback),
    Local(LocalPostFrameEntry),
}

impl PendingPostFrame {
    fn id(&self) -> CallbackId {
        match self {
            Self::Shared(entry) => entry.id,
            Self::Local(entry) => entry.id,
        }
    }

    fn invoke(self, timing: &FrameTiming) {
        match self {
            Self::Shared(entry) => (entry.callback)(timing),
            Self::Local(entry) => (entry.callback)(timing),
        }
    }
}

impl UpdateScheduler {
    pub(super) fn dispatch_post_frame_callbacks(
        &self,
        lane: Option<&LocalPostFrameLane>,
        timing: &FrameTiming,
    ) -> std::thread::Result<()> {
        let (mut callbacks, shared_callbacks, local_callbacks) = {
            let _registration = self.inner.callbacks.post_frame_registration.lock();
            let mut cbs = self.inner.callbacks.post_frame.lock();
            let mut snapshot: Vec<_> = cbs.drain(..).map(PendingPostFrame::Shared).collect();
            let shared_callbacks = snapshot.len();
            let mut local_callbacks = 0;
            if let Some(lane) = lane
                && let Ok(local_entries) = lane.take_queue_for(self)
            {
                local_callbacks = local_entries.len();
                snapshot.extend(local_entries.into_iter().map(PendingPostFrame::Local));
            }
            (snapshot, shared_callbacks, local_callbacks)
        };
        callbacks.sort_unstable_by_key(|entry| entry.id().get());
        // Keep the iterator outside the unwind boundary: the panicking
        // entry is consumed, but its uninvoked siblings remain owned here.
        let mut callbacks = callbacks.into_iter();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // A subscriber is arbitrary user code and may panic. Keep this
            // event inside the same recovery boundary as callback delivery so
            // such a panic restores the still-unconsumed batch instead of
            // dropping it after the queues have already been drained.
            tracing::debug!(
                shared_callbacks,
                local_callbacks,
                total_callbacks = callbacks.len(),
                "draining post-frame callback batch"
            );
            for entry in callbacks.by_ref() {
                if self.inner.callbacks.cancelled.contains_key(&entry.id()) {
                    continue;
                }
                entry.invoke(timing);
            }
        }));

        if result.is_err() {
            let mut shared = Vec::new();
            let mut local = Vec::new();
            for entry in callbacks {
                match entry {
                    PendingPostFrame::Shared(entry) => shared.push(entry),
                    PendingPostFrame::Local(entry) => local.push(entry),
                }
            }
            // Move only: no user callback or capture is dropped under a
            // queue guard. Original IDs put this tail ahead of reentrant
            // registrations when the next completed frame sorts its batch.
            let _registration = self.inner.callbacks.post_frame_registration.lock();
            self.inner.callbacks.post_frame.lock().extend(shared);
            if !local.is_empty() {
                lane.expect("BUG: local post-frame tail has a validated source lane")
                    .restore_queue(local);
            }
            // Keep cancellations for the restored tail and other queues.
        } else {
            self.inner.callbacks.cancelled.clear();
        }
        result
    }
}
