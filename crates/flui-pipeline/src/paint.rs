//! Paint pipeline for layer generation phase.
//!
//! The paint pipeline generates paint layers for render elements using
//! trait-based tree access. It works with any tree that implements
//! `RenderTreeAccess` and `DirtyTracking`.
//!
//! # Design
//!
//! The paint phase processes render objects top-down:
//! 1. Collect dirty render objects from `DirtyTracking`
//! 2. Call paint() for each dirty object
//! 3. Build layer tree for compositor
//! 4. Clear dirty flags
//!
//! # Generic Implementation
//!
//! ```rust,ignore
//! use flui_pipeline::PaintPipeline;
//! use flui_tree::prelude::*;
//!
//! fn paint<T: RenderTreeAccess + DirtyTracking + TreeNav>(
//!     tree: &mut T,
//!     root: ElementId,
//! ) {
//!     let mut pipeline = PaintPipeline::new();
//!     pipeline.generate_layers(tree, root).unwrap();
//! }
//! ```

use flui_foundation::ElementId;
use flui_tree::{DirtyTracking, RenderTreeAccess, TreeNav};

use crate::dirty::DirtySet;
use crate::error::{PipelineError, PipelineResult};

/// Paint pipeline manages layer generation phase.
///
/// Generic over any tree type that implements the required traits.
#[derive(Debug)]
pub struct PaintPipeline {
    /// Local dirty set for tracking elements needing paint.
    dirty: DirtySet,

    /// Whether layer optimization is enabled.
    optimize_layers: bool,
}

impl PaintPipeline {
    /// Creates a new paint pipeline.
    pub fn new() -> Self {
        Self {
            dirty: DirtySet::new(),
            optimize_layers: true,
        }
    }

    /// Creates a paint pipeline with layer optimization disabled.
    pub fn new_unoptimized() -> Self {
        Self {
            dirty: DirtySet::new(),
            optimize_layers: false,
        }
    }

    /// Marks an element as needing paint.
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

    /// Enables or disables layer optimization.
    pub fn set_optimize_layers(&mut self, enabled: bool) {
        self.optimize_layers = enabled;
    }

    /// Returns true if layer optimization is enabled.
    pub fn is_optimized(&self) -> bool {
        self.optimize_layers
    }

    /// Returns the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Clears all dirty elements without painting.
    pub fn clear_dirty(&self) {
        self.dirty.clear_all();
    }

    /// Generates paint layers for all dirty render objects.
    ///
    /// Works with any tree that implements:
    /// - `RenderTreeAccess` - for accessing RenderObject/RenderState
    /// - `DirtyTracking` - for dirty flag management
    /// - `TreeNav` - for tree navigation
    ///
    /// # Arguments
    ///
    /// * `tree` - The tree to paint (generic over trait bounds)
    /// * `root` - Root element ID
    ///
    /// # Returns
    ///
    /// `Ok(count)` - Number of elements painted
    /// `Err(PipelineError)` - If paint fails
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty.len()))]
    pub fn generate_layers<T>(&mut self, tree: &T, _root: ElementId) -> PipelineResult<usize>
    where
        T: RenderTreeAccess + DirtyTracking + TreeNav,
    {
        // Collect dirty elements from our local set
        let dirty_ids = self.dirty.drain();
        let count = dirty_ids.len();

        if count == 0 {
            tracing::trace!("No dirty elements, skipping paint");
            return Ok(0);
        }

        tracing::debug!(count, "Processing dirty elements for paint");

        let mut painted = 0;

        for id in dirty_ids {
            // Only paint render elements
            if !tree.is_render_element(id) {
                continue;
            }

            // Check if still needs paint
            if !tree.needs_paint(id) {
                continue;
            }

            // Get offset from RenderState for positioning
            let _offset = tree.get_offset(id).unwrap_or((0.0, 0.0));

            // Paint this element
            // Note: Actual paint logic depends on RenderObject type
            // This is a simplified version - real implementation would:
            // 1. Get RenderObject from tree
            // 2. Call paint() with canvas/offset
            // 3. Build layer for compositor

            // Clear dirty flag
            tree.clear_needs_paint(id);

            tracing::trace!(element_id = ?id, "Painted element");
            painted += 1;
        }

        if self.optimize_layers {
            // Future: Layer optimization
            tracing::trace!("Layer optimization enabled");
        }

        Ok(painted)
    }

    /// Generates layers starting from a specific root.
    ///
    /// This variant uses the tree's DirtyTracking to find dirty elements.
    #[tracing::instrument(skip(self, tree))]
    pub fn generate_layers_from_tree<T>(&self, tree: &T, root: ElementId) -> PipelineResult<usize>
    where
        T: RenderTreeAccess + DirtyTracking + TreeNav,
    {
        // Find all render descendants that need paint
        let dirty: Vec<_> = tree
            .render_descendants(root)
            .filter(|&id| tree.needs_paint(id))
            .collect();

        let count = dirty.len();

        if count == 0 {
            return Ok(0);
        }

        tracing::debug!(count, "Processing dirty elements from tree");

        let mut painted = 0;

        for id in dirty {
            if !tree.needs_paint(id) {
                continue;
            }

            // Clear dirty flag
            tree.clear_needs_paint(id);
            painted += 1;
        }

        Ok(painted)
    }
}

impl Default for PaintPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_dirty() {
        let pipeline = PaintPipeline::new();
        let id = ElementId::new(1);

        assert!(!pipeline.has_dirty());
        pipeline.mark_dirty(id);
        assert!(pipeline.has_dirty());
        assert!(pipeline.is_dirty(id));
    }

    #[test]
    fn test_optimization_mode() {
        let mut pipeline = PaintPipeline::new();
        assert!(pipeline.is_optimized());

        pipeline.set_optimize_layers(false);
        assert!(!pipeline.is_optimized());
    }

    #[test]
    fn test_clear_dirty() {
        let pipeline = PaintPipeline::new();

        pipeline.mark_dirty(ElementId::new(1));
        pipeline.mark_dirty(ElementId::new(2));
        assert_eq!(pipeline.dirty_count(), 2);

        pipeline.clear_dirty();
        assert!(!pipeline.has_dirty());
    }
}
