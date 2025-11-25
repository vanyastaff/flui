//! Build pipeline for widget rebuild phase.
//!
//! The build pipeline manages widget rebuilds triggered by state changes.
//! It supports batching to coalesce multiple setState() calls into single rebuilds.
//!
//! # Design
//!
//! The build phase processes component elements top-down:
//! 1. Collect dirty components (marked for rebuild)
//! 2. Sort by depth (parents before children)
//! 3. Call View::build() for each component
//! 4. Reconcile old/new widget trees
//! 5. Schedule children for layout
//!
//! # Batching
//!
//! When batching is enabled, multiple setState() calls within a time window
//! are combined into a single rebuild:
//!
//! ```rust,ignore
//! use flui_pipeline::BuildPipeline;
//! use std::time::Duration;
//!
//! let mut pipeline = BuildPipeline::new();
//! pipeline.enable_batching(Duration::from_millis(16));
//!
//! // Multiple setState calls
//! pipeline.schedule(id1, 0);
//! pipeline.schedule(id1, 0);  // Deduplicated!
//! pipeline.schedule(id2, 1);
//!
//! // Later, flush batch
//! if pipeline.should_flush_batch() {
//!     pipeline.flush_batch();
//! }
//! ```
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use flui_pipeline::BuildPipeline;
//! use flui_foundation::ElementId;
//!
//! fn rebuild_dirty(pipeline: &mut BuildPipeline) {
//!     let dirty = pipeline.drain_dirty();
//!     for (id, depth) in dirty {
//!         // Rebuild component at this id...
//!         println!("Rebuilding {:?} at depth {}", id, depth);
//!     }
//! }
//! ```

use flui_foundation::ElementId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Build batcher for coalescing multiple setState() calls.
///
/// When enabled, multiple rebuilds scheduled within a time window
/// are combined into a single rebuild pass, reducing redundant work.
///
/// # Performance Benefits
///
/// - Animations triggering 60+ setState/second → batched to 60 rebuilds
/// - User input updating multiple widgets → single rebuild
/// - Computed values with cascading updates → deduplicated
#[derive(Debug)]
pub struct BuildBatcher {
    /// Pending elements: ElementId → depth
    pending: HashMap<ElementId, usize>,

    /// When the current batch started
    batch_start: Option<Instant>,

    /// How long to wait before flushing
    batch_duration: Duration,

    /// Statistics: total batches flushed
    batches_flushed: u64,

    /// Statistics: builds saved by deduplication
    builds_saved: u64,
}

impl BuildBatcher {
    /// Create a new batcher with specified duration.
    pub fn new(batch_duration: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            batch_start: None,
            batch_duration,
            batches_flushed: 0,
            builds_saved: 0,
        }
    }

    /// Schedule an element for rebuild.
    ///
    /// If element is already pending, this is a no-op (deduplication).
    /// Returns `true` if element was newly added, `false` if deduplicated.
    pub fn schedule(&mut self, id: ElementId, depth: usize) -> bool {
        // Start batch timer if first element
        if self.pending.is_empty() {
            self.batch_start = Some(Instant::now());
        }

        // Insert (or deduplicate)
        if self.pending.insert(id, depth).is_some() {
            self.builds_saved += 1;
            false // Deduplicated
        } else {
            true // Newly added
        }
    }

    /// Check if batch is ready to flush.
    ///
    /// Returns `true` if batch duration has elapsed since first element was added.
    pub fn should_flush(&self) -> bool {
        match self.batch_start {
            Some(start) => start.elapsed() >= self.batch_duration,
            None => false,
        }
    }

    /// Take all pending elements, clearing the batch.
    ///
    /// Returns elements sorted by depth (parents before children).
    pub fn drain(&mut self) -> Vec<(ElementId, usize)> {
        self.batches_flushed += 1;
        self.batch_start = None;

        let mut elements: Vec<_> = self.pending.drain().collect();
        elements.sort_by_key(|(_, depth)| *depth);
        elements
    }

    /// Check if any elements are pending.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Get number of pending elements.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get statistics: (batches_flushed, builds_saved).
    pub fn stats(&self) -> (u64, u64) {
        (self.batches_flushed, self.builds_saved)
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.batches_flushed = 0;
        self.builds_saved = 0;
    }

    /// Get batch duration.
    pub fn batch_duration(&self) -> Duration {
        self.batch_duration
    }

    /// Set batch duration.
    pub fn set_batch_duration(&mut self, duration: Duration) {
        self.batch_duration = duration;
    }
}

