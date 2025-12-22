//! Core rendering abstraction for the FLUI framework.
//!
//! This module defines [`RenderObject`], the fundamental building block of FLUI's
//! render tree. Every visual element in a FLUI application is backed by a render
//! object that knows how to lay out and paint itself.
//!
//! # Architecture
//!
//! FLUI uses a three-tree architecture inspired by Flutter:
//!
//! ```text
//! View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
//! ```
//!
//! The render tree is responsible for:
//! - **Layout**: Computing sizes and positions of UI elements
//! - **Painting**: Drawing UI elements to the screen
//! - **Hit Testing**: Determining which elements are under a pointer
//! - **Semantics**: Providing accessibility information

use downcast_rs::{impl_downcast, DowncastSync};
use flui_foundation::{Diagnosticable, LayerId, SemanticsId};

use crate::constraints::BoxConstraints;
use crate::hit_testing::HitTestTarget;
use crate::parent_data::ParentData;
use crate::pipeline::PipelineOwner;
use crate::semantics::{SemanticsConfiguration, SemanticsEvent, SemanticsNode};

/// The base trait for all render objects in the render tree.
///
/// This is a minimal trait definition. The architecture is being reorganized
/// and methods will be added as needed.
pub trait RenderObject: Diagnosticable + HitTestTarget + DowncastSync {
    // ========================================================================
    // Tree Structure
    // ========================================================================

    /// Returns the parent render object, if any.
    fn parent(&self) -> Option<&dyn RenderObject> {
        None
    }

    /// Returns the depth of this node in the render tree.
    fn depth(&self) -> usize;

    /// Sets the depth of this node in the render tree.
    fn set_depth(&mut self, depth: usize);

    /// Returns the pipeline owner that manages this render object.
    fn owner(&self) -> Option<&PipelineOwner>;

    /// Sets the parent reference for this render object.
    fn set_parent(&mut self, parent: Option<*const dyn RenderObject>);

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Attaches this render object to a pipeline owner.
    fn attach(&mut self, owner: &PipelineOwner);

    /// Detaches this render object from its pipeline owner.
    fn detach(&mut self);

    /// Releases any resources held by this render object.
    fn dispose(&mut self) {}

