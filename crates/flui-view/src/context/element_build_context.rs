//! ElementBuildContext - concrete BuildContext implementation for Elements.
//!
//! This provides the BuildContext interface during the build phase,
//! giving Views access to tree information and dependency injection.

use super::build_context::BuildContext;
use crate::element::Notification;
use crate::owner::BuildOwner;
use crate::tree::ElementTree;
use flui_foundation::{ElementId, RenderId};
use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::sync::Arc;

/// Concrete BuildContext implementation for Elements.
///
/// `ElementBuildContext` provides the bridge between Elements and the
/// BuildContext trait, giving Views access to:
/// - Element identity and tree position
/// - InheritedView lookups (O(1) via BuildOwner registry)
/// - Ancestor traversal
/// - Rebuild scheduling
///
/// # Thread Safety
///
/// This struct holds Arc references to shared state, making it safe to
/// use across threads. The actual tree/owner access is synchronized via RwLock.
///
/// # Flutter Equivalent
///
/// In Flutter, `Element` implements `BuildContext` directly. Here we use
/// a separate struct to avoid borrow checker issues with self-referential
/// borrows during build.
pub struct ElementBuildContext {
    /// The ElementId this context represents.
    element_id: ElementId,

    /// Depth in the element tree.
    depth: usize,

    /// Whether the element is currently mounted.
    mounted: bool,

    /// Reference to the element tree.
    tree: Arc<RwLock<ElementTree>>,

    /// Reference to the build owner.
    owner: Arc<RwLock<BuildOwner>>,

    /// Whether we're currently in a build phase (debug only).
    #[cfg(debug_assertions)]
    is_building: bool,
}

impl std::fmt::Debug for ElementBuildContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementBuildContext")
            .field("element_id", &self.element_id)
            .field("depth", &self.depth)
            .field("mounted", &self.mounted)
            .finish_non_exhaustive()
    }
}

impl ElementBuildContext {
    /// Create a new ElementBuildContext.
    ///
    /// # Arguments
    ///
    /// * `element_id` - The ElementId this context represents
    /// * `depth` - Depth in the element tree
    /// * `mounted` - Whether the element is currently mounted
    /// * `tree` - Shared reference to the element tree
    /// * `owner` - Shared reference to the build owner
    pub fn new(
        element_id: ElementId,
        depth: usize,
        mounted: bool,
        tree: Arc<RwLock<ElementTree>>,
        owner: Arc<RwLock<BuildOwner>>,
    ) -> Self {
        Self {
            element_id,
            depth,
            mounted,
            tree,
            owner,
            #[cfg(debug_assertions)]
            is_building: false,
        }
    }

    /// Create a context for a specific element from the tree.
    ///
    /// Returns None if the element doesn't exist in the tree.
    pub fn for_element(
        element_id: ElementId,
        tree: Arc<RwLock<ElementTree>>,
        owner: Arc<RwLock<BuildOwner>>,
    ) -> Option<Self> {
        let tree_guard = tree.read();
        let node = tree_guard.get(element_id)?;

        Some(Self {
            element_id,
            depth: node.depth(),
            mounted: node.element().mounted(),
            tree: tree.clone(),
            owner,
            #[cfg(debug_assertions)]
            is_building: false,
        })
    }

    /// Set the building flag (debug only).
    #[cfg(debug_assertions)]
    pub fn set_building(&mut self, building: bool) {
        self.is_building = building;
    }

    /// Get a reference to the tree.
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get a reference to the owner.
    pub fn build_owner(&self) -> &Arc<RwLock<BuildOwner>> {
        &self.owner
    }

    /// Create a minimal context for use when full tree/owner aren't available.
    ///
    /// This is useful for StatelessElement::perform_build where we just need
    /// a context to pass to view.build() but don't have full tree infrastructure.
    pub fn new_minimal(depth: usize) -> Self {
        // Create dummy tree and owner for minimal context
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));
        // ElementId::new(1) is safe - 1 is non-zero
        let element_id = ElementId::new(1);

        Self {
            element_id,
            depth,
            mounted: true,
            tree,
            owner,
            #[cfg(debug_assertions)]
            is_building: true,
        }
    }
}