/// Build pipeline manages widget rebuild phase.
///
/// Tracks which component elements need rebuilding with their depths,
/// and supports batching for performance optimization.
///
/// # Depth Tracking
///
/// Each element is tracked with its depth (0 = root). This ensures
/// parents build before children, which is critical for correct
/// widget tree construction.
///
/// # Example
///
/// ```rust
/// use flui_pipeline::BuildPipeline;
/// use flui_foundation::ElementId;
///
/// let mut pipeline = BuildPipeline::new();
///
/// // Schedule elements for rebuild
/// pipeline.schedule(ElementId::new(1), 0);  // Root
/// pipeline.schedule(ElementId::new(2), 1);  // Child
///
/// // Drain returns sorted by depth
/// let dirty = pipeline.drain_dirty();
/// assert_eq!(dirty[0].1, 0);  // Parent first
/// assert_eq!(dirty[1].1, 1);  // Child second
/// ```
#[derive(Debug)]
pub struct BuildPipeline {
    /// Dirty elements with depths: (ElementId, depth)
    dirty: Vec<(ElementId, usize)>,

    /// Optional batcher for coalescing rebuilds
    batcher: Option<BuildBatcher>,

    /// Build count for statistics
    build_count: u64,

    /// Whether state changes are locked (during build)
    locked: bool,
}

impl BuildPipeline {
    /// Create a new build pipeline.
    pub fn new() -> Self {
        Self {
            dirty: Vec::new(),
            batcher: None,
            build_count: 0,
            locked: false,
        }
    }

    /// Create a build pipeline with batching enabled.
    pub fn with_batching(batch_duration: Duration) -> Self {
        Self {
            dirty: Vec::new(),
            batcher: Some(BuildBatcher::new(batch_duration)),
            build_count: 0,
            locked: false,
        }
    }

    // =========================================================================
    // Scheduling
    // =========================================================================

    /// Schedule an element for rebuild.
    ///
    /// # Parameters
    ///
    /// - `id`: Element to rebuild
    /// - `depth`: Depth in tree (0 = root)
    ///
    /// If batching is enabled, element is added to batch.
    /// Otherwise, added directly to dirty list.
    ///
    /// Returns `false` if scheduling was blocked (locked state).
    pub fn schedule(&mut self, id: ElementId, depth: usize) -> bool {
        if self.locked {
            tracing::warn!(
                element_id = ?id,
                "Attempted to schedule rebuild while locked"
            );
            return false;
        }

        if let Some(ref mut batcher) = self.batcher {
            batcher.schedule(id, depth);
        } else {
            self.dirty.push((id, depth));
        }

        true
    }

    /// Check if any elements are dirty.
    pub fn has_dirty(&self) -> bool {
        !self.dirty.is_empty() || self.batcher.as_ref().is_some_and(|b| b.has_pending())
    }

