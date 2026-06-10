//! Generic element core with arity-based child management.
//!
//! This module provides `ElementCore<V, A>`, a generic struct that contains
//! all common element state and lifecycle logic. By using this core, element
//! implementations can delegate most boilerplate to generic code.
//!
//! # Boilerplate Elimination
//!
//! ElementCore eliminates:
//! - ✅ Lifecycle boilerplate (mount/unmount/activate/deactivate) - ~40 lines
//! - ✅ View type casting boilerplate - ~10 lines
//! - ✅ PipelineOwner propagation boilerplate - ~15 lines
//! - ✅ Child management patterns - ~30 lines
//! - ✅ Trivial getters (lifecycle(), depth(), etc.) - ~10 lines
//!
//! **Total: ~105 lines eliminated per element type**
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::element::{ElementCore, Single};
//!
//! pub struct StatelessElement<V: StatelessView> {
//!     core: ElementCore<V, Single>,  // All common state
//! }
//!
//! impl<V: StatelessView> ElementBase for StatelessElement<V> {
//!     // Simple delegations (one-liners)
//!     fn lifecycle(&self) -> Lifecycle { self.core.lifecycle() }
//!     fn mount(&mut self, p: Option<ElementId>, s: usize) { self.core.mount(p, s) }
//!
//!     // Only view-specific logic remains
//!     fn perform_build(&mut self) {
//!         if !self.core.should_build() { return; }
//!
//!         let ctx = ElementBuildContext::new_minimal(self.core.depth());
//!         let child_view = self.core.view().build(&ctx);
//!
//!         self.core.update_or_create_child(child_view);
//!         self.core.clear_dirty();
//!     }
//! }
//! ```

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use flui_foundation::{ElementId, ListenerCallback, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;

use super::arity::ElementArity;
use crate::{element::Lifecycle, view::View};

/// Generic element core with arity-based child management.
///
/// This struct contains all common element state and lifecycle logic,
/// parameterized by:
/// - `V`: The View type (must be Clone + Send + Sync + 'static)
/// - `A`: The arity type (Leaf, Single, Optional, Variable)
///
/// # Type Parameters
///
/// * `V` - The View type this element manages. Must be cloneable because Views
///   are recreated each build cycle.
/// * `A` - The arity type (Leaf/Single/Optional/Variable) determining how many
///   children this element can have.
///
/// # Design Pattern
///
/// ElementCore is used as a **composition pattern**, not inheritance:
///
/// ```rust,ignore
/// pub struct StatelessElement<V: StatelessView> {
///     core: ElementCore<V, Single>,  // Composition
/// }
/// ```
///
/// This preserves object safety (ElementBase has no generics) while
/// enabling generic code internally.
pub struct ElementCore<V, A>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    /// The current View configuration.
    ///
    /// Views are immutable and recreated each build cycle, so we clone
    /// and store the current version.
    view: V,

    /// Current lifecycle state.
    ///
    /// Tracks whether the element is Initial, Active, Inactive, or Defunct.
    lifecycle: Lifecycle,

    /// Depth in the element tree (root = 0).
    ///
    /// Used for build order and z-index calculations.
    depth: usize,

    // E3 (atomic box→arena swap): the per-element `children: A::Storage`
    // box graph is gone. Children are slab-resident nodes addressed by
    // `ElementId` in the single [`ElementTree`](crate::tree::ElementTree);
    // a node's child list is its
    // [`ElementNode::child_ids`](crate::tree::ElementNode). `A: ElementArity`
    // still enforces the child-count constraint at the type level — it
    // simply no longer carries storage. The build half returns OWNED child
    // views (`build_into_views`); the reconcile half runs against the slab
    // in `BuildOwner::build_scope`.
    /// Whether this element needs to rebuild.
    ///
    /// Uses `Arc<AtomicBool>` for interior mutability, allowing listener
    /// callbacks to mark the element dirty without mutable access.
    /// This is essential for AnimatedBehavior and other reactive patterns.
    dirty: Arc<AtomicBool>,

    /// PipelineOwner for render tree access.
    ///
    /// Propagated from parent elements, used to access RenderTree for
    /// RenderObjectElements.
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,

    /// Parent's RenderId for tree structure.
    ///
    /// Used by RenderObjectElements to attach their RenderObject
    /// as a child of the parent's RenderObject.
    parent_render_id: Option<RenderId>,

    /// This element's own `ElementId` in the surrounding `ElementTree`.
    ///
    /// Plan §U15: stamped by `ElementTree::insert` /
    /// `mount_root_with_pipeline_owner` immediately after slab insertion
    /// (via [`ElementBase::set_self_id`](crate::ElementBase::set_self_id),
    /// forwarded to [`Self::set_self_id`]) so the element can schedule its
    /// OWN rebuild: the element's `set_state_scheduled` pushes
    /// `(self_id, depth)` onto the dirty heap that
    /// [`BuildOwner::build_scope`](crate::BuildOwner) drains.
    self_id: Option<ElementId>,

    /// Phantom data for generic parameter A.
    _phantom: PhantomData<A>,
}

