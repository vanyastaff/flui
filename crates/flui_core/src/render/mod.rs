//! RenderObject trait - the rendering layer
//!
//! RenderObjects perform layout and painting. This is the third tree in
//! Flutter's three-tree architecture: Widget → Element → RenderObject

use std::fmt;
use std::sync::Arc;

use downcast_rs::{impl_downcast, DowncastSync};
use flui_types::{events::HitTestResult, Offset, Size};
use glam::Mat4;
use parking_lot::RwLock;

use crate::{BoxConstraints, ParentData, PipelineOwner};

pub mod parent_data;
pub mod widget;

/// RenderObject - handles layout and painting
///
/// Similar to Flutter's RenderObject. These are created by RenderObjectWidgets
/// and handle the actual layout computation and painting.
///
/// The trait provides downcasting capabilities via the `downcast-rs` crate.
///
/// # Layout Protocol
///
/// 1. Parent sets constraints on child
/// 2. Child chooses size within constraints
/// 3. Parent positions child (sets offset)
/// 4. Parent returns its own size
///
/// # Painting Protocol
///
/// 1. Paint yourself
/// 2. Paint children in order
/// 3. Children are painted at their offsets
///
/// # Example
///
/// ```rust,ignore
/// struct MyRenderObject {
///     size: Size,
///     needs_layout: bool,
/// }
///
/// impl RenderObject for MyRenderObject {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         self.size = constraints.biggest();
///         self.needs_layout = false;
///         self.size
///     }
///
///     fn paint(&self, painter: &egui::Painter, offset: Offset) {
///         // Paint at offset position
///         let rect = egui::Rect::from_min_size(
///             offset.to_pos2(),
///             egui::vec2(self.size.width, self.size.height),
///         );
///         painter.rect_filled(rect, 0.0, egui::Color32::BLUE);
///     }
///
///     fn size(&self) -> Size {
///         self.size
///     }
///
///     fn mark_needs_layout(&mut self) {
///         self.needs_layout = true;
///     }
///
///     // ... other methods
/// }
/// ```
pub trait RenderObject: DowncastSync + fmt::Debug {
    /// Perform layout with given constraints
    ///
    /// Returns the size this render object chose within the constraints.
    /// Must satisfy: `constraints.is_satisfied_by(returned_size)`
    ///
    /// # Layout Rules
    ///
    /// - Child must respect parent's constraints
    /// - Child cannot read its own size during layout (causes cycles)
    /// - Parent sets child's offset AFTER child's layout returns
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// The painter is positioned at the render object's offset.
    /// Offset is relative to parent's coordinate space.
    ///
    /// # Painting Rules
    ///
    /// - Paint yourself first (background)
    /// - Then paint children in order
    /// - Children are clipped to parent bounds (optional)
    fn paint(&self, painter: &egui::Painter, offset: Offset);

    /// Get the current size (after layout)
    ///
    /// Returns the size chosen during the most recent layout pass.
    fn size(&self) -> Size;

    /// Get the constraints used in last layout
    ///
    /// Useful for debugging and introspection.
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    /// Check if this render object needs layout
    ///
    /// Returns true if layout() needs to be called.
    fn needs_layout(&self) -> bool {
        false
    }

    /// Mark this render object as needing layout
    ///
    /// Called when configuration changes or parent requests relayout.
    fn mark_needs_layout(&mut self);

    /// Check if this render object needs paint
    ///
    /// Returns true if paint() needs to be called.
    fn needs_paint(&self) -> bool {
        false
    }

    /// Mark this render object as needing paint
    ///
    /// Called when appearance changes or parent requests repaint.
    fn mark_needs_paint(&mut self);

    // Intrinsic sizing methods
    //
    // These help determine natural sizes before layout.
    // Used by widgets like IntrinsicWidth/IntrinsicHeight.

    /// Get minimum intrinsic width for given height
    ///
    /// Returns the smallest width this render object can have while
    /// maintaining its proportions if given this height.
    fn get_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Get maximum intrinsic width for given height
    ///
    /// Returns the largest width this render object would need
    /// if given this height.
    fn get_max_intrinsic_width(&self, _height: f32) -> f32 {
        f32::INFINITY
    }

    /// Get minimum intrinsic height for given width
    ///
    /// Returns the smallest height this render object can have while
    /// maintaining its proportions if given this width.
    fn get_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Get maximum intrinsic height for given width
    ///
    /// Returns the largest height this render object would need
    /// if given this width.
    fn get_max_intrinsic_height(&self, _width: f32) -> f32 {
        f32::INFINITY
    }

    /// Hit test this render object and its children
    ///
    /// Position is in the coordinate space of this render object.
    /// Returns true if this or any child was hit.
    ///
    /// The default implementation:
    /// 1. Checks if position is within bounds
    /// 2. Calls hit_test_children()
    /// 3. Calls hit_test_self() if children didn't consume the event
    /// 4. Adds entry to result if hit
    ///
    /// Override for custom hit testing behavior.
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check bounds first
        if position.dx < 0.0
            || position.dx >= self.size().width
            || position.dy < 0.0
            || position.dy >= self.size().height
        {
            return false;
        }

        // Check children first (front to back)
        let hit_child = self.hit_test_children(result, position);

        // Then check self (if children didn't consume the event)
        let hit_self = self.hit_test_self(position);

        // Add to result if hit
        if hit_child || hit_self {
            result.add(flui_types::events::HitTestEntry::new(
                position,
                self.size(),
            ));
            return true;
        }

