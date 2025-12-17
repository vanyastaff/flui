//! BuildOwner - Manages the build phase.
//!
//! The BuildOwner is responsible for:
//! - Tracking dirty elements that need rebuilding
//! - Processing rebuilds in depth-first order
//! - Managing GlobalKey registry
//! - Coordinating InheritedElement lookups

use crate::tree::ElementTree;
use flui_foundation::ElementId;
use std::any::TypeId;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Entry in the dirty elements heap.
///
/// Sorted by depth (shallowest first) for top-down processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirtyElement {
    id: ElementId,
    depth: usize,
}

impl Ord for DirtyElement {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Min-heap by depth (process shallowest first)
        self.depth.cmp(&other.depth)
    }
}

impl PartialOrd for DirtyElement {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Manages the build phase of the element lifecycle.
///
/// BuildOwner tracks which elements need rebuilding and processes them
/// in the correct order (depth-first, shallowest first).
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `BuildOwner` class.
///
/// # Responsibilities
///
/// - Maintain list of dirty elements
/// - Process rebuilds in correct order
/// - Manage GlobalKey registry
/// - Track InheritedElement locations for O(1) lookup
/// - Track inactive elements for finalization
pub struct BuildOwner {
    /// Elements that need rebuild, sorted by depth.
    dirty_elements: BinaryHeap<Reverse<DirtyElement>>,

    /// Set of dirty element IDs (for deduplication).
    dirty_set: std::collections::HashSet<ElementId>,

    /// GlobalKey registry: key hash -> element ID.
    global_keys: HashMap<u64, ElementId>,

    /// InheritedElement registry: TypeId -> element ID.
    /// Used for O(1) InheritedView lookup.
    inherited_elements: HashMap<TypeId, ElementId>,

    /// Elements that have been deactivated and are pending unmount.
    /// These are unmounted in `finalize_tree()`.
    inactive_elements: Vec<InactiveElement>,

    /// Whether we're currently in a build phase.
    #[cfg(debug_assertions)]
    building: bool,

    /// Build scope nesting depth.
    #[cfg(debug_assertions)]
    scope_depth: usize,

    /// Callback to be called when a build is scheduled.
    #[allow(clippy::type_complexity)]
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
}

/// An element that has been deactivated and is pending unmount.
#[derive(Debug, Clone, Copy)]
struct InactiveElement {
    id: ElementId,
    depth: usize,
}

impl Default for BuildOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl BuildOwner {
    /// Create a new BuildOwner.
    pub fn new() -> Self {
        Self {
            dirty_elements: BinaryHeap::new(),
            dirty_set: std::collections::HashSet::new(),
            global_keys: HashMap::new(),
            inherited_elements: HashMap::new(),
            inactive_elements: Vec::new(),
            #[cfg(debug_assertions)]
            building: false,
            #[cfg(debug_assertions)]
            scope_depth: 0,
            on_build_scheduled: None,
        }
    }

    /// Set the callback for when a build is scheduled.
    ///
    /// This is called by `schedule_build_for` to notify the binding
    /// that a visual update is needed.
    pub fn set_on_build_scheduled<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_build_scheduled = Some(Box::new(callback));
    }