impl<V, A> ElementCore<V, A>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    /// Create a new ElementCore with the given view.
    ///
    /// # Arguments
    ///
    /// * `view` - The initial View configuration
    ///
    /// # Returns
    ///
    /// A new ElementCore in Initial lifecycle state.
    pub fn new(view: V) -> Self {
        Self {
            view,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            dirty: Arc::new(AtomicBool::new(true)),
            pipeline_owner: None,
            parent_render_id: None,
            self_id: None,
            _phantom: PhantomData,
        }
    }

    /// Set this element's own `ElementId`. Called by
    /// [`crate::tree::ElementTree`] immediately after slab insertion
    /// via [`crate::view::ElementBase::set_self_id`]. Plan §U15.
    pub(crate) fn set_self_id(&mut self, id: ElementId) {
        self.self_id = Some(id);
    }

    /// Push this element onto the dirty heap so
    /// [`BuildOwner::build_scope`](crate::BuildOwner) reaches it.
    ///
    /// E3 (atomic box→arena swap): the slab/drain model rebuilds only
    /// elements on the heap, so `setState` flips dirty AND schedules via
    /// this. Uses the element's own stamped `self_id` + `depth`. If
    /// `self_id` is unset — a hand-rolled element that bypassed
    /// `ElementTree::insert` / `mount_root_*` — scheduling is skipped (the
    /// element is not slab-addressable, so `build_scope` could not reach
    /// it anyway); a `debug_assert!` makes that framework-invariant
    /// violation loud in tests.
    pub(crate) fn schedule_self_build(&self, owner: &mut crate::ElementOwner<'_>) {
        debug_assert!(
            self.self_id.is_some(),
            "ElementCore::schedule_self_build called before set_self_id: \
             a slab-resident element must be stamped with its ElementId at \
             mount (ElementTree::insert / mount_root_* do this) before any \
             setState can schedule it."
        );
        if let Some(id) = self.self_id {
            owner.schedule_build_for(id, self.depth);
        }
    }

    // NOTE: `self_id` is read directly via `self.self_id` inside
    // `update_or_create_children` rather than through a getter; the
    // single in-crate consumer doesn't justify the boilerplate.

    // ========================================================================
    // Lifecycle Methods (eliminates ~40 lines of boilerplate per element)
    // ========================================================================

    /// Mount this element into the tree.
    ///
    /// Sets lifecycle to Active and stores depth.
    /// Delegates child mounting to the storage implementation.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent ElementId (if any)
    /// * `slot` - The slot/depth in the tree
    /// * `_owner` - Split-borrow handle into the BuildOwner. Currently
    ///   unused at this layer because `update_or_create_child` /
    ///   `update_or_create_children` (called during `perform_build`)
    ///   handle child mounting outside this method's scope; threading
    ///   the parameter through keeps the trait surface consistent and
    ///   gives downstream units (U9-U14) a hook for GlobalKey
    ///   registration during mount.
    pub fn mount(
        &mut self,
        _parent: Option<ElementId>,
        slot: usize,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        self.lifecycle = Lifecycle::Active;
        self.depth = slot;
        self.dirty.store(true, Ordering::Relaxed);

        // Children will be mounted during perform_build
        tracing::debug!(
            "ElementCore::mount lifecycle={:?} depth={} view_type={:?}",
            self.lifecycle,
            self.depth,
            TypeId::of::<V>()
        );
    }

    /// Unmount this element (permanently removed).
    ///
    /// Sets lifecycle to Defunct. E3: children are slab-resident nodes —
    /// the [`ElementTree`](crate::tree::ElementTree) drives the
    /// deepest-first id-unmount of descendants (via
    /// `BuildOwner::finalize_tree` / `collect_elements_to_unmount`), so
    /// this element no longer frees a child subtree implicitly. The
    /// split-borrow `owner` handle is kept on the signature so behavior
    /// `on_unmount` hooks (GlobalKey deregistration, dependent-set
    /// cleanup) still run through the unified `Element::unmount`.
    pub fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {
        self.lifecycle = Lifecycle::Defunct;

        tracing::debug!(
            "ElementCore::unmount lifecycle={:?} view_type={:?}",
            self.lifecycle,
            TypeId::of::<V>()
        );
    }

    /// Activate this element (re-inserted into tree).
    ///
    /// Sets lifecycle to Active. E3: child activation is the slab's job
    /// (descendants are independent nodes), not a recursive walk from
    /// here.
    pub fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;

        tracing::debug!(
            "ElementCore::activate lifecycle={:?} view_type={:?}",
            self.lifecycle,
            TypeId::of::<V>()
        );
    }

    /// Deactivate this element (temporarily removed from tree).
    ///
    /// Sets lifecycle to Inactive. E3: child deactivation is the slab's
    /// job (descendants are independent nodes), not a recursive walk from
    /// here.
    pub fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;

        tracing::debug!(
            "ElementCore::deactivate lifecycle={:?} view_type={:?}",
            self.lifecycle,
            TypeId::of::<V>()
        );
    }

    // ========================================================================
    // View Update (eliminates type casting boilerplate ~10 lines)
    // ========================================================================

    /// Update this element with a new View of the same type.
    ///
    /// FR-021 (Phase 3 §U27): dispatch routes through
    /// `crate::element::dispatch::dispatch_view_update` (`pub(crate)`)
    /// which discriminates on `TypeId` and extracts the typed inner
    /// via `Downcast::into_any` + `Box::downcast::<V>` — the literal
    /// `downcast_ref::<V>()` pattern FR-033's port-check grep
    /// forbids is gone from this path entirely. On type mismatch
    /// the dispatch returns `false` without `tracing::warn!`; the
    /// caller (reconciler) replaces the element rather than
    /// continuing with stale state.
    ///
    /// # Arguments
    ///
    /// * `new_view` - The new View configuration
    ///
    /// # Returns
    ///
    /// `true` if update succeeded, `false` if the type mismatched
    /// (caller replaces the element).
    pub fn update_view(&mut self, new_view: &dyn View) -> bool
    where
        V: View,
    {
        crate::element::dispatch::dispatch_view_update(self, new_view)
    }

    // ========================================================================
    // Dispatch-internal setters (Phase 1 §U8)
    //
    // These are `pub(crate)` because `crate::element::dispatch` needs to
    // mutate `ElementCore::view` and `ElementCore::dirty` without
    // ElementCore::update_view's body owning them. Phase 3 §U27
    // replaces the dispatch function body and may retire these
    // setters; until then they keep the dispatch module free of
    // direct field access to ElementCore's private state.
    // ========================================================================

    /// Replace the stored view. Used by
    /// [`crate::element::dispatch::dispatch_view_update`] after the
    /// `TypeId`-keyed `Box::downcast::<V>` succeeds (Phase 3 §U27).
    pub(crate) fn replace_view_for_dispatch(&mut self, view: V) {
        self.view = view;
    }

    /// Mark the element as needing rebuild. Used by
    /// [`crate::element::dispatch::dispatch_view_update`] after the
    /// view is replaced.
    pub(crate) fn mark_dirty_for_dispatch(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    // E3 (atomic box→arena swap): the child-management methods
    // (`update_or_create_child` / `update_or_create_children` /
    // `rebuild_children`) are gone. They reconciled and recursively built
    // a box-owned child graph in place. The element now only PRODUCES its
    // child views (`build_into_views`, on the behavior); the reconcile +
    // recursive build runs against the slab-resident
    // [`ElementTree`](crate::tree::ElementTree) in
    // [`BuildOwner::build_scope`](crate::BuildOwner) via
    // [`reconcile_children_by_id`](crate::tree::id_reconcile), which
    // schedules each child as its own drain entry. No element ever holds a
    // `&mut` into the slab across a second slab mutation.

    // ========================================================================
    // Pipeline Owner (eliminates propagation boilerplate ~15 lines)
    // ========================================================================

    /// Set the PipelineOwner for this element.
    ///
    /// Downcasts from `Arc<dyn Any>` to `Arc<RwLock<PipelineOwner>>`.
    /// This pattern is required for object safety of ElementBase.
    ///
    /// # Arguments
    ///
    /// * `owner` - `Arc<dyn Any>` that should downcast to
    ///   `Arc<RwLock<PipelineOwner>>`
    pub fn set_pipeline_owner_any(&mut self, owner: Arc<dyn Any + Send + Sync>) {
        if let Ok(pipeline_owner) = owner.downcast::<RwLock<PipelineOwner>>() {
            self.pipeline_owner = Some(pipeline_owner);
            tracing::debug!(
                "ElementCore::set_pipeline_owner_any received PipelineOwner for view_type={:?}",
                TypeId::of::<V>()
            );
        } else {
            tracing::warn!(
                "ElementCore::set_pipeline_owner_any received wrong type for view_type={:?}",
                TypeId::of::<V>()
            );
        }
    }

    /// Set the parent's RenderId for tree structure.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The parent's RenderId
    pub fn set_parent_render_id(&mut self, parent_id: Option<RenderId>) {
        self.parent_render_id = parent_id;
        tracing::debug!(
            "ElementCore::set_parent_render_id parent_id={:?} for view_type={:?}",
            parent_id,
            TypeId::of::<V>()
        );
    }

    /// The `RenderId` that this element's *children* should attach their
    /// `RenderObject`s under.
    ///
    /// E3 propagation contract: when the slab inserts a child below this
    /// element, the child's `set_parent_render_id` receives this value.
    /// For a component element (Stateless/Stateful/Proxy/Inherited) it is
    /// the `parent_render_id` this element itself received — the nearest
    /// ancestor `RenderObject` is passed straight through. A
    /// `RenderObjectElement` overrides the effective value at the
    /// `ElementBase` layer (it returns its own `render_id`), since its
    /// children attach under *it*. Defaults here to the pass-through.
    pub fn child_parent_render_id(&self) -> Option<RenderId> {
        self.parent_render_id
    }

    // ========================================================================
    // Getters (eliminates trivial getters ~10 lines)
    // ========================================================================

    /// Get the current lifecycle state.
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Get the depth in the element tree.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Get a reference to the current View.
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get a mutable reference to the current View.
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    /// Check if this element should build.
    ///
    /// Returns `true` if dirty and lifecycle allows building.
    pub fn should_build(&self) -> bool {
        self.dirty.load(Ordering::Relaxed) && self.lifecycle.can_build()
    }

    /// Check if this element is dirty (needs rebuild).
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// Mark this element as needing a rebuild.
    pub fn mark_dirty(&mut self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Clear the dirty flag.
    ///
    /// Should be called after perform_build completes.
    pub fn clear_dirty(&mut self) {
        self.dirty.store(false, Ordering::Relaxed);
    }

    /// Create a callback that can mark this element dirty.
    ///
    /// This is useful for AnimatedBehavior and other behaviors that need to
    /// trigger rebuilds from listener callbacks without mutable access.
    ///
    /// # Returns
    ///
    /// A shareable callback that marks this element dirty when called.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mark_dirty = core.create_mark_dirty_callback();
    /// animation.add_listener(mark_dirty);
    /// ```
    pub fn create_mark_dirty_callback(&self) -> ListenerCallback {
        let dirty = Arc::clone(&self.dirty);
        Arc::new(move || {
            dirty.store(true, Ordering::Relaxed);
        })
    }

    /// Get the PipelineOwner, if set.
    pub fn pipeline_owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        // PORT-CHECK-OK-SP6: ElementCore pipeline_owner accessor; pre-existing SP-6
        self.pipeline_owner.as_ref()
    }

    /// Get the parent RenderId, if set.
    pub fn parent_render_id(&self) -> Option<RenderId> {
        self.parent_render_id
    }

    // E3 (atomic box→arena swap): `visit_children` / `children` /
    // `children_mut` / `has_children` / `child_count` are gone —
    // `ElementCore` no longer owns a child graph. Children are
    // slab-resident; traverse them via
    // `tree.get(id).child_ids()` on the single
    // [`ElementTree`](crate::tree::ElementTree).
}

