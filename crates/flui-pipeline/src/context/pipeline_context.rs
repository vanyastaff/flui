//! PipelineBuildContext - Concrete implementation of BuildContext
//!
//! This is the runtime context passed to views during the build phase.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::sync::Arc;

use flui_element::BuildContext;
use flui_element::ElementTree;
use flui_foundation::ElementId;
use parking_lot::RwLock;

use crate::dirty::DirtySet;

/// PipelineBuildContext - Concrete implementation of BuildContext
///
/// Provides access to:
/// - Current element ID
/// - Element tree (via Arc<RwLock>)
/// - Dirty set for scheduling rebuilds
///
/// # Thread Safety
///
/// This context is `Send + Sync` and can be used across threads.
/// The tree is protected by `RwLock` for concurrent access.
///
/// # Example
///
/// ```rust,ignore
/// let ctx = PipelineBuildContext::new(
///     element_id,
///     tree.clone(),
///     dirty_set.clone(),
/// );
///
/// // Use as &dyn BuildContext
/// view_object.build(&ctx);
/// ```
pub struct PipelineBuildContext {
    /// Current element being built
    element_id: ElementId,

    /// Reference to the element tree
    tree: Arc<RwLock<ElementTree>>,

    /// Dirty set for scheduling rebuilds
    dirty_set: Arc<RwLock<DirtySet>>,

    /// Cached depth (computed lazily)
    depth: RefCell<Option<usize>>,

    /// Cached parent ID (computed lazily)
    parent_id: RefCell<Option<Option<ElementId>>>,
}

impl PipelineBuildContext {
    /// Create a new PipelineBuildContext
    pub fn new(
        element_id: ElementId,
        tree: Arc<RwLock<ElementTree>>,
        dirty_set: Arc<RwLock<DirtySet>>,
    ) -> Self {
        Self {
            element_id,
            tree,
            dirty_set,
            depth: RefCell::new(None),
            parent_id: RefCell::new(None),
        }
    }

    /// Create context for a child element (reusing tree/dirty_set)
    pub fn for_child(&self, child_id: ElementId) -> Self {
        Self {
            element_id: child_id,
            tree: self.tree.clone(),
            dirty_set: self.dirty_set.clone(),
            depth: RefCell::new(None),
            parent_id: RefCell::new(None),
        }
    }

    /// Get the element tree (for advanced usage)
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get the dirty set (for advanced usage)
    pub fn dirty_set(&self) -> &Arc<RwLock<DirtySet>> {
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

        // Compute and cache
        let tree = self.tree.read();
        let parent = tree.get(self.element_id).and_then(|e| e.parent());
        *self.parent_id.borrow_mut() = Some(parent);
        parent
    }

    fn depth(&self) -> usize {
        // Use cached value if available
        if let Some(cached) = *self.depth.borrow() {
            return cached;
        }

        // Compute and cache
        let tree = self.tree.read();
        let depth = tree.depth(self.element_id).unwrap_or(0);
        *self.depth.borrow_mut() = Some(depth);
        depth
    }

    fn mark_dirty(&self) {
        let dirty = self.dirty_set.write();
        dirty.mark(self.element_id);
        tracing::trace!(element_id = ?self.element_id, "Element marked dirty");
    }

    fn schedule_rebuild(&self, element_id: ElementId) {
        let dirty = self.dirty_set.write();
        dirty.mark(element_id);
        tracing::trace!(element_id = ?element_id, "Rebuild scheduled");
    }

    fn create_rebuild_callback(&self) -> Box<dyn Fn() + Send + Sync> {
        // Capture dirty_set and element_id for rebuild scheduling
        let dirty_set = self.dirty_set.clone();
        let element_id = self.element_id;

        Box::new(move || {
            dirty_set.write().mark(element_id);
            tracing::trace!(element_id = ?element_id, "Rebuild triggered from callback");
        })
    }

    fn depend_on_raw(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        // Walk up the parent chain to find a provider
        // Start from parent (not current element itself)
        let mut current_id = {
            let tree = self.tree.read();
            let element = tree.get(self.element_id)?;
            element.parent()?
        };

        loop {
            // Check if this element is a provider and get the value
            let provided_arc = {
                let tree = self.tree.read();
                let element = tree.get(current_id)?;

                if !element.is_provider() {
                    // Not a provider, move to parent
                    current_id = element.parent()?;
                    continue;
                }

                // Get the view object and check if it provides the right type
                let view_object = element.view_object()?;

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
                let mut tree_mut = self.tree.write();
                if let Some(provider_elem) = tree_mut.get_mut(current_id) {
                    if let Some(provider_vo) = provider_elem.view_object_mut() {
                        provider_vo.add_dependent(self.element_id);
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
            let tree = self.tree.read();
            let element = tree.get(self.element_id)?;
            element.parent()?
        };

        loop {
            let (provided_arc, next_parent) = {
                let tree = self.tree.read();
                let element = tree.get(current_id)?;

                if !element.is_provider() {
                    (None, element.parent())
                } else {
                    let view_object = element.view_object()?;
                    let provided = view_object.provided_value()?;

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
            let tree = self.tree.read();
            tree.get(self.element_id).and_then(|e| e.parent())
        };

        while let Some(id) = current_id {
            if !visitor(id) {
                break;
            }
            current_id = {
                let tree = self.tree.read();
                tree.get(id).and_then(|e| e.parent())
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
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let dirty_set = Arc::new(RwLock::new(DirtySet::new()));

        // Insert a root element
        let root_id = {
            let mut t = tree.write();
            t.insert(flui_element::Element::empty())
        };

        PipelineBuildContext::new(root_id, tree, dirty_set)
    }

    #[test]
    fn test_context_creation() {
        let ctx = create_test_context();
        assert!(ctx.element_id().get() > 0);
    }

    #[test]
    fn test_mark_dirty() {
        let ctx = create_test_context();
        ctx.mark_dirty();

        let dirty = ctx.dirty_set.read();
        assert!(dirty.is_dirty(ctx.element_id()));
    }

    #[test]
    fn test_schedule_rebuild() {
        let ctx = create_test_context();
        let other_id = ElementId::new(999);

        ctx.schedule_rebuild(other_id);

        let dirty = ctx.dirty_set.read();
        assert!(dirty.is_dirty(other_id));
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
        // Same tree and dirty_set
        assert!(Arc::ptr_eq(ctx.tree(), child_ctx.tree()));
        assert!(Arc::ptr_eq(ctx.dirty_set(), child_ctx.dirty_set()));
    }

    #[test]
    fn test_downcast() {
        let ctx = create_test_context();
        let dyn_ctx: &dyn BuildContext = &ctx;

        let downcasted = dyn_ctx.downcast_ref::<PipelineBuildContext>();
        assert!(downcasted.is_some());
    }
}