    /// Returns whether this render object is attached to a pipeline owner.
    fn attached(&self) -> bool {
        self.owner().is_some()
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Marks a render object as a child of this object.
    fn adopt_child(&mut self, child: &mut dyn RenderObject);

    /// Removes a render object from this object's children.
    fn drop_child(&mut self, child: &mut dyn RenderObject);

    /// Adjusts a child's depth to be greater than this node's depth.
    fn redepth_child(&mut self, child: &mut dyn RenderObject);

    /// Adjusts the depth of this node's children.
    fn redepth_children(&mut self) {}

    // ========================================================================
    // Dirty State Queries
    // ========================================================================

    /// Returns whether this render object needs layout.
    fn needs_layout(&self) -> bool;

    /// Returns whether this render object needs paint.
    fn needs_paint(&self) -> bool;

    /// Returns whether this render object needs compositing bits update.
    fn needs_compositing_bits_update(&self) -> bool;

    /// Returns whether this render object is a relayout boundary.
    fn is_relayout_boundary(&self) -> bool;

    // ========================================================================
    // Dirty Marking
    // ========================================================================

    /// Marks this render object as needing layout.
    fn mark_needs_layout(&mut self);

    /// Marks this render object as needing paint.
    fn mark_needs_paint(&mut self);

    /// Marks this render object as needing compositing bits update.
    fn mark_needs_compositing_bits_update(&mut self);

    /// Marks this render object as needing semantics update.
    fn mark_needs_semantics_update(&mut self);

    /// Clears the needs_layout flag.
    fn clear_needs_layout(&mut self);

    /// Clears the needs_paint flag.
    fn clear_needs_paint(&mut self);

    /// Clears the needs_compositing_bits_update flag.
    fn clear_needs_compositing_bits_update(&mut self);

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout for this render object.
    fn layout(&mut self, constraints: BoxConstraints, parent_uses_size: bool);

    /// Performs layout without receiving new constraints.
    fn layout_without_resize(&mut self);

    /// Performs the actual layout computation.
    fn perform_layout_impl(&mut self) {}

    /// Computes the size when sized_by_parent is true.
    fn perform_resize(&mut self) {}

    /// Returns cached constraints.
    fn cached_constraints(&self) -> Option<BoxConstraints>;

    /// Sets cached constraints.
    fn set_cached_constraints(&mut self, constraints: BoxConstraints);

    // ========================================================================
    // Layout Dirty Propagation
    // ========================================================================

    /// Marks the parent as needing layout.
    fn mark_parent_needs_layout(&mut self);

    /// Marks this object dirty and notifies the parent about sized_by_parent changes.
    fn mark_needs_layout_for_sized_by_parent_change(&mut self) {
        self.mark_needs_layout();
        self.mark_parent_needs_layout();
    }

    /// Schedules the initial layout for the render tree.
    fn schedule_initial_layout(&mut self);

    /// Schedules the initial paint for the render tree.
    fn schedule_initial_paint(&mut self);

    // ========================================================================
    // Layout Configuration
    // ========================================================================

    /// Returns whether this object's size depends only on constraints.
    fn sized_by_parent(&self) -> bool {
        false
    }

    /// Returns whether this render object is a repaint boundary.
    fn is_repaint_boundary(&self) -> bool;

    /// Returns whether this object was a repaint boundary during the last paint.
    fn was_repaint_boundary(&self) -> bool;

    /// Sets whether this object was a repaint boundary.
    fn set_was_repaint_boundary(&mut self, value: bool);

    /// Returns whether this object always needs compositing.
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ========================================================================
    // Compositing
    // ========================================================================

    /// Returns whether this render object needs compositing.
    fn needs_compositing(&self) -> bool;

    /// Sets whether this render object needs compositing.
    fn set_needs_compositing(&mut self, value: bool);

    /// Updates the compositing bits for this render object and its descendants.
    fn update_compositing_bits(&mut self) {
        if !self.needs_compositing_bits_update() {
            return;
        }

        let mut child_needs_compositing = false;
        self.visit_children_mut(&mut |child| {
            child.update_compositing_bits();
            if child.needs_compositing() {
                child_needs_compositing = true;
            }
        });

        let needs_compositing = child_needs_compositing
            || self.is_repaint_boundary()
            || self.always_needs_compositing();
        self.set_needs_compositing(needs_compositing);
        self.clear_needs_compositing_bits_update();

        if self.needs_compositing() != child_needs_compositing {
            self.mark_needs_paint();
        }
    }

    /// Marks this object as needing a composited layer update.
    fn mark_needs_composited_layer_update(&mut self) {
        self.mark_needs_paint();
    }

    /// Returns whether this render object has a compositing layer.
    fn has_layer(&self) -> bool {
        false
    }

    /// Returns the layer ID for this render object, if any.
    fn layer_id(&self) -> Option<LayerId> {
        None
    }

    /// Replaces the root layer for this render object.
    fn replace_root_layer(&mut self) {}

    /// Updates the composited layer for this render object.
    fn update_composited_layer(&mut self) {}

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Sets up parent data for a child.
    fn setup_parent_data(&self, _child: &mut dyn RenderObject) {}

    /// Returns this object's parent data, if any.
    fn parent_data(&self) -> Option<&dyn ParentData>;

    /// Returns mutable access to this object's parent data.
    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData>;

    /// Sets this object's parent data.
    fn set_parent_data(&mut self, data: Box<dyn ParentData>);

    // ========================================================================
    // Children
    // ========================================================================

    /// Visits each child render object.
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject));

    /// Visits each child render object with mutable access.
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject));

    // ========================================================================
    // Painting
    // ========================================================================

    /// Returns the bounds within which this object will paint.
    fn paint_bounds(&self) -> flui_types::Rect;

    /// Paints this render object.
    fn paint(&self, context: &mut crate::context::CanvasContext, offset: flui_types::Offset) {
        let _ = (context, offset);
    }

    /// Applies the paint transform for a child.
    fn apply_paint_transform(&self, child: &dyn RenderObject, transform: &mut [f32; 16]) {
        let _ = (child, transform);
    }

    /// Returns the approximate clip rectangle for a child.
    fn describe_approximate_paint_clip(
        &self,
        _child: &dyn RenderObject,
    ) -> Option<flui_types::Rect> {
        None
    }

    /// Returns the semantics clip for a child.
    fn describe_semantics_clip(&self, _child: &dyn RenderObject) -> Option<flui_types::Rect> {
        None
    }

    /// Returns the semantic bounds of this render object.
    fn semantic_bounds(&self) -> flui_types::Rect {
        self.paint_bounds()
    }

    /// Returns whether a child would be painted.
    fn paints_child(&self, _child: &dyn RenderObject) -> bool {
        true
    }

    /// Computes the transform from this object to a target ancestor.
    fn get_transform_to(&self, _target: Option<&dyn RenderObject>) -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    // ========================================================================
    // Hit Testing & Events
    // ========================================================================

    /// Handles a pointer event delivered to this render object.
    fn handle_event(
        &self,
        _event: &crate::hit_testing::PointerEvent,
        _entry: &crate::hit_testing::HitTestEntry,
    ) {
    }

    /// Attempts to make this render object visible.
    fn show_on_screen(&self) {}

    // ========================================================================
    // Semantics
    // ========================================================================

    /// Schedules the initial semantics update for this render tree.
    fn schedule_initial_semantics(&mut self) {
        self.mark_needs_semantics_update();
    }

    /// Describes the semantic configuration for this render object.
    fn describe_semantics_configuration(&self, _config: &mut SemanticsConfiguration) {}

    /// Visits children for semantics tree building.
    fn visit_children_for_semantics(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        self.visit_children(visitor);
    }

    /// Clears any cached semantics information.
    fn clear_semantics(&mut self) {}

    /// Sends a semantics event from this render object.
    fn send_semantics_event(&self, _event: SemanticsEvent) {}

    /// Assembles the semantics node for this render object.
    fn assemble_semantics_node(
        &self,
        node: &mut SemanticsNode,
        config: &SemanticsConfiguration,
        children: Vec<SemanticsId>,
    ) {
        node.set_config(config.clone());
        for child_id in children {
            node.add_child(child_id);
        }
    }

    // ========================================================================
    // Hot Reload Support
    // ========================================================================

    /// Forces the entire subtree to be marked dirty.
    fn reassemble(&mut self) {
        self.mark_needs_layout();
        self.mark_needs_compositing_bits_update();
        self.mark_needs_paint();
        self.mark_needs_semantics_update();
        self.visit_children_mut(&mut |child| {
            child.reassemble();
        });
    }

    // ========================================================================
    // Layout Callbacks
    // ========================================================================

    /// Allows mutations to descendants during layout.
    fn invoke_layout_callback(&mut self, _callback: Box<dyn FnOnce() + Send>) {}
}

impl_downcast!(sync RenderObject);
