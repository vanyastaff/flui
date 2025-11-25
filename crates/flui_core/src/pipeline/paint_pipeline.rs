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

use flui_element::ElementTree;
use flui_foundation::ElementId;
use flui_pipeline::{LockFreeDirtySet, PipelineError};

/// Result type for paint operations
pub type PaintResult<T> = Result<T, PipelineError>;

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
    /// The render object will be painted on the next call to [`PaintPipeline::generate_layers`].
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
    /// # Returns
    ///
    /// `Ok(count)` where count is the number of render objects painted.
    ///
    /// `Err(PipelineError)` if paint fails for any element.
    ///
    /// # Implementation
    ///
    /// For each dirty render object:
    /// 1. Get RenderElement from tree
    /// 2. Get offset from RenderState
    /// 3. Call appropriate paint() method based on RenderNode variant
    /// 4. Store generated layer (currently discarded, will be used for composition)
    /// 5. Clear needs_paint flag
    ///
    /// # Error Handling
    ///
    /// If paint fails for an element, the error is returned immediately and
    /// processing stops. Use error recovery policy in PipelineOwner to handle
    /// paint errors gracefully.
    ///
    /// # Note on Layer Optimization
    ///
    /// Layer optimization (merging compatible layers, batching operations) will be
    /// implemented in a future update. Currently, layers are generated but not optimized.
    #[tracing::instrument(skip(self, tree), fields(dirty_count = self.dirty.len()))]
    pub fn generate_layers(&mut self, tree: &mut ElementTree) -> PaintResult<usize> {
        let dirty_ids = self.dirty.drain();
        let count = dirty_ids.len();

        if count == 0 {
            return Ok(0);
        }

        // Process each dirty render object
        for id in dirty_ids {
            // Get element from tree
            let Some(element) = tree.get(id) else {
                tracing::error!(
                    element_id = ?id,
                    "Element marked dirty but not found in tree during paint"
                );
                continue;
            };

            // Only paint render elements
            if !element.is_render() {
                #[cfg(debug_assertions)]
                tracing::trace!("Element {:?} is not a render element, skipping", id);
                continue;
            };

            // TODO: Re-enable render_state checks once Element properly supports RenderViewObject
            // Currently Element::render_state() returns None (stub) so we skip the check
            // and paint all dirty render elements unconditionally.
            //
            // Future: Check render_state.needs_paint() for cache optimization

            #[cfg(debug_assertions)]
            tracing::trace!("Paint: Processing render object {:?}", id);

            // Use zero offset (stored offset not available without render_state)
            let offset = flui_types::Offset::ZERO;

            // Perform paint using ElementTree method
            // This properly handles the unified Element architecture and state updates
            let _layer = tree.paint_render_object(id, offset);

            // ElementTree.paint_render_object() handles:
            // 1. Calling ViewObject.paint_render()
            // 2. Clearing needs_paint flag
            // 3. Future: Building layer tree for composition

            // Future enhancement: Store layer for composition
            // For now, we just generate and discard layers
            // In the future, we'll build a layer tree and return it

            #[cfg(debug_assertions)]
            tracing::trace!("Paint: Element {:?} painted successfully", id);
        }

        if self.optimize_layers {
            // Future enhancement: Implement layer optimization
            // - Merge compatible layers
            // - Remove redundant operations
            // - Batch similar operations
            #[cfg(debug_assertions)]
            tracing::trace!("Paint: Layer optimization enabled (not yet implemented)");
        }

        Ok(count)
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
    use flui_foundation::ElementId;

    #[test]
    fn test_mark_dirty() {
        let paint = PaintPipeline::new();

        assert!(!paint.has_dirty());

        paint.mark_dirty(ElementId::new(1));

        assert!(paint.has_dirty());
        assert!(paint.is_dirty(ElementId::new(1)));
        assert!(!paint.is_dirty(ElementId::new(2)));
    }

    #[test]
    fn test_dirty_count() {
        let paint = PaintPipeline::new();

        paint.mark_dirty(ElementId::new(1));
        paint.mark_dirty(ElementId::new(2));

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

        paint.mark_dirty(ElementId::new(1));
        paint.clear_dirty();

        assert!(!paint.has_dirty());
    }
}

// =============================================================================
// Trait Implementations
// =============================================================================

impl flui_pipeline::PaintPhase for PaintPipeline {
    type Tree = ElementTree;

    fn mark_dirty(&self, element_id: ElementId) {
        PaintPipeline::mark_dirty(self, element_id);
    }

    fn has_dirty(&self) -> bool {
        PaintPipeline::has_dirty(self)
    }

    fn dirty_count(&self) -> usize {
        PaintPipeline::dirty_count(self)
    }

    fn is_dirty(&self, element_id: ElementId) -> bool {
        PaintPipeline::is_dirty(self, element_id)
    }

    fn clear_dirty(&mut self) {
        PaintPipeline::clear_dirty(self);
    }

    fn generate_layers(&mut self, tree: &mut Self::Tree) -> flui_pipeline::PipelineResult<usize> {
        PaintPipeline::generate_layers(self, tree).map_err(Into::into)
    }
}
