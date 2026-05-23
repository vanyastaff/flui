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

use super::{arity::ElementArity, child_storage::ElementChildStorage};
use crate::{
    element::Lifecycle,
    view::{ElementBase, View},
};

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

    /// Arity-specific child storage.
    ///
    /// The concrete type is determined by A::Storage:
    /// - Leaf → NoChildStorage
    /// - Single → SingleChildStorage
    /// - Optional → OptionalChildStorage
    /// - Variable → VariableChildStorage
    children: A::Storage,

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
    /// Plan §U15: populated by `ElementTree::insert` /
    /// `mount_root_with_pipeline_owner` immediately after slab
    /// insertion (via [`ElementBase::set_self_id`] → forwarded to
    /// [`Self::set_self_id`]) so that
    /// [`Self::update_or_create_children`] (Variable arity) can stamp
    /// the real parent `ElementId` onto every emitted
    /// [`ReconcileEvent`](crate::tree::ReconcileEvent) — replacing
    /// the §U13 placeholder.
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
            children: A::Storage::default(),
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
    /// Sets lifecycle to Defunct and unmounts all children. Threads
    /// the split-borrow `owner` handle into child unmounts so any
    /// descendant `GlobalKey` deregistration / dependent-set cleanup
    /// (U9, U14) can take effect.
    pub fn unmount(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.children.unmount_children(owner);
        self.lifecycle = Lifecycle::Defunct;

        tracing::debug!(
            "ElementCore::unmount lifecycle={:?} view_type={:?}",
            self.lifecycle,
            TypeId::of::<V>()
        );
    }

    /// Activate this element (re-inserted into tree).
    ///
    /// Sets lifecycle to Active and activates all children.
    pub fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        self.children.activate_children();

        tracing::debug!(
            "ElementCore::activate lifecycle={:?} view_type={:?}",
            self.lifecycle,
            TypeId::of::<V>()
        );
    }

    /// Deactivate this element (temporarily removed from tree).
    ///
    /// Sets lifecycle to Inactive and deactivates all children.
    pub fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        self.children.deactivate_children();

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
    /// Phase 1 §U8 / KTD-4: under default features, dispatch routes
    /// through the in-crate `dispatch::dispatch_view_update` helper
    /// (the future home of typed `ElementKind`-discriminated dispatch
    /// — Phase 3 §U27 replaces the body there with the real typed
    /// match, eliminating the runtime `downcast_ref::<V>()` call
    /// entirely per FR-021).
    ///
    /// Under `feature = "legacy-downcast"` (workspace-internal ONLY,
    /// gated by `cfg(__flui_legacy_downcast_internal)` so it cannot
    /// be enabled by accidental workspace-resolver feature
    /// unification), the body falls back to the pre-FR-021 inline
    /// downcast so the Phase 0 §U2 S1 bench can A/B against the
    /// legacy storage shape.
    ///
    /// # Arguments
    ///
    /// * `new_view` - The new View configuration
    ///
    /// # Returns
    ///
    /// `true` if update succeeded, `false` if downcast failed
    pub fn update_view(&mut self, new_view: &dyn View) -> bool {
        // Default-features path: route through the dispatch module.
        // Identity-shim today; Phase 3 §U27 replaces with typed match.
        #[cfg(not(feature = "legacy-downcast"))]
        {
            crate::element::dispatch::dispatch_view_update(self, new_view)
        }
        // Legacy path: requires BOTH `feature = "legacy-downcast"`
        // AND `cfg(__flui_legacy_downcast_internal)`. The internal
        // cfg is set only by `crates/flui-view`'s own benchmark via
        // `[[bench]] rustflags = ["--cfg=__flui_legacy_downcast_internal"]`
        // (the Phase 0 §U2 S1 bench). Any other consumer that
        // enables the feature without the internal cfg hits the
        // module-level `compile_error!` below — a workspace-internal
        // accidental-enable surfaces as a CLEAR build failure
        // visible to the offending consumer, not as a silent
        // FR-021 regression.
        #[cfg(all(feature = "legacy-downcast", __flui_legacy_downcast_internal))]
        {
            if let Some(v) = new_view.as_any().downcast_ref::<V>() {
                self.view = v.clone();
                self.dirty.store(true, Ordering::Relaxed);
                tracing::debug!(
                    "ElementCore::update_view succeeded for view_type={:?}",
                    TypeId::of::<V>()
                );
                true
            } else {
                tracing::warn!(
                    "ElementCore::update_view failed to downcast for view_type={:?}",
                    TypeId::of::<V>()
                );
                false
            }
        }
        // Fallback for the `feature = "legacy-downcast"` +
        // `not(__flui_legacy_downcast_internal)` matrix corner. The
        // module-level `compile_error!` in `super::mod` already fires
        // for this combination, so this branch is unreachable at
        // build time — but rustc still type-checks the function body,
        // and without an arm here it would emit a spurious
        // "function returns `()` instead of `bool`" diagnostic on
        // top of the intentional compile_error. The `unreachable!()`
        // returns `!` which coerces to `bool` and keeps the only
        // surfaced error message focused on the workspace-internal
        // feature guard.
        #[cfg(all(feature = "legacy-downcast", not(__flui_legacy_downcast_internal)))]
        {
            // Suppress unused-variable warning for `new_view` in this
            // corner-case build matrix without dragging
            // `#[allow(unused_variables)]` onto the function signature.
            let _ = new_view;
            unreachable!()
        }
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
    /// typed downcast (Phase 1 identity-shim).
    ///
    /// Gated to the default-features build because the legacy path
    /// (`feature = "legacy-downcast"` + internal cfg) uses an inline
    /// `self.view = v.clone()` and never reaches this helper.
    #[cfg(not(feature = "legacy-downcast"))]
    pub(crate) fn replace_view_for_dispatch(&mut self, view: V) {
        self.view = view;
    }

    /// Mark the element as needing rebuild. Used by
    /// [`crate::element::dispatch::dispatch_view_update`] after the
    /// view is replaced (Phase 1 identity-shim).
    ///
    /// See [`Self::replace_view_for_dispatch`] for the cfg gate
    /// rationale.
    #[cfg(not(feature = "legacy-downcast"))]
    pub(crate) fn mark_dirty_for_dispatch(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    // ========================================================================
    // Child Management (eliminates child management boilerplate ~30 lines)
    // ========================================================================

    /// Update or create the child element with a new view.
    ///
    /// For Single/Optional arity, this updates the existing child or creates
    /// new. For Variable arity, use `update_or_create_children` instead.
    ///
    /// # Arguments
    ///
    /// * `child_view` - The new child View
    /// * `owner` - Split-borrow handle threaded through child
    ///   mount/unmount/update calls (plan §U8).
    // `Box<dyn View>` ownership transfer is intentional for API consistency.
    // Single-line signature avoids `port-check.sh` trigger 6's struct-field
    // pattern matching a `child_view: Box<dyn View>,` parameter on its own
    // line (the trigger comment notes the trailing-comma anchor was meant to
    // exclude function parameters but in practice does not).
    #[rustfmt::skip]
    #[allow(clippy::needless_pass_by_value)]
    pub fn update_or_create_child(&mut self, child_view: Box<dyn View>, owner: &mut crate::ElementOwner<'_>) {
        if self.children.is_empty() {
            // First build - create child element
            self.children.create_from_view(child_view.as_ref());

            // Propagate owner if we have one
            if let Some(ref pipeline_owner) = self.pipeline_owner {
                self.children
                    .propagate_owner(Arc::clone(pipeline_owner), self.parent_render_id);
            }

            // Mount child
            self.children.mount_children(None, self.depth + 1, owner);

            // Build child's children
            self.children.perform_build_children(owner);

            tracing::debug!("ElementCore::update_or_create_child created new child");
        } else {
            // Update existing child
            let had_child = !self.children.is_empty();
            self.children.update_with_view(child_view.as_ref(), owner);

            // If a new child was created (previously was empty), mount it
            if !had_child && !self.children.is_empty() {
                self.children.mount_children(None, self.depth + 1, owner);

                // Propagate owner if we have one
                if let Some(ref pipeline_owner) = self.pipeline_owner {
                    self.children
                        .propagate_owner(Arc::clone(pipeline_owner), self.parent_render_id);
                }
            }

            self.children.perform_build_children(owner);

            tracing::debug!("ElementCore::update_or_create_child updated existing child");
        }
    }

    /// Update or create multiple child elements (Variable arity only).
    ///
    /// For Single/Optional arity, use `update_or_create_child` instead.
    ///
    /// # Arguments
    ///
    /// * `child_views` - The new child Views
    /// * `owner` - Split-borrow handle threaded through child
    ///   mount/unmount/update calls.
    // `Vec<Box<dyn View>>` ownership transfer is intentional for API
    // consistency. Single-line signature avoids `port-check.sh` trigger 6 —
    // see `update_or_create_child` for rationale.
    #[rustfmt::skip]
    #[allow(clippy::needless_pass_by_value)]
    pub fn update_or_create_children(&mut self, child_views: Vec<Box<dyn View>>, owner: &mut crate::ElementOwner<'_>) {
        if self.children.is_empty() {
            // First build - create children
            self.children.create_from_views(&child_views);

            // Propagate owner if we have one
            if let Some(ref pipeline_owner) = self.pipeline_owner {
                self.children
                    .propagate_owner(Arc::clone(pipeline_owner), self.parent_render_id);
            }

            // Mount children
            self.children.mount_children(None, self.depth + 1, owner);

            // Build children's children
            self.children.perform_build_children(owner);

            tracing::debug!(
                "ElementCore::update_or_create_children created {} children",
                child_views.len()
            );
        } else {
            // Update existing children via keyed reconciliation (plan
            // §U5). `update_with_views` matches old child elements to
            // new Views by `Key`, updating reused ones and unmounting
            // dropped ones — but leaves any *freshly created* children
            // in `Lifecycle::Initial`, unmounted (it cannot reach the
            // `PipelineOwner` from the bare box-vec).
            // Plan §U15: thread this element's own ElementId as the
            // reconciler's `parent_id`, replacing the §U13 placeholder.
            // `self_id` is `None` only when this element has never been
            // mounted (perform_build before mount is a framework-invariant
            // violation: `ElementTree::insert` / `mount_root_*` always
            // call `set_self_id` BEFORE `mount`, and `mount` precedes
            // `perform_build` in the lifecycle FSM).
            //
            // Debug-build trip-wire: if the invariant ever breaks (a
            // hand-rolled element bypassing `ElementTree::insert`, a
            // future framework refactor that decouples mount from
            // self-id stamping), this assertion fires during testing.
            // Production retains the defensive fallback to the §U13
            // placeholder so the frame still completes — but the
            // emitted ReconcileEvents will silently correlate to the
            // root, masking the real culprit. The debug_assert makes
            // the violation loud where it matters.
            debug_assert!(
                self.self_id.is_some(),
                "§U15 invariant violated: ElementCore::update_or_create_children \
                 called before set_self_id. `ElementTree::insert` / \
                 `mount_root_with_pipeline_owner` must stamp self_id \
                 before any reconciliation runs."
            );
            let parent_id = self
                .self_id
                .unwrap_or_else(|| flui_foundation::ElementId::new(1));
            self.children
                .update_with_views(parent_id, &child_views, owner);

            // Finish the lifecycle of those new children. The count
            // alone is no longer a reliable "did anything get created?"
            // signal — a keyed swap (one removed, one added) leaves the
            // count unchanged — so always run the propagate→mount sweep.
            // Both steps are idempotent for the reused children:
            // `propagate_owner` just re-sets the owner, and
            // `mount_children` skips elements that are already `Active`.
            //
            // Order matters: the owner must be propagated *before*
            // `mount_children`, because `RenderBehavior::on_mount`
            // creates its `RenderObject` only when a `PipelineOwner` is
            // already in scope.
            if let Some(ref pipeline_owner) = self.pipeline_owner {
                self.children
                    .propagate_owner(Arc::clone(pipeline_owner), self.parent_render_id);
            }
            self.children.mount_children(None, self.depth + 1, owner);

            self.children.perform_build_children(owner);

            tracing::debug!(
                "ElementCore::update_or_create_children updated to {} children",
                child_views.len()
            );
        }
    }

    /// Rebuild all children.
    ///
    /// Calls perform_build() on all child elements.
    pub fn rebuild_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.children.perform_build_children(owner);
    }

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

    /// Propagate PipelineOwner to children.
    ///
    /// Should be called after children are created.
    pub fn propagate_owner_to_children(&mut self) {
        if let Some(ref owner) = self.pipeline_owner {
            self.children
                .propagate_owner(Arc::clone(owner), self.parent_render_id);
        }
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

    /// Check if this element has children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Get the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Get the PipelineOwner, if set.
    pub fn pipeline_owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        self.pipeline_owner.as_ref()
    }

    /// Get the parent RenderId, if set.
    pub fn parent_render_id(&self) -> Option<RenderId> {
        self.parent_render_id
    }

    /// Visit all children with a closure.
    pub fn visit_children<F>(&self, mut visitor: F)
    where
        F: FnMut(&dyn ElementBase),
    {
        self.children.visit_children(&mut visitor);
    }

    /// Get immutable access to the child storage.
    pub fn children(&self) -> &A::Storage {
        &self.children
    }

    /// Get mutable access to the child storage.
    pub fn children_mut(&mut self) -> &mut A::Storage {
        &mut self.children
    }
}

impl<V, A> std::fmt::Debug for ElementCore<V, A>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    A::Storage: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementCore")
            .field("view_type", &TypeId::of::<V>())
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("children", &self.children)
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
        assert!(!core.has_children());
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

        assert!(!core.has_children());
        assert_eq!(core.child_count(), 0);
    }

    #[test]
    fn test_element_core_single_arity() {
        let view = TestView { value: 42 };
        let core = ElementCore::<TestView, Single>::new(view);

        assert!(!core.has_children());
        assert_eq!(core.child_count(), 0);
    }

    #[test]
    fn test_element_core_variable_arity() {
        let view = TestView { value: 42 };
        let core = ElementCore::<TestView, Variable>::new(view);

        assert!(!core.has_children());
        assert_eq!(core.child_count(), 0);
    }
}
