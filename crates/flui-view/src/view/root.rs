//! RootRenderView - The root widget that bootstraps the render tree.
//!
//! This module provides the root View that connects the Element tree
//! to the RenderObject tree through PipelineOwner.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `_RawViewInternal` and `_RawViewElement`
//! which bootstrap the render tree for a FlutterView.

use std::{
    any::{Any, TypeId},
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId};
use flui_rendering::{
    pipeline::PipelineOwner,
    storage::RenderNode,
    view::{RenderView as RenderViewObject, RenderViewAdapter, ViewConfiguration},
};
use flui_types::{Size, geometry::px};
use parking_lot::RwLock;

use crate::{
    element::{Lifecycle, RenderObjectElement, RenderSlot, RenderTreeRootElement},
    view::{ElementBase, View},
};

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
/// This corresponds to Flutter's `_RawViewElement` which extends
/// `RenderTreeRootElement`.
pub struct RootRenderElement<V: View + Clone> {
    /// The View configuration
    view: RootRenderView<V>,
    /// The root RenderObject ID in RenderTree
    render_id: Option<RenderId>,
    /// PipelineOwner for this render tree
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
    /// Lifecycle state
    lifecycle: Lifecycle,
    /// Depth (always 0 for root)
    depth: usize,
    /// Current slot
    slot: RenderSlot,
    /// Whether this element needs a rebuild (its child reconcile must run).
    ///
    /// E3 (atomic box→arena swap): the root's single child is a
    /// slab-resident node reconciled by `BuildOwner::build_scope`, which
    /// SKIPS any popped dirty entry whose `is_dirty()` is false (a clean
    /// element's empty build would otherwise make the reconcile wrongly
    /// remove its children). `attach_root_widget` schedules the root via
    /// `schedule_build_for` WITHOUT a `mark_needs_build`, so without an
    /// honest dirty flag the root would be popped, skipped, and its child
    /// never reconciled — the app would render nothing. This flag is the
    /// root's dirty bit: `true` at construction + mount, cleared once
    /// `build_into_views` has handed the child to the reconciler.
    needs_build: bool,
}