    /// Get number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        let direct = self.dirty.len();
        let batched = self.batcher.as_ref().map_or(0, |b| b.pending_count());
        direct + batched
    }

    /// Drain all dirty elements, sorted by depth.
    ///
    /// Returns elements sorted by depth (parents before children).
    /// Deduplicates elements that appear multiple times.
    pub fn drain_dirty(&mut self) -> Vec<(ElementId, usize)> {
        // Flush batch first if present
        if let Some(ref mut batcher) = self.batcher {
            let batched = batcher.drain();
            self.dirty.extend(batched);
        }

        // Take and deduplicate
        let mut elements = std::mem::take(&mut self.dirty);

        // Deduplicate by ElementId (keep first occurrence)
        let mut seen = std::collections::HashSet::new();
        elements.retain(|(id, _)| seen.insert(*id));

        // Sort by depth (parents before children)
        elements.sort_by_key(|(_, depth)| *depth);

        self.build_count += elements.len() as u64;
        elements
    }

    /// Clear all dirty elements without rebuilding.
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
        if let Some(ref mut batcher) = self.batcher {
            batcher.drain(); // Discard
        }
    }

    // =========================================================================
    // Batching
    // =========================================================================

    /// Enable batching with specified duration.
    pub fn enable_batching(&mut self, duration: Duration) {
        self.batcher = Some(BuildBatcher::new(duration));
    }

    /// Disable batching.
    pub fn disable_batching(&mut self) {
        // Flush any pending before disabling
        if let Some(ref mut batcher) = self.batcher {
            let batched = batcher.drain();
            self.dirty.extend(batched);
        }
        self.batcher = None;
    }

    /// Check if batching is enabled.
    pub fn is_batching_enabled(&self) -> bool {
        self.batcher.is_some()
    }

    /// Check if batch should be flushed.
    pub fn should_flush_batch(&self) -> bool {
        self.batcher.as_ref().is_some_and(|b| b.should_flush())
    }

    /// Flush batch to dirty list.
    ///
    /// Moves all batched elements to the main dirty list.
    pub fn flush_batch(&mut self) {
        if let Some(ref mut batcher) = self.batcher {
            let batched = batcher.drain();
            self.dirty.extend(batched);
        }
    }

    /// Get batching statistics: (batches_flushed, builds_saved).
    pub fn batching_stats(&self) -> (u64, u64) {
        self.batcher.as_ref().map_or((0, 0), |b| b.stats())
    }

    /// Set batch duration (if batching enabled).
    pub fn set_batch_duration(&mut self, duration: Duration) {
        if let Some(ref mut batcher) = self.batcher {
            batcher.set_batch_duration(duration);
        }
    }

    // =========================================================================
    // Locking
    // =========================================================================

    /// Lock state changes (prevent scheduling during build).
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// Unlock state changes.
    pub fn unlock(&mut self) {
        self.locked = false;
    }

    /// Check if locked.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Execute closure with state locked.
    ///
    /// Prevents new schedules during the closure execution.
    pub fn with_lock<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.locked = true;
        let result = f(self);
        self.locked = false;
        result
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get total build count.
    pub fn build_count(&self) -> u64 {
        self.build_count
    }

    /// Reset statistics.
    pub fn reset_stats(&mut self) {
        self.build_count = 0;
        if let Some(ref mut batcher) = self.batcher {
            batcher.reset_stats();
        }
    }
}

impl Default for BuildPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_and_drain() {
        let mut pipeline = BuildPipeline::new();

        pipeline.schedule(ElementId::new(1), 0);
        pipeline.schedule(ElementId::new(2), 1);

        assert!(pipeline.has_dirty());
        assert_eq!(pipeline.dirty_count(), 2);

        let dirty = pipeline.drain_dirty();
        assert_eq!(dirty.len(), 2);
        assert_eq!(dirty[0].1, 0); // Sorted by depth
        assert_eq!(dirty[1].1, 1);