impl BuildContext for ElementBuildContext {
    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn mounted(&self) -> bool {
        self.mounted
    }

    fn is_building(&self) -> bool {
        #[cfg(debug_assertions)]
        {
            self.is_building
        }
        #[cfg(not(debug_assertions))]
        {
            false
        }
    }

    fn owner(&self) -> Option<&BuildOwner> {
        // We can't return a reference to data behind RwLock directly
        // This method may need redesign or the trait needs adjustment
        // For now, return None - callers should use build_owner() method instead
        None
    }

    fn depend_on_inherited(&self, type_id: TypeId) -> Option<&dyn Any> {
        // O(1) lookup via BuildOwner's inherited registry
        let owner = self.owner.read();
        let element_id = owner.inherited_element(type_id)?;
        drop(owner);

        // Get the inherited element and extract data
        let tree = self.tree.read();
        let node = tree.get(element_id)?;

        // TODO: Register dependency for rebuild notifications
        // For now, just return the data
        // The actual data extraction requires InheritedElement to expose its data

        // Note: We can't return a reference to data inside RwLock guard
        // This requires architectural changes - either:
        // 1. Store inherited data separately with longer lifetime
        // 2. Return owned data (clone)
        // 3. Use callback pattern instead of returning reference

        let _ = node;
        None // Placeholder - needs architectural solution
    }

    fn get_inherited(&self, type_id: TypeId) -> Option<&dyn Any> {
        // Same as depend_on_inherited but without registering dependency
        let owner = self.owner.read();
        let element_id = owner.inherited_element(type_id)?;
        drop(owner);

        let tree = self.tree.read();
        let _node = tree.get(element_id)?;

        None // Placeholder - needs architectural solution
    }

    fn find_ancestor_element(&self, type_id: TypeId) -> Option<ElementId> {
        let tree = self.tree.read();

        // Walk up from current element
        let mut current_id = self.element_id;
        loop {
            let node = tree.get(current_id)?;
            let parent_id = node.parent()?;

            let parent_node = tree.get(parent_id)?;
            if parent_node.element().view_type_id() == type_id {
                return Some(parent_id);
            }

            current_id = parent_id;
        }
    }

    fn find_ancestor_view(&self, type_id: TypeId) -> Option<&dyn Any> {
        // Similar lifetime issue as depend_on_inherited
        let _ = type_id;
        None
    }

    fn find_ancestor_state(&self, type_id: TypeId) -> Option<&dyn Any> {
        let _ = type_id;
        None
    }

    fn find_root_ancestor_state(&self, type_id: TypeId) -> Option<&dyn Any> {
        let _ = type_id;
        None
    }

    fn find_render_object(&self) -> Option<RenderId> {
        // Walk down to find first RenderObject
        // For now, return None - this requires RenderElement integration
        None
    }

    fn visit_ancestor_elements(&self, visitor: &mut dyn FnMut(ElementId) -> bool) {
        let tree = self.tree.read();

        let mut current_id = self.element_id;
        loop {
            let node = match tree.get(current_id) {
                Some(n) => n,
                None => break,
            };

            let parent_id = match node.parent() {
                Some(p) => p,
                None => break,
            };

            if !visitor(parent_id) {
                break;
            }

            current_id = parent_id;
        }
    }

    fn visit_child_elements(&self, visitor: &mut dyn FnMut(ElementId)) {
        #[cfg(debug_assertions)]
        {
            assert!(
                !self.is_building,
                "visit_child_elements cannot be called during build"
            );
        }

        let tree = self.tree.read();
        if let Some(node) = tree.get(self.element_id) {
            node.element().visit_children(visitor);
        }
    }

    fn mark_needs_build(&self) {
        let mut owner = self.owner.write();
        owner.schedule_build_for(self.element_id, self.depth);
    }

    fn dispatch_notification(&self, notification: &dyn Notification) {
        let tree = self.tree.read();

        // Bubble up from current element
        let mut current_id = self.element_id;
        loop {
            let node = match tree.get(current_id) {
                Some(n) => n,
                None => break,
            };

            // Check if this element handles the notification
            // This requires NotifiableElement trait check
            // For now, just walk up
            let _ = notification;

            let parent_id = match node.parent() {
                Some(p) => p,
                None => break,
            };

            current_id = parent_id;
        }
    }
}

