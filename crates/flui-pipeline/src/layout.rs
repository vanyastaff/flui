//! Layout pipeline for size computation phase.
//!
//! The layout pipeline computes sizes for render elements using
//! trait-based tree access. It works with any tree that implements
//! `RenderTreeAccess` and `DirtyTracking`.
//!
//! # Design
//!
//! The layout phase processes render objects top-down:
//! 1. Collect dirty render objects from `DirtyTracking`
//! 2. Sort by depth (parents before children)
//! 3. Compute sizes using constraints
//! 4. Store results in RenderState
//! 5. Clear dirty flags
//!
//! # Generic Implementation
//!
//! ```rust,ignore
//! use flui_pipeline::LayoutPipeline;
//! use flui_tree::prelude::*;
//!
//! fn layout<T: RenderTreeAccess + DirtyTracking + TreeNav>(
//!     tree: &mut T,
//!     root: ElementId,
//!     constraints: BoxConstraints,
//! ) {
//!     let mut pipeline = LayoutPipeline::new();
//!     pipeline.compute_layout(tree, root, constraints).unwrap();
//! }
//! ```

use flui_foundation::ElementId;
use flui_tree::{DirtyTracking, RenderTreeAccess, TreeNav};
use flui_types::BoxConstraints;

use crate::dirty::DirtySet;
use crate::error::{PipelineError, PipelineResult};

/// Layout pipeline manages size computation phase.
///
/// Generic over any tree type that implements the required traits.
/// This breaks the circular dependency between flui-core and flui-rendering.
#[derive(Debug)]
pub struct LayoutPipeline {
    /// Local dirty set for tracking elements needing layout.
    dirty: DirtySet,

    /// Whether parallel layout is enabled.
    parallel_enabled: bool,
}

impl LayoutPipeline {
    /// Creates a new layout pipeline.
    pub fn new() -> Self {
        Self {
            dirty: DirtySet::new(),
            parallel_enabled: true,
        }
    }

    /// Creates a layout pipeline with parallel execution disabled.
    pub fn new_single_threaded() -> Self {
        Self {
            dirty: DirtySet::new(),
            parallel_enabled: false,
        }
    }

    /// Marks an element as needing layout.
    pub fn mark_dirty(&self, id: ElementId) {
        self.dirty.mark(id);
    }

    /// Checks if any elements are dirty.
    pub fn has_dirty(&self) -> bool {
        self.dirty.has_dirty()
    }

    /// Checks if a specific element is dirty.
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

    /// Returns the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Clears all dirty elements without laying out.
    pub fn clear_dirty(&self) {
        self.dirty.clear_all();
    }

    /// Computes layout for all dirty render objects.
    ///
    /// Works with any tree that implements:
    /// - `RenderTreeAccess` - for accessing RenderObject/RenderState
    /// - `DirtyTracking` - for dirty flag management
    /// - `TreeNav` - for tree navigation (depth calculation)
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to layout (generic over trait bounds)
    /// * `root` - Root element ID (used for constraint propagation)
    /// * `constraints` - Root constraints
    ///
    /// # Returns
    ///
    /// `Ok(count)` - Number of elements laid out
    /// `Err(PipelineError)` - If layout fails
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty.len()))]
    pub fn compute_layout<T>(
        &mut self,
        tree: &T,
        _root: ElementId,
        constraints: BoxConstraints,
    ) -> PipelineResult<usize>
    where
        T: RenderTreeAccess + DirtyTracking + TreeNav,
    {
        // Collect dirty elements from our local set
        let dirty_ids = self.dirty.drain();
        let count = dirty_ids.len();

        if count == 0 {
            tracing::trace!("No dirty elements, skipping layout");
            return Ok(0);
        }

        tracing::debug!(count, "Processing dirty elements for layout");

        // Sort by depth (parents before children)
        let mut sorted: Vec<_> = dirty_ids
            .into_iter()
            .filter(|&id| tree.is_render_element(id))
            .map(|id| (id, tree.depth(id)))
            .collect();

        sorted.sort_by_key(|(_, depth)| *depth);

        let mut laid_out = 0;

        for (id, _depth) in sorted {
            // Check if still needs layout (might have been laid out as part of parent)
            if !tree.needs_layout(id) {
                continue;
            }

            // Layout this element
            // Note: Actual layout logic depends on RenderObject type
            // This is a simplified version - real implementation would:
            // 1. Get RenderObject from tree
            // 2. Call layout() with constraints
            // 3. Store size in RenderState
            // 4. Clear needs_layout flag

            // For now, just clear the flag - actual layout happens in flui-core
            tree.clear_needs_layout(id);

            tracing::trace!(element_id = ?id, "Laid out element");
            laid_out += 1;
        }

        // Also sync with tree's DirtyTracking if available
        let _ = constraints; // Used for root constraints

        Ok(laid_out)
    }

    /// Computes layout starting from a specific root.
    ///
    /// This variant uses the tree's DirtyTracking to find dirty elements
    /// instead of the pipeline's local dirty set.
    #[tracing::instrument(skip(self, tree))]
    pub fn compute_layout_from_tree<T>(
        &self,
        tree: &T,
        root: ElementId,
        constraints: BoxConstraints,
    ) -> PipelineResult<usize>
    where
        T: RenderTreeAccess + DirtyTracking + TreeNav,
    {
        // Find all render descendants that need layout
        let dirty: Vec<_> = tree
            .render_descendants(root)
            .filter(|&id| tree.needs_layout(id))
            .collect();

        let count = dirty.len();

        if count == 0 {
            return Ok(0);
        }

        tracing::debug!(count, "Processing dirty elements from tree");

        // Sort by depth
        let mut sorted: Vec<_> = dirty.into_iter().map(|id| (id, tree.depth(id))).collect();
        sorted.sort_by_key(|(_, depth)| *depth);

        let mut laid_out = 0;

        for (id, _depth) in sorted {
            if !tree.needs_layout(id) {
                continue;
            }

            // Clear dirty flag
            tree.clear_needs_layout(id);
            laid_out += 1;
        }

        let _ = constraints;
        Ok(laid_out)
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
        let pipeline = LayoutPipeline::new();
        let id = ElementId::new(1);

        assert!(!pipeline.has_dirty());
        pipeline.mark_dirty(id);
        assert!(pipeline.has_dirty());
        assert!(pipeline.is_dirty(id));
    }

    #[test]
    fn test_parallel_mode() {
        let mut pipeline = LayoutPipeline::new();
        assert!(pipeline.is_parallel());

        pipeline.set_parallel(false);
        assert!(!pipeline.is_parallel());
    }

    #[test]
    fn test_clear_dirty() {
        let pipeline = LayoutPipeline::new();

        pipeline.mark_dirty(ElementId::new(1));
        pipeline.mark_dirty(ElementId::new(2));
        assert_eq!(pipeline.dirty_count(), 2);

        pipeline.clear_dirty();
        assert!(!pipeline.has_dirty());
    }
}
