//! Child storage abstractions for element arity system.
//!
//! This module encapsulates ALL child management operations, eliminating
//! 80-120 lines of boilerplate per element type.
//!
//! Each arity type (Leaf, Single, Optional, Variable) has a corresponding
//! storage implementation that handles:
//! - Child creation from views
//! - Child updates with new views
//! - Lifecycle propagation (mount/unmount/activate/deactivate)
//! - PipelineOwner propagation
//! - Recursive building

use std::{any::Any, sync::Arc};

use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;

use crate::view::{ElementBase, View};

/// Trait for managing element children with different arities.
///
/// This trait abstracts over the different ways elements can have children:
/// - `NoChildStorage` - No children (Leaf)
/// - `SingleChildStorage` - Exactly one child (Single)
/// - `OptionalChildStorage` - Zero or one child (Optional)
/// - `VariableChildStorage` - N children (Variable)
///
/// By implementing this trait, we eliminate duplicate child management
/// code across all element types.
pub trait ElementChildStorage: Default + Send + Sync + std::fmt::Debug + 'static {
    /// Check if there are no children.
    fn is_empty(&self) -> bool;

    /// Get the number of children.
    fn len(&self) -> usize;

    /// Create child element(s) from a view.
    ///
    /// For Single/Variable arities, this creates the initial child element(s).
    /// For Leaf arity, this is a no-op.
    fn create_from_view(&mut self, view: &dyn View);

    /// Create child element(s) from multiple views.
    ///
    /// Used by Variable arity to create multiple children at once.
    /// For Single arity, uses only the first view.
    /// For Leaf arity, this is a no-op.
    fn create_from_views(&mut self, views: &[Box<dyn View>]);

    /// Update existing child element(s) with new view(s).
    ///
    /// For Single arity, updates the single child or creates if missing.
    /// For Variable arity, reconciles the children list with new views.
    /// For Leaf arity, this is a no-op.
    ///
    /// Threads the [`ElementOwner`](crate::ElementOwner) split-borrow
    /// handle into the child's `update` call so downstream registries
    /// (dependent sets, dirty heap) stay coherent. Plan §U8.
    fn update_with_view(&mut self, view: &dyn View, owner: &mut crate::ElementOwner<'_>);

    /// Update existing child element(s) with multiple views.
    ///
    /// Used by Variable arity for updating multiple children. Threads
    /// the owner handle into each child's `update` call.
    fn update_with_views(&mut self, views: &[Box<dyn View>], owner: &mut crate::ElementOwner<'_>);

    /// Mount all children.
    ///
    /// Called after children are created to mount them into the tree.
    /// Threads the owner handle into each child's `mount` call so
    /// `GlobalKey` registration / dirty scheduling can take effect.
    fn mount_children(
        &mut self,
        parent: Option<ElementId>,
        depth: usize,
        owner: &mut crate::ElementOwner<'_>,
    );

    /// Propagate PipelineOwner and parent RenderId to all children.
    ///
    /// This eliminates the repetitive propagation boilerplate in
    /// StatelessElement, RenderElement, etc.
    fn propagate_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>, parent_id: Option<RenderId>);

    /// Deactivate all children (temporarily removed from tree).
    fn deactivate_children(&mut self);

    /// Activate all children (re-inserted into tree).
    fn activate_children(&mut self);

    /// Unmount all children (permanently removed from tree).
    ///
    /// Threads the owner handle into each child's `unmount` call so
    /// `GlobalKey` deregistration / dependent-set cleanup can take
    /// effect during the recursive unmount.
    fn unmount_children(&mut self, owner: &mut crate::ElementOwner<'_>);

    /// Recursively build all children.
    ///
    /// Calls `perform_build()` on all child elements. Threads the owner
    /// handle so descendants can schedule rebuilds for inherited-data
    /// dependents (U9) without re-borrowing the
    /// [`BuildOwner`](crate::BuildOwner).
    fn perform_build_children(&mut self, owner: &mut crate::ElementOwner<'_>);

    /// Visit all children with a closure.
    ///
    /// Used for tree traversal and inspection.
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn ElementBase));

    /// Get immutable access to the first child, if any.
    ///
    /// Returns None for Leaf and Optional (when empty).
    fn first_child(&self) -> Option<&dyn ElementBase>;

    /// Get mutable access to the first child, if any.
    fn first_child_mut(&mut self) -> Option<&mut dyn ElementBase>;
}

