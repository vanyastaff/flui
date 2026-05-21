//! ElementBuildContext - concrete BuildContext implementation for Elements.
//!
//! This provides the BuildContext interface during the build phase,
//! giving Views access to tree information and dependency injection.

use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId};
use parking_lot::RwLock;

use super::build_context::BuildContext;
use crate::{element::Notification, owner::BuildOwner, tree::ElementTree};

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
    #[allow(clippy::needless_pass_by_value)] // Arc is cloned into Self, taking by value is idiomatic
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

    /// Walk ancestors of `self.element_id` looking for an element whose
    /// `view_type_id()` matches `type_id`. Returns the first matching
    /// ancestor's `ElementId`.
    ///
    /// Shared helper for U9 (`depend_on_inherited`) and U10
    /// (`get_inherited`). Both perform the same ancestor scan; only the
    /// dependent-recording side differs. Extracting the helper now also
    /// gives U11/U12 an obvious reuse target.
    ///
    /// Flutter parity: `framework.dart:5028-5060` `getElementForInheritedWidgetOfExactType` —
    /// Flutter uses a per-element `_inheritedElements: PersistentHashMap`,
    /// flui walks the ancestor chain directly because the per-element
    /// hash-map isn't necessary at our scale and avoids the
    /// reconciliation-time map-clone cost.
    fn walk_ancestors_for_inherited(&self, type_id: std::any::TypeId) -> Option<ElementId> {
        let tree = self.tree.read();

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

    /// Create a minimal context for use when full tree/owner aren't available.
    ///
    /// This is useful for StatelessElement::perform_build where we just need
    /// a context to pass to view.build() but don't have full tree
    /// infrastructure.
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

    fn depend_on_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Walk ancestors looking for an Element whose view_type_id
        // matches; the first one is the nearest InheritedView<T>.
        //
        // Records this element in the matched InheritedElement's
        // dependent map so a subsequent rebuild with
        // `update_should_notify == true` schedules us for rebuild
        // (R16, plan §U9).
        //
        // Flutter parity: `framework.dart:5081`
        // `dependOnInheritedWidgetOfExactType` -> the matched
        // `InheritedElement` then has `updateDependencies(self, null)`
        // called on it (`framework.dart:5034`).
        let Some(ancestor_id) = self.walk_ancestors_for_inherited(type_id) else {
            return false;
        };

        // Acquire a write lock so we can mutate the matched
        // InheritedElement's dependent map AND invoke the callback with
        // the inherited view in the same critical section. Reading the
        // view itself only needs a read lock, but we need write access
        // to record the dependency, so a single write lock is
        // sufficient.
        let mut tree = self.tree.write();
        let Some(node) = tree.get_mut(ancestor_id) else {
            // Tree shape changed between lookup and write-lock; treat
            // as miss. Should not happen under normal flow.
            return false;
        };

        // Capture self's depth before we hand `node` out so we can
        // record it as the dependent's depth (used by
        // BuildOwner::schedule_build_for during R16 notify).
        let self_depth = self.depth;
        let self_id = self.element_id;

        // The element behind `node.element_mut()` is a
        // `Box<dyn ElementBase>`; downcast to the parametric
        // `InheritedElement<V>` using its TypeId. Because we only have
        // the user-facing TypeId (which is `TypeId::of::<V>()`, the
        // view's TypeId), we can't directly downcast to
        // `InheritedElement<V>` without `V` in scope. Instead we use
        // the `InheritedElementAccess` object-safe protocol surface
        // declared on `ElementBase` via the optional helper trait.
        let Some(accessor) = node.element_mut().as_inherited_mut() else {
            // Matched view-type but not actually an InheritedElement;
            // means a non-inherited view shares the TypeId, which is
            // impossible under TypeId semantics — defensive return.
            return false;
        };

        // Register dependency (id + depth).
        accessor.record_dependent(self_id, self_depth);

        // Hand the view out to the callback. The view reference is
        // borrowed for the lifetime of the callback only; it cannot
        // escape into `build()` because the closure is `FnOnce`.
        let view_any = accessor.view_as_any();
        callback(view_any);

        true
    }

    fn get_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool {
        // Same ancestor walk as depend_on_inherited, but does NOT
        // record a dependency. Reserved for U10 — for now we share the
        // walk + downcast logic and skip the `record_dependent` call.
        let Some(ancestor_id) = self.walk_ancestors_for_inherited(type_id) else {
            return false;
        };

        let tree = self.tree.read();
        let Some(node) = tree.get(ancestor_id) else {
            return false;
        };
        let Some(accessor) = node.element().as_inherited() else {
            return false;
        };
        let view_any = accessor.view_as_any();
        callback(view_any);
        true
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
        while let Some(node) = tree.get(current_id) {
            let Some(parent_id) = node.parent() else {
                break;
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
        while let Some(node) = tree.get(current_id) {
            // Check if this element handles the notification
            // This requires NotifiableElement trait check
            // For now, just walk up
            let _ = notification;

            let Some(parent_id) = node.parent() else {
                break;
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
    #[must_use]
    pub fn tree(mut self, tree: Arc<RwLock<ElementTree>>) -> Self {
        self.tree = Some(tree);
        self
    }

    /// Set the build owner.
    #[must_use]
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
        #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
        name: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(self.clone())
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
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
        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());

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
        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());

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
        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());

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

        let root_id = tree
            .write()
            .mount_root(&root_view, &mut owner.write().element_owner_mut());
        let child_id = tree.write().insert(
            &child_view,
            root_id,
            0,
            &mut owner.write().element_owner_mut(),
        );
        let grandchild_id = tree.write().insert(
            &grandchild_view,
            child_id,
            0,
            &mut owner.write().element_owner_mut(),
        );

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

        let root_id = tree
            .write()
            .mount_root(&view, &mut owner.write().element_owner_mut());
        let child_id = tree.write().insert(
            &child_view,
            root_id,
            0,
            &mut owner.write().element_owner_mut(),
        );

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
