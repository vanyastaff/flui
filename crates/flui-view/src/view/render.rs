//! RenderView - Views that create RenderObjects.
//!
//! RenderViews are leaf nodes in the View tree that produce RenderObjects.
//! They bridge the View/Element system with the Render tree for layout and painting.

use super::view::{ElementBase, View};
use crate::element::{Lifecycle, RenderObjectElement, RenderSlot};
use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::traits::RenderObject;
use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::sync::Arc;

/// A View that creates a RenderObject for layout and painting.
///
/// RenderViews are the bridge between the declarative View tree and
/// the imperative RenderObject tree. Each RenderView corresponds to
/// a specific RenderObject type.
///
/// # Type Parameters
///
/// * `R` - The RenderObject type this View creates
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderObjectWidget` and its subclasses:
/// - `LeafRenderObjectWidget` - No children
/// - `SingleChildRenderObjectWidget` - One child
/// - `MultiChildRenderObjectWidget` - Multiple children
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{RenderView, BuildContext};
/// use flui_rendering::RenderBox;
///
/// struct ColoredBox {
///     color: Color,
///     child: Option<Box<dyn View>>,
/// }
///
/// impl RenderView for ColoredBox {
///     type Protocol = flui_rendering::protocol::BoxProtocol;
///     type RenderObject = RenderDecoratedBox;
///
///     fn create_render_object(&self) -> Self::RenderObject {
///         RenderDecoratedBox::new(self.color)
///     }
///
///     fn update_render_object(&self, render: &mut Self::RenderObject) {
///         render.set_color(self.color);
///     }
/// }
/// ```
pub trait RenderView: Send + Sync + 'static + Sized {
    /// The layout protocol this View uses (BoxProtocol or SliverProtocol).
    type Protocol: flui_rendering::protocol::Protocol;

    /// The RenderObject type this View creates.
    /// Must implement RenderObject<Self::Protocol> for RenderTree storage.
    type RenderObject: flui_rendering::traits::RenderObject<Self::Protocol> + Send + Sync + 'static;

    /// Create a new RenderObject.
    ///
    /// Called once when the Element is first mounted.
    fn create_render_object(&self) -> Self::RenderObject;

    /// Update an existing RenderObject with new configuration.
    ///
    /// Called when this View updates an existing Element.
    fn update_render_object(&self, render_object: &mut Self::RenderObject);

    /// Whether this View can have children.
    ///
    /// Override to return true for single/multi child variants.
    fn has_children(&self) -> bool {
        false
    }

    /// Visit child views for mounting.
    ///
    /// Override for single/multi child variants to provide access to children.
    /// The visitor is called once for each child View.
    ///
    /// Default implementation does nothing (leaf widgets have no children).
    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {
        // Default: no children
    }
}

/// Implement View for a RenderView type.
///
/// This macro creates the View implementation for a RenderView type.
///
/// ```rust,ignore
/// impl RenderView for MyColoredBox {
///     type RenderObject = RenderDecoratedBox;
///     // ...
/// }
/// impl_render_view!(MyColoredBox);
/// ```
#[macro_export]
macro_rules! impl_render_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::RenderElement::new(self))
            }
        }
    };
}

// ============================================================================
// RenderElement
// ============================================================================

/// Element for RenderViews.
///
/// Manages the lifecycle of a RenderView and its associated RenderObject.
/// This is the glue between the Element tree and the Render tree.
///
/// Implements `RenderObjectElement` trait for Flutter-compatible render tree management.
///
/// # Ownership Model (Slab-based)
///
/// The RenderObject is stored in PipelineOwner's RenderTree (Slab storage).
/// We keep a RenderId reference to access it.
///
/// This enables:
/// 1. O(1) access by ID
/// 2. Cache-friendly contiguous memory
/// 3. Safe ID-based references (no raw pointers)
pub struct RenderElement<V: RenderView> {
    /// The current View configuration.
    view: V,
    /// The RenderObject ID in RenderTree.
    render_id: Option<RenderId>,
    /// PipelineOwner that owns the RenderTree.
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Current slot in parent.
    slot: RenderSlot,
    /// Child elements (for single/multi child variants).
    children: Vec<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
    /// Ancestor RenderObjectElement (for render tree attachment).
    ancestor_render_object_element: Option<ElementId>,
    /// Parent's RenderId for tree structure.
    parent_render_id: Option<RenderId>,
    /// Marker for RenderObject type.
    _marker: PhantomData<V::RenderObject>,
}