// ============================================================================
// NoChildStorage - for Leaf arity (0 children)
// ============================================================================

/// Storage for elements with no children (Leaf arity).
///
/// All methods are no-ops since there are no children to manage.
#[derive(Default, Debug)]
pub struct NoChildStorage;

impl ElementChildStorage for NoChildStorage {
    fn is_empty(&self) -> bool {
        true
    }

    fn len(&self) -> usize {
        0
    }

    fn create_from_view(&mut self, _view: &dyn View) {
        // No children allowed
    }

    fn create_from_views(&mut self, _views: &[Box<dyn View>]) {
        // No children allowed
    }

    fn update_with_view(&mut self, _view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {
        // No children to update
    }

    fn update_with_views(
        &mut self,
        _views: &[Box<dyn View>],
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        // No children to update
    }

    fn mount_children(
        &mut self,
        _parent: Option<ElementId>,
        _depth: usize,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
        // No children to mount
    }

    fn propagate_owner(
        &mut self,
        _owner: Arc<RwLock<PipelineOwner>>,
        _parent_id: Option<RenderId>,
    ) {
        // No children to propagate to
    }

    fn deactivate_children(&mut self) {
        // No children to deactivate
    }

    fn activate_children(&mut self) {
        // No children to activate
    }

    fn unmount_children(&mut self, _owner: &mut crate::ElementOwner<'_>) {
        // No children to unmount
    }

    fn perform_build_children(&mut self, _owner: &mut crate::ElementOwner<'_>) {
        // No children to build
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn ElementBase)) {
        // No children to visit
    }

    fn first_child(&self) -> Option<&dyn ElementBase> {
        None
    }

    fn first_child_mut(&mut self) -> Option<&mut dyn ElementBase> {
        None
    }
}

// ============================================================================
// SingleChildStorage - for Single arity (1 child)
// ============================================================================

/// Storage for elements with exactly one child (Single arity).
///
/// Used by StatelessElement, StatefulElement, ProxyElement.
#[derive(Default)]
pub struct SingleChildStorage {
    child: Option<Box<dyn ElementBase>>,
}

impl std::fmt::Debug for SingleChildStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleChildStorage")
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl ElementChildStorage for SingleChildStorage {
    fn is_empty(&self) -> bool {
        self.child.is_none()
    }

    fn len(&self) -> usize {
        if self.child.is_some() { 1 } else { 0 }
    }

    fn create_from_view(&mut self, view: &dyn View) {
        if self.child.is_none() {
            self.child = Some(view.create_element());
        }
    }

    fn create_from_views(&mut self, views: &[Box<dyn View>]) {
        // Single arity - take only the first view
        if let Some(view) = views.first() {
            self.create_from_view(view.as_ref());
        }
    }

    fn update_with_view(&mut self, view: &dyn View, owner: &mut crate::ElementOwner<'_>) {
        if let Some(ref mut child) = self.child {
            // Update existing child
            child.update(view, owner);
        } else {
            // Create new child if missing
            self.child = Some(view.create_element());
        }
    }

    fn update_with_views(&mut self, views: &[Box<dyn View>], owner: &mut crate::ElementOwner<'_>) {
        // Single arity - use only the first view
        if let Some(view) = views.first() {
            self.update_with_view(view.as_ref(), owner);
        }
    }

    fn mount_children(
        &mut self,
        parent: Option<ElementId>,
        depth: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        if let Some(ref mut child) = self.child {
            child.mount(parent, depth, owner);
        }
    }

    fn propagate_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>, parent_id: Option<RenderId>) {
        if let Some(ref mut child) = self.child {
            let owner_any: Arc<dyn Any + Send + Sync> = owner as Arc<dyn Any + Send + Sync>;
            child.set_pipeline_owner_any(owner_any);
            child.set_parent_render_id(parent_id);
            tracing::debug!(
                "SingleChildStorage: propagated owner and parent_id={:?}",
                parent_id
            );
        }
    }

    fn deactivate_children(&mut self) {
        if let Some(ref mut child) = self.child {
            child.deactivate();
        }
    }

    fn activate_children(&mut self) {
        if let Some(ref mut child) = self.child {
            child.activate();
        }
    }

    fn unmount_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        if let Some(ref mut child) = self.child {
            child.unmount(owner);
        }
        self.child = None;
    }

    fn perform_build_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        if let Some(ref mut child) = self.child {
            child.perform_build(owner);
        }
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn ElementBase)) {
        if let Some(ref child) = self.child {
            visitor(child.as_ref());
        }
    }

    fn first_child(&self) -> Option<&dyn ElementBase> {
        self.child.as_deref()
    }

    fn first_child_mut(&mut self) -> Option<&mut dyn ElementBase> {
        self.child.as_deref_mut()
    }
}

