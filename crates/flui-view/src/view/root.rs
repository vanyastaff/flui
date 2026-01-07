//! RootRenderView - The root widget that bootstraps the render tree.
//!
//! This module provides the root View that connects the Element tree
//! to the RenderObject tree through PipelineOwner.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `_RawViewInternal` and `_RawViewElement`
//! which bootstrap the render tree for a FlutterView.

use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::view::{RenderView as RenderViewObject, ViewConfiguration};
use flui_types::Size;
use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::sync::Arc;

use crate::element::{Lifecycle, RenderObjectElement, RenderSlot, RenderTreeRootElement};
use crate::view::{ElementBase, View};

// ============================================================================
// RootRenderView - The root widget
// ============================================================================

/// The root View that bootstraps a render tree.
///
/// `RootRenderView` wraps the application's widget tree and:
/// 1. Creates a `RenderViewObject` (the root RenderObject)
/// 2. Sets it as `pipelineOwner.rootNode`
/// 3. Renders child widgets into the RenderView
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `_RawViewInternal` widget.
#[derive(Clone)]
pub struct RootRenderView<V: View + Clone> {
    /// The child widget to render
    child: V,
    /// Window/view size
    size: (f32, f32),
}

impl<V: View + Clone + std::fmt::Debug> std::fmt::Debug for RootRenderView<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootRenderView")
            .field("child", &self.child)
            .field("size", &self.size)
            .finish()
    }
}

impl<V: View + Clone> RootRenderView<V> {
    /// Create a new RootRenderView wrapping the given child.
    pub fn new(child: V, width: f32, height: f32) -> Self {
        Self {
            child,
            size: (width, height),
        }
    }
}

impl<V: View + Clone + Send + Sync + 'static> View for RootRenderView<V> {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(RootRenderElement::new(self))
    }
}

// ============================================================================
// RootRenderElement - The root element
// ============================================================================

/// Element for RootRenderView that bootstraps the render tree.
///
/// This element:
/// 1. Creates a PipelineOwner (or uses a provided one)
/// 2. Creates RenderViewObject as the root RenderObject in RenderTree
/// 3. Sets `pipelineOwner.root_id = render_id`
/// 4. Builds child widgets
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `_RawViewElement` which extends `RenderTreeRootElement`.
pub struct RootRenderElement<V: View + Clone> {
    /// The View configuration
    view: RootRenderView<V>,
    /// The root RenderObject ID in RenderTree
    render_id: Option<RenderId>,
    /// PipelineOwner for this render tree
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
    /// Child element
    child_element: Option<Box<dyn ElementBase>>,
    /// Lifecycle state
    lifecycle: Lifecycle,
    /// Depth (always 0 for root)
    depth: usize,
    /// Current slot
    slot: RenderSlot,
}

impl<V: View + Clone + Send + Sync + 'static> RootRenderElement<V> {
    /// Create a new RootRenderElement.
    pub fn new(view: &RootRenderView<V>) -> Self {
        Self {
            view: view.clone(),
            render_id: None,
            pipeline_owner: None,
            child_element: None,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            slot: RenderSlot::Single,
        }
    }

    /// Set the PipelineOwner for this render tree.
    pub fn set_pipeline_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        self.pipeline_owner = Some(owner);
    }

    /// Get the PipelineOwner.
    pub fn pipeline_owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        self.pipeline_owner.as_ref()
    }

    /// Get the RenderView ID.
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }
}

impl<V: View + Clone + Send + Sync + 'static> std::fmt::Debug for RootRenderElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootRenderElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("render_id", &self.render_id)
            .field("has_pipeline_owner", &self.pipeline_owner.is_some())
            .field("has_child", &self.child_element.is_some())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// ElementBase Implementation
// ============================================================================