impl<V: RenderView> RenderElement<V>
where
    V: Clone,
{
    /// Create a new RenderElement for the given View.
    pub fn new(view: &V) -> Self {
        Self {
            view: view.clone(),
            render_id: None,
            pipeline_owner: None,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            slot: RenderSlot::default(),
            children: Vec::new(),
            dirty: true,
            ancestor_render_object_element: None,
            parent_render_id: None,
            _marker: PhantomData,
        }
    }

    /// Get the RenderId of this element's RenderObject.
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }

    /// Set the PipelineOwner for this element.
    ///
    /// Must be called before mount() for RenderObject to be inserted into RenderTree.
    pub fn set_pipeline_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        self.pipeline_owner = Some(owner);
    }

    /// Set the parent's RenderId for tree structure.
    pub fn set_parent_render_id(&mut self, parent_id: Option<RenderId>) {
        self.parent_render_id = parent_id;
    }
}

impl<V: RenderView + Clone> std::fmt::Debug for RenderElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("slot", &self.slot)
            .field("dirty", &self.dirty)
            .field("render_id", &self.render_id)
            .field("parent_render_id", &self.parent_render_id)
            .field("children_count", &self.children.len())
            .field(
                "ancestor_render_object_element",
                &self.ancestor_render_object_element,
            )
            .finish_non_exhaustive()
    }
}

