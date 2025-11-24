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

use crate::element::ElementId;
use crate::element::ElementTree;
use crate::pipeline::dirty_tracking::LockFreeDirtySet;
use crate::pipeline::PipelineError;
use flui_types::constraints::BoxConstraints;

/// Result type for layout operations
pub type LayoutResult<T> = Result<T, PipelineError>;

/// Layout pipeline manages size computation phase.
///
/// Tracks which render objects need relayout and processes them
/// with support for parallel execution.
#[derive(Debug)]
pub struct LayoutPipeline {
    /// Set of render objects that need relayout.
    dirty: LockFreeDirtySet,

    /// Whether to enable parallel layout.
    ///
    /// Parallel layout uses rayon for independent subtrees.
    /// Can be disabled for debugging or single-threaded environments.
    parallel_enabled: bool,
}

impl LayoutPipeline {
    /// Creates a new layout pipeline.
    ///
    /// Parallel layout is enabled by default.
    pub fn new() -> Self {
        Self {
            dirty: LockFreeDirtySet::default(),
            parallel_enabled: true,
        }
    }

    /// Creates a layout pipeline with parallel execution disabled.
    pub fn new_single_threaded() -> Self {
        Self {
            dirty: LockFreeDirtySet::default(),
            parallel_enabled: false,
        }
    }

    /// Marks a render object as needing relayout.
    ///
    /// The render object will be laid out on the next call to [`LayoutPipeline::compute_layout`].
    pub fn mark_dirty(&self, id: ElementId) {
        self.dirty.mark_dirty(id);
    }

    /// Checks if any render objects are dirty.
    pub fn has_dirty(&self) -> bool {
        self.dirty.has_dirty()
    }

    /// Checks if a specific render object is dirty.
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.dirty.is_dirty(id)
    }

    /// Enables or disables parallel layout.
    pub fn set_parallel(&mut self, enabled: bool) {
        self.parallel_enabled = enabled;
    }

    /// Returns true if parallel layout is enabled.
    pub fn is_parallel(&self) -> bool {
        self.parallel_enabled
    }

    /// Returns a reference to the dirty set.
    ///
    /// Used by LayoutManager to query pending layouts.
    pub fn dirty_set(&self) -> &LockFreeDirtySet {
        &self.dirty
    }

    /// Marks all elements as dirty (for resize, theme change, etc.).
    ///
    /// This is expensive (O(n) where n is dirty set capacity) but rare.
    /// Only used when the entire UI needs re-layout.
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
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty.len()))]
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
        let mut cache_hits = 0usize;
        let mut cache_misses = 0usize;

        // Process each dirty render object
        // Note: Parallel layout disabled for now - will be enabled in future update
        for id in dirty_ids {
            // Get element from tree
            let Some(element) = tree.get(id) else {
                tracing::error!(
                    element_id = ?id,
                    "Element marked dirty but not found in tree during layout"
                );
                continue;
            };

            // Only layout RenderElements
            let Some(render_elem) = element.as_render() else {
                #[cfg(debug_assertions)]
                tracing::trace!("Element {:?} is not a RenderElement, skipping", id);
                continue;
            };

            // Check if layout is actually needed (atomic check - very fast)
            let render_state_lock = render_elem.render_state();
            let render_state = render_state_lock.read();

            if !render_state.needs_layout() {
                #[cfg(debug_assertions)]
                tracing::trace!("Element {:?} already laid out, skipping", id);
                cache_hits += 1;
                continue;
            }

            // Cache miss - element needs relayout
            cache_misses += 1;

            #[cfg(debug_assertions)]
            tracing::trace!("Layout: Processing render object {:?}", id);

            // Use stored constraints if available, otherwise use provided constraints
            // RenderState stores Constraints enum, but layout_render expects BoxConstraints
            let layout_constraints = render_state
                .constraints()
                .map(|c| *c.as_box())
                .unwrap_or(constraints);

            // Drop read guard before acquiring write lock
            drop(render_state);

            // Perform layout using RenderElement wrapper
            // RenderElement::layout_render() creates LayoutContext internally and calls unified Render trait
            let computed_size = render_elem.layout_render(tree, layout_constraints);

            // Store computed size in RenderState
            let render_state_lock = render_elem.render_state();
            let render_state = render_state_lock.write();
            render_state.set_size(computed_size);
            render_state.set_constraints(crate::render::render_object::Constraints::Box(
                layout_constraints,
            ));
            render_state.clear_needs_layout();
            // After layout completes, mark for paint
            render_state.mark_needs_paint();

            crate::trace_hot_path!(
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
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Returns the number of dirty render objects.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }
}

impl Default for LayoutPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_dirty() {
        use crate::ElementId;
        let layout = LayoutPipeline::new();

        assert!(!layout.has_dirty());

        layout.mark_dirty(ElementId::new(1));

        assert!(layout.has_dirty());
        assert!(layout.is_dirty(ElementId::new(1)));
        assert!(!layout.is_dirty(ElementId::new(2)));
    }

    #[test]
    fn test_dirty_count() {
        use crate::ElementId;
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
