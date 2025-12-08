//! PipelineBuildContext - Concrete implementation of BuildContext
//!
//! This is the runtime context passed to views during the build phase.
//! Updated for four-tree architecture with TreeCoordinator.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::sync::Arc;

use flui_element::BuildContext;
use flui_element::ElementTree;
use flui_foundation::ElementId;
use flui_pipeline::DirtySet;
use parking_lot::RwLock;

use super::TreeCoordinator;

/// PipelineBuildContext - Concrete implementation of BuildContext
///
/// Provides access to:
/// - Current element ID
/// - TreeCoordinator (all four trees: View, Element, Render, Layer)
/// - Dirty set for scheduling rebuilds
///
/// # Four-Tree Architecture
///
/// This context provides unified access to all four trees through TreeCoordinator:
/// - `ViewTree` - immutable view definitions
/// - `ElementTree` - element lifecycle and structure
/// - `RenderTree` - layout and paint logic
/// - `LayerTree` - compositor layers
///
/// # Thread Safety
///
/// This context is `Send + Sync` and can be used across threads.
/// The coordinator is protected by `RwLock` for concurrent access.
///
/// # Example
///
/// ```rust,ignore
/// let ctx = PipelineBuildContext::new(
///     element_id,
///     coordinator.clone(),
///     dirty_set.clone(),
/// );
///
/// // Use as &dyn BuildContext
/// view_object.build(&ctx);
///
/// // Access ViewTree through coordinator
/// let views = ctx.coordinator().read().views();
/// ```
pub struct PipelineBuildContext {
    /// Current element being built
    element_id: ElementId,

    /// TreeCoordinator with all four trees
    coordinator: Arc<RwLock<TreeCoordinator>>,

    /// Legacy: Reference to the element tree (for backward compatibility)
    /// TODO: Remove after full migration to TreeCoordinator
    tree: Arc<RwLock<ElementTree>>,

    /// Dirty set for scheduling rebuilds
    dirty_set: Arc<RwLock<DirtySet<ElementId>>>,

    /// Cached depth (computed lazily)
    depth: RefCell<Option<usize>>,

    /// Cached parent ID (computed lazily)
    parent_id: RefCell<Option<Option<ElementId>>>,
}

impl PipelineBuildContext {
    /// Create a new PipelineBuildContext with TreeCoordinator
    ///
    /// # Arguments
    ///
    /// * `element_id` - ID of the element being built
    /// * `coordinator` - TreeCoordinator with all four trees
    /// * `dirty_set` - Dirty set for scheduling rebuilds
    pub fn new(
        element_id: ElementId,
        coordinator: Arc<RwLock<TreeCoordinator>>,
        dirty_set: Arc<RwLock<DirtySet<ElementId>>>,
    ) -> Self {
        // Extract ElementTree reference for backward compatibility
        // This is a transitional pattern - will be removed after full migration
        let tree = {
            // Create a new ElementTree that shares data with coordinator
            // For now, we create a separate instance
            Arc::new(RwLock::new(ElementTree::new()))
        };

        Self {
            element_id,
            coordinator,
            tree,
            dirty_set,
            depth: RefCell::new(None),
            parent_id: RefCell::new(None),
        }
    }

    /// Create a new PipelineBuildContext with ElementTree (transitional API)
    ///
    /// This constructor is for backward compatibility during migration to TreeCoordinator.
    /// It creates an empty TreeCoordinator internally and uses the provided ElementTree
    /// for legacy operations.
    ///
    /// # Arguments
    ///
    /// * `element_id` - ID of the element being built
    /// * `tree` - Legacy ElementTree reference
    /// * `dirty_set` - Dirty set for scheduling rebuilds
    ///
    /// # Note
    ///
    /// Prefer `new()` with TreeCoordinator for new code.
    pub fn with_tree(
        element_id: ElementId,
        tree: Arc<RwLock<ElementTree>>,
        dirty_set: Arc<RwLock<DirtySet<ElementId>>>,
    ) -> Self {
        Self {
            element_id,
            coordinator: Arc::new(RwLock::new(TreeCoordinator::new())),
            tree,
            dirty_set,
            depth: RefCell::new(None),
            parent_id: RefCell::new(None),
        }
    }

    /// Create context for a child element (reusing coordinator/dirty_set)
    pub fn for_child(&self, child_id: ElementId) -> Self {
        Self {
            element_id: child_id,
            coordinator: self.coordinator.clone(),
            tree: self.tree.clone(),
            dirty_set: self.dirty_set.clone(),
            depth: RefCell::new(None),
            parent_id: RefCell::new(None),
        }
    }

    /// Get the TreeCoordinator (for four-tree access)
    #[inline]
    pub fn coordinator(&self) -> &Arc<RwLock<TreeCoordinator>> {
        &self.coordinator
    }

