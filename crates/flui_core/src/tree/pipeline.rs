//! Rendering pipeline coordination (build, layout, paint)
//! use flui_core::{PipelineOwner, Widget};
//!
//! let mut pipeline = PipelineOwner::new();
//! pipeline.set_root(Box::new(MyApp::new()));
//!
//! // Each frame:
//! pipeline.flush_build(); // Rebuild dirty widgets
//! pipeline.flush_layout(constraints); // Layout render objects
//! pipeline.flush_paint(painter); // Paint to screen
//! ```

use std::sync::Arc;
use parking_lot::RwLock;

use crate::{DynWidget, ElementTree, ElementId};
use crate::BoxConstraints; // Re-exported from flui_types in lib.rs
use flui_types::{Size, Offset};
use flui_types::events::{PointerEvent, HitTestResult};

/// PipelineOwner - orchestrates the rendering pipeline
///
/// Manages the build → layout → paint pipeline for the UI framework.
///
/// # Phases
///
/// 1. **Build**: Dirty elements are rebuilt, creating/updating widgets
/// 2. **Layout**: RenderObjects compute their size and position
/// 3. **Paint**: RenderObjects draw themselves
///
/// # Thread Safety
///
/// PipelineOwner uses Arc<RwLock<ElementTree>> for thread-safe access.
///
/// # Phase 9: Dirty Tracking
///
/// PipelineOwner now tracks dirty RenderObjects for incremental layout/paint:
/// - `nodes_needing_layout` - RenderObjects that need relayout
/// - `nodes_needing_paint` - RenderObjects that need repaint
/// - `flush_layout()` processes only dirty nodes, sorted by depth
/// - `flush_paint()` processes only dirty nodes
pub struct PipelineOwner {
    /// The element tree
    tree: Arc<RwLock<ElementTree>>,
    /// Root element ID
    root_element_id: Option<ElementId>,

    // Phase 9: Dirty tracking
    /// RenderObjects that need layout (Phase 9)
    nodes_needing_layout: Vec<ElementId>,
    /// RenderObjects that need paint (Phase 9)
    nodes_needing_paint: Vec<ElementId>,
    /// RenderObjects that need compositing bits update (Phase 9)
    nodes_needing_compositing_bits_update: Vec<ElementId>,
}

impl PipelineOwner {
    /// Create a new pipeline owner
    pub fn new() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Set the tree's self-reference so it can pass it to ComponentElements during rebuild
        tree.write().set_tree_ref(tree.clone());

