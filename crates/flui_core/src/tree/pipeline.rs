//! Pipeline Owner - manages the rendering pipeline
//!
//! This module provides PipelineOwner, which orchestrates the rebuild, layout, and paint phases.
//!
//! # Architecture
//!
//! PipelineOwner is similar to Flutter's PipelineOwner. It coordinates:
//!
//! 1. **Build Phase**: Rebuilds dirty elements via ElementTree
//! 2. **Layout Phase**: Walks render tree and performs layout
//! 3. **Paint Phase**: Walks render tree and paints to screen
//!
//! # Usage
//!
//! ```rust,ignore
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

use crate::{ElementTree, ElementId, Widget};
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
pub struct PipelineOwner {
    /// The element tree
    tree: Arc<RwLock<ElementTree>>,
    /// Root element ID
    root_element_id: Option<ElementId>,
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
    pub fn mount_root(&mut self, root_widget: Box<dyn Widget>) -> ElementId {
        let mut tree_guard = self.tree.write();
        let id = tree_guard.mount_root(root_widget);

        // Set tree reference so ComponentElements can mount children
        tree_guard.set_element_tree_ref(id, self.tree.clone());

        drop(tree_guard);
        self.root_element_id = Some(id);
        id
    }

    /// Mount the root widget - short form
    ///
    /// Rust-idiomatic short name. See [mount_root](Self::mount_root).
    pub fn set_root(&mut self, root_widget: Box<dyn Widget>) -> ElementId {
        self.mount_root(root_widget)
    }

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

    /// Flush the layout phase
    ///
    /// Performs layout on all render objects in the tree.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root constraints (typically screen size)
    ///
    /// # Returns
    ///
    /// The size of the root render object, or None if no root
    pub fn flush_layout(&mut self, constraints: BoxConstraints) -> Option<Size> {
        let mut tree_guard = self.tree.write();
        let render_object = tree_guard.root_render_object_mut()?;
        let size = render_object.layout(constraints);
        Some(size)
    }

    /// Flush the paint phase
    ///
    /// Paints all render objects in the tree.
    ///
    /// # Parameters
    ///
    /// - `painter`: egui Painter for drawing
    /// - `offset`: Global offset for painting
    pub fn flush_paint(&self, painter: &egui::Painter, offset: Offset) {
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