impl<V: View + Clone + Send + Sync + 'static> RootRenderElement<V> {
    /// Create a new RootRenderElement.
    pub fn new(view: &RootRenderView<V>) -> Self {
        Self {
            view: view.clone(),
            render_id: None,
            pipeline_owner: None,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            slot: RenderSlot::Single,
            needs_build: true,
        }
    }

    /// Set the PipelineOwner for this render tree.
    pub fn set_pipeline_owner(&mut self, owner: Arc<RwLock<PipelineOwner>>) {
        self.pipeline_owner = Some(owner);
    }

    /// Get the PipelineOwner.
    pub fn pipeline_owner(&self) -> Option<&Arc<RwLock<PipelineOwner>>> {
        // PORT-CHECK-OK-SP6: RootView pipeline_owner accessor; pre-existing SP-6; consolidation tracked
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

    fn mount(
        &mut self,
        parent: Option<ElementId>,
        _slot: usize,
        _element_owner: &mut crate::ElementOwner<'_>,
    ) {
        // Root elements must have no parent
        debug_assert!(parent.is_none(), "RootRenderElement cannot have a parent");

        self.lifecycle = Lifecycle::Active;
        // The child reconcile has not run yet — stay dirty so the scheduled
        // build in `BuildOwner::build_scope` is not skipped by its dirty guard.
        self.needs_build = true;

        // Create RenderView and insert into RenderTree
        let (width, height) = self.view.size;
        let mut render_view = RenderViewObject::new();
        let physical_size = Size::new(px(width), px(height));
        let config = ViewConfiguration::from_size(physical_size, 1.0);
        render_view.set_configuration(config);
        // Bootstrap the root transform + root layer. Without this,
        // RenderView::perform_layout asserts on the missing transform
        // the first time the pipeline lays out the root — the adapter
        // path never attaches an owner pointer, so the without-owner
        // variant (idempotent) is the right bootstrap here.
        render_view.prepare_initial_frame_without_owner();

        // Insert into PipelineOwner's RenderTree via RenderViewAdapter
        if let Some(pipeline_owner) = &self.pipeline_owner {
            let mut owner = pipeline_owner.write();
            let adapter = RenderViewAdapter::new(render_view);
            let node = RenderNode::new_box(Box::new(adapter));
            let render_id = owner.insert_render_node(node);
            owner.set_root_id(Some(render_id));
            self.render_id = Some(render_id);

            // Add to dirty lists
            owner.add_node_needing_layout(render_id, 0);
            owner.add_node_needing_paint(render_id, 0);
            owner.request_visual_update();

            tracing::debug!(
                "RootRenderElement::mount created RenderView render_id={:?} size={}x{}",
                render_id,
                width,
                height
            );
        }

        // E3 (atomic box→arena swap): the child is NOT built here. It is a
        // slab-resident node now — `build_into_views` returns the child
        // view and the slab id-reconciler in `BuildOwner::build_scope`
        // creates / updates it (and schedules it for its own build). The
        // RenderObject bootstrap above is the only mount-time work.
    }

    fn unmount(&mut self, _element_owner: &mut crate::ElementOwner<'_>) {
        // Detach from PipelineOwner's RenderTree
        if let (Some(pipeline_owner), Some(render_id)) = (&self.pipeline_owner, self.render_id) {
            // Cycle 3 T-1: cascade-by-default `remove` brings the whole
            // RenderTree subtree down with the root element. The
            // pre-cycle non-cascade `remove` (now `remove_shallow`)
            // would have orphaned descendants in the slab.

            let mut owner = pipeline_owner.write();
            owner.set_root_id(None);
            // Dispose protocol: the owner evicts the subtree's dirty
            // entries before freeing the slots (a Drop impl cannot —
            // it has no &PipelineOwner).
            owner.remove_render_object(render_id);
        }
        self.render_id = None;

        // E3: the child is a slab-resident node; the
        // [`ElementTree`](crate::tree::ElementTree) unmounts it
        // deepest-first via `child_ids`, so the root does not recurse into
        // a box child here.
        self.lifecycle = Lifecycle::Defunct;
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
    }

    fn update(&mut self, new_view: &dyn View, _element_owner: &mut crate::ElementOwner<'_>) {
        if let Some(v) = new_view.as_any().downcast_ref::<RootRenderView<V>>() {
            self.view = v.clone();
            // The child config may have changed — re-reconcile next build.
            self.needs_build = true;
            // Update configuration if size changed
            if let (Some(pipeline_owner), Some(render_id)) = (&self.pipeline_owner, self.render_id)
            {
                // U2 exemplar refactor: mutable access to the render object goes
                // through `&mut RenderTree` (`render_tree_mut().get_mut`) rather
                // than acquiring a per-node `RwLock` write guard. The pipeline
                // owner is still locked via its outer `Arc<RwLock<PipelineOwner>>`
                // (shared-infrastructure lock, allowed per `docs/PORT.md`).
                let mut owner = pipeline_owner.write();
                if let Some(node) = owner.render_tree_mut().get_mut(render_id) {
                    // RenderView uses BoxProtocol
                    let render_object = node.box_render_object_mut();
                    if let Some(render_view) = render_object
                        .as_any_mut()
                        .downcast_mut::<RenderViewObject>()
                    {
                        let (width, height) = self.view.size;
                        let physical_size = Size::new(px(width), px(height));
                        let config = ViewConfiguration::from_size(physical_size, 1.0);
                        render_view.set_configuration(config);
                    }
                }
            }
        }
    }

    fn is_dirty(&self) -> bool {
        self.needs_build
    }

    fn mark_needs_build(&mut self) {
        self.needs_build = true;
    }

    fn build_into_views(
        &mut self,
        _element_owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        // E3 (atomic box→arena swap): the root's single child is now a
        // slab-resident node. Return the child view; the slab
        // id-reconciler in `BuildOwner::build_scope` creates / updates the
        // child element under this root (propagating this element's
        // `PipelineOwner` + `render_id` via `ElementTree::insert`, so the
        // child's `on_mount` attaches its `RenderObject` under the
        // RenderView) and schedules it for its own build. The old inline
        // mount + recursive build + manual render-attach is gone.
        //
        // `build_scope` only calls this when `is_dirty()` is true, so the
        // returned child is always the one to reconcile; clear the flag now
        // that it has been handed off.
        self.needs_build = false;
        vec![dyn_clone::clone_box(&self.view.child as &dyn View)]
    }

    fn pipeline_owner_any(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        self.pipeline_owner
            .as_ref()
            .map(|po| Arc::clone(po) as Arc<dyn Any + Send + Sync>)
    }

    fn child_render_id(&self) -> Option<RenderId> {
        // The root's child attaches its RenderObject under the RenderView.
        self.render_id
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
            if let (Some(pipeline_owner), Some(parent_id)) = (&self.pipeline_owner, self.render_id)
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
            if let (Some(pipeline_owner), Some(parent_id)) = (&self.pipeline_owner, self.render_id)
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

    // Attach / detach to the PipelineOwner is handled inline in `mount()`
    // and `unmount()` above (see lines ~180-220): mount inserts the
    // `RenderView` via `RenderViewAdapter` into
    // `pipeline_owner.render_tree_mut()`, calls `set_root_id`, and
    // requests a visual update; unmount inverts that sequence. The
    // pre-Mythos `attach_to_pipeline_owner` / `detach_from_pipeline_owner`
    // trait stubs were removed in framework-spine-repair U15 because
    // their bodies were panicking placeholders (Constitution Principle 6
    // forbids panic in production paths) and they had zero callers in
    // the workspace.
}

#[cfg(test)]
mod tests {
    use flui_rendering::pipeline::PipelineOwner;

    use super::*;

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

        fn mount(
            &mut self,
            _parent: Option<ElementId>,
            _slot: usize,
            _owner: &mut crate::ElementOwner<'_>,
        ) {
        }
        fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {}
        fn activate(&mut self) {}
        fn deactivate(&mut self) {}
        fn update(&mut self, _new_view: &dyn View, _owner: &mut crate::ElementOwner<'_>) {}
        fn mark_needs_build(&mut self) {}
        fn build_into_views(&mut self, _owner: &mut crate::ElementOwner<'_>) -> Vec<Box<dyn View>> {
            Vec::new()
        }
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
        let mut build_owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut build_owner.element_owner_mut());

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        // render_id is set after mount with pipeline owner
        assert!(element.render_id.is_some());
    }
}