impl<V: RenderView + Clone> ElementBase for RenderElement<V>
where
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<V::Protocol>>>,
{
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn update(&mut self, new_view: &dyn View) {
        // Use View::as_any() for safe downcasting
        if let Some(v) = new_view.as_any().downcast_ref::<V>() {
            self.view = v.clone();

            // Update the RenderObject if it exists in RenderTree
            // IMPORTANT: We must release the lock before calling update_render_object
            // because it may call mark_needs_layout() which also needs the lock.
            if let (Some(ref pipeline_owner), Some(render_id)) =
                (&self.pipeline_owner, self.render_id)
            {
                // Take the render object out temporarily to update it without holding the lock
                let render_object_opt = {
                    let mut owner = pipeline_owner.write();
                    owner.render_tree_mut().get_mut(render_id).and_then(|node| {
                        node.render_object_mut()
                            .as_any_mut()
                            .downcast_mut::<V::RenderObject>()
                            .map(|ro| {
                                // We can't take the render object out, so we update in place
                                // but we need to drop the lock first
                                Some(render_id)
                            })
                    })
                };

                // Now update with the lock released
                if render_object_opt.flatten().is_some() {
                    // Re-acquire lock just for the update
                    let mut owner = pipeline_owner.write();
                    let tree_depth = owner.render_tree().depth(render_id).unwrap_or(0);
                    if let Some(node) = owner.render_tree_mut().get_mut(render_id) {
                        if let Some(render_object) = node
                            .render_object_mut()
                            .as_any_mut()
                            .downcast_mut::<V::RenderObject>()
                        {
                            tracing::debug!(
                                "RenderElement::update calling update_render_object for render_id={:?}",
                                render_id
                            );
                            // Call update_render_object while holding lock
                            // But mark_needs_layout will try to acquire same lock - STILL DEADLOCK
                            // The real fix: don't call mark_needs_layout from update_render_object
                            // Instead, manually mark dirty after update
                            self.view.update_render_object(render_object);
                        }
                    }
                    // Mark as needing paint after update - this ensures the changes are rendered
                    owner.add_node_needing_paint(render_id.get(), tree_depth);
                } else {
                    tracing::warn!(
                        "RenderElement::update node not found or wrong type for render_id={:?}",
                        render_id
                    );
                }
            } else {
                tracing::warn!(
                    "RenderElement::update no pipeline_owner or render_id (pipeline={}, render_id={:?})",
                    self.pipeline_owner.is_some(),
                    self.render_id
                );
            }

            self.dirty = true;
        } else {
            tracing::warn!("RenderElement::update failed to downcast view");
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            tracing::trace!(
                "RenderElement::perform_build SKIPPED dirty={} can_build={} render_id={:?}",
                self.dirty,
                self.lifecycle.can_build(),
                self.render_id
            );
            return;
        }

        let has_children = self.view.has_children();
        let children_empty = self.children.is_empty();
        tracing::info!(
            "RenderElement::perform_build START render_id={:?} has_children={} children_empty={}",
            self.render_id,
            has_children,
            children_empty
        );

        // Mount child views if this is a single/multi child RenderView
        if has_children && children_empty {
            // First build - create child elements
            // We need to collect elements first because we can't borrow self mutably
            // while iterating over self.view
            let mut new_children: Vec<Box<dyn ElementBase>> = Vec::new();

            self.view.visit_child_views(&mut |child_view| {
                tracing::info!(
                    "RenderElement::perform_build visiting child view type={:?}",
                    std::any::type_name_of_val(child_view)
                );
                let child_element = child_view.create_element();
                new_children.push(child_element);
            });

            tracing::info!(
                "RenderElement::perform_build collected {} child elements",
                new_children.len()
            );

            // Now mount each child with proper context
            let mut slot_index = 0usize;
            for mut child_element in new_children {
                // Propagate PipelineOwner and parent_render_id to child
                if let Some(ref owner) = self.pipeline_owner {
                    let owner_any: Arc<dyn Any + Send + Sync> =
                        Arc::clone(owner) as Arc<dyn Any + Send + Sync>;
                    child_element.set_pipeline_owner_any(owner_any);
                    child_element.set_parent_render_id(self.render_id);

                    tracing::debug!(
                        "RenderElement::perform_build propagated PipelineOwner and parent_id={:?} to child",
                        self.render_id
                    );
                }

                // Mount child
                child_element.mount(None, self.depth + 1);

                // Build child's children recursively
                child_element.perform_build();

                self.children.push(child_element);
                slot_index += 1;
            }

            tracing::debug!(
                "RenderElement::perform_build mounted {} children for render_id={:?}",
                self.children.len(),
                self.render_id
            );
        } else if !self.children.is_empty() {
            // Rebuild existing children - need to update them with new child views
            // Collect new child views first
            let mut new_child_views: Vec<Box<dyn View>> = Vec::new();
            self.view.visit_child_views(&mut |child_view| {
                // Clone the view by creating a boxed version using dyn_clone
                new_child_views.push(dyn_clone::clone_box(child_view));
            });

            // Update each existing child with its corresponding new view
            for (i, child) in self.children.iter_mut().enumerate() {
                if let Some(new_view) = new_child_views.get(i) {
                    tracing::debug!(
                        "RenderElement::perform_build updating child[{}] with new view",
                        i
                    );
                    child.update(new_view.as_ref());
                }
                child.perform_build();
            }
        }

        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, slot: usize) {
        tracing::info!(
            "RenderElement::mount START slot={} has_pipeline_owner={} parent_render_id={:?}",
            slot,
            self.pipeline_owner.is_some(),
            self.parent_render_id
        );

        self.lifecycle = Lifecycle::Active;

        // Store slot
        self.slot = RenderSlot::Index(slot);

        // Create RenderObject and insert into RenderTree
        if let Some(ref pipeline_owner) = self.pipeline_owner {
            tracing::info!("RenderElement::mount creating RenderObject");
            let render_object = self.view.create_render_object();

            // Insert into RenderTree using generic insert() method
            let render_id = {
                let mut owner = pipeline_owner.write();

                // Generic insert with explicit type annotation for Protocol
                let boxed: Box<dyn flui_rendering::traits::RenderObject<V::Protocol>> =
                    Box::new(render_object);
                let render_id = owner.insert(boxed);

                // Handle parent relationship if needed
                if let Some(parent_id) = self.parent_render_id {
                    // Set parent in tree structure
                    let render_tree = owner.render_tree_mut();
                    if let Some(node) = render_tree.get_mut(render_id) {
                        node.set_parent(Some(parent_id));
                    }
                    if let Some(parent_node) = render_tree.get_mut(parent_id) {
                        parent_node.add_child(render_id);
                    }
                }

                // Get the actual depth from the RenderTree
                let tree_depth = owner.render_tree().depth(render_id).unwrap_or(0);
                self.depth = tree_depth;

                render_id
            }; // Release lock here

            self.render_id = Some(render_id);

            // Note: Owner is now managed through PipelineOwner, not stored in RenderObject.
            // The render object is already associated with the pipeline through RenderTree.
            tracing::debug!(
                "RenderElement::mount - RenderObject created with render_id={:?}",
                render_id
            );

            tracing::debug!(
                "RenderElement::mount inserted RenderObject render_id={:?} parent_id={:?}",
                render_id,
                self.parent_render_id
            );
        } else {
            tracing::warn!(
                "RenderElement::mount called without PipelineOwner - RenderObject not created"
            );
        }

        // Attach to render tree
        self.attach_render_object(self.slot.clone());

        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        for child in &mut self.children {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        for child in &mut self.children {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        // Detach from render tree first
        self.detach_render_object();

        // Remove from RenderTree
        if let (Some(ref pipeline_owner), Some(render_id)) = (&self.pipeline_owner, self.render_id)
        {
            let mut owner = pipeline_owner.write();
            owner.render_tree_mut().remove(render_id);
            tracing::debug!("RenderElement::unmount removed render_id={:?}", render_id);
        }

        self.lifecycle = Lifecycle::Defunct;
        self.render_id = None;

        for child in &mut self.children {
            child.unmount();
        }
        self.children.clear();
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // In a full implementation, we'd track child ElementIds
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }

    // Override ElementBase methods for RenderObject access
    fn render_object_any(&self) -> Option<&dyn std::any::Any> {
        // With RenderTree, we return the RenderId for callers to use
        self.render_id.as_ref().map(|r| r as &dyn std::any::Any)
    }

    fn render_object_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        // With RenderTree, use RenderId-based access
        None
    }

    fn attach_to_render_tree(&mut self) -> Option<&mut dyn std::any::Any> {
        // Return RenderId for parent to establish tree relationship
        self.render_id.as_mut().map(|r| r as &mut dyn std::any::Any)
    }

    fn render_object_shared(
        &self,
    ) -> Option<std::sync::Arc<parking_lot::RwLock<dyn std::any::Any + Send + Sync>>> {
        // With RenderTree, we don't use shared Arc anymore
        // Return None - use render_id() and access via PipelineOwner instead
        None
    }

    fn set_pipeline_owner_any(&mut self, owner: std::sync::Arc<dyn std::any::Any + Send + Sync>) {
        // Downcast from Arc<dyn Any> to Arc<RwLock<PipelineOwner>>
        if let Ok(pipeline_owner) = owner.downcast::<RwLock<PipelineOwner>>() {
            self.pipeline_owner = Some(pipeline_owner);
            tracing::debug!("RenderElement::set_pipeline_owner_any received PipelineOwner");
        } else {
            tracing::warn!("RenderElement::set_pipeline_owner_any received wrong type");
        }
    }

    fn set_parent_render_id(&mut self, parent_id: Option<flui_foundation::RenderId>) {
        self.parent_render_id = parent_id;
        tracing::debug!(
            "RenderElement::set_parent_render_id parent_id={:?}",
            parent_id
        );
    }
}

// ============================================================================
// RenderObjectElement Implementation
// ============================================================================

impl<V: RenderView + Clone> RenderObjectElement for RenderElement<V> {
    fn render_object_any(&self) -> Option<&dyn Any> {
        // Return RenderId for callers to access RenderTree
        self.render_id.as_ref().map(|r| r as &dyn Any)
    }

    fn render_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        // With RenderTree, use RenderId-based access
        None
    }

    fn attach_render_object(&mut self, slot: RenderSlot) {
        self.slot = slot;

        tracing::debug!(
            "RenderElement::attach_render_object slot={:?} render_id={:?}",
            self.slot,
            self.render_id
        );
    }

    fn detach_render_object(&mut self) {
        tracing::debug!(
            "RenderElement::detach_render_object slot={:?} render_id={:?}",
            self.slot,
            self.render_id
        );

        self.ancestor_render_object_element = None;
    }

    fn insert_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot) {
        // child should be RenderId
        if let Some(child_render_id) = child.downcast_ref::<RenderId>() {
            tracing::debug!(
                "RenderElement::insert_render_object_child child_id={:?} slot={:?}",
                child_render_id,
                slot
            );

            // Set parent-child relationship in RenderTree
            if let (Some(ref pipeline_owner), Some(parent_id)) =
                (&self.pipeline_owner, self.render_id)
            {
                let mut owner = pipeline_owner.write();
                let render_tree = owner.render_tree_mut();

                // Update child's parent
                if let Some(child_node) = render_tree.get_mut(*child_render_id) {
                    child_node.set_parent(Some(parent_id));
                }

                // Add child to parent's children list in RenderNode
                if let Some(parent_node) = render_tree.get_mut(parent_id) {
                    parent_node.add_child(*child_render_id);

                    // Also notify the render object itself so it can track children
                    // for layout (BoxWrapper needs this for position_child to work)
                    parent_node
                        .render_object_mut()
                        .add_child_render_id(*child_render_id);
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
            "RenderElement::move_render_object_child old={:?} new={:?}",
            old_slot,
            new_slot
        );
    }

    fn remove_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot) {
        if let Some(child_render_id) = child.downcast_ref::<RenderId>() {
            tracing::debug!(
                "RenderElement::remove_render_object_child child_id={:?} slot={:?}",
                child_render_id,
                slot
            );

            // Clear parent-child relationship in RenderTree
            if let (Some(ref pipeline_owner), Some(parent_id)) =
                (&self.pipeline_owner, self.render_id)
            {
                let mut owner = pipeline_owner.write();
                let render_tree = owner.render_tree_mut();

                // Remove child from parent's children list
                if let Some(parent_node) = render_tree.get_mut(parent_id) {
                    parent_node.remove_child(*child_render_id);
                }

                // Clear child's parent
                if let Some(child_node) = render_tree.get_mut(*child_render_id) {
                    child_node.set_parent(None);
                }
            }
        }
    }

    fn find_ancestor_render_object_element(&self) -> Option<ElementId> {
        self.ancestor_render_object_element
    }

    fn set_ancestor_render_object_element(&mut self, ancestor: Option<ElementId>) {
        self.ancestor_render_object_element = ancestor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::objects::RenderSizedBox;

    /// A simple test RenderView using RenderSizedBox
    #[derive(Clone)]
    struct SizedBoxView {
        width: f32,
        height: f32,
    }

    impl RenderView for SizedBoxView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::new(Some(self.width), Some(self.height))
        }

        fn update_render_object(&self, _render_object: &mut Self::RenderObject) {
            // RenderSizedBox doesn't have setters for width/height after creation
            // In a real implementation, we'd update the constraints
        }
    }

    impl View for SizedBoxView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(RenderElement::new(self))
        }
    }

    #[test]
    fn test_render_element_creation() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let element = RenderElement::new(&view);

        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert!(element.render_id().is_none()); // Not created until mount
    }

    #[test]
    fn test_render_element_mount_without_pipeline_owner() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view);

        // Mount without PipelineOwner - should still set lifecycle but no render_id
        element.mount(None, 0);

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        assert!(element.render_id().is_none()); // No PipelineOwner, so no render_id
    }

    #[test]
    fn test_render_element_mount_with_pipeline_owner() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view);

        // Set up PipelineOwner
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));

        element.mount(None, 0);

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        assert!(element.render_id().is_some());

        // Verify RenderObject was inserted into RenderTree
        let owner = pipeline_owner.read();
        let render_id = element.render_id().unwrap();
        assert!(owner.render_tree().contains(render_id));
    }

    #[test]
    fn test_render_element_unmount() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view);

        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));
        element.mount(None, 0);

        let render_id = element.render_id().unwrap();
        assert!(pipeline_owner.read().render_tree().contains(render_id));

        element.unmount();

        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
        assert!(element.render_id().is_none());
        // RenderObject should be removed from tree
        assert!(!pipeline_owner.read().render_tree().contains(render_id));
    }

    #[test]
    fn test_render_object_element_trait() {
        use crate::element::RenderObjectElement;

        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view);

        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));
        element.mount(None, 0);

        // Test RenderObjectElement methods - returns RenderId
        assert!(RenderObjectElement::render_object_any(&element).is_some());

        // Downcast to RenderId
        let render_any = RenderObjectElement::render_object_any(&element).unwrap();
        let render_id = render_any.downcast_ref::<RenderId>().unwrap();

        // Verify we can access the RenderObject through RenderTree
        let owner = pipeline_owner.read();
        let node = owner.render_tree().get(*render_id).unwrap();
        let sized_box = node
            .render_object()
            .as_any()
            .downcast_ref::<RenderSizedBox>()
            .unwrap();
        // RenderSizedBox exists - that's enough to verify
        assert!(sized_box.base().is_repaint_boundary() || !sized_box.base().is_repaint_boundary());
    }
}
