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
/// # Dirty Tracking
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

    // Dirty tracking
    /// RenderObjects that need layout
    nodes_needing_layout: Vec<ElementId>,
    /// RenderObjects that need paint
    nodes_needing_paint: Vec<ElementId>,
    /// RenderObjects that need compositing bits update
    nodes_needing_compositing_bits_update: Vec<ElementId>,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_element_id", &self.root_element_id)
            .field("nodes_needing_layout_count", &self.nodes_needing_layout.len())
            .field("nodes_needing_paint_count", &self.nodes_needing_paint.len())
            .field("nodes_needing_compositing_bits_count", &self.nodes_needing_compositing_bits_update.len())
            .finish()
    }
}

impl PipelineOwner {
    /// Create a new pipeline owner
    pub fn new() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        Self {
            tree,
            root_element_id: None,
            // Initialize dirty tracking lists
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

        // Set tree reference on root element
        if let Some(elem) = tree_guard.get_mut(id) {
            elem.set_tree_ref(self.tree.clone());
        }

        drop(tree_guard);
        self.root_element_id = Some(id);
        id
    }

    // ========== Dirty Tracking API ==========

    /// Request layout for a RenderObject
    ///
    /// Adds the node to the layout dirty list if not already present.
    /// Called by RenderObject::mark_needs_layout().
    pub fn request_layout(&mut self, node_id: ElementId) {
        if !self.nodes_needing_layout.contains(&node_id) {
            self.nodes_needing_layout.push(node_id);
            tracing::trace!("PipelineOwner: requested layout for {:?}", node_id);
        }
    }

    /// Request paint for a RenderObject
    ///
    /// Adds the node to the paint dirty list if not already present.
    /// Called by RenderObject::mark_needs_paint().
    pub fn request_paint(&mut self, node_id: ElementId) {
        if !self.nodes_needing_paint.contains(&node_id) {
            self.nodes_needing_paint.push(node_id);
            tracing::trace!("PipelineOwner: requested paint for {:?}", node_id);
        }
    }

    /// Request compositing bits update for a RenderObject
    ///
    /// Adds the node to the compositing dirty list if not already present.
    pub fn request_compositing_bits_update(&mut self, node_id: ElementId) {
        if !self.nodes_needing_compositing_bits_update.contains(&node_id) {
            self.nodes_needing_compositing_bits_update.push(node_id);
            tracing::trace!("PipelineOwner: requested compositing bits update for {:?}", node_id);
        }
    }

    /// Get count of nodes needing layout
    pub fn layout_dirty_count(&self) -> usize {
        self.nodes_needing_layout.len()
    }

    /// Get count of nodes needing paint
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

        let tree_arc = self.tree.clone();
        let mut tree_guard = self.tree.write();
        tree_guard.rebuild(tree_arc);
        let remaining = tree_guard.dirty_element_count();
        tracing::debug!("PipelineOwner::flush_build: remaining dirty after rebuild: {}", remaining);
    }

    /// Find the first element with a RenderObject, starting from root
    ///
    /// Root might be a ComponentElement (StatelessWidget) which doesn't have
    /// a RenderObject. This method traverses children until finding one.
    fn find_root_render_object_element(&self) -> Option<ElementId> {
        let root_id = self.root_element_id?;
        tracing::debug!("find_root_render_object_element: starting from root {}", root_id);
        let tree_guard = self.tree.read();

        let mut current_id = root_id;
        let mut depth = 0;
        loop {
            let element = tree_guard.get(current_id)?;
            tracing::debug!("  depth {}: checking element {}, has_render_object={}", depth, current_id, element.render_object().is_some());

            // Check if this element has a RenderObject
            if element.render_object().is_some() {
                tracing::info!("find_root_render_object_element: found render object at element {}", current_id);
                return Some(current_id);
            }

            // No RenderObject, try first child
            let mut children = element.children_iter();
            if let Some(child_id) = children.next() {
                current_id = child_id;
                depth += 1;
            } else {
                // No children, no RenderObject found
                tracing::warn!("find_root_render_object_element: no RenderObject found (reached leaf at depth {})", depth);
                return None;
            }
        }
    }

    /// Flush the layout phase
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
        tracing::info!("PipelineOwner::flush_layout: called with constraints {:?}", constraints);

        // Process dirty nodes if any
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

        // Find the first element with a RenderObject (might not be root if root is ComponentElement)
        let render_object_element_id = self.find_root_render_object_element()?;

        tracing::debug!("PipelineOwner::flush_layout: found render object at element {}", render_object_element_id);

        // Get read access to tree - layout() uses interior mutability via Mutex
        let tree_guard = self.tree.read();

        if let Some(root_elem) = tree_guard.get(render_object_element_id) {
            if let Some(ro) = root_elem.render_object() {
                tracing::info!("PipelineOwner::flush_layout: performing layout on element {}", render_object_element_id);

                // Create RenderContext with the real tree so RenderObjects can access children
                let ctx = crate::render::RenderContext::new(&*tree_guard, render_object_element_id);

                // Ensure render_state exists and get it
                tree_guard.ensure_render_state(render_object_element_id);
                let mut state = tree_guard.render_state_mut(render_object_element_id)
                    .expect("render_state should exist after ensure_render_state");

                let size = ro.layout(&mut *state, constraints, &ctx);
                tracing::debug!("PipelineOwner::flush_layout: layout complete, size = {:?}", size);
                return Some(size);
            }
        }

        None
    }

    /// Flush the paint phase
    ///
    /// Paints dirty RenderObjects only for incremental rendering.
    /// If no dirty nodes, paints the entire tree.
    ///
    /// # Parameters
    ///
    /// - `painter`: egui Painter for drawing
    /// - `offset`: Global offset for painting
    pub fn flush_paint(&mut self, painter: &egui::Painter, offset: Offset) {
        // Process dirty nodes if any
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

        // Find the first element with a RenderObject and paint it
        if let Some(render_object_element_id) = self.find_root_render_object_element() {
            // Get RenderObject and paint it
            let tree_guard = self.tree.read();
            let ctx = crate::render::RenderContext::new(&*tree_guard, render_object_element_id);

            if let Some(elem) = tree_guard.get(render_object_element_id) {
                if let Some(ro) = elem.render_object() {
                    tracing::debug!("PipelineOwner::flush_paint: painting element {}", render_object_element_id);

                    // Get render_state for painting
                    if let Some(state) = tree_guard.render_state(render_object_element_id) {
                        ro.paint(&*state, painter, offset, &ctx);
                    } else {
                        tracing::warn!("flush_paint: element {} has no render_state", render_object_element_id);
                    }
                }
            }
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

        // Get root render object
        if let Some(root_id) = self.root_element_id {
            if let Some(root_elem) = tree_guard.get(root_id) {
                if let Some(render_object) = root_elem.render_object() {
                    let position = event.position();

                    // Create RenderContext for hit testing
                    let ctx = crate::render::RenderContext::new(&tree_guard, root_id);
                    let hit = render_object.hit_test(&mut result, position, &ctx);

                    if hit {
                        tracing::debug!(
                            "Hit test for {:?} at {:?}: {} entries",
                            event,
                            position,
                            result.entries().len()
                        );
                    }
                }
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
