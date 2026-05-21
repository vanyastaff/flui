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

impl<V, A, B> ElementBase for Element<V, A, B>
where
    V: Clone + Send + Sync + 'static,
    A: ElementArity,
    B: ElementBehavior<V, A>,
{
    // ========================================================================
    // Simple Delegations to ElementCore
    // ========================================================================

    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
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
    // Lifecycle Methods with Behavior Hooks
    // ========================================================================

    fn update(&mut self, new_view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {
        // Snapshot the previous view so `on_view_updated` can pass it to state
        // hooks (e.g. `ViewState::did_update_view`).
        let old_view = self.core.view().clone();
        if self.core.update_view(new_view) {
            // Notify behavior of update
            self.behavior.on_update(&self.core);
            self.behavior.on_view_updated(&self.core, &old_view);
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
        self.behavior.on_mount(&mut self.core);
    }

    fn unmount(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.behavior.on_unmount(&mut self.core);
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
    V: RenderView,
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

    /// Register a dependent element.
    pub fn add_dependent(&mut self, element: ElementId) {
        self.behavior.add_dependent(element);
    }

    /// Remove a dependent element.
    pub fn remove_dependent(&mut self, element: ElementId) {
        self.behavior.remove_dependent(element);
    }

    /// Get all dependent elements.
    pub fn dependents(&self) -> &[ElementId] {
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