        false
    }

    /// Hit test only this render object (not children)
    ///
    /// Default: return true if within bounds (handled by hit_test())
    /// Override to customize self hit testing.
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    /// Hit test children
    ///
    /// Default: no children
    /// Override to test children in paint order (front to back)
    fn hit_test_children(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        false
    }

    /// Visit all children (read-only)
    ///
    /// Default: no children (leaf render object)
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {
        // Default: no children
    }

    /// Visit all children (mutable)
    ///
    /// Default: no children (leaf render object)
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        // Default: no children
    }

    // ============================================================================
    // Flutter-like extended API
    // ============================================================================

    // --- ParentData ---

    /// Get parent data for this render object
    ///
    /// Parent data is set by the parent and can store child-specific layout information
    /// like position, flex factor, etc.
    fn parent_data(&self) -> Option<&dyn ParentData> {
        None
    }

    /// Get mutable parent data for this render object
    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
        None
    }

    /// Set parent data for this render object
    ///
    /// Called by parent when adopting a child.
    fn set_parent_data(&mut self, _parent_data: Box<dyn ParentData>) {
        // Default: no parent data storage
    }

    /// Setup parent data for a child
    ///
    /// Override this to initialize parent data with the correct type for your children.
    /// Called by adoptChild before the child is added to the child list.
    fn setup_parent_data(&self, _child: &mut dyn RenderObject) {
        // Default: no setup needed
    }

    // --- Tree Structure ---

    /// Get the parent render object
    ///
    /// Returns None if this is the root of the render tree.
    fn parent(&self) -> Option<&dyn RenderObject> {
        None
    }

    /// Get the depth of this render object in the tree
    ///
    /// The root has depth 0, its children have depth 1, etc.
    fn depth(&self) -> i32 {
        0
    }

    // --- Lifecycle ---

    /// Attach this render object to a PipelineOwner
    ///
    /// Called when the render object is inserted into the render tree.
    /// The render object should mark itself dirty if needed.
    fn attach(&mut self, _owner: Arc<RwLock<PipelineOwner>>) {
        // Default: no attachment needed
        // Most implementations will want to mark_needs_layout() here
    }

    /// Detach this render object from its PipelineOwner
    ///
    /// Called when the render object is removed from the render tree.
    fn detach(&mut self) {
        // Default: no detachment needed
    }

    /// Dispose of this render object
    ///
    /// Called when the render object is no longer needed.
    /// Should release any expensive resources like images or layers.
    fn dispose(&mut self) {
        // Default: no disposal needed
    }

    /// Adopt a child render object
    ///
    /// Called when adding a child to this render object.
    /// Sets up parent data and updates the child's parent pointer.
    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {
        // Default implementation in RenderBox
    }

    /// Drop a child render object
    ///
    /// Called when removing a child from this render object.
    /// Clears the child's parent pointer.
    fn drop_child(&mut self, _child: &mut dyn RenderObject) {
        // Default implementation in RenderBox
    }

    // --- Layout Optimization ---

    /// Whether the size of this render object depends only on the constraints
    ///
    /// If true, performResize() will be called during layout instead of performLayout().
    /// This is an optimization for render objects whose size doesn't depend on their children.
    ///
    /// Example: RenderConstrainedBox with tight constraints
    fn sized_by_parent(&self) -> bool {
        false
    }

    /// Perform resize when sized_by_parent is true
    ///
    /// This is called instead of performLayout() when sized_by_parent returns true.
    /// Should only update the size, not touch children.
    fn perform_resize(&mut self, _constraints: BoxConstraints) {
        // Default: do nothing
        // Override if sized_by_parent returns true
    }

    /// Perform layout (internal implementation)
    ///
    /// This is the actual layout implementation, separated from the public layout() method.
    /// When sized_by_parent is true, perform_resize() is called first, then this method.
    fn perform_layout(&mut self, _constraints: BoxConstraints) {
        // Default: delegates to layout()
        // Most implementations will override this instead of layout()
    }

    // --- Compositing and Layers ---

    /// Whether this render object paints in its own layer
    ///
    /// If true, this render object acts as a repaint boundary - changes to this subtree
    /// don't cause repaints of ancestors.
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Whether this render object or any descendant needs compositing
    ///
    /// Compositing means using layers for rendering. This is automatically maintained
    /// by the framework.
    fn needs_compositing(&self) -> bool {
        false
    }

    /// Mark needs compositing bits update
    ///
    /// Called when isRepaintBoundary changes or when children change.
    fn mark_needs_compositing_bits_update(&mut self) {
        // Default: no compositing
    }

    // --- Transforms ---

    /// Apply the transform from this render object to the given child
    ///
    /// Used for hit testing and coordinate conversion.
    /// The transform should map from the parent's coordinate system to the child's.
    ///
    /// Default: applies the child's offset from BoxParentData if available.
    fn apply_paint_transform(&self, _child: &dyn RenderObject, transform: &mut Mat4) {
        // Default: identity transform (no change)
        // Most render boxes will apply the child's offset here
        let _ = transform; // Suppress unused warning
    }

    /// Get the transform from this render object to the target
    ///
    /// Returns the transformation matrix that maps points in this object's
    /// coordinate system to the target's coordinate system.
    fn get_transform_to(&self, _target: Option<&dyn RenderObject>) -> Mat4 {
        // Default: identity
        Mat4::IDENTITY
    }

    // --- Relayout Boundaries ---

    /// Whether this render object is a relayout boundary
    ///
    /// A relayout boundary means that when this object is marked dirty,
    /// the layout doesn't propagate past this node to ancestors.
    fn is_relayout_boundary(&self) -> bool {
        false
    }

    /// Mark this render object and ancestors as needing layout
    ///
    /// Propagates up the tree until hitting a relayout boundary.
    fn mark_parent_needs_layout(&mut self) {
        // Default: mark self
        self.mark_needs_layout();
    }
}

// Enable downcasting for RenderObject trait objects
impl_downcast!(sync RenderObject);