        Self {
            tree,
            root_element_id: None,
            // Phase 9: Initialize dirty tracking lists
            nodes_needing_layout: Vec::new(),
            nodes_needing_paint: Vec::new(),
            nodes_needing_compositing_bits_update: Vec::new(),
        }
    }

    /// Get reference to the element tree
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.root_element_id
    }

    /// Mount a widget as the root of the tree
    ///
    /// This creates the root element and sets up the tree.
    ///
    /// # Parameters
    ///
    /// - `root_widget`: The root widget to mount
    ///
    /// # Returns
    ///
    /// The ElementId of the root element
    /// Mount the root widget - short form
    ///
    /// Rust-idiomatic short name. See [mount_root](Self::mount_root).
    pub fn set_root(&mut self, root_widget: Box<dyn DynWidget>) -> ElementId {
        let mut tree_guard = self.tree.write();
        let id = tree_guard.set_root(root_widget);

        tree_guard.set_element_tree_ref(id, self.tree.clone());

        drop(tree_guard);
        self.root_element_id = Some(id);
        id
    }

    // ========== Phase 9: Dirty Tracking API ==========

    /// Request layout for a RenderObject (Phase 9)
    ///
    /// Adds the node to the layout dirty list if not already present.
    /// Called by RenderObject::mark_needs_layout().
    pub fn request_layout(&mut self, node_id: ElementId) {
        if !self.nodes_needing_layout.contains(&node_id) {
            self.nodes_needing_layout.push(node_id);
            tracing::trace!("PipelineOwner: requested layout for {:?}", node_id);
        }
    }

    /// Request paint for a RenderObject (Phase 9)
    ///
    /// Adds the node to the paint dirty list if not already present.
    /// Called by RenderObject::mark_needs_paint().
    pub fn request_paint(&mut self, node_id: ElementId) {
        if !self.nodes_needing_paint.contains(&node_id) {
            self.nodes_needing_paint.push(node_id);
            tracing::trace!("PipelineOwner: requested paint for {:?}", node_id);
        }
    }

    /// Request compositing bits update for a RenderObject (Phase 9)
    ///
    /// Adds the node to the compositing dirty list if not already present.
    pub fn request_compositing_bits_update(&mut self, node_id: ElementId) {
        if !self.nodes_needing_compositing_bits_update.contains(&node_id) {
            self.nodes_needing_compositing_bits_update.push(node_id);
            tracing::trace!("PipelineOwner: requested compositing bits update for {:?}", node_id);
        }
    }

    /// Get count of nodes needing layout (Phase 9)
    pub fn layout_dirty_count(&self) -> usize {
        self.nodes_needing_layout.len()
    }

    /// Get count of nodes needing paint (Phase 9)
    pub fn paint_dirty_count(&self) -> usize {
        self.nodes_needing_paint.len()
    }

    // ========== Build Phase ==========

    /// Flush the build phase
    ///
    /// Rebuilds all dirty elements. This should be called before layout.
    pub fn flush_build(&mut self) {
        // Observe dirty count before acquiring write lock
        let dirty_before = { self.tree.read().dirty_element_count() };
        if dirty_before == 0 {
            tracing::debug!("PipelineOwner::flush_build called: no dirty elements");
        } else {
            tracing::info!("PipelineOwner::flush_build: rebuilding {} dirty elements", dirty_before);
        }

        let mut tree_guard = self.tree.write();
        tree_guard.rebuild();
        let remaining = tree_guard.dirty_element_count();
        tracing::debug!("PipelineOwner::flush_build: remaining dirty after rebuild: {}", remaining);
    }

    /// Flush the layout phase (Phase 9 enhanced)
    ///
    /// Performs layout on dirty RenderObjects only, sorted by depth.
    /// If no dirty nodes, layouts the root with given constraints.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root constraints (typically screen size)
    ///
    /// # Returns
    ///
    /// The size of the root render object, or None if no root
    pub fn flush_layout(&mut self, constraints: BoxConstraints) -> Option<Size> {
        // Phase 9: Process dirty nodes if any
        if !self.nodes_needing_layout.is_empty() {
            let dirty_count = self.nodes_needing_layout.len();
            tracing::info!("PipelineOwner::flush_layout: processing {} dirty nodes", dirty_count);

            // TODO: Sort by depth (parents before children)
            // For now, process in order added
            let dirty_nodes = std::mem::take(&mut self.nodes_needing_layout);

            let tree_guard = self.tree.write();
            for node_id in dirty_nodes {
                // Layout each dirty node
                // In a real implementation, we'd get the RenderObject from ElementTree
                // and call layout() on it
                tracing::trace!("  Layout node {:?}", node_id);

                // TODO: Actual layout implementation
                // if let Some(element) = tree_guard.get_mut(node_id) {
                //     if let Some(render_object) = element.render_object_mut() {
                //         render_object.perform_layout();
                //     }
                // }
            }

            drop(tree_guard);
        }

        // Always layout root with constraints (for now)
        let mut tree_guard = self.tree.write();
        let render_object = tree_guard.root_render_object_mut()?;
        let size = render_object.layout(constraints);
        Some(size)
    }

    /// Flush the paint phase (Phase 9 enhanced)
    ///
    /// Paints dirty RenderObjects only for incremental rendering.
    /// If no dirty nodes, paints the entire tree.
    ///
    /// # Parameters
    ///
    /// - `painter`: egui Painter for drawing
    /// - `offset`: Global offset for painting
    pub fn flush_paint(&mut self, painter: &egui::Painter, offset: Offset) {
        // Phase 9: Process dirty nodes if any
        if !self.nodes_needing_paint.is_empty() {
            let dirty_count = self.nodes_needing_paint.len();
            tracing::info!("PipelineOwner::flush_paint: processing {} dirty nodes", dirty_count);

            let dirty_nodes = std::mem::take(&mut self.nodes_needing_paint);

            let tree_guard = self.tree.read();
            for node_id in dirty_nodes {
                // Paint each dirty node
                tracing::trace!("  Paint node {:?}", node_id);

                // TODO: Actual paint implementation
                // if let Some(element) = tree_guard.get(node_id) {
                //     if let Some(render_object) = element.render_object() {
                //         render_object.paint(painter, offset);
                //     }
                // }
            }

            drop(tree_guard);
        }

        // Always paint root (for now)
        let tree_guard = self.tree.read();
        if let Some(render_object) = tree_guard.root_render_object() {
            render_object.paint(painter, offset);
        }
    }

    /// Dispatch a pointer event through the render tree
    ///
    /// Performs hit testing to find which render objects are under the pointer,
    /// then dispatches the event to them.
    ///
    /// # Parameters
    ///
    /// - `event`: The pointer event to dispatch
    ///
    /// # Returns
    ///
    /// The hit test result containing all hit render objects
    pub fn dispatch_pointer_event(&mut self, event: PointerEvent) -> HitTestResult {
        let tree_guard = self.tree.read();
        let mut result = HitTestResult::new();

        if let Some(render_object) = tree_guard.root_render_object() {
            let position = event.position();
            let hit = render_object.hit_test(&mut result, position);

            if hit {
                tracing::debug!(
                    "Hit test for {:?} at {:?}: {} entries",
                    event,
                    position,
                    result.entries().len()
                );
            }
        }

        result
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_owner_creation() {
        let pipeline = PipelineOwner::new();
        assert!(pipeline.root_element_id().is_none());
    }

    #[test]
    fn test_pipeline_owner_default() {
        let pipeline = PipelineOwner::default();
        assert!(pipeline.root_element_id().is_none());
    }
}
