//! Rebuild queue for deferred component rebuild scheduling.
//!
//! Provides a thread-safe queue for scheduling component rebuilds from signal changes.
//! This allows signals to trigger rebuilds without direct access to BuildPipeline.

use flui_foundation::ElementId;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;

/// Thread-safe queue for deferred rebuild requests.
///
/// When signals change, they push rebuild requests to this queue.
/// The PipelineOwner drains the queue at the start of each frame.
///
/// # Thread-Safety
///
/// Uses `Arc<Mutex<HashSet>>` for thread-safe access from multiple threads.
/// Multiple signals can schedule rebuilds concurrently.
///
/// # Example
///
/// ```rust,ignore
/// // Create queue
/// let queue = RebuildQueue::new();
///
/// // Schedule rebuild from signal
/// queue.push(element_id, depth);
///
/// // Later, in pipeline
/// for (element_id, depth) in queue.drain() {
///     pipeline.schedule(element_id, depth);
/// }
/// ```
#[derive(Clone)]
pub struct RebuildQueue {
    /// Pending rebuilds: (ElementId, depth)
    /// Uses HashSet to deduplicate multiple requests for same element
    inner: Arc<Mutex<HashSet<(ElementId, usize)>>>,
}

impl RebuildQueue {
    /// Create a new rebuild queue
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Schedule an element for rebuild
    ///
    /// If the element is already scheduled, this is a no-op (deduplication).
    ///
    /// # Parameters
    ///
    /// - `element_id`: The element to rebuild
    /// - `depth`: The depth of the element in the tree (for ordering)
    pub fn push(&self, element_id: ElementId, depth: usize) {
        let mut pending = self.inner.lock();
        let is_new = pending.insert((element_id, depth));

        #[cfg(debug_assertions)]
        if is_new {
            tracing::trace!(
                "[REBUILD_QUEUE] Scheduled rebuild: element={:?}, depth={}",
                element_id,
                depth
            );
        } else {
            tracing::trace!(
                "[REBUILD_QUEUE] Already scheduled: element={:?}, depth={} (deduplicated)",
                element_id,
                depth
            );
        }
    }

    /// Drain all pending rebuilds
    ///
    /// Returns all pending rebuilds and clears the queue.
    /// The returned vector is sorted by depth (parents before children).
    ///
    /// # Returns
    ///
    /// Vec of (ElementId, depth) tuples, sorted by depth ascending
    pub fn drain(&self) -> Vec<(ElementId, usize)> {
        let pending = std::mem::take(&mut *self.inner.lock());

        if !pending.is_empty() {
            #[cfg(debug_assertions)]
            tracing::debug!(
                "[REBUILD_QUEUE] Draining {} pending rebuilds",
                pending.len()
            );

            // Convert to Vec and sort by depth (parents before children)
            let mut rebuilds: Vec<_> = pending.into_iter().collect();
            rebuilds.sort_by_key(|(_, depth)| *depth);
            rebuilds
        } else {
            Vec::new()
        }
    }

    /// Check if any rebuilds are pending
    pub fn has_pending(&self) -> bool {
        !self.inner.lock().is_empty()
    }

    /// Get the number of pending rebuilds
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl Default for RebuildQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RebuildQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RebuildQueue")
            .field("pending_count", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_drain() {
        let queue = RebuildQueue::new();

        assert!(queue.is_empty());

        queue.push(ElementId::new(1), 0);
        queue.push(ElementId::new(2), 1);

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 2);

        let rebuilds = queue.drain();
        assert_eq!(rebuilds.len(), 2);
        assert!(queue.is_empty());

        // Check sorted by depth
        assert_eq!(rebuilds[0].1, 0); // depth 0 first
        assert_eq!(rebuilds[1].1, 1); // depth 1 second
    }

    #[test]
    fn test_deduplication() {
        let queue = RebuildQueue::new();

        let id = ElementId::new(42);

        queue.push(id, 0);
        queue.push(id, 0); // Duplicate
        queue.push(id, 0); // Duplicate

        // Should only have one entry
        assert_eq!(queue.len(), 1);

        let rebuilds = queue.drain();
        assert_eq!(rebuilds.len(), 1);
        assert_eq!(rebuilds[0].0, id);
    }

    #[test]
    fn test_sorting_by_depth() {
        let queue = RebuildQueue::new();

        // Add in reverse depth order
        queue.push(ElementId::new(3), 2);
        queue.push(ElementId::new(1), 0);
        queue.push(ElementId::new(2), 1);

        let rebuilds = queue.drain();

        // Should be sorted by depth
        assert_eq!(rebuilds[0].1, 0);
        assert_eq!(rebuilds[1].1, 1);
        assert_eq!(rebuilds[2].1, 2);
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let queue = RebuildQueue::new();
        let queue_clone = queue.clone();

        let handle = thread::spawn(move || {
            for i in 1..101 {
                queue_clone.push(ElementId::new(i), 0);
            }
        });

        for i in 101..201 {
            queue.push(ElementId::new(i), 0);
        }

        handle.join().unwrap();

        // All 200 elements should be scheduled
        assert_eq!(queue.len(), 200);
    }
}