    /// Schedule an element for rebuild.
    ///
    /// Elements are processed in depth order (shallowest first) to ensure
    /// parent rebuilds happen before child rebuilds.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize) {
        if self.dirty_set.insert(id) {
            self.dirty_elements
                .push(Reverse(DirtyElement { id, depth }));

            // Notify that a build was scheduled
            if let Some(ref callback) = self.on_build_scheduled {
                callback();
            }
        }
    }

    /// Check if there are dirty elements.
    pub fn has_dirty_elements(&self) -> bool {
        !self.dirty_elements.is_empty()
    }

    /// Get the number of dirty elements.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }

    /// Process all dirty elements.
    ///
    /// Rebuilds elements in depth order (shallowest first). This ensures
    /// that when a parent rebuilds, any children that become dirty are
    /// processed after the parent.
    ///
    /// # Arguments
    ///
    /// * `tree` - The element tree to rebuild
    pub fn build_scope(&mut self, tree: &mut ElementTree) {
        #[cfg(debug_assertions)]
        {
            assert!(!self.building, "build_scope called while already building");
            self.building = true;
            self.scope_depth += 1;
        }

        // Process dirty elements in depth order
        while let Some(Reverse(dirty)) = self.dirty_elements.pop() {
            self.dirty_set.remove(&dirty.id);

            // Skip if element no longer exists
            if let Some(node) = tree.get_mut(dirty.id) {
                // Only rebuild if still active
                if node.element().lifecycle().can_build() {
                    node.element_mut().perform_build();
                }
            }
        }

        #[cfg(debug_assertions)]
        {
            self.building = false;
            self.scope_depth -= 1;
        }
    }

    // ========================================================================
    // Inactive Elements (for finalization)
    // ========================================================================

    /// Add an element to the inactive list.
    ///
    /// Called when an element is deactivated (e.g., its parent rebuilds without it).
    /// The element will be unmounted in `finalize_tree()`.
    pub fn add_to_inactive(&mut self, id: ElementId, depth: usize) {
        self.inactive_elements.push(InactiveElement { id, depth });
    }

    /// Remove an element from the inactive list.
    ///
    /// Called when an element is reactivated (e.g., moved via GlobalKey).
    pub fn remove_from_inactive(&mut self, id: ElementId) {
        self.inactive_elements.retain(|e| e.id != id);
    }

    /// Check if there are inactive elements pending unmount.
    pub fn has_inactive_elements(&self) -> bool {
        !self.inactive_elements.is_empty()
    }

    /// Complete the element build pass by unmounting inactive elements.
    ///
    /// This is called by `WidgetsBinding.draw_frame()` after `build_scope()`
    /// and `super.draw_frame()` (layout/paint).
    ///
    /// Elements are unmounted in reverse depth order (deepest first) to ensure
    /// children are unmounted before parents.
    pub fn finalize_tree(&mut self, tree: &mut ElementTree) {
        if self.inactive_elements.is_empty() {
            return;
        }

        tracing::debug!(
            count = self.inactive_elements.len(),
            "Finalizing tree - unmounting inactive elements"
        );

        // Sort by depth (deepest first for unmounting)
        self.inactive_elements.sort_by(|a, b| b.depth.cmp(&a.depth));

        // Take ownership of inactive elements to avoid borrow conflicts
        let inactive_elements: Vec<_> = self.inactive_elements.drain(..).collect();

        // Collect all elements to unmount (including children)
        let mut elements_to_unmount = Vec::new();
        for inactive in &inactive_elements {
            Self::collect_elements_to_unmount(tree, inactive.id, &mut elements_to_unmount);
        }

        // Unmount all elements (deepest first - already sorted by collect order)
        for id in elements_to_unmount.iter().rev() {
            if let Some(node) = tree.get_mut(*id) {
                node.element_mut().unmount();
            }
        }

        // Remove all elements from tree
        for id in elements_to_unmount {
            tree.remove(id);
        }

        tracing::debug!("Finalize tree complete");
    }

    /// Recursively collect all element IDs to unmount (breadth-first).
    fn collect_elements_to_unmount(tree: &ElementTree, id: ElementId, result: &mut Vec<ElementId>) {
        // Add this element
        result.push(id);

        // Collect children
        if let Some(node) = tree.get(id) {
            let mut children = Vec::new();
            node.element().visit_children(&mut |child_id| {
                children.push(child_id);
            });

            for child_id in children {
                Self::collect_elements_to_unmount(tree, child_id, result);
            }
        }
    }

    /// Lock the build scope (for debugging).
    ///
    /// Returns a guard that unlocks when dropped.
    #[cfg(debug_assertions)]
    pub fn lock_build_scope(&mut self) -> BuildScopeGuard<'_> {
        assert!(!self.building, "Already in build scope");
        self.building = true;
        BuildScopeGuard { owner: self }
    }

    // ========================================================================
    // GlobalKey Registry
    // ========================================================================

    /// Register a GlobalKey for an element.
    ///
    /// GlobalKeys allow elements to be found and reparented across the tree.
    pub fn register_global_key(&mut self, key_hash: u64, element: ElementId) {
        self.global_keys.insert(key_hash, element);
    }

    /// Unregister a GlobalKey.
    pub fn unregister_global_key(&mut self, key_hash: u64) {
        self.global_keys.remove(&key_hash);
    }

    /// Look up an element by GlobalKey.
    pub fn element_for_global_key(&self, key_hash: u64) -> Option<ElementId> {
        self.global_keys.get(&key_hash).copied()
    }

    // ========================================================================
    // InheritedElement Registry
    // ========================================================================

    /// Register an InheritedElement for O(1) lookup.
    ///
    /// This allows `depend_on<T>()` to be O(1) instead of O(depth).
    pub fn register_inherited(&mut self, type_id: TypeId, element: ElementId) {
        self.inherited_elements.insert(type_id, element);
    }

    /// Unregister an InheritedElement.
    pub fn unregister_inherited(&mut self, type_id: TypeId) {
        self.inherited_elements.remove(&type_id);
    }

    /// Look up an InheritedElement by type.
    pub fn inherited_element(&self, type_id: TypeId) -> Option<ElementId> {
        self.inherited_elements.get(&type_id).copied()
    }

    /// Check if we're currently building.
    #[cfg(debug_assertions)]
    pub fn is_building(&self) -> bool {
        self.building
    }

    /// Get the current scope depth.
    #[cfg(debug_assertions)]
    pub fn scope_depth(&self) -> usize {
        self.scope_depth
    }
}

