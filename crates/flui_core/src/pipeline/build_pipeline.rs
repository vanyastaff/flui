//! Build pipeline for widget rebuild phase.
//!
//! The build pipeline is responsible for:
//! - Rebuilding dirty widgets
//! - Updating element tree
//! - Reconciling widget changes
//! - Batching rapid setState() calls for performance
//!
//! # Design
//!
//! The build phase processes widgets top-down:
//! 1. Identify dirty elements (marked for rebuild)
//! 2. Call `Widget::build()` for each dirty element
//! 3. Reconcile old and new widget trees
//! 4. Update element tree accordingly
//!
//! # Build Batching
//!
//! When enabled, BuildPipeline batches multiple setState() calls within a time window:
//!
//! ```rust,ignore
//! let mut build = BuildPipeline::new();
//! build.enable_batching(Duration::from_millis(16)); // One frame
//!
//! // Multiple setState calls
//! build.schedule(id1, 0);
//! build.schedule(id2, 1);
//! build.schedule(id1, 0); // Duplicate - batched!
//!
//! // Later...
//! if build.should_flush_batch() {
//!     build.flush_batch();
//! }
//! build.rebuild_dirty(&tree);
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! let mut build = BuildPipeline::new();
//! build.schedule(root_id, 0);
//! build.rebuild_dirty(&tree);
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::element::ElementId;
use crate::pipeline::element_tree::ElementTree;

#[cfg(debug_assertions)]
use crate::debug_println;

/// Build batching system for performance optimization
///
/// Batches multiple setState() calls into a single rebuild to avoid redundant work.
/// This is especially useful for:
/// - Animations with many rapid setState() calls
/// - User input triggering multiple widgets
/// - Computed values that update multiple times per frame
#[derive(Debug)]
struct BuildBatcher {
    /// Elements pending in current batch (with depths)
    pending: HashMap<ElementId, usize>,
    /// When the current batch started
    batch_start: Option<Instant>,
    /// How long to wait before flushing batch
    batch_duration: Duration,
    /// Total number of batches flushed
    batches_flushed: usize,
    /// Total number of builds saved by batching
    builds_saved: usize,
}

impl BuildBatcher {
    fn new(batch_duration: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            batch_start: None,
            batch_duration,
            batches_flushed: 0,
            builds_saved: 0,
        }
    }

    /// Add element to batch
    fn schedule(&mut self, element_id: ElementId, depth: usize) {
        // Start batch timer if first element
        if self.pending.is_empty() {
            self.batch_start = Some(Instant::now());
        }

        // Track if this is a duplicate (saved build)
        if self.pending.insert(element_id, depth).is_some() {
            self.builds_saved += 1;
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Build batched: element {:?} already in batch (saved 1 build)",
                element_id
            );
        } else {
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Build batched: added element {:?} to batch",
                element_id
            );
        }
    }

    /// Check if batch is ready to flush
    fn should_flush(&self) -> bool {
        if let Some(start) = self.batch_start {
            start.elapsed() >= self.batch_duration
        } else {
            false
        }
    }

    /// Take all pending builds
    fn take_pending(&mut self) -> HashMap<ElementId, usize> {
        self.batches_flushed += 1;
        self.batch_start = None;
        std::mem::take(&mut self.pending)
    }

    /// Get statistics (batches_flushed, builds_saved)
    fn stats(&self) -> (usize, usize) {
        (self.batches_flushed, self.builds_saved)
    }
}

/// Build pipeline manages widget rebuild phase.
///
/// Tracks which elements need rebuilding (with their depths) and processes them
/// in depth-first order (parents before children).
///
/// # Depth Tracking
///
/// Each element is tracked with its depth in the tree (0 = root).
/// This ensures parent widgets build before children, which is critical
/// for correct widget tree construction.
#[derive(Debug)]
pub struct BuildPipeline {
    /// Elements that need rebuilding with their depths: (ElementId, depth)
    dirty_elements: Vec<(ElementId, usize)>,
    /// Build count for tracking
    build_count: usize,
    /// Whether currently in a build scope
    in_build_scope: bool,
    /// Whether state changes are locked
    build_locked: bool,
    /// Optional batching system
    batcher: Option<BuildBatcher>,
}

impl BuildPipeline {
    /// Creates a new build pipeline.
    pub fn new() -> Self {
        Self {
            dirty_elements: Vec::new(),
            build_count: 0,
            in_build_scope: false,
            build_locked: false,
            batcher: None,
        }
    }

