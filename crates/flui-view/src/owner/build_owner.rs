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

    /// Whether we're currently in a build phase.
    #[cfg(debug_assertions)]
    building: bool,

    /// Build scope nesting depth.
    #[cfg(debug_assertions)]
    scope_depth: usize,
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
            #[cfg(debug_assertions)]
            building: false,
            #[cfg(debug_assertions)]
            scope_depth: 0,
        }
    }

    /// Schedule an element for rebuild.
    ///
    /// Elements are processed in depth order (shallowest first) to ensure
    /// parent rebuilds happen before child rebuilds.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize) {
        if self.dirty_set.insert(id) {
            self.dirty_elements
                .push(Reverse(DirtyElement { id, depth }));
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
    use crate::BuildContext;
    use crate::{StatelessElement, StatelessView, View};

    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(TestView)
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(StatelessElement::new(self))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
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
