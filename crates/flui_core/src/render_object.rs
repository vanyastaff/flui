//! RenderObject trait - the rendering layer
//!
//! RenderObjects perform layout and painting. This is the third tree in
//! Flutter's three-tree architecture: Widget → Element → RenderObject

use std::fmt;

use downcast_rs::{impl_downcast, DowncastSync};
use flui_types::{events::HitTestResult, Offset, Size};

use crate::BoxConstraints;

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
}

// Enable downcasting for RenderObject trait objects
impl_downcast!(sync RenderObject);