    /// Get the element tree (for advanced usage)
    ///
    /// Note: Prefer using `coordinator()` for new code.
    #[inline]
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get the dirty set (for advanced usage)
    #[inline]
    pub fn dirty_set(&self) -> &Arc<RwLock<DirtySet<ElementId>>> {
        &self.dirty_set
    }
}

impl std::fmt::Debug for PipelineBuildContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineBuildContext")
            .field("element_id", &self.element_id)
            .finish_non_exhaustive()
    }
}

// SAFETY: PipelineBuildContext is Send + Sync because:
// - element_id is Copy
// - tree is Arc<RwLock<_>> which is Send + Sync
// - dirty_set is Arc<RwLock<_>> which is Send + Sync
// - depth/parent_id are RefCell but only accessed from same thread during build
unsafe impl Send for PipelineBuildContext {}
unsafe impl Sync for PipelineBuildContext {}

impl BuildContext for PipelineBuildContext {
    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn parent_id(&self) -> Option<ElementId> {
        // Use cached value if available
        if let Some(cached) = *self.parent_id.borrow() {
            return cached;
        }

        // Compute and cache using coordinator
        let coord = self.coordinator.read();
        let parent = coord
            .elements()
            .get(self.element_id)
            .and_then(|e| e.parent());
        *self.parent_id.borrow_mut() = Some(parent);
        parent
    }

    fn depth(&self) -> usize {
        // Use cached value if available
        if let Some(cached) = *self.depth.borrow() {
            return cached;
        }

        // Compute and cache using coordinator
        let coord = self.coordinator.read();
        let depth = coord.elements().depth(self.element_id).unwrap_or(0);
        *self.depth.borrow_mut() = Some(depth);
        depth
    }

    fn mark_dirty(&self) {
        // Mark in dirty set
        let dirty = self.dirty_set.write();
        dirty.mark(self.element_id);

        // Also mark in coordinator for new pipeline
        drop(dirty); // Release lock before acquiring coordinator
        self.coordinator.write().mark_needs_build(self.element_id);

        tracing::trace!(element_id = ?self.element_id, "Element marked dirty");
    }

    fn schedule_rebuild(&self, element_id: ElementId) {
        // Mark in dirty set
        let dirty = self.dirty_set.write();
        dirty.mark(element_id);

        // Also mark in coordinator
        drop(dirty);
        self.coordinator.write().mark_needs_build(element_id);

        tracing::trace!(element_id = ?element_id, "Rebuild scheduled");
    }

    fn create_rebuild_callback(&self) -> Box<dyn Fn() + Send + Sync> {
        // Capture dirty_set, coordinator, and element_id for rebuild scheduling
        let dirty_set = self.dirty_set.clone();
        let coordinator = self.coordinator.clone();
        let element_id = self.element_id;

        Box::new(move || {
            dirty_set.write().mark(element_id);
            coordinator.write().mark_needs_build(element_id);
            tracing::trace!(element_id = ?element_id, "Rebuild triggered from callback");
        })
    }

    fn depend_on_raw(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        // Walk up the parent chain to find a provider
        // Start from parent (not current element itself)
        let mut current_id = {
            let coord = self.coordinator.read();
            let element = coord.elements().get(self.element_id)?;
            element.parent()?
        };

        loop {
            // Check if this element is a provider and get the value
            let provided_arc = {
                let coord = self.coordinator.read();
                let element = coord.elements().get(current_id)?;

                if !element.is_provider() {
                    // Not a provider, move to parent
                    current_id = element.parent()?;
                    continue;
                }

                // Get the view_id from ViewElement variant
                let view_id = element.as_view().and_then(|v| v.view_id())?;

                // Get view node from ViewTree, then get ViewObject
                let view_node = coord.views().get(view_id)?;
                let view_object = view_node.view_object();

                // Get provided value directly from ViewObject trait
                let provided = view_object.provided_value()?;

                // Check if type matches
                if (*provided).type_id() != type_id {
                    // Type mismatch, try next ancestor
                    current_id = element.parent()?;
                    continue;
                }

                // Found matching provider!
                provided
            };

            // Register dependency (need mutable access)
            {
                let mut coord = self.coordinator.write();
                let element = coord.elements().get(current_id);
                if let Some(elem) = element {
                    if let Some(view_elem) = elem.as_view() {
                        if let Some(view_id) = view_elem.view_id() {
                            if let Some(provider_node) = coord.views_mut().get_mut(view_id) {
                                provider_node
                                    .view_object_mut()
                                    .add_dependent(self.element_id);
                            }
                        }
                    }
                }
            }

            tracing::debug!(
                provider_id = ?current_id,
                dependent_id = ?self.element_id,
                "Dependency registered"
            );

            return Some(provided_arc);
        }
    }