// ============================================================================
// Builder for ElementBuildContext
// ============================================================================

/// Builder for creating ElementBuildContext instances.
#[derive(Debug)]
pub struct ElementBuildContextBuilder {
    tree: Option<Arc<RwLock<ElementTree>>>,
    owner: Option<Arc<RwLock<BuildOwner>>>,
}

impl Default for ElementBuildContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ElementBuildContextBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            tree: None,
            owner: None,
        }
    }

    /// Set the element tree.
    pub fn tree(mut self, tree: Arc<RwLock<ElementTree>>) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Set the build owner.
    pub fn owner(mut self, owner: Arc<RwLock<BuildOwner>>) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Build a context for the given element.
    pub fn build_for(self, element_id: ElementId) -> Option<ElementBuildContext> {
        let tree = self.tree?;
        let owner = self.owner?;

        ElementBuildContext::for_element(element_id, tree, owner)
    }

    /// Create shared tree and owner, returning them along with the builder.
    pub fn with_new_tree_and_owner(
        self,
    ) -> (Self, Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        (self.tree(tree.clone()).owner(owner.clone()), tree, owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StatelessElement, StatelessView, View};

    #[derive(Clone)]
    struct TestView {
        name: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(self.clone())
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(StatelessElement::new(self))
        }
    }

    #[test]
    fn test_context_creation() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        // Mount a root element
        let view = TestView {
            name: "root".to_string(),
        };
        let root_id = tree.write().mount_root(&view);

        // Create context for it
        let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone());

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.element_id(), root_id);
        assert_eq!(ctx.depth(), 0);
        assert!(ctx.mounted());
    }

    #[test]
    fn test_context_builder() {
        let (builder, tree, owner) = ElementBuildContextBuilder::new().with_new_tree_and_owner();

        let view = TestView {
            name: "test".to_string(),
        };
        let root_id = tree.write().mount_root(&view);

        let ctx = builder.build_for(root_id);
        assert!(ctx.is_some());
    }

    #[test]
    fn test_mark_needs_build() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        let view = TestView {
            name: "root".to_string(),
        };
        let root_id = tree.write().mount_root(&view);

        let ctx = ElementBuildContext::for_element(root_id, tree.clone(), owner.clone()).unwrap();

        assert!(!owner.read().has_dirty_elements());

        ctx.mark_needs_build();

        assert!(owner.read().has_dirty_elements());
    }

    #[test]
    fn test_visit_ancestor_elements() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        // Create tree: root -> child -> grandchild
        let root_view = TestView {
            name: "root".to_string(),
        };
        let child_view = TestView {
            name: "child".to_string(),
        };
        let grandchild_view = TestView {
            name: "grandchild".to_string(),
        };

        let root_id = tree.write().mount_root(&root_view);
        let child_id = tree.write().insert(&child_view, root_id, 0);
        let grandchild_id = tree.write().insert(&grandchild_view, child_id, 0);

        // Create context for grandchild
        let ctx =
            ElementBuildContext::for_element(grandchild_id, tree.clone(), owner.clone()).unwrap();

        // Visit ancestors
        let mut ancestors = Vec::new();
        ctx.visit_ancestor_elements(&mut |id| {
            ancestors.push(id);
            true
        });

        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0], child_id);
        assert_eq!(ancestors[1], root_id);
    }

    #[test]
    fn test_find_ancestor_element() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let owner = Arc::new(RwLock::new(BuildOwner::new()));

        let view = TestView {
            name: "root".to_string(),
        };
        let child_view = TestView {
            name: "child".to_string(),
        };

        let root_id = tree.write().mount_root(&view);
        let child_id = tree.write().insert(&child_view, root_id, 0);

        let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

        // Find ancestor of same type
        let ancestor = ctx.find_ancestor_element(TypeId::of::<TestView>());
        assert_eq!(ancestor, Some(root_id));
    }

    #[test]
    fn test_context_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ElementBuildContext>();
    }
}
