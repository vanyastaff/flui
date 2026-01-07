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

use super::arity::ElementArity;
use super::child_storage::ElementChildStorage;
use crate::element::Lifecycle;
use crate::view::{ElementBase, View};
use flui_foundation::{ElementId, ListenerCallback, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Generic element core with arity-based child management.
///
/// This struct contains all common element state and lifecycle logic,
/// parameterized by:
/// - `V`: The View type (must be Clone + Send + Sync + 'static)
/// - `A`: The arity type (Leaf, Single, Optional, Variable)
///
/// # Type Parameters
///
/// * `V` - The View type this element manages. Must be cloneable because
///         Views are recreated each build cycle.
/// * `A` - The arity type (Leaf/Single/Optional/Variable) determining
///         how many children this element can have.
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
    /// Uses Arc<AtomicBool> for interior mutability, allowing listener
    /// callbacks to mark the element dirty without mutable access.
    /// This is essential for AnimationBehavior and other reactive patterns.
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
            _phantom: PhantomData,
        }
    }

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
    pub fn mount(&mut self, _parent: Option<ElementId>, slot: usize) {
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
    /// Sets lifecycle to Defunct and unmounts all children.
    pub fn unmount(&mut self) {
        self.children.unmount_children();
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
    /// Uses downcasting to safely extract the concrete view type V.
    /// If the downcast succeeds, clones the new view and marks dirty.
    ///
    /// # Arguments
    ///
    /// * `new_view` - The new View configuration
    ///
    /// # Returns
    ///
    /// `true` if update succeeded, `false` if downcast failed
    pub fn update_view(&mut self, new_view: &dyn View) -> bool {
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

    // ========================================================================
    // Child Management (eliminates child management boilerplate ~30 lines)
    // ========================================================================

    /// Update or create the child element with a new view.
    ///
    /// For Single/Optional arity, this updates the existing child or creates new.
    /// For Variable arity, use `update_or_create_children` instead.
    ///
    /// # Arguments
    ///
    /// * `child_view` - The new child View
    pub fn update_or_create_child(&mut self, child_view: Box<dyn View>) {
        if self.children.is_empty() {
            // First build - create child element
            self.children.create_from_view(child_view.as_ref());

            // Propagate owner if we have one
            if let Some(ref owner) = self.pipeline_owner {
                self.children.propagate_owner(Arc::clone(owner), self.parent_render_id);
            }

            // Mount child
            self.children.mount_children(None, self.depth + 1);

            // Build child's children
            self.children.perform_build_children();

            tracing::debug!("ElementCore::update_or_create_child created new child");
        } else {
            // Update existing child
            self.children.update_with_view(child_view.as_ref());
            self.children.perform_build_children();

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
    pub fn update_or_create_children(&mut self, child_views: Vec<Box<dyn View>>) {
        if self.children.is_empty() {
            // First build - create children
            self.children.create_from_views(&child_views);

            // Propagate owner if we have one
            if let Some(ref owner) = self.pipeline_owner {
                self.children.propagate_owner(Arc::clone(owner), self.parent_render_id);
            }

            // Mount children
            self.children.mount_children(None, self.depth + 1);

            // Build children's children
            self.children.perform_build_children();

            tracing::debug!("ElementCore::update_or_create_children created {} children", child_views.len());
        } else {
            // Update existing children
            self.children.update_with_views(&child_views);
            self.children.perform_build_children();

            tracing::debug!("ElementCore::update_or_create_children updated to {} children", child_views.len());
        }
    }

    /// Rebuild all children.
    ///
    /// Calls perform_build() on all child elements.
    pub fn rebuild_children(&mut self) {
        self.children.perform_build_children();
    }

    // ========================================================================
    // Pipeline Owner (eliminates propagation boilerplate ~15 lines)
    // ========================================================================

    /// Set the PipelineOwner for this element.
    ///
    /// Downcasts from Arc<dyn Any> to Arc<RwLock<PipelineOwner>>.
    /// This pattern is required for object safety of ElementBase.
    ///
    /// # Arguments
    ///
    /// * `owner` - Arc<dyn Any> that should downcast to Arc<RwLock<PipelineOwner>>
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
            self.children.propagate_owner(Arc::clone(owner), self.parent_render_id);
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
    /// This is useful for AnimationBehavior and other behaviors that need to
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

        core.mount(None, 5);

        assert_eq!(core.lifecycle(), Lifecycle::Active);
        assert_eq!(core.depth(), 5);
    }

    #[test]
    fn test_element_core_lifecycle() {
        let view = TestView { value: 42 };
        let mut core = ElementCore::<TestView, Single>::new(view);

        core.mount(None, 0);
        assert_eq!(core.lifecycle(), Lifecycle::Active);

        core.deactivate();
        assert_eq!(core.lifecycle(), Lifecycle::Inactive);

        core.activate();
        assert_eq!(core.lifecycle(), Lifecycle::Active);

        core.unmount();
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