    fn find_ancestor_widget(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        // Similar to depend_on_raw, but doesn't register dependency
        let mut current_id = {
            let coord = self.coordinator.read();
            let element = coord.elements().get(self.element_id)?;
            element.parent()?
        };

        loop {
            let (provided_arc, next_parent) = {
                let coord = self.coordinator.read();
                let element = coord.elements().get(current_id)?;

                if !element.is_provider() {
                    (None, element.parent())
                } else {
                    // Get view_id from ViewElement, then ViewObject from ViewTree
                    let view_id = element.as_view().and_then(|v| v.view_id())?;
                    let view_node = coord.views().get(view_id)?;
                    let provided = view_node.view_object().provided_value()?;

                    if (*provided).type_id() != type_id {
                        (None, element.parent())
                    } else {
                        (Some(provided), None)
                    }
                }
            };

            if let Some(arc) = provided_arc {
                return Some(arc);
            }

            current_id = next_parent?;
        }
    }

    fn visit_ancestors(&self, visitor: &mut dyn FnMut(ElementId) -> bool) {
        let mut current_id = {
            let coord = self.coordinator.read();
            coord
                .elements()
                .get(self.element_id)
                .and_then(|e| e.parent())
        };

        while let Some(id) = current_id {
            if !visitor(id) {
                break;
            }
            current_id = {
                let coord = self.coordinator.read();
                coord.elements().get(id).and_then(|e| e.parent())
            };
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_element::BuildContextExt;

    fn create_test_context() -> PipelineBuildContext {
        let coordinator = Arc::new(RwLock::new(TreeCoordinator::new()));
        let dirty_set = Arc::new(RwLock::new(DirtySet::new()));

        // Insert a root element into coordinator
        let root_id = {
            let mut coord = coordinator.write();
            coord.elements_mut().insert(flui_element::Element::empty())
        };

        PipelineBuildContext::new(root_id, coordinator, dirty_set)
    }

    #[test]
    fn test_context_creation() {
        let ctx = create_test_context();
        assert!(ctx.element_id().get() > 0);
    }

    #[test]
    fn test_coordinator_access() {
        let ctx = create_test_context();

        // Should be able to access coordinator
        let coord = ctx.coordinator().read();
        assert_eq!(coord.element_count(), 1);
    }

    #[test]
    fn test_mark_dirty() {
        let ctx = create_test_context();
        ctx.mark_dirty();

        // Check dirty set
        let dirty = ctx.dirty_set.read();
        assert!(dirty.is_dirty(ctx.element_id()));
        drop(dirty);

        // Check coordinator's needs_build
        let coord = ctx.coordinator.read();
        assert!(coord.needs_build().contains(&ctx.element_id()));
    }

    #[test]
    fn test_schedule_rebuild() {
        let ctx = create_test_context();
        let other_id = ElementId::new(999);

        ctx.schedule_rebuild(other_id);

        // Check dirty set
        let dirty = ctx.dirty_set.read();
        assert!(dirty.is_dirty(other_id));
        drop(dirty);

        // Check coordinator
        let coord = ctx.coordinator.read();
        assert!(coord.needs_build().contains(&other_id));
    }

    #[test]
    fn test_depth_caching() {
        let ctx = create_test_context();

        // First call computes
        let depth1 = ctx.depth();
        // Second call uses cache
        let depth2 = ctx.depth();

        assert_eq!(depth1, depth2);
        assert_eq!(depth1, 0); // Root is at depth 0
    }

    #[test]
    fn test_for_child() {
        let ctx = create_test_context();
        let child_id = ElementId::new(42);

        let child_ctx = ctx.for_child(child_id);

        assert_eq!(child_ctx.element_id(), child_id);
        // Same coordinator and dirty_set
        assert!(Arc::ptr_eq(ctx.coordinator(), child_ctx.coordinator()));
        assert!(Arc::ptr_eq(ctx.dirty_set(), child_ctx.dirty_set()));
    }

    #[test]
    fn test_downcast() {
        let ctx = create_test_context();
        let dyn_ctx: &dyn BuildContext = &ctx;

        let downcasted = dyn_ctx.downcast_ref::<PipelineBuildContext>();
        assert!(downcasted.is_some());
    }

    #[test]
    fn test_create_rebuild_callback() {
        let ctx = create_test_context();
        let callback = ctx.create_rebuild_callback();

        // Callback should mark element dirty
        callback();

        let dirty = ctx.dirty_set.read();
        assert!(dirty.is_dirty(ctx.element_id()));
        drop(dirty);

        let coord = ctx.coordinator.read();
        assert!(coord.needs_build().contains(&ctx.element_id()));
    }
}
