//! Unified Element implementation.
//!
//! This module provides a single generic Element<V, A, B> that replaces
//! StatelessElement, ProxyElement, StatefulElement, and RenderElement.
//!
//! The Element delegates to:
//! - ElementCore<V, A> for common element logic
//! - B: ElementBehavior<V, A> for view-specific build logic

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;

use super::{
    RenderObjectElement, RenderSlot, Single, Variable,
    arity::ElementArity,
    behavior::{ElementBehavior, InheritedBehavior, RenderBehavior, StatefulBehavior},
    generic::ElementCore,
};
use crate::{
    element::Lifecycle,
    view::{ElementBase, InheritedView, RenderView, StatefulView, View},
};

// ============================================================================
// Unified Element
// ============================================================================

/// Unified element with behavior-based specialization.
///
/// # Type Parameters
///
/// * `V` - The View type
/// * `A` - The arity (Leaf, Single, Optional, Variable)
/// * `B` - The behavior (Stateless, Proxy, Stateful, Render)
///
/// # Examples
///
/// ```ignore
/// // Stateless element with single child
/// type StatelessElement<V> = Element<V, Single, StatelessBehavior>;
///
/// // Render element with variable children
/// type RenderElement<V> = Element<V, Variable, RenderBehavior<V>>;
/// ```
pub struct Element<V, A, B>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    B: ElementBehavior<V, A>,
{
    /// Generic element core handling all common logic.
    core: ElementCore<V, A>,
    /// Behavior handling view-specific logic.
    behavior: B,
    /// Marker for generic types.
    _phantom: PhantomData<V>,
}

impl<V, A, B> Element<V, A, B>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    B: ElementBehavior<V, A>,
{
    /// Create a new Element with the given view and behavior.
    pub fn new(view: &V, behavior: B) -> Self {
        Self {
            core: ElementCore::new(view.clone()),
            behavior,
            _phantom: PhantomData,
        }
    }

    /// Get a reference to the element core.
    pub fn core(&self) -> &ElementCore<V, A> {
        &self.core
    }

    /// Get a mutable reference to the element core.
    pub fn core_mut(&mut self) -> &mut ElementCore<V, A> {
        &mut self.core
    }

    /// Get a reference to the behavior.
    pub fn behavior(&self) -> &B {
        &self.behavior
    }

    /// Get a mutable reference to the behavior.
    pub fn behavior_mut(&mut self) -> &mut B {
        &mut self.behavior
    }
}

impl<V, A, B> std::fmt::Debug for Element<V, A, B>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    B: ElementBehavior<V, A> + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use the behavior's kind name so type aliases like `StatelessElement`
        // and `StatefulElement` render with a familiar struct name. The core
        // is flattened into the same struct so `Debug` output keeps the
        // `lifecycle` field accessible at the top level.
        f.debug_struct(self.behavior.debug_kind())
            .field("lifecycle", &self.core.lifecycle())
            .field("depth", &self.core.depth())
            .field("behavior", &self.behavior)
            .finish()
    }
}

// ============================================================================
// ElementBase Implementation
// ============================================================================

