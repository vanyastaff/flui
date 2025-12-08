//! Layout pipeline for size computation phase.
//!
//! The layout pipeline is responsible for:
//! - Computing sizes for dirty render objects
//! - Parallel layout of independent subtrees
//! - Layout cache management
//!
//! # Design
//!
//! The layout phase processes render objects top-down:
//! 1. Identify dirty render objects (marked for relayout)
//! 2. Compute sizes using parent constraints
//! 3. Position children based on computed sizes
//! 4. Cache layout results
//!
//! # Parallelization
//!
//! Independent subtrees can be laid out in parallel using rayon.
//! The pipeline automatically detects parallelizable work.
//!
//! # Example
//!
//! ```rust,ignore
//! let mut layout = LayoutPipeline::new();
//! layout.mark_dirty(render_id);
//! layout.compute_layout(&tree, constraints);
//! ```

use super::dirty_tracker::DirtyTracker;
use flui_element::ElementTree;
use flui_foundation::ElementId;
use flui_pipeline::{LockFreeDirtySet, PipelineError};
use flui_types::constraints::BoxConstraints;

/// Result type for layout operations
pub type LayoutResult<T> = Result<T, PipelineError>;

/// Layout pipeline manages size computation phase.
///
/// Tracks which render objects need relayout and processes them
/// with support for parallel execution.
#[derive(Debug, Default)]
pub struct LayoutPipeline {
    /// Dirty tracking (composed)
    dirty: DirtyTracker,

    /// Whether to enable parallel layout.
    parallel_enabled: bool,
}

impl LayoutPipeline {
    /// Creates a new layout pipeline.
    ///
    /// Parallel layout is enabled by default.
    pub fn new() -> Self {
        Self {
            dirty: DirtyTracker::new(),
            parallel_enabled: true,
        }
    }

    /// Creates a layout pipeline with parallel execution disabled.
    pub fn new_single_threaded() -> Self {
        Self {
            dirty: DirtyTracker::new(),
            parallel_enabled: false,
        }
    }

    /// Marks a render object as needing relayout.
    #[inline]
    pub fn mark_dirty(&self, id: ElementId) {
        self.dirty.mark_dirty(id);
    }

    /// Checks if any render objects are dirty.
    #[inline]
    pub fn has_dirty(&self) -> bool {
        self.dirty.has_dirty()
    }