// ============================================================================
// OptionalChildStorage - for Optional arity (0-1 children)
// ============================================================================

/// Storage for elements with zero or one child (Optional arity).
///
/// Similar to SingleChildStorage but allows empty state.
#[derive(Default)]
pub struct OptionalChildStorage {
    child: Option<Box<dyn ElementBase>>,
}

impl std::fmt::Debug for OptionalChildStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptionalChildStorage")
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl ElementChildStorage for OptionalChildStorage {
    fn is_empty(&self) -> bool {
        self.child.is_none()
    }

    fn len(&self) -> usize {
        if self.child.is_some() { 1 } else { 0 }
    }

    fn create_from_view(&mut self, view: &dyn View) {
        self.child = Some(view.create_element());
    }

    fn create_from_views(&mut self, views: &[Box<dyn View>]) {
        if let Some(view) = views.first() {
            self.create_from_view(view.as_ref());
        }
    }

    fn update_with_view(&mut self, view: &dyn View, owner: &mut crate::ElementOwner<'_>) {
        if let Some(ref mut child) = self.child {
            child.update(view, owner);
        } else {
            self.child = Some(view.create_element());
        }
    }

    fn update_with_views(&mut self, views: &[Box<dyn View>], owner: &mut crate::ElementOwner<'_>) {
        if let Some(view) = views.first() {
            self.update_with_view(view.as_ref(), owner);
        } else {
            // No views provided - clear child
            self.unmount_children(owner);
        }
    }

    fn mount_children(
        &mut self,
        parent: Option<ElementId>,
        depth: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        if let Some(ref mut child) = self.child {
            child.mount(parent, depth, owner);
        }
    }

    fn propagate_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>, parent_id: Option<RenderId>) {
        if let Some(ref mut child) = self.child {
            let owner_any: Arc<dyn Any + Send + Sync> = owner as Arc<dyn Any + Send + Sync>;
            child.set_pipeline_owner_any(owner_any);
            child.set_parent_render_id(parent_id);
            tracing::debug!(
                "OptionalChildStorage: propagated owner and parent_id={:?}",
                parent_id
            );
        }
    }

    fn deactivate_children(&mut self) {
        if let Some(ref mut child) = self.child {
            child.deactivate();
        }
    }

    fn activate_children(&mut self) {
        if let Some(ref mut child) = self.child {
            child.activate();
        }
    }

    fn unmount_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        if let Some(ref mut child) = self.child {
            child.unmount(owner);
        }
        self.child = None;
    }

    fn perform_build_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        if let Some(ref mut child) = self.child {
            child.perform_build(owner);
        }
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn ElementBase)) {
        if let Some(ref child) = self.child {
            visitor(child.as_ref());
        }
    }

    fn first_child(&self) -> Option<&dyn ElementBase> {
        self.child.as_deref()
    }

    fn first_child_mut(&mut self) -> Option<&mut dyn ElementBase> {
        self.child.as_deref_mut()
    }
}

// ============================================================================
// VariableChildStorage - for Variable arity (N children)
// ============================================================================

/// Storage for elements with multiple children (Variable arity).
///
/// Used by RenderElement and other multi-child elements.
#[derive(Default)]
pub struct VariableChildStorage {
    children: Vec<Box<dyn ElementBase>>,
}

impl std::fmt::Debug for VariableChildStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VariableChildStorage")
            .field("count", &self.children.len())
            .finish()
    }
}