// `V: View` is required so `current_key_hash` can forward to
// `View::key()`. Every `Element<V, A, B>` ever boxed as `dyn ElementBase`
// originates from `View::create_element()`, so its `V` is always a
// `View` — the bound exiles no real instantiation, it just makes the
// already-true invariant visible to the type system.
impl<V, A, B> ElementBase for Element<V, A, B>
where
    V: View + Clone + Send + Sync + 'static,
    A: ElementArity,
    B: ElementBehavior<V, A>,
{
    // ========================================================================
    // Simple Delegations to ElementCore
    // ========================================================================

    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn current_key_hash(&self) -> Option<u64> {
        // The View configuration always lives in `ElementCore`; read its
        // key and hash it. `View::key()` returns `None` for keyless
        // views — keyed reconciliation then falls back to positional
        // matching for this child.
        self.core
            .view()
            .key()
            .map(flui_foundation::ViewKey::key_hash)
    }

    fn current_key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        // Plan §U12 / FR-024 (c): expose the underlying `&dyn ViewKey`
        // so the reconciler can do semantic `key_eq` on a hash hit and
        // reject silent collisions across distinct keys with the same
        // `u64`. `core.view().key()` already returns
        // `Option<&dyn ViewKey>` — forward directly.
        self.core.view().key()
    }

    fn set_self_id(&mut self, id: ElementId) {
        // Plan §U15: forward to `ElementCore::set_self_id` so the
        // Variable-arity reconciler stamp can use the real parent id.
        self.core.set_self_id(id);
    }

    fn lifecycle(&self) -> Lifecycle {
        self.core.lifecycle()
    }

    fn depth(&self) -> usize {
        self.core.depth()
    }

    fn mark_needs_build(&mut self) {
        self.core.mark_dirty();
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {
        // Children are managed internally
    }

    fn set_pipeline_owner_any(&mut self, owner: Arc<dyn Any + Send + Sync>) {
        self.core.set_pipeline_owner_any(owner);
    }

    fn set_parent_render_id(&mut self, parent_id: Option<RenderId>) {
        self.core.set_parent_render_id(parent_id);
    }

    // ========================================================================
    // Inherited-element protocol (U9 / R4)
    //
    // Delegates to the behavior; only `InheritedBehavior<V>` returns
    // `Some(...)`. Every other behavior keeps the trait-default `None`.
    // ========================================================================

    fn as_inherited(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        self.behavior.as_inherited_access()
    }

    fn as_inherited_mut(&mut self) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        self.behavior.as_inherited_access_mut()
    }

    // ========================================================================
    // Ancestor-finder protocol (U11 / R6, R7, R8)
    //
    // Both accessors route through the behavior. `view_as_any` is
    // uniform across every behavior — the View configuration always
    // lives in `ElementCore` — so we hand out the core's view directly.
    // `state_as_any` is behavior-specific: only `StatefulBehavior<V>`
    // overrides the default `None`.
    // ========================================================================

    fn view_as_any(&self) -> Option<&dyn Any> {
        Some(self.core.view() as &dyn Any)
    }

    fn state_as_any(&self) -> Option<&dyn Any> {
        self.behavior.state_as_any()
    }

    // ========================================================================
    // RenderObject-finder protocol (U12 / R9)
    //
    // Routes through the behavior. Only `RenderBehavior<V>` overrides
    // the default `None` — and even then, returns `None` until
    // `on_mount` ran with a `PipelineOwner` in scope. Stateless / Proxy
    // / Inherited / Stateful / Animation behaviors all keep the
    // trait-default `None`.
    // ========================================================================

    fn render_id(&self) -> Option<RenderId> {
        self.behavior.render_id()
    }

    // ========================================================================
    // Notification handler protocol (U13 / R10)
    //
    // Routes through the behavior. The trait-default `false` keeps
    // non-listener elements out of the bubble walk. Production
    // listener behaviors (e.g. a future NotificationListener<N> widget)
    // override `ElementBehavior::on_notification` to translate the
    // object-safe `(TypeId, &dyn Any)` shape into the typed
    // `NotifiableElement<N>` callback at the impl site — single dyn
    // boundary at dispatch only (plan §D3, Constitution Principle 4).
    // ========================================================================

    fn on_notification(&self, type_id: std::any::TypeId, notification: &dyn Any) -> bool {
        self.behavior.on_notification(type_id, notification)
    }

    // ========================================================================
    // Dependency-change typed-hook dispatch (U14 / R16, audit V-19)
    //
    // Routes through the behavior. Only `StatefulBehavior<V>` and
    // `AnimatedBehavior<V>` override the default no-op — they forward
    // to `ViewState::did_change_dependencies` on the owned state. Other
    // behaviors (Stateless, Proxy, Inherited, Render) own no state, so
    // the scheduled rebuild alone is the right response.
    // ========================================================================

    fn notify_dependency_change(&mut self) {
        self.behavior.did_change_dependencies(&self.core);
    }

    // ========================================================================
    // Lifecycle Methods with Behavior Hooks
    // ========================================================================

    fn update(&mut self, new_view: &dyn View, owner: &mut crate::ElementOwner<'_>) {
        // Snapshot the previous view so `on_view_updated` can pass it to state
        // hooks (e.g. `ViewState::did_update_view`,
        // `InheritedBehavior::on_view_updated` for dependent notification).
        let old_view = self.core.view().clone();
        if self.core.update_view(new_view) {
            // Notify behavior of update
            self.behavior.on_update(&self.core);
            self.behavior.on_view_updated(&self.core, &old_view, owner);
        }
    }

    fn perform_build(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.behavior.perform_build(&mut self.core, owner);
    }

    fn mount(
        &mut self,
        parent: Option<ElementId>,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        self.core.mount(parent, slot, owner);
        self.behavior.on_mount(&mut self.core, owner);
    }

    fn unmount(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.behavior.on_unmount(&mut self.core, owner);
        self.core.unmount(owner);
    }

    fn activate(&mut self) {
        self.core.activate();
        self.behavior.on_activate(&mut self.core);
    }

    fn deactivate(&mut self) {
        self.behavior.on_deactivate(&mut self.core);
        self.core.deactivate();
    }
}

// ============================================================================
// RenderObjectElement Implementation for Element<V, Variable,
// RenderBehavior<V>>
// ============================================================================