impl<V: View + Clone + Send + Sync + 'static> ElementBase for RootRenderElement<V> {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<RootRenderView<V>>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        // Root elements must have no parent
        debug_assert!(parent.is_none(), "RootRenderElement cannot have a parent");

        self.lifecycle = Lifecycle::Active;

        // Create RenderView and insert into RenderTree
        let (width, height) = self.view.size;
        let mut render_view = RenderViewObject::new();
        let physical_size = Size::new(width, height);
        let config = ViewConfiguration::from_size(physical_size, 1.0);
        render_view.set_configuration(config);

        // Insert into PipelineOwner's RenderTree and get RenderId
        if let Some(ref pipeline_owner) = self.pipeline_owner {
            let mut owner = pipeline_owner.write();
            // Use PipelineOwner::insert which handles conversion to RenderNode
            let render_id = owner.insert(Box::new(render_view));
            owner.set_root_id(Some(render_id));
            self.render_id = Some(render_id);

            // Add to dirty lists
            owner.add_node_needing_layout(render_id.get(), 0);
            owner.add_node_needing_paint(render_id.get(), 0);
            owner.request_visual_update();

            tracing::debug!(
                "RootRenderElement::mount created RenderView render_id={:?} size={}x{}",
                render_id,
                width,
                height
            );
        }

        // Build child
        self.perform_build();
    }

    fn unmount(&mut self) {
        // Detach from PipelineOwner's RenderTree
        if let (Some(ref pipeline_owner), Some(render_id)) = (&self.pipeline_owner, self.render_id)
        {
            let mut owner = pipeline_owner.write();
            owner.set_root_id(None);
            owner.render_tree_mut().remove(render_id);
        }
        self.render_id = None;

        // Unmount child
        if let Some(ref mut child) = self.child_element {
            child.unmount();
        }
        self.child_element = None;

        self.lifecycle = Lifecycle::Defunct;
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        if let Some(ref mut child) = self.child_element {
            child.activate();
        }
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        if let Some(ref mut child) = self.child_element {
            child.deactivate();
        }
    }

    fn update(&mut self, new_view: &dyn View) {
        if let Some(v) = new_view.as_any().downcast_ref::<RootRenderView<V>>() {
            self.view = v.clone();
            // Update configuration if size changed
            if let (Some(ref pipeline_owner), Some(render_id)) =
                (&self.pipeline_owner, self.render_id)
            {
                let owner = pipeline_owner.write();
                if let Some(node) = owner.render_tree().get(render_id) {
                    // RenderView uses BoxProtocol
                    let mut render_object = node.box_render_object_mut();
                    if let Some(render_view) = render_object
                        .as_any_mut()
                        .downcast_mut::<RenderViewObject>()
                    {
                        let (width, height) = self.view.size;
                        let physical_size = Size::new(width, height);
                        let config = ViewConfiguration::from_size(physical_size, 1.0);
                        render_view.set_configuration(config);
                    }
                }
            }
        }
    }

    fn mark_needs_build(&mut self) {
        // Schedule rebuild
    }

    fn perform_build(&mut self) {
        // Create child element from child View
        if self.child_element.is_none() {
            // First build - create child element
            let mut child_element = self.view.child.create_element();

            // Pass PipelineOwner and parent RenderId to child via trait methods
            // Child needs these to insert its RenderObject into the RenderTree
            if let Some(ref pipeline_owner) = self.pipeline_owner {
                // Use ElementBase trait methods for pipeline propagation
                // This works for any element type that implements the trait
                let owner_any: Arc<dyn std::any::Any + Send + Sync> =
                    Arc::clone(pipeline_owner) as Arc<dyn std::any::Any + Send + Sync>;
                child_element.set_pipeline_owner_any(owner_any);
                child_element.set_parent_render_id(self.render_id);

                tracing::debug!(
                    "RootRenderElement::perform_build propagated PipelineOwner and parent_id={:?} to child",
                    self.render_id
                );
            }

            child_element.mount(None, 1); // Child's depth is 1 (root is 0)

            // Child element needs to build its children too
            child_element.perform_build();

            // Attach child's RenderObject to RenderTree as child of RenderView
            // Get child's RenderId and establish parent-child relationship
            if let (Some(ref pipeline_owner), Some(parent_id)) =
                (&self.pipeline_owner, self.render_id)
            {
                if let Some(child_render_id) = child_element
                    .render_object_any()
                    .and_then(|any| any.downcast_ref::<RenderId>().copied())
                {
                    let mut owner = pipeline_owner.write();
                    let render_tree = owner.render_tree_mut();

                    // Set parent on child node
                    if let Some(child_node) = render_tree.get_mut(child_render_id) {
                        child_node.set_parent(Some(parent_id));
                    }

                    // Add child to parent's children list
                    if let Some(parent_node) = render_tree.get_mut(parent_id) {
                        parent_node.add_child(child_render_id);
                        // Parent-child relationships are fully managed by NodeLinks
                        // No need to notify render objects directly
                    }

                    tracing::debug!(
                        "RootRenderElement::perform_build attached child render_id={:?} to parent render_id={:?}",
                        child_render_id,
                        parent_id
                    );
                }
            }

            self.child_element = Some(child_element);
        } else {
            // Rebuild - update child element
            if let Some(ref mut child) = self.child_element {
                let child_view: &dyn View = &self.view.child;
                child.update(child_view);
                child.perform_build();
            }
        }
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // In a full implementation, we'd have the child's ElementId
        let _ = visitor;
    }

    fn set_pipeline_owner_any(&mut self, owner: Arc<dyn Any + Send + Sync>) {
        // Downcast from Arc<dyn Any> to Arc<RwLock<PipelineOwner>>
        if let Ok(pipeline_owner) = owner.downcast::<RwLock<PipelineOwner>>() {
            self.pipeline_owner = Some(pipeline_owner);
            tracing::debug!("RootRenderElement::set_pipeline_owner_any received PipelineOwner");
        } else {
            tracing::warn!("RootRenderElement::set_pipeline_owner_any received wrong type");
        }
    }

    fn set_parent_render_id(&mut self, _parent_id: Option<RenderId>) {
        // Root element has no parent render object
    }
}

