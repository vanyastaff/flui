//! RenderBox trait for 2D box layout with Arity-based child management.

use flui_types::{Point, Rect, Size};

use crate::arity::Arity;
use crate::constraints::BoxConstraints;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use crate::hit_testing::HitTestBehavior;
use crate::parent_data::ParentData;

// ============================================================================
// RenderBox Trait with Arity and ParentData
// ============================================================================

/// Trait for render objects that use 2D cartesian coordinates.
///
/// ## Associated Types
///
/// - `Arity` - Defines child count at compile time (Leaf, Optional, Variable)
/// - `ParentData` - Metadata type that parent stores on children
///
/// ## Example
///
/// ```ignore
/// // Simple leaf with default BoxParentData
/// struct RenderColoredBox { color: Color, size: Size }
///
/// impl RenderBox for RenderColoredBox {
///     type Arity = Leaf;
///     type ParentData = BoxParentData;
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Leaf, BoxParentData>) -> Size {
///         ctx.constraints().constrain(self.size)
///     }
/// }
///
/// // Flex container with FlexParentData on children
/// struct RenderFlex { children: Vec<...> }
///
/// impl RenderBox for RenderFlex {
///     type Arity = Variable;
///     type ParentData = FlexParentData;  // Children get FlexParentData
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) -> Size {
///         for child in ctx.iter_children() {
///             // Type-safe access to FlexParentData
///             let flex = child.parent_data().flex;
///             let fit = child.parent_data().fit;
///         }
///     }
/// }
///
/// // Stack container with StackParentData on children
/// struct RenderStack { ... }
///
/// impl RenderBox for RenderStack {
///     type Arity = Variable;
///     type ParentData = StackParentData;  // Children get positioning info
///     ...
/// }
/// ```
/// Trait for render objects that use 2D cartesian coordinates.
///
/// Users implement this trait for their custom render objects.
/// Use `BoxWrapper<T>` to bridge to `RenderObject` for storage in `RenderTree`.
///
/// # Features
///
/// - Intrinsic dimension queries (min/max width/height)
/// - Baseline support for text alignment
/// - Dry layout (compute size without actual layout)
/// - Coordinate conversion (local ↔ global)
pub trait RenderBox: Send + Sync + std::fmt::Debug + 'static {
    /// The arity of this render box (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    /// The parent data type for children of this render box.
    ///
    /// This determines what metadata the parent can store on each child:
    /// - `BoxParentData` - Basic offset only (default for simple containers)
    /// - `FlexParentData` - Flex factor, fit mode (for Row/Column)
    /// - `StackParentData` - Positioning constraints (for Stack)
    /// - `TableCellParentData` - Row/column span (for Table)
    type ParentData: ParentData + Default;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this render object.
    ///
    /// The context provides:
    /// - Constraints from parent via `ctx.constraints()`
    /// - Type-safe child access via `ctx.layout_child()`, `ctx.position_child()`
    /// - Completion via `ctx.complete_with_size()`
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) {
    ///     let child_size = ctx.layout_single_child_loose();
    ///     ctx.position_single_child_at_origin();
    ///     ctx.complete_with_size(ctx.constrain(child_size));
    /// }
    /// ```
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Self::Arity, Self::ParentData>);

    /// Returns the current size of this render object.
    fn size(&self) -> Size;

    /// Sets the size of this render object.
    fn set_size(&mut self, size: Size);

    /// Returns whether this render object has undergone layout and has a size.
    fn has_size(&self) -> bool {
        true
    }

    // ========================================================================
    // Paint
    // ========================================================================

    /// Paints this render object.
    ///
    /// The context provides:
    /// - Canvas access via `ctx.canvas()`
    /// - Current offset via `ctx.offset()`
    /// - Children access via `ctx.children_mut()`
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
    ///     // Draw background
    ///     let rect = Rect::from_size(self.size).translate(ctx.offset());
    ///     ctx.canvas().draw_rect(rect, &Paint::fill(Color::WHITE));
    ///
    ///     // Paint children
    ///     ctx.children_mut().for_each(|child| {
    ///         child.paint(ctx.canvas_context_mut());
    ///     });
    /// }
    /// ```
    fn paint(&mut self, ctx: &mut BoxPaintContext<'_, Self::Arity, Self::ParentData>);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Returns the hit test behavior for this render object.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }

    /// Hit tests this render object.
    ///
    /// The context provides:
    /// - Position via `ctx.position()` or `ctx.x()`, `ctx.y()`
    /// - Bounds checking via `ctx.is_within_size(w, h)`
    /// - Child testing via `ctx.hit_test_child_at_offset()`
    /// - Result management via `ctx.add_self(id)`
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn hit_test(&self, ctx: &mut BoxHitTestContext<Single, BoxParentData>) -> bool {
    ///     if !ctx.is_within_size(self.size.width, self.size.height) {
    ///         return false;
    ///     }
    ///     // Test children first
    ///     if ctx.hit_test_child_at_offset(0, child_offset) {
    ///         return true;
    ///     }
    ///     ctx.add_self(self.id);
    ///     true
    /// }
    /// ```
    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Self::Arity, Self::ParentData>) -> bool;

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Creates default parent data for a child.
    ///
    /// Called when a child is adopted. Override if you need custom initialization.
    fn create_default_parent_data() -> Self::ParentData {
        Self::ParentData::default()
    }

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Converts a point from global coordinates to local coordinates.
    fn global_to_local(&self, point: Point) -> Point {
        point
    }

    /// Converts a point from local coordinates to global coordinates.
    fn local_to_global(&self, point: Point) -> Point {
        point
    }

    // ========================================================================
    // Intrinsic Dimensions
    // ========================================================================

    /// Returns the minimum intrinsic width for a given height.
    fn get_min_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_min_intrinsic_width(height)
    }

    /// Returns the maximum intrinsic width for a given height.
    fn get_max_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_max_intrinsic_width(height)
    }

    /// Returns the minimum intrinsic height for a given width.
    fn get_min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_min_intrinsic_height(width)
    }

    /// Returns the maximum intrinsic height for a given width.
    fn get_max_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }

    /// Computes the minimum intrinsic width for a given height.
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width for a given height.
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height for a given width.
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height for a given width.
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    // ========================================================================
    // Dry Layout
    // ========================================================================

    /// Returns the size this box would like to be given the constraints.
    fn get_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(constraints)
    }

    /// Computes the size this box would have given the constraints,
    /// without actually laying out.
    fn compute_dry_layout(&self, _constraints: BoxConstraints) -> Size {
        Size::ZERO
    }

    // ========================================================================
    // Baseline
    // ========================================================================

    /// Returns the distance from the top of the box to the first baseline.
    fn get_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_actual_baseline(baseline)
    }

    /// Returns the distance from the top of the box to its first baseline
    /// for the given constraints (dry layout).
    fn get_dry_baseline(&self, constraints: BoxConstraints, baseline: TextBaseline) -> Option<f32> {
        self.compute_dry_baseline(constraints, baseline)
    }

    /// Computes the distance from the top of the box to its first baseline.
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Computes the dry baseline for the given constraints.
    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        _baseline: TextBaseline,
    ) -> Option<f32> {
        None
    }

    // ========================================================================
    // Paint Bounds
    // ========================================================================

    /// Returns the paint bounds of this render box.
    fn box_paint_bounds(&self) -> Rect {
        let size = self.size();
        Rect::new(0.0, 0.0, size.width, size.height)
    }
}

/// Text baseline types for baseline alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    /// The alphabetic baseline.
    Alphabetic,
    /// The ideographic baseline.
    Ideographic,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_test_behavior_default() {
        // HitTestBehavior is now imported from flui_interaction via hit_testing
        let behavior = HitTestBehavior::default();
        assert_eq!(behavior, HitTestBehavior::DeferToChild);
    }

    // BoxHitTestResult and BoxHitTestEntry tests are now in hit_testing/result.rs
}