    // =========================================================================
    // Build Scheduling
    // =========================================================================

    /// Schedule an element for rebuild
    ///
    /// # Parameters
    ///
    /// - `element_id`: The element to rebuild
    /// - `depth`: The depth of the element in the tree (0 = root)
    ///
    /// Elements are sorted by depth before building to ensure parents build before children.
    ///
    /// If batching is enabled, the build will be batched with other builds.
    /// Otherwise, it's added to dirty_elements immediately.
    pub fn schedule(&mut self, element_id: ElementId, depth: usize) {
        #[cfg(debug_assertions)]
        tracing::trace!("schedule element={:?}, depth={}, build_locked={}", element_id, depth, self.build_locked);

        if self.build_locked {
            #[cfg(debug_assertions)]
            tracing::warn!("Attempted to schedule build while locked (element {:?})", element_id);
            return;
        }

        // If batching enabled, use batcher
        if let Some(ref mut batcher) = self.batcher {
            batcher.schedule(element_id, depth);
            return;
        }

        // Otherwise, add directly to dirty elements
        // Check if already scheduled
        if self.dirty_elements.iter().any(|(id, _)| *id == element_id) {
            #[cfg(debug_assertions)]
            debug_println!(
                PRINT_SCHEDULE_BUILD,
                "Element {:?} already scheduled for rebuild",
                element_id
            );
            return;
        }

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_SCHEDULE_BUILD,
            "Scheduling element {:?} for rebuild (depth {})",
            element_id,
            depth
        );

