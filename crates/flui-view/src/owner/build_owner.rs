//! BuildOwner - Manages the build phase.
//!
//! The BuildOwner is responsible for:
//! - Tracking dirty elements that need rebuilding
//! - Processing rebuilds in depth-first order
//! - Managing GlobalKey registry
//! - Coordinating InheritedElement lookups

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::{Arc, OnceLock},
};

use flui_foundation::ElementId;
use parking_lot::RwLock;

use crate::tree::ElementTree;

/// Entry in the dirty elements heap.
///
/// Sorted by depth (shallowest first) for top-down processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirtyElement {
    id: ElementId,
    depth: usize,
}

impl DirtyElement {
    /// Construct a new dirty-elements heap entry.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self { id, depth }
    }

    /// The element id queued for rebuild.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order the heap (shallowest first).
    ///
    /// Currently consumed only by inline tests; U9+ will read it during
    /// dirty-element drain dispatching. The `Ord` impl reads
    /// `self.depth` directly (private field access from the same `impl`
    /// block), so the accessor stays on the surface for future
    /// `ElementOwner` consumers.
    #[allow(dead_code)]
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
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
    ///
    /// `pub(crate)` so [`ElementOwner`](super::ElementOwner)'s
    /// split-borrow can pin a `&mut` reference to just this field
    /// during the recursive Element traversal — no full `&mut
    /// BuildOwner` needed.
    pub(crate) dirty_elements: BinaryHeap<Reverse<DirtyElement>>,

    /// Set of dirty element IDs (for deduplication).
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) dirty_set: std::collections::HashSet<ElementId>,

    /// GlobalKey registry: key hash -> element ID.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) global_keys: HashMap<u64, ElementId>,

    /// Elements that have been deactivated and are pending unmount.
    /// These are unmounted in `finalize_tree()`.
    ///
    /// `pub(crate)` for the [`ElementOwner`](super::ElementOwner)
    /// split-borrow.
    pub(crate) inactive_elements: Vec<InactiveElement>,

    /// Whether we're currently in a build phase.
    #[cfg(debug_assertions)]
    building: bool,

    /// Build scope nesting depth.
    #[cfg(debug_assertions)]
    scope_depth: usize,

    /// Callback to be called when a build is scheduled.
    ///
    /// `pub(crate)` so the [`ElementOwner`](super::ElementOwner)
    /// split-borrow can fire it from `schedule_build_for` without
    /// re-borrowing the owner.
    #[allow(clippy::type_complexity)]
    pub(crate) on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
}

/// An element that has been deactivated and is pending unmount.
///
/// Made `pub(crate)` so [`ElementOwner`](super::ElementOwner) can hold a
/// `&mut Vec<InactiveElement>` split-borrow reference. End-of-frame
/// finalization (`BuildOwner::finalize_tree`) drains the queue
/// deepest-first using the recorded `depth`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InactiveElement {
    id: ElementId,
    depth: usize,
}

impl InactiveElement {
    /// Construct a new inactive-element record.
    pub(crate) fn new(id: ElementId, depth: usize) -> Self {
        Self { id, depth }
    }

    /// The element id queued for end-of-frame unmount.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// Depth used to order finalization (deepest first).
    #[allow(dead_code)] // Used by finalize_tree's sort, kept for symmetry.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
}