impl<V> RenderObjectElement for Element<V, Variable, RenderBehavior<V>>
where
    // `View` is required transitively: the `ElementBase` super-trait
    // bound now demands `V: View` (see the `impl ElementBase` block).
    // Every `RenderView` also implements `View` (via `impl_render_view!`
    // or a hand-written impl), so this exiles no real type.
    V: RenderView + View,
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<V::Protocol>>>,
{
    fn render_object_any(&self) -> Option<&dyn Any> {
        self.behavior
            .render_id_ref()
            .as_ref()
            .map(|r| r as &dyn Any)
    }

    fn render_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    fn attach_render_object(&mut self, slot: RenderSlot) {
        self.behavior.set_slot(slot);
        tracing::debug!(
            "Element::attach_render_object slot={:?} render_id={:?}",
            self.behavior.slot(),
            self.behavior.render_id()
        );
    }

    fn detach_render_object(&mut self) {
        tracing::debug!(
            "Element::detach_render_object slot={:?} render_id={:?}",
            self.behavior.slot(),
            self.behavior.render_id()
        );
        self.behavior.set_ancestor_render_object_element(None);
    }

    fn insert_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot) {
        if let Some(child_render_id) = child.downcast_ref::<RenderId>() {
            tracing::debug!(
                "Element::insert_render_object_child child_id={:?} slot={:?}",
                child_render_id,
                slot
            );

            if let Some(parent_id) = self.behavior.render_id()
                && let Some(pipeline_owner) = self.core.pipeline_owner()
            {
                let mut owner = pipeline_owner.write();
                let render_tree = owner.render_tree_mut();

                if let Some(child_node) = render_tree.get_mut(*child_render_id) {
                    child_node.set_parent(Some(parent_id));
                }

                if let Some(parent_node) = render_tree.get_mut(parent_id) {
                    parent_node.add_child(*child_render_id);
                }
            }
        }
    }

    fn move_render_object_child(
        &mut self,
        _child: &dyn Any,
        old_slot: RenderSlot,
        new_slot: RenderSlot,
    ) {
        tracing::debug!(
            "Element::move_render_object_child old={:?} new={:?}",
            old_slot,
            new_slot
        );
    }

    fn remove_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot) {
        if let Some(child_render_id) = child.downcast_ref::<RenderId>() {
            tracing::debug!(
                "Element::remove_render_object_child child_id={:?} slot={:?}",
                child_render_id,
                slot
            );

            if let Some(parent_id) = self.behavior.render_id()
                && let Some(pipeline_owner) = self.core.pipeline_owner()
            {
                let mut owner = pipeline_owner.write();
                let render_tree = owner.render_tree_mut();

                if let Some(parent_node) = render_tree.get_mut(parent_id) {
                    parent_node.remove_child(*child_render_id);
                }

                if let Some(child_node) = render_tree.get_mut(*child_render_id) {
                    child_node.set_parent(None);
                }
            }
        }
    }

    fn find_ancestor_render_object_element(&self) -> Option<ElementId> {
        self.behavior.ancestor_render_object_element()
    }

    fn set_ancestor_render_object_element(&mut self, ancestor: Option<ElementId>) {
        self.behavior.set_ancestor_render_object_element(ancestor);
    }
}

// ============================================================================
// Convenience Methods for Element<V, Variable, RenderBehavior<V>>
// ============================================================================

// ============================================================================
// StatefulElement-specific methods
// ============================================================================

impl<V> Element<V, Single, StatefulBehavior<V>>
where
    V: StatefulView,
{
    /// Get a reference to the state.
    pub fn state(&self) -> &V::State {
        &self.behavior.state
    }

    /// Get a mutable reference to the state.
    pub fn state_mut(&mut self) -> &mut V::State {
        &mut self.behavior.state
    }

    /// Mark as needing rebuild (like Flutter's setState).
    pub fn set_state<F>(&mut self, f: F)
    where
        F: FnOnce(&mut V::State),
    {
        f(&mut self.behavior.state);
        self.core.mark_dirty();
    }
}

// ============================================================================
// InheritedElement-specific methods
// ============================================================================

impl<V> Element<V, Single, InheritedBehavior<V>>
where
    V: InheritedView,
{
    /// Get the provided data.
    pub fn data(&self) -> &V::Data {
        self.behavior.data()
    }

    /// Register a dependent element with its tree depth.
    ///
    /// `depth` is the dependent's depth in the element tree, used by
    /// `BuildOwner::schedule_build_for` when this InheritedElement
    /// rebuilds with `update_should_notify == true`.
    pub fn add_dependent(&mut self, element: ElementId, depth: usize) {
        self.behavior.add_dependent(element, depth);
    }

    /// Remove a dependent element.
    pub fn remove_dependent(&mut self, element: ElementId) {
        self.behavior.remove_dependent(element);
    }

    /// Get all dependent elements as an id -> depth map.
    pub fn dependents(&self) -> &std::collections::HashMap<ElementId, usize> {
        self.behavior.dependents()
    }
}

// ============================================================================
// RenderElement-specific methods
// ============================================================================

impl<V> Element<V, Variable, RenderBehavior<V>>
where
    V: RenderView,
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<V::Protocol>>>,
{
    /// Get the RenderId of this element's RenderObject.
    pub fn render_id(&self) -> Option<RenderId> {
        self.behavior.render_id()
    }

    /// Set the PipelineOwner for this element.
    pub fn set_pipeline_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        let owner_any: Arc<dyn Any + Send + Sync> = owner as Arc<dyn Any + Send + Sync>;
        self.core.set_pipeline_owner_any(owner_any);
    }
}