        assert!(!pipeline.has_dirty());
    }

    #[test]
    fn test_deduplication() {
        let mut pipeline = BuildPipeline::new();

        let id = ElementId::new(42);
        pipeline.schedule(id, 0);
        pipeline.schedule(id, 0); // Duplicate
        pipeline.schedule(id, 0); // Duplicate

        let dirty = pipeline.drain_dirty();
        assert_eq!(dirty.len(), 1); // Deduplicated
    }

    #[test]
    fn test_depth_sorting() {
        let mut pipeline = BuildPipeline::new();

        // Add in reverse order
        pipeline.schedule(ElementId::new(3), 2);
        pipeline.schedule(ElementId::new(1), 0);
        pipeline.schedule(ElementId::new(2), 1);

        let dirty = pipeline.drain_dirty();

        assert_eq!(dirty[0].1, 0);
        assert_eq!(dirty[1].1, 1);
        assert_eq!(dirty[2].1, 2);
    }

    #[test]
    fn test_batching_deduplication() {
        let mut pipeline = BuildPipeline::with_batching(Duration::from_millis(16));

        let id = ElementId::new(42);
        pipeline.schedule(id, 0);
        pipeline.schedule(id, 0); // Should be deduplicated
        pipeline.schedule(id, 0); // Should be deduplicated

        pipeline.flush_batch();
        let dirty = pipeline.drain_dirty();

        assert_eq!(dirty.len(), 1);

        let (batches, saved) = pipeline.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }

    #[test]
    fn test_batching_timing() {
        let mut pipeline = BuildPipeline::with_batching(Duration::from_millis(10));

        pipeline.schedule(ElementId::new(1), 0);

        // Should not flush immediately
        assert!(!pipeline.should_flush_batch());

        // Wait for batch duration
        std::thread::sleep(Duration::from_millis(15));

        // Should flush now
        assert!(pipeline.should_flush_batch());
    }

    #[test]
    fn test_enable_disable_batching() {
        let mut pipeline = BuildPipeline::new();
        assert!(!pipeline.is_batching_enabled());

        pipeline.enable_batching(Duration::from_millis(16));
        assert!(pipeline.is_batching_enabled());

        // Schedule while batched
        pipeline.schedule(ElementId::new(1), 0);

        // Disable should flush
        pipeline.disable_batching();
        assert!(!pipeline.is_batching_enabled());
        assert_eq!(pipeline.dirty_count(), 1);
    }

    #[test]
    fn test_locking() {
        let mut pipeline = BuildPipeline::new();

        pipeline.schedule(ElementId::new(1), 0);
        assert_eq!(pipeline.dirty_count(), 1);

        pipeline.lock();
        let result = pipeline.schedule(ElementId::new(2), 0);
        assert!(!result); // Blocked
        assert_eq!(pipeline.dirty_count(), 1); // Still 1

        pipeline.unlock();
        let result = pipeline.schedule(ElementId::new(2), 0);
        assert!(result); // Allowed
        assert_eq!(pipeline.dirty_count(), 2);
    }

    #[test]
    fn test_with_lock() {
        let mut pipeline = BuildPipeline::new();

        let result = pipeline.with_lock(|p| {
            assert!(p.is_locked());
            42
        });

        assert!(!pipeline.is_locked());
        assert_eq!(result, 42);
    }

    #[test]
    fn test_clear_dirty() {
        let mut pipeline = BuildPipeline::new();

        pipeline.schedule(ElementId::new(1), 0);
        pipeline.schedule(ElementId::new(2), 1);

        pipeline.clear_dirty();
        assert!(!pipeline.has_dirty());
    }

    #[test]
    fn test_build_count() {
        let mut pipeline = BuildPipeline::new();

        pipeline.schedule(ElementId::new(1), 0);
        pipeline.schedule(ElementId::new(2), 1);

        assert_eq!(pipeline.build_count(), 0);

        pipeline.drain_dirty();
        assert_eq!(pipeline.build_count(), 2);

        pipeline.schedule(ElementId::new(3), 0);
        pipeline.drain_dirty();
        assert_eq!(pipeline.build_count(), 3);
    }

    // =========================================================================
    // BuildBatcher tests
    // =========================================================================

    #[test]
    fn test_batcher_schedule() {
        let mut batcher = BuildBatcher::new(Duration::from_millis(16));

        assert!(!batcher.has_pending());

        let added = batcher.schedule(ElementId::new(1), 0);
        assert!(added);
        assert!(batcher.has_pending());
        assert_eq!(batcher.pending_count(), 1);

        // Duplicate
        let added = batcher.schedule(ElementId::new(1), 0);
        assert!(!added);
        assert_eq!(batcher.pending_count(), 1);
    }

    #[test]
    fn test_batcher_drain_sorted() {
        let mut batcher = BuildBatcher::new(Duration::from_millis(16));

        batcher.schedule(ElementId::new(3), 2);
        batcher.schedule(ElementId::new(1), 0);
        batcher.schedule(ElementId::new(2), 1);

        let elements = batcher.drain();

        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0].1, 0);
        assert_eq!(elements[1].1, 1);
        assert_eq!(elements[2].1, 2);

        assert!(!batcher.has_pending());
    }

    #[test]
    fn test_batcher_stats() {
        let mut batcher = BuildBatcher::new(Duration::from_millis(16));

        let id = ElementId::new(1);
        batcher.schedule(id, 0);
        batcher.schedule(id, 0); // +1 saved
        batcher.schedule(id, 0); // +1 saved

        batcher.drain(); // +1 batch

        let (batches, saved) = batcher.stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }
}