impl Default for BuildOwner {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-global cache of the dummy `ElementTree` handed out by
/// [`ElementBuildContext::new_minimal`](crate::ElementBuildContext::new_minimal).
///
/// Plan §U12 / R15 — audit V-13 (cheap separable part). Each
/// `StatelessView::build` / `StatefulView::build` allocates a fresh
/// `ElementBuildContext` to satisfy the `&dyn BuildContext` parameter
/// shape. Before V-13 each one called
/// `Arc::new(RwLock::new(ElementTree::new()))` — heap-allocating an Arc
/// inner, a `RwLock` payload, and an empty `Slab`-backed `ElementTree`
/// per build. For animation-driven full-tree rebuilds, that is N heap
/// allocations per frame.
///
/// The dummy is functionally read-only on the production path:
/// `BuildContext::find_ancestor_*`, `depend_on_inherited`, and
/// `find_render_object` all return `None`/`false` immediately because
/// the dummy tree is empty. Every build can safely share one
/// `Arc<RwLock<ElementTree>>` — clones of the shared Arc bump the
/// atomic refcount only.
///
/// The cache is initialized lazily via `OnceLock` and lives for the
/// lifetime of the process. A test or future code path that wants
/// strictly per-binding isolation can still construct an
/// `ElementBuildContext` manually via
/// [`ElementBuildContext::new`](crate::ElementBuildContext::new).
static SHARED_DUMMY_TREE: OnceLock<Arc<RwLock<ElementTree>>> = OnceLock::new();

/// Process-global cache of the dummy `BuildOwner` handed out by
/// [`ElementBuildContext::new_minimal`](crate::ElementBuildContext::new_minimal). Companion to
/// [`SHARED_DUMMY_TREE`] — see that doc for the rationale.
///
/// The inner `BuildOwner` is itself constructed via [`BuildOwner::new`],
/// which sets `on_build_scheduled = None`, so calls to
/// `BuildContext::mark_needs_build` from inside a stateless `build()`
/// (a Flutter-forbidden anti-pattern; flui matches Flutter's policy by
/// design) silently accumulate entries in this shared dummy's
/// `dirty_elements` heap. The accumulation is bounded by however many
/// times misuse occurs and never read because nothing ever calls
/// `build_scope` on the shared dummy.
static SHARED_DUMMY_OWNER: OnceLock<Arc<RwLock<BuildOwner>>> = OnceLock::new();

impl BuildOwner {
    /// Create a new BuildOwner.
    pub fn new() -> Self {
        Self {
            dirty_elements: BinaryHeap::new(),
            dirty_set: std::collections::HashSet::new(),
            global_keys: HashMap::new(),
            inactive_elements: Vec::new(),
            #[cfg(debug_assertions)]
            building: false,
            #[cfg(debug_assertions)]
            scope_depth: 0,
            on_build_scheduled: None,
        }
    }

    /// Acquire a clone of the process-shared dummy `ElementTree` handle
    /// used to back [`ElementBuildContext::new_minimal`](crate::ElementBuildContext::new_minimal).
    ///
    /// First call lazily allocates the empty tree behind a `OnceLock`;
    /// every subsequent call returns an `Arc::clone` of the same inner
    /// pointer — observable via `Arc::ptr_eq`. Audit V-13 (cheap part)
    /// — eliminates the per-build `Arc::new(RwLock::new(_))` allocation
    /// in the stateless/stateful build paths.
    pub fn shared_dummy_tree() -> Arc<RwLock<ElementTree>> {
        Arc::clone(SHARED_DUMMY_TREE.get_or_init(|| Arc::new(RwLock::new(ElementTree::new()))))
    }

    /// Acquire a clone of the process-shared dummy `BuildOwner` handle
    /// used to back [`ElementBuildContext::new_minimal`](crate::ElementBuildContext::new_minimal). See
    /// [`shared_dummy_tree`](Self::shared_dummy_tree) for the
    /// allocation-elimination rationale.
    pub fn shared_dummy_owner() -> Arc<RwLock<BuildOwner>> {
        Arc::clone(SHARED_DUMMY_OWNER.get_or_init(|| Arc::new(RwLock::new(BuildOwner::new()))))
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
                .push(Reverse(DirtyElement::new(id, depth)));

            // Notify that a build was scheduled
            if let Some(ref callback) = self.on_build_scheduled {
                callback();
            }
        }
    }