impl std::fmt::Debug for BuildOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOwner")
            .field("dirty_count", &self.dirty_elements.len())
            .field("global_keys", &self.global_keys.len())
            .field("inherited_elements", &self.inherited_elements.len())
            .finish()
    }
}

/// Guard for build scope (debug only).
#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct BuildScopeGuard<'a> {
    owner: &'a mut BuildOwner,
}

#[cfg(debug_assertions)]
impl Drop for BuildScopeGuard<'_> {
    fn drop(&mut self) {
        self.owner.building = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::ElementTree;
    use crate::{Lifecycle, View};

    /// A leaf element that doesn't create children (prevents infinite recursion)
    struct LeafElement {
        depth: usize,
        lifecycle: Lifecycle,
    }

    impl LeafElement {
        fn new() -> Self {
            Self {
                depth: 0,
                lifecycle: Lifecycle::Initial,
            }
        }
    }

    impl crate::ElementBase for LeafElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<TestView>()
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn lifecycle(&self) -> Lifecycle {
            self.lifecycle
        }

        fn mount(&mut self, _parent: Option<ElementId>, slot: usize) {
            self.depth = slot;
            self.lifecycle = Lifecycle::Active;
        }

        fn unmount(&mut self) {
            self.lifecycle = Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View) {}

        fn mark_needs_build(&mut self) {}

        fn perform_build(&mut self) {
            // Leaf - no children to build
        }

        fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {
            // No children
        }
    }

    /// A leaf view that creates a LeafElement (no children)
    #[derive(Clone)]
    struct TestView;

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(LeafElement::new())
        }
    }

    #[test]
    fn test_build_owner_creation() {
        let owner = BuildOwner::new();
        assert!(!owner.has_dirty_elements());
        assert_eq!(owner.dirty_count(), 0);
    }

    #[test]
    fn test_schedule_build() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(1);

        owner.schedule_build_for(id, 0);
        assert!(owner.has_dirty_elements());
        assert_eq!(owner.dirty_count(), 1);

        // Duplicate scheduling should not increase count
        owner.schedule_build_for(id, 0);
        assert_eq!(owner.dirty_count(), 1);
    }

    #[test]
    fn test_build_scope() {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();

        let view = TestView;
        let root_id = tree.mount_root(&view);

        owner.schedule_build_for(root_id, 0);
        assert!(owner.has_dirty_elements());

        owner.build_scope(&mut tree);
        assert!(!owner.has_dirty_elements());
    }

    #[test]
    fn test_depth_ordering() {
        let mut owner = BuildOwner::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        // Schedule in reverse depth order
        owner.schedule_build_for(id3, 2);
        owner.schedule_build_for(id1, 0);
        owner.schedule_build_for(id2, 1);

        // Should process shallowest first
        let Reverse(first) = owner.dirty_elements.pop().unwrap();
        assert_eq!(first.depth, 0);

        let Reverse(second) = owner.dirty_elements.pop().unwrap();
        assert_eq!(second.depth, 1);

        let Reverse(third) = owner.dirty_elements.pop().unwrap();
        assert_eq!(third.depth, 2);
    }

    #[test]
    fn test_global_key_registry() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(42);
        let key_hash = 12345u64;

        owner.register_global_key(key_hash, id);
        assert_eq!(owner.element_for_global_key(key_hash), Some(id));

        owner.unregister_global_key(key_hash);
        assert_eq!(owner.element_for_global_key(key_hash), None);
    }

    #[test]
    fn test_inherited_registry() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(42);
        let type_id = TypeId::of::<String>();

        owner.register_inherited(type_id, id);
        assert_eq!(owner.inherited_element(type_id), Some(id));

        owner.unregister_inherited(type_id);
        assert_eq!(owner.inherited_element(type_id), None);
    }
}
