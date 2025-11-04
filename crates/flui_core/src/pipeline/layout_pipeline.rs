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
use crate::pipeline::dirty_tracking::LockFreeDirtySet;
use crate::pipeline::element_tree::ElementTree;
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
    /// The render object will be laid out on the next call to [`compute_layout`].
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
    pub fn compute_layout(
        &mut self,
        tree: &mut ElementTree,
        constraints: BoxConstraints,
    ) -> LayoutResult<usize> {
        let dirty_ids = self.dirty.drain();
        let count = dirty_ids.len();

        if count == 0 {
            return Ok(0);
        }

        #[cfg(debug_assertions)]
        tracing::debug!("compute_layout: Processing {} dirty render objects", count);

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
            let crate::element::Element::Render(render_elem) = element else {
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
                continue;
            }

            #[cfg(debug_assertions)]
            tracing::trace!("Layout: Processing render object {:?}", id);

            // Use stored constraints if available, otherwise use provided constraints
            let layout_constraints = render_state.constraints().unwrap_or(constraints);

            // Drop read guard before acquiring write lock
            drop(render_state);

            // Perform layout based on RenderNode variant
            let render_object = render_elem.render_object();
            let computed_size = match &*render_object {
                crate::render::RenderNode::Leaf(_leaf) => {
                    // SAFETY: We need mutable access to call layout(), but we only have &self.
                    // We use interior mutability through RwLock in the render object.
                    // This is safe because:
                    // 1. We're calling layout() which requires &mut self
                    // 2. RwLock ensures exclusive access during layout
                    // 3. No other code can access this render object during layout

                    // Drop read guard to get write guard
                    drop(render_object);
                    let mut render_object_mut = render_elem.render_object_mut();

                    if let crate::render::RenderNode::Leaf(leaf) = &mut *render_object_mut {
                        leaf.layout(layout_constraints)
                    } else {
                        unreachable!("RenderNode variant changed during layout")
                    }
                }

                crate::render::RenderNode::Single { child, .. } => {
                    // Handle case where Single node doesn't have child yet (mounting phase)
                    let child_id_copy = *child;

                    match child_id_copy {
                        Some(child_id) => {
                            // Drop read guard to get write guard
                            drop(render_object);
                            let mut render_object_mut = render_elem.render_object_mut();

                            if let crate::render::RenderNode::Single { render, .. } = &mut *render_object_mut {
                                render.layout(tree, child_id, layout_constraints)
                            } else {
                                unreachable!("RenderNode variant changed during layout")
                            }
                        }
                        None => {
                            tracing::warn!(
                                element_id = ?id,
                                "Single render node has no child. Returning zero size."
                            );
                            // Return zero size constrained by layout_constraints
                            layout_constraints.constrain(flui_types::Size::ZERO)
                        }
                    }
                }

                crate::render::RenderNode::Multi { children, .. } => {
                    let children_ids = children.clone();

                    // Drop read guard to get write guard
                    drop(render_object);
                    let mut render_object_mut = render_elem.render_object_mut();

                    if let crate::render::RenderNode::Multi { render, .. } = &mut *render_object_mut {
                        render.layout(tree, &children_ids, layout_constraints)
                    } else {
                        unreachable!("RenderNode variant changed during layout")
                    }
                }
            };

            // Store computed size in RenderState
            let render_state_lock = render_elem.render_state();
            let render_state = render_state_lock.write();
            render_state.set_size(computed_size);
            render_state.set_constraints(layout_constraints);
            render_state.clear_needs_layout();

            #[cfg(debug_assertions)]
            tracing::trace!(
                "Layout: Element {:?} computed size {:?}",
                id,
                computed_size
            );
        }

        #[cfg(debug_assertions)]
        tracing::debug!("compute_layout: Complete ({} objects processed)", count);

        Ok(count)
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
        let layout = LayoutPipeline::new();

        assert!(!layout.has_dirty());

        layout.mark_dirty(1);

        assert!(layout.has_dirty());
        assert!(layout.is_dirty(1));
        assert!(!layout.is_dirty(2));
    }

    #[test]
    fn test_dirty_count() {
        let layout = LayoutPipeline::new();

        layout.mark_dirty(1);
        layout.mark_dirty(2);

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

        layout.mark_dirty(1);
        layout.clear_dirty();

        assert!(!layout.has_dirty());
    }
}