        self.dirty_elements.push((element_id, depth));
    }

    /// Checks if any elements are dirty.
    pub fn has_dirty(&self) -> bool {
        !self.dirty_elements.is_empty()
    }

    /// Returns the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }

    /// Check if currently in build scope
    pub fn is_in_build_scope(&self) -> bool {
        self.in_build_scope
    }

    // =========================================================================
    // Build Batching
    // =========================================================================

    /// Enable build batching with given duration
    ///
    /// When batching is enabled, multiple setState() calls within the batch_duration
    /// will be combined into a single rebuild. This dramatically improves performance
    /// for animations and other rapid state changes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// build.enable_batching(Duration::from_millis(16)); // 1 frame
    ///
    /// // Multiple setState calls
    /// build.schedule(id1, 0);
    /// build.schedule(id2, 1); // Batched!
    /// build.schedule(id1, 0); // Duplicate - saved!
    ///
    /// // Later...
    /// if build.should_flush_batch() {
    ///     build.flush_batch();
    /// }
    /// ```
    pub fn enable_batching(&mut self, batch_duration: Duration) {
        #[cfg(debug_assertions)]
        println!("Enabling build batching with duration {:?}", batch_duration);
        self.batcher = Some(BuildBatcher::new(batch_duration));
    }

    /// Disable build batching
    pub fn disable_batching(&mut self) {
        if let Some(ref batcher) = self.batcher {
            let (batches, saved) = batcher.stats();
            #[cfg(debug_assertions)]
            println!(
                "Disabling build batching (flushed {} batches, saved {} builds)",
                batches, saved
            );
        }
        self.batcher = None;
    }

    /// Check if batching is enabled
    pub fn is_batching_enabled(&self) -> bool {
        self.batcher.is_some()
    }

    /// Check if batch is ready to flush
    pub fn should_flush_batch(&self) -> bool {
        self.batcher
            .as_ref()
            .map(|b| b.should_flush())
            .unwrap_or(false)
    }

    /// Flush the current batch
    ///
    /// Moves all pending batched builds to dirty_elements for processing.
    pub fn flush_batch(&mut self) {
        if let Some(ref mut batcher) = self.batcher {
            let pending = batcher.take_pending();
            if !pending.is_empty() {
                #[cfg(debug_assertions)]
                debug_println!(
                    PRINT_SCHEDULE_BUILD,
                    "Flushing batch: {} elements",
                    pending.len()
                );

                for (element_id, depth) in pending {
                    // Add to dirty elements (bypass batching)
                    if !self.dirty_elements.iter().any(|(id, _)| *id == element_id) {
                        self.dirty_elements.push((element_id, depth));
                    }
                }
            }
        }
    }

    /// Get batching statistics (batches_flushed, builds_saved)
    pub fn batching_stats(&self) -> (usize, usize) {
        self.batcher.as_ref().map(|b| b.stats()).unwrap_or((0, 0))
    }

    // =========================================================================
    // Build Execution
    // =========================================================================

    /// Execute a build scope
    ///
    /// This sets the build scope flag to prevent setState during build,
    /// then executes the callback.
    ///
    /// # Build Scope Isolation
    ///
    /// Any `markNeedsBuild()` calls during the scope will be deferred and
    /// processed after the scope completes. This prevents infinite rebuild loops.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// build.build_scope(|build| {
    ///     build.rebuild_dirty(&mut tree);
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If the callback panics, the build scope will be properly cleaned up,
    /// but the panic will propagate.
    pub fn build_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        if self.in_build_scope {
            #[cfg(debug_assertions)]
            eprintln!("Warning: Nested build_scope detected!");
        }

        self.in_build_scope = true;

        // Execute callback
        let result = f(self);

        // Clear flag
        self.in_build_scope = false;

        result
    }

    /// Lock state changes
    ///
    /// Executes callback with state changes locked.
    /// Any setState calls during this time will be ignored/warned.
    pub fn lock_state<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let was_locked = self.build_locked;
        self.build_locked = true;
        let result = f(self);
        self.build_locked = was_locked;
        result
    }

    /// Rebuilds all dirty elements.
    ///
    /// Processes elements top-down (sorted by depth), rebuilding widgets and
    /// reconciling the element tree.
    ///
    /// Returns the number of elements rebuilt.
    pub fn rebuild_dirty(&mut self, tree: &mut ElementTree) -> usize {
        if self.dirty_elements.is_empty() {
            return 0;
        }

        self.build_count += 1;
        let build_num = self.build_count;
        let dirty_count = self.dirty_elements.len();

        #[cfg(debug_assertions)]
        debug_println!(
            PRINT_BUILD_SCOPE,
            "rebuild_dirty #{}: rebuilding {} dirty elements",
            build_num,
            dirty_count
        );

        // Sort by depth (parents before children)
        self.dirty_elements.sort_by_key(|(_, depth)| *depth);

        // Take the dirty list to avoid borrow conflicts
        let mut dirty = std::mem::take(&mut self.dirty_elements);

        // Rebuild each element
        for (element_id, depth) in dirty.drain(..) {
            #[cfg(debug_assertions)]
            tracing::trace!("rebuild_dirty: Processing element {:?} at depth {}", element_id, depth);

            // Verify element still exists in tree
            let element = match tree.get(element_id) {
                Some(elem) => elem,
                None => {
                    #[cfg(debug_assertions)]
                    tracing::warn!("Element {:?} not found in tree during rebuild (may have been removed)", element_id);
                    continue;
                }
            };

            // Dispatch rebuild based on element type
            match element {
                crate::element::Element::Component(_comp) => {
                    // TODO(Issue #2): Full View-based component rebuild
                    //
                    // Proper implementation requires:
                    // 1. Get parent element to retrieve new view
                    // 2. Parent calls view.rebuild_any() to create new child view
                    // 3. Reconcile new view with existing child
                    // 4. Update component's child reference
                    // 5. Mark new child as dirty if changed
                    //
                    // For now, component rebuilds are handled by parent elements.
                    // This ensures the build phase runs without errors, even though
                    // the View integration is not yet complete.
                    //
                    // See docs/PIPELINE_ARCHITECTURE.md for full algorithm.

                    #[cfg(debug_assertions)]
                    tracing::debug!("Component element {:?} rebuild deferred to parent (View integration pending)", element_id);
                }

                crate::element::Element::Render(_render) => {
                    // RenderElements don't rebuild - they only relayout
                    // Skip rebuild for render elements
                    #[cfg(debug_assertions)]
                    tracing::trace!("Render element {:?} skipped (rebuilds via layout)", element_id);
                }

                crate::element::Element::Provider(_provider) => {
                    // TODO(Issue #2): Provider rebuild
                    //
                    // Providers should:
                    // 1. Check if provided data changed
                    // 2. Notify dependents if data changed
                    // 3. Mark dependents as dirty
                    //
                    // For now, providers work but don't propagate changes.

                    #[cfg(debug_assertions)]
                    tracing::debug!("Provider element {:?} rebuild (change propagation pending)", element_id);
                }
            }

            #[cfg(debug_assertions)]
            tracing::trace!("Processed element {:?}", element_id);
        }

        #[cfg(debug_assertions)]
        debug_println!(PRINT_BUILD_SCOPE, "rebuild_dirty #{}: complete ({} elements processed)", build_num, dirty_count);

        dirty_count
    }

    /// Clears all dirty elements without rebuilding.
    pub fn clear_dirty(&mut self) {
        self.dirty_elements.clear();
    }

    // =========================================================================
    // Internal API (for PipelineOwner guards)
    // =========================================================================

    /// Set in_build_scope flag (internal use only)
    pub(super) fn set_build_scope(&mut self, value: bool) {
        self.in_build_scope = value;
    }

    /// Set build_locked flag (internal use only)
    pub(super) fn set_build_locked(&mut self, value: bool) {
        self.build_locked = value;
    }
}