impl ElementChildStorage for VariableChildStorage {
    fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    fn len(&self) -> usize {
        self.children.len()
    }

    fn create_from_view(&mut self, view: &dyn View) {
        self.children.push(view.create_element());
    }

    fn create_from_views(&mut self, views: &[Box<dyn View>]) {
        for view in views {
            self.create_from_view(view.as_ref());
        }
    }

    fn update_with_view(&mut self, _view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {
        // For Variable arity, use update_with_views instead
        tracing::warn!(
            "VariableChildStorage::update_with_view called - use update_with_views instead"
        );
    }

    fn update_with_views(&mut self, views: &[Box<dyn View>], owner: &mut crate::ElementOwner<'_>) {
        // Keyed 5-phase reconciliation (plan §U5 / origin R12). Matches
        // old child elements to new Views by `Key`, falling back to
        // positional matching for keyless children — so a keyed widget
        // moved to a new slot keeps its element (and thus its state).
        //
        // `reconcile_children` leaves newly created elements unmounted
        // (it operates on the bare box-vec and cannot reach the
        // `PipelineOwner`); `ElementCore::update_or_create_children`
        // finishes their lifecycle (propagate owner → mount → build).
        let view_refs: Vec<&dyn View> = views.iter().map(std::convert::AsRef::as_ref).collect();
        crate::reconcile_children(&mut self.children, &view_refs, owner);
    }

    fn mount_children(
        &mut self,
        parent: Option<ElementId>,
        depth: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        // Idempotent: only `Initial` children are mounted. After keyed
        // reconciliation the box-vec holds a mix of *reused* children
        // (already `Active` — re-mounting them would reset their depth
        // and, for `RenderObjectElement`s, create a duplicate
        // `RenderObject`) and *freshly created* children still in
        // `Initial`. Skipping the active ones makes a post-reconcile
        // `mount_children` sweep mount exactly the new elements.
        for (i, child) in self.children.iter_mut().enumerate() {
            if child.lifecycle() == crate::element::Lifecycle::Initial {
                child.mount(parent, depth + i, &mut *owner);
            }
        }
    }

    fn propagate_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>, parent_id: Option<RenderId>) {
        for child in &mut self.children {
            let owner_any: Arc<dyn Any + Send + Sync> =
                Arc::clone(&owner) as Arc<dyn Any + Send + Sync>;
            child.set_pipeline_owner_any(owner_any);
            child.set_parent_render_id(parent_id);
        }
        tracing::debug!(
            "VariableChildStorage: propagated owner and parent_id={:?} to {} children",
            parent_id,
            self.children.len()
        );
    }

    fn deactivate_children(&mut self) {
        for child in &mut self.children {
            child.deactivate();
        }
    }

    fn activate_children(&mut self) {
        for child in &mut self.children {
            child.activate();
        }
    }

    fn unmount_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        for child in &mut self.children {
            child.unmount(&mut *owner);
        }
        self.children.clear();
    }

    fn perform_build_children(&mut self, owner: &mut crate::ElementOwner<'_>) {
        for child in &mut self.children {
            child.perform_build(&mut *owner);
        }
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn ElementBase)) {
        for child in &self.children {
            visitor(child.as_ref());
        }
    }

    fn first_child(&self) -> Option<&dyn ElementBase> {
        self.children.first().map(std::convert::AsRef::as_ref)
    }

    fn first_child_mut(&mut self) -> Option<&mut dyn ElementBase> {
        self.children.first_mut().map(std::convert::AsMut::as_mut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Basic compile-time checks
    fn assert_storage_implemented<S: ElementChildStorage>() {}

    #[test]
    fn test_storage_types_implement_trait() {
        assert_storage_implemented::<NoChildStorage>();
        assert_storage_implemented::<SingleChildStorage>();
        assert_storage_implemented::<OptionalChildStorage>();
        assert_storage_implemented::<VariableChildStorage>();
    }

    #[test]
    fn test_no_child_storage() {
        let storage = NoChildStorage;
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_single_child_storage() {
        let storage = SingleChildStorage::default();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);

        // After setting child (in real usage), len would be 1
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_optional_child_storage() {
        let storage = OptionalChildStorage::default();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_variable_child_storage() {
        let storage = VariableChildStorage::default();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }
}