    /// Acquire an [`ElementOwner`](super::ElementOwner) split-borrow
    /// handle for the duration of an Element lifecycle traversal.
    ///
    /// The returned handle holds disjoint `&mut` references to
    /// `global_keys`, `dirty_elements`, `dirty_set`, and
    /// `inactive_elements` — every field an `Element::mount` /
    /// `unmount` / `update` path may write. The borrow checker proves
    /// non-aliasing because each field is borrowed once.
    ///
    /// Threading reference: `docs/plans/2026-05-21-002-feat-framework-spine-repair-plan.md` §U8, §D1.
    pub fn element_owner_mut(&mut self) -> super::ElementOwner<'_> {
        super::ElementOwner {
            global_keys: &mut self.global_keys,
            dirty_elements: &mut self.dirty_elements,
            dirty_set: &mut self.dirty_set,
            inactive_elements: &mut self.inactive_elements,
            on_build_scheduled: self.on_build_scheduled.as_deref(),
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

        // Process dirty elements in depth order.
        //
        // Each iteration pops one entry, then builds it with a fresh
        // split-borrow handle. We cannot pre-build the handle once
        // across the whole loop because `pop()` mutates
        // `self.dirty_elements` (one of the fields the handle aliases);
        // popping each iteration's entry first releases that aliasing
        // before the handle is reborrowed.
        while let Some(Reverse(dirty)) = self.dirty_elements.pop() {
            self.dirty_set.remove(&dirty.id());

            // Skip if element no longer exists
            if let Some(node) = tree.get_mut(dirty.id()) {
                // Only rebuild if still active
                if node.element().lifecycle().can_build() {
                    let mut element_owner = super::ElementOwner {
                        global_keys: &mut self.global_keys,
                        dirty_elements: &mut self.dirty_elements,
                        dirty_set: &mut self.dirty_set,
                        inactive_elements: &mut self.inactive_elements,
                        on_build_scheduled: self.on_build_scheduled.as_deref(),
                    };
                    node.element_mut().perform_build(&mut element_owner);
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
    /// Called when an element is deactivated (e.g., its parent rebuilds without
    /// it). The element will be unmounted in `finalize_tree()`.
    pub fn add_to_inactive(&mut self, id: ElementId, depth: usize) {
        self.inactive_elements.push(InactiveElement::new(id, depth));
    }

    /// Remove an element from the inactive list.
    ///
    /// Called when an element is reactivated (e.g., moved via GlobalKey).
    pub fn remove_from_inactive(&mut self, id: ElementId) {
        self.inactive_elements.retain(|e| e.id() != id);
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
        self.inactive_elements
            .sort_by_key(|entry| std::cmp::Reverse(entry.depth()));

        // Take ownership of inactive elements to avoid borrow conflicts.
        // `mem::take` snapshots the queue before the recursive unmount so
        // mid-iteration `ElementOwner::push_inactive` calls (e.g. children
        // deactivating as a parent unmounts) land in the *next* frame's
        // queue rather than re-entering this drain — same snapshot-then-fire
        // discipline as `ChangeNotifier::notify_listeners` (foundation
        // notifier.rs:158-163).
        let inactive_elements: Vec<_> = std::mem::take(&mut self.inactive_elements);

        // Collect all elements to unmount (including children)
        let mut elements_to_unmount = Vec::new();
        for inactive in &inactive_elements {
            Self::collect_elements_to_unmount(tree, inactive.id(), &mut elements_to_unmount);
        }

        // Build the split-borrow handle once for the entire unmount sweep.
        // The handle survives `tree.get_mut` borrows because it points into
        // disjoint `BuildOwner` fields.
        let mut element_owner = super::ElementOwner {
            global_keys: &mut self.global_keys,
            dirty_elements: &mut self.dirty_elements,
            dirty_set: &mut self.dirty_set,
            inactive_elements: &mut self.inactive_elements,
            on_build_scheduled: self.on_build_scheduled.as_deref(),
        };

        // Finalize all elements (deepest first - already sorted by collect order).
        //
        // `remove_finalized` (plan §U14 / R14) bypasses the soft-remove
        // path that `remove` takes for keyed elements. At this point
        // we've already given mid-frame state migration its chance —
        // anything still in the inactive queue is genuinely going away,
        // so we slab-remove + unregister the GlobalKey directly.
        for id in elements_to_unmount.iter().rev() {
            tree.remove_finalized(*id, &mut element_owner);
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

    /// Number of `GlobalKey`s currently registered.
    ///
    /// Test surface — production code reads
    /// [`BuildOwner::element_for_global_key`] on a single hash rather
    /// than scanning size. Tests use this to confirm the registry
    /// stays at the expected size across mount / unmount cycles.
    pub fn global_keys_len(&self) -> usize {
        self.global_keys.len()
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
    use std::any::TypeId;

    use super::*;
    use crate::{Lifecycle, View, tree::ElementTree};

    /// A leaf element that doesn't create children (prevents infinite
    /// recursion)
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

        fn mount(
            &mut self,
            _parent: Option<ElementId>,
            slot: usize,
            _owner: &mut super::super::ElementOwner<'_>,
        ) {
            self.depth = slot;
            self.lifecycle = Lifecycle::Active;
        }

        fn unmount(&mut self, _owner: &mut super::super::ElementOwner<'_>) {
            self.lifecycle = Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View, _owner: &mut super::super::ElementOwner<'_>) {}

        fn mark_needs_build(&mut self) {}

        fn perform_build(&mut self, _owner: &mut super::super::ElementOwner<'_>) {
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
        let root_id = tree.mount_root(&view, &mut owner.element_owner_mut());

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
        assert_eq!(first.depth(), 0);

        let Reverse(second) = owner.dirty_elements.pop().unwrap();
        assert_eq!(second.depth(), 1);

        let Reverse(third) = owner.dirty_elements.pop().unwrap();
        assert_eq!(third.depth(), 2);
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

    // ========================================================================
    // V-13 (cheap part) — process-shared dummy tree / owner reuse
    // ========================================================================

    /// `BuildOwner::shared_dummy_tree` returns `Arc::clone`s of the same
    /// inner pointer on every call — proven via `Arc::ptr_eq`. This is
    /// the cache-reuse contract underpinning
    /// `ElementBuildContext::new_minimal`.
    #[test]
    fn test_shared_dummy_tree_returns_ptr_equal_handles() {
        let first = BuildOwner::shared_dummy_tree();
        let second = BuildOwner::shared_dummy_tree();
        let third = BuildOwner::shared_dummy_tree();

        assert!(
            Arc::ptr_eq(&first, &second),
            "two shared_dummy_tree calls must alias the same Arc inner"
        );
        assert!(
            Arc::ptr_eq(&second, &third),
            "every shared_dummy_tree call must alias the same Arc inner"
        );
    }

    /// Companion test for `shared_dummy_owner` — same Arc-aliasing
    /// guarantee.
    #[test]
    fn test_shared_dummy_owner_returns_ptr_equal_handles() {
        let first = BuildOwner::shared_dummy_owner();
        let second = BuildOwner::shared_dummy_owner();

        assert!(
            Arc::ptr_eq(&first, &second),
            "two shared_dummy_owner calls must alias the same Arc inner"
        );
    }

    /// End-to-end: two `ElementBuildContext::new_minimal` calls reuse
    /// the same dummy `tree` and `owner` Arc handles. Proves the
    /// per-build allocation is eliminated on the production stateless /
    /// stateful build path.
    #[test]
    fn test_new_minimal_reuses_shared_dummy_handles() {
        let ctx_a = crate::ElementBuildContext::new_minimal(0);
        let ctx_b = crate::ElementBuildContext::new_minimal(3);

        assert!(
            Arc::ptr_eq(ctx_a.tree(), ctx_b.tree()),
            "two new_minimal contexts must share the dummy ElementTree Arc"
        );
        assert!(
            Arc::ptr_eq(ctx_a.build_owner(), ctx_b.build_owner()),
            "two new_minimal contexts must share the dummy BuildOwner Arc"
        );
    }

    /// The per-call `depth` argument is recorded on the context even
    /// though the underlying Arc handles are shared. Pins the
    /// "depth varies, infrastructure shared" contract.
    #[test]
    fn test_new_minimal_records_per_call_depth() {
        use crate::BuildContext as _;

        let shallow = crate::ElementBuildContext::new_minimal(0);
        let deeper = crate::ElementBuildContext::new_minimal(7);

        assert_eq!(shallow.depth(), 0);
        assert_eq!(deeper.depth(), 7);
    }
}