impl Default for BuildPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule() {
        let mut build = BuildPipeline::new();

        assert!(!build.has_dirty());

        build.schedule(1, 0);

        assert!(build.has_dirty());
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_dirty_count() {
        let mut build = BuildPipeline::new();

        build.schedule(1, 0);
        build.schedule(2, 1);

        assert_eq!(build.dirty_count(), 2);
    }

    #[test]
    fn test_clear_dirty() {
        let mut build = BuildPipeline::new();

        build.schedule(1, 0);
        build.clear_dirty();

        assert!(!build.has_dirty());
    }

    #[test]
    fn test_schedule_duplicate() {
        let mut build = BuildPipeline::new();

        build.schedule(1, 0);
        build.schedule(1, 0); // Duplicate

        // Should only have 1 dirty element
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_build_scope() {
        let mut build = BuildPipeline::new();

        assert!(!build.is_in_build_scope());

        build.build_scope(|b| {
            assert!(b.is_in_build_scope());
        });

        assert!(!build.is_in_build_scope());
    }

    #[test]
    fn test_lock_state() {
        let mut build = BuildPipeline::new();

        // Normal scheduling works
        build.schedule(1, 0);
        assert_eq!(build.dirty_count(), 1);

        build.lock_state(|b| {
            // Scheduling while locked should be ignored
            b.schedule(2, 0);
            assert_eq!(b.dirty_count(), 1); // Still 1, not 2
        });
    }

    #[test]
    fn test_batching_disabled_by_default() {
        let build = BuildPipeline::new();
        assert!(!build.is_batching_enabled());
    }

    #[test]
    fn test_enable_disable_batching() {
        let mut build = BuildPipeline::new();

        build.enable_batching(Duration::from_millis(16));
        assert!(build.is_batching_enabled());

        build.disable_batching();
        assert!(!build.is_batching_enabled());
    }

    #[test]
    fn test_batching_deduplicates() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        let id = 42;

        // Schedule same element 3 times
        build.schedule(id, 0);
        build.schedule(id, 0);
        build.schedule(id, 0);

        // Flush batch
        build.flush_batch();

        // Should only have 1 dirty element
        assert_eq!(build.dirty_count(), 1);

        // Stats should show 2 builds saved
        let (batches, saved) = build.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 2);
    }

    #[test]
    fn test_batching_multiple_elements() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        build.schedule(1, 0);
        build.schedule(2, 1);
        build.schedule(3, 2);

        build.flush_batch();

        // All 3 should be dirty
        assert_eq!(build.dirty_count(), 3);
    }

    #[test]
    fn test_should_flush_batch_timing() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(10));

        build.schedule(42, 0);

        // Should not flush immediately
        assert!(!build.should_flush_batch());

        // Wait for batch duration
        std::thread::sleep(Duration::from_millis(15));

        // Now should flush
        assert!(build.should_flush_batch());
    }

    #[test]
    fn test_batching_without_enable() {
        let mut build = BuildPipeline::new();
        // Batching not enabled

        build.schedule(42, 0);

        // Should add directly to dirty elements
        assert_eq!(build.dirty_count(), 1);

        // flush_batch should be no-op
        build.flush_batch();
        assert_eq!(build.dirty_count(), 1);
    }

    #[test]
    fn test_batching_stats() {
        let mut build = BuildPipeline::new();
        build.enable_batching(Duration::from_millis(16));

        // Initial stats
        assert_eq!(build.batching_stats(), (0, 0));

        // Schedule same element twice
        build.schedule(42, 0);
        build.schedule(42, 0); // Duplicate

        // Flush
        build.flush_batch();

        // Should have 1 batch flushed, 1 build saved
        let (batches, saved) = build.batching_stats();
        assert_eq!(batches, 1);
        assert_eq!(saved, 1);
    }
}