impl<V, A> std::fmt::Debug for ElementCore<V, A>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementCore")
            .field("view_type", &TypeId::of::<V>())
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty.load(Ordering::Relaxed))
            .field("has_pipeline_owner", &self.pipeline_owner.is_some())
            .field("parent_render_id", &self.parent_render_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::arity::{Leaf, Single, Variable};

    #[derive(Clone)]
    struct TestView {
        #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
        value: i32,
    }

    #[test]
    fn test_element_core_creation() {
        let view = TestView { value: 42 };
        let core = ElementCore::<TestView, Single>::new(view);

        assert_eq!(core.lifecycle(), Lifecycle::Initial);
        assert_eq!(core.depth(), 0);
        assert!(core.is_dirty());
    }

    #[test]
    fn test_element_core_mount() {
        let view = TestView { value: 42 };
        let mut core = ElementCore::<TestView, Single>::new(view);

        let mut build_owner = crate::BuildOwner::new();
        let mut owner = build_owner.element_owner_mut();
        core.mount(None, 5, &mut owner);

        assert_eq!(core.lifecycle(), Lifecycle::Active);
        assert_eq!(core.depth(), 5);
    }

    #[test]
    fn test_element_core_lifecycle() {
        let view = TestView { value: 42 };
        let mut core = ElementCore::<TestView, Single>::new(view);

        let mut build_owner = crate::BuildOwner::new();
        {
            let mut owner = build_owner.element_owner_mut();
            core.mount(None, 0, &mut owner);
        }
        assert_eq!(core.lifecycle(), Lifecycle::Active);

        core.deactivate();
        assert_eq!(core.lifecycle(), Lifecycle::Inactive);

        core.activate();
        assert_eq!(core.lifecycle(), Lifecycle::Active);

        {
            let mut owner = build_owner.element_owner_mut();
            core.unmount(&mut owner);
        }
        assert_eq!(core.lifecycle(), Lifecycle::Defunct);
    }

    #[test]
    fn test_element_core_dirty_flag() {
        let view = TestView { value: 42 };
        let mut core = ElementCore::<TestView, Single>::new(view);

        assert!(core.is_dirty());

        core.clear_dirty();
        assert!(!core.is_dirty());

        core.mark_dirty();
        assert!(core.is_dirty());
    }

    #[test]
    fn test_element_core_leaf_arity() {
        let view = TestView { value: 42 };
        let core = ElementCore::<TestView, Leaf>::new(view);

        // E3: child-count lives on the slab node now, not the core.
        assert_eq!(core.lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_element_core_single_arity() {
        let view = TestView { value: 42 };
        let core = ElementCore::<TestView, Single>::new(view);

        assert_eq!(core.lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_element_core_variable_arity() {
        let view = TestView { value: 42 };
        let core = ElementCore::<TestView, Variable>::new(view);

        assert_eq!(core.lifecycle(), Lifecycle::Initial);
    }
}