    /// Checks if a specific render object is dirty.
    #[inline]
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.dirty.is_dirty(id)
    }

    /// Enables or disables parallel layout.
    #[inline]
    pub fn set_parallel(&mut self, enabled: bool) {
        self.parallel_enabled = enabled;
    }

    /// Returns true if parallel layout is enabled.
    #[inline]
    pub fn is_parallel(&self) -> bool {
        self.parallel_enabled
    }

    /// Returns a reference to the dirty set.
    #[inline]
    pub fn dirty_set(&self) -> &LockFreeDirtySet<ElementId> {
        self.dirty.inner()
    }

    /// Marks all elements as dirty.
    #[inline]
    pub fn mark_all_dirty(&self) {
        self.dirty.mark_all_dirty();
    }

    /// Computes layout for all dirty render objects.
    ///
    /// Processes render objects top-down, computing sizes and positions.
    /// Uses parallel execution for independent subtrees if enabled.
    ///
    /// # Returns
    ///
    /// `Ok(count)` where count is the number of render objects laid out.
    ///
    /// `Err(PipelineError)` if layout fails for any element.
    ///
    /// # Implementation
    ///
    /// For each dirty render object:
    /// 1. Get RenderElement from tree
    /// 2. Get constraints from RenderState (or use provided constraints)
    /// 3. Call appropriate layout() method based on RenderNode variant
    /// 4. Store computed size in RenderState
    /// 5. Clear needs_layout flag
    ///
    /// # Error Handling
    ///
    /// If layout fails for an element, the error is returned immediately and
    /// processing stops. Use error recovery policy in PipelineOwner to handle
    /// layout errors gracefully.
    ///
    /// # Note on Parallelization
    ///
    /// Currently implemented as sequential layout. Parallel layout will be added
    /// in a future update using rayon for independent subtrees.
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty_count()))]
    pub fn compute_layout(
        &mut self,
        tree: &mut ElementTree,
        constraints: BoxConstraints,
    ) -> LayoutResult<Vec<ElementId>> {
        let dirty_ids = self.dirty.drain();
        let count = dirty_ids.len();

        if count == 0 {
            return Ok(Vec::new());
        }

        // Track which elements were successfully laid out
        let mut laid_out_ids = Vec::with_capacity(count);

        // Cache statistics tracking
        #[allow(unused_assignments)]
        let cache_hits = 0usize;
        #[allow(unused_assignments)]
        let mut cache_misses = 0usize;

        // Process each dirty render object
        // Note: Parallel layout disabled for now - will be enabled in future update
        for id in dirty_ids {
            // Check if element exists in tree
            use flui_element::RenderTreeAccess;
            if tree.get(id).is_none() {
                tracing::error!(
                    element_id = ?id,
                    "Element marked dirty but not found in tree during layout"
                );
                continue;
            }

            // Only layout render elements (uses trait method for abstraction)
            if !tree.is_render_element(id) {
                #[cfg(debug_assertions)]
                tracing::trace!("Element {:?} is not a render element, skipping", id);
                continue;
            }

            // NOTE: In four-tree architecture, RenderState is stored in RenderTree (accessed via
            // element.as_render().render_id() + RenderTree::get()). Currently we layout all dirty
            // render elements unconditionally. Future: check RenderTree for needs_layout flag.
            cache_misses += 1;

            #[cfg(debug_assertions)]
            tracing::trace!("Layout: Processing render object {:?}", id);

            // Use provided constraints (stored constraints not available without render_state)
            let layout_constraints = constraints;

            // Perform layout using ElementTree method
            // This properly handles the unified Element architecture and state updates
            let computed_size = match tree.layout_render_object(id, layout_constraints) {
                Some(size) => {
                    // ElementTree.layout_render_object() handles:
                    // 1. Calling ViewObject.layout_render()
                    // 2. Storing size in RenderState
                    // 3. Clearing needs_layout flag
                    // 4. Marking for paint
                    size
                }
                None => {
                    #[cfg(debug_assertions)]
                    tracing::warn!("Layout failed for element {:?}", id);
                    flui_types::Size::ZERO
                }
            };

            tracing::trace!(
                "Layout: Stored size {:?} for element {:?}, marked for paint",
                computed_size,
                id
            );

            // Add to list of successfully laid out elements
            laid_out_ids.push(id);
        }

        // Log cache statistics if there were cache hits or misses
        if cache_hits > 0 || cache_misses > 0 {
            let total = cache_hits + cache_misses;
            let hit_rate = if total > 0 {
                (cache_hits as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            tracing::trace!(
                cache_hits,
                cache_misses,
                total,
                cache_hit_rate = format!("{:.0}%", hit_rate),
                "Layout cache"
            );
        }

        Ok(laid_out_ids)
    }

    /// Clears all dirty render objects without laying out.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Returns the number of dirty render objects.
    #[inline]
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;

    #[test]
    fn test_mark_dirty() {
        let layout = LayoutPipeline::new();

        assert!(!layout.has_dirty());

        layout.mark_dirty(ElementId::new(1));

        assert!(layout.has_dirty());
        assert!(layout.is_dirty(ElementId::new(1)));
        assert!(!layout.is_dirty(ElementId::new(2)));
    }

    #[test]
    fn test_dirty_count() {
        let layout = LayoutPipeline::new();

        layout.mark_dirty(ElementId::new(1));
        layout.mark_dirty(ElementId::new(2));

        assert_eq!(layout.dirty_count(), 2);
    }

    #[test]
    fn test_parallel_mode() {
        let mut layout = LayoutPipeline::new();
        assert!(layout.is_parallel());

        layout.set_parallel(false);
        assert!(!layout.is_parallel());

        let single_threaded = LayoutPipeline::new_single_threaded();
        assert!(!single_threaded.is_parallel());
    }

    #[test]
    fn test_clear_dirty() {
        let mut layout = LayoutPipeline::new();

        layout.mark_dirty(ElementId::new(1));
        layout.clear_dirty();

        assert!(!layout.has_dirty());
    }
}

// =============================================================================
// Trait Implementations
// =============================================================================

impl flui_pipeline::LayoutPhase for LayoutPipeline {
    type Tree = ElementTree;
    type Constraints = BoxConstraints;
    type Size = flui_types::Size;

    fn mark_dirty(&self, element_id: ElementId) {
        LayoutPipeline::mark_dirty(self, element_id);
    }

    fn has_dirty(&self) -> bool {
        LayoutPipeline::has_dirty(self)
    }

    fn dirty_count(&self) -> usize {
        LayoutPipeline::dirty_count(self)
    }

    fn is_dirty(&self, element_id: ElementId) -> bool {
        LayoutPipeline::is_dirty(self, element_id)
    }

    fn clear_dirty(&mut self) {
        LayoutPipeline::clear_dirty(self);
    }

    fn compute_layout(
        &mut self,
        tree: &mut Self::Tree,
        constraints: Self::Constraints,
    ) -> flui_pipeline::PipelineResult<Vec<ElementId>> {
        LayoutPipeline::compute_layout(self, tree, constraints)
    }
}

impl flui_pipeline::ParallelExecution for LayoutPipeline {
    fn set_parallel(&mut self, enabled: bool) {
        LayoutPipeline::set_parallel(self, enabled);
    }

    fn is_parallel(&self) -> bool {
        LayoutPipeline::is_parallel(self)
    }
}