// ============================================================================
// RenderObjectElement Implementation
// ============================================================================

impl<V: View + Clone + Send + Sync + 'static> RenderObjectElement for RootRenderElement<V> {
    fn render_object_any(&self) -> Option<&dyn Any> {
        // With RenderTree, we don't have direct access to RenderObject
        // Use render_id and access via PipelineOwner.render_tree()
        None
    }

    fn render_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    fn attach_render_object(&mut self, slot: RenderSlot) {
        self.slot = slot;
        // RootRenderElement handles attachment in mount()
    }

    fn detach_render_object(&mut self) {
        // RootRenderElement handles detachment in unmount()
    }

    fn insert_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot) {
        // child should be RenderId of the child RenderObject
        if let Some(child_render_id) = child.downcast_ref::<RenderId>() {
            tracing::debug!(
                "RootRenderElement::insert_render_object_child child_id={:?} slot={:?}",
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

                // Add child to parent's children list
                if let Some(parent_node) = render_tree.get_mut(parent_id) {
                    parent_node.add_child(*child_render_id);
                    // Parent-child relationships are fully managed by NodeLinks
                    // No need to notify render objects directly
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
            "RootRenderElement::move_render_object_child old={:?} new={:?}",
            old_slot,
            new_slot
        );
    }

    fn remove_render_object_child(&mut self, child: &dyn Any, slot: RenderSlot) {
        if let Some(child_render_id) = child.downcast_ref::<RenderId>() {
            tracing::debug!(
                "RootRenderElement::remove_render_object_child child_id={:?} slot={:?}",
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
        // Root element has no ancestor
        None
    }

    fn set_ancestor_render_object_element(&mut self, _ancestor: Option<ElementId>) {
        // Root element ignores this - it has no ancestor
    }
}

// ============================================================================
// RenderTreeRootElement Implementation
// ============================================================================

impl<V: View + Clone + Send + Sync + 'static> RenderTreeRootElement for RootRenderElement<V> {
    fn pipeline_owner(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        self.pipeline_owner
            .as_ref()
            .map(|p| Arc::clone(p) as Arc<dyn Any + Send + Sync>)
    }

    fn set_pipeline_owner(&mut self, owner: Arc<dyn Any + Send + Sync>) {
        if let Ok(pipeline) = owner.downcast::<RwLock<PipelineOwner>>() {
            self.pipeline_owner = Some(pipeline);
        }
    }

    fn attach_to_pipeline_owner(&mut self) {
        // TODO: Migrate to RenderTree-based approach
        // 1. Insert RenderView into pipeline_owner.render_tree_mut()
        // 2. Get RenderId back
        // 3. Call pipeline_owner.set_root_id(Some(render_id))
        // 4. Store render_id in self instead of Arc<RwLock<RenderView>>
        unimplemented!("attach_to_pipeline_owner needs migration to RenderTree/RenderId")
    }

    fn detach_from_pipeline_owner(&mut self) {
        // TODO: Migrate to RenderTree-based approach
        // 1. Call pipeline_owner.set_root_id(None)
        // 2. Remove from pipeline_owner.render_tree_mut()
        unimplemented!("detach_from_pipeline_owner needs migration to RenderTree/RenderId")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::pipeline::PipelineOwner;

    #[derive(Clone)]
    struct TestView;

    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(TestElement)
        }
    }

    struct TestElement;

    impl ElementBase for TestElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<TestView>()
        }

        fn lifecycle(&self) -> Lifecycle {
            Lifecycle::Active
        }

        fn depth(&self) -> usize {
            1
        }

        fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {}
        fn unmount(&mut self) {}
        fn activate(&mut self) {}
        fn deactivate(&mut self) {}
        fn update(&mut self, _new_view: &dyn View) {}
        fn mark_needs_build(&mut self) {}
        fn perform_build(&mut self) {}
        fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {}
    }

    #[test]
    fn test_root_render_view_creation() {
        let child = TestView;
        let root = RootRenderView::new(child, 800.0, 600.0);
        assert_eq!(root.size, (800.0, 600.0));
    }

    #[test]
    fn test_root_render_element_mount() {
        let child = TestView;
        let root = RootRenderView::new(child, 800.0, 600.0);
        let mut element = RootRenderElement::new(&root);

        // Set up PipelineOwner
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));

        // Mount
        element.mount(None, 0);

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        // render_id is set after mount with pipeline owner
        assert!(element.render_id.is_some());
    }
}
