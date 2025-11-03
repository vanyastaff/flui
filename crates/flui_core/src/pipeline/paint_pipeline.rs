//! Paint pipeline for layer generation phase.
//!
//! The paint pipeline is responsible for:
//! - Generating paint layers for dirty render objects
//! - Building layer tree
//! - Optimizing layer composition
//!
//! # Design
//!
//! The paint phase processes render objects top-down:
//! 1. Identify dirty render objects (marked for repaint)
//! 2. Call `render_object.paint()` for each dirty object
//! 3. Build layer tree for compositor
//! 4. Optimize layer operations
//!
//! # Example
//!
//! ```rust,ignore
//! let mut paint = PaintPipeline::new();
//! paint.mark_dirty(render_id);
//! let layers = paint.generate_layers(&tree);
//! ```

use crate::element::ElementId;
use crate::pipeline::dirty_tracking::LockFreeDirtySet;
use crate::pipeline::element_tree::ElementTree;

/// Paint pipeline manages layer generation phase.
///
/// Tracks which render objects need repainting and generates
/// the layer tree for the compositor.
#[derive(Debug)]
pub struct PaintPipeline {
    /// Set of render objects that need repainting.
    dirty: LockFreeDirtySet,

    /// Whether to enable layer optimization.
    ///
    /// Layer optimization merges compatible layers and removes
    /// redundant operations. Can be disabled for debugging.
    optimize_layers: bool,
}

impl PaintPipeline {
    /// Creates a new paint pipeline.
    ///
    /// Layer optimization is enabled by default.
    pub fn new() -> Self {
        Self {
            dirty: LockFreeDirtySet::default(),
            optimize_layers: true,
        }
    }

    /// Creates a paint pipeline with layer optimization disabled.
    pub fn new_unoptimized() -> Self {
        Self {
            dirty: LockFreeDirtySet::default(),
            optimize_layers: false,
        }
    }

    /// Marks a render object as needing repaint.
    ///
    /// The render object will be painted on the next call to [`generate_layers`].
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

    /// Enables or disables layer optimization.
    pub fn set_optimize_layers(&mut self, enabled: bool) {
        self.optimize_layers = enabled;
    }

    /// Returns true if layer optimization is enabled.
    pub fn is_optimized(&self) -> bool {
        self.optimize_layers
    }

    /// Generates paint layers for all dirty render objects.
    ///
    /// Processes render objects top-down, calling paint methods and
    /// building the layer tree.
    ///
    /// Returns the number of render objects painted.
    pub fn generate_layers(&mut self, tree: &mut ElementTree) -> usize {
        let dirty_ids = self.dirty.drain();
        let count = dirty_ids.len();

        if count == 0 {
            return 0;
        }

        #[cfg(debug_assertions)]
        tracing::debug!("generate_layers: Processing {} dirty render objects", count);

        // TODO(2025-02): Implement actual paint logic.
        // For each dirty render object:
        // 1. Get render object from tree
        // 2. Create paint context
        // 3. Call render_object.paint(context)
        // 4. Build layer tree from paint operations
        //
        // TODO(2025-02): Implement layer optimization.
        // If optimize_layers is true:
        // 1. Merge compatible layers
        // 2. Remove redundant operations
        // 3. Batch similar operations
        //
        // See docs/PIPELINE_ARCHITECTURE.md for detailed algorithm.

        for id in dirty_ids {
            // Verify element exists
            if tree.get(id).is_none() {
                #[cfg(debug_assertions)]
                tracing::warn!("Render object {:?} not found during paint", id);
                continue;
            }

            #[cfg(debug_assertions)]
            tracing::trace!("Paint: Processing render object {:?}", id);

            // Placeholder: actual paint would:
            // - Create PaintContext
            // - Call render_object.paint(context)
            // - Collect paint operations into layer
        }

        if self.optimize_layers {
            // TODO(2025-02): Optimize layer tree
            #[cfg(debug_assertions)]
            tracing::trace!("Paint: Layer optimization enabled (not yet implemented)");
        }

        #[cfg(debug_assertions)]
        tracing::debug!("generate_layers: Complete ({} objects painted)", count);

        count
    }

    /// Clears all dirty render objects without painting.
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Returns the number of dirty render objects.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
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
        let paint = PaintPipeline::new();

        assert!(!paint.has_dirty());

        paint.mark_dirty(1);

        assert!(paint.has_dirty());
        assert!(paint.is_dirty(1));
        assert!(!paint.is_dirty(2));
    }

    #[test]
    fn test_dirty_count() {
        let paint = PaintPipeline::new();

        paint.mark_dirty(1);
        paint.mark_dirty(2);

        assert_eq!(paint.dirty_count(), 2);
    }

    #[test]
    fn test_optimization_mode() {
        let mut paint = PaintPipeline::new();
        assert!(paint.is_optimized());

        paint.set_optimize_layers(false);
        assert!(!paint.is_optimized());

        let unoptimized = PaintPipeline::new_unoptimized();
        assert!(!unoptimized.is_optimized());
    }

    #[test]
    fn test_clear_dirty() {
        let mut paint = PaintPipeline::new();

        paint.mark_dirty(1);
        paint.clear_dirty();

        assert!(!paint.has_dirty());
    }
}
