//! RenderBox trait for 2D cartesian layout

use crate::constraints::BoxConstraints;
use crate::geometry::Size;
use crate::traits::RenderObject;
use flui_types::Offset;

/// Trait for render objects using the Box protocol
///
/// RenderBox objects use 2D Cartesian coordinates and have a fixed size
/// (width and height). They receive BoxConstraints as input and produce
/// a Size as output during layout.
///
/// # Layout Process
///
/// 1. Parent calls `perform_layout(constraints)` on child
/// 2. Child computes its size within the constraints
/// 3. Child returns the computed Size
/// 4. Parent can query the size later via `size()`
///
/// # Coordinate System
///
/// - Origin (0, 0) is at top-left corner
/// - X-axis extends right
/// - Y-axis extends down
/// - Position is specified via `Offset` (dx, dy)
///
/// # Example
///
/// ```ignore
/// impl RenderBox for RenderMyWidget {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Compute size based on constraints
///         let width = constraints.max_width.min(200.0);
///         let height = constraints.max_height.min(100.0);
///
///         let size = Size::new(width, height);
///         self._size = size;
///         size
///     }
///
///     fn size(&self) -> Size {
///         self._size
///     }
///
///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
///         // Paint at the given offset
///         let rect = Rect::from_origin_size(offset, self.size());
///         context.canvas().draw_rect(rect, &paint);
///     }
/// }
/// ```
pub trait RenderBox: RenderObject {
    // ===== Layout =====

    /// Computes the size of this render object given the constraints
    ///
    /// This is the core layout method. The implementation must:
    /// - Respect the constraints (return a size that satisfies them)
    /// - Store the computed size for later access via `size()`
    /// - Layout any children if needed
    ///
    /// # Arguments
    ///
    /// - `constraints`: Size bounds from the parent
    ///
    /// # Returns
    ///
    /// The computed size, which must satisfy the constraints.
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Returns the current size of this render object
    ///
    /// This must return the size computed during the most recent layout.
    /// Only valid after `perform_layout` has been called.
    fn size(&self) -> Size;

    // ===== Paint =====

    /// Paints this render object at the given offset
    ///
    /// # Arguments
    ///
    /// - `context`: Painting context providing canvas and layer management
    /// - `offset`: Position at which to paint (parent's origin + child's offset)
    ///
    /// # Notes
    ///
    /// - This method receives `&self` (immutable) to enforce that painting
    ///   doesn't modify layout state
    /// - Children should be painted via `context.paint_child(child, offset)`
    /// - Effects (clip, opacity, etc.) should use `context.push_*` methods
    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset);

    // ===== Hit Testing =====

    /// Tests whether a pointer event at the given position hits this object
    ///
    /// # Arguments
    ///
    /// - `result`: Accumulates hit test results
    /// - `position`: Position in local coordinates
    ///
    /// # Returns
    ///
    /// `true` if the position is within this object's bounds or hit a child.
    ///
    /// # Default Implementation
    ///
    /// The default checks if position is within size, then delegates to
    /// `hit_test_children` and `hit_test_self`.
    fn hit_test(&self, result: &mut dyn BoxHitTestResult, position: Offset) -> bool {
        if position.dx >= 0.0
            && position.dx < self.size().width
            && position.dy >= 0.0
            && position.dy < self.size().height
        {
            self.hit_test_children(result, position) || self.hit_test_self(position)
        } else {
            false
        }
    }

    /// Tests whether this object itself (not children) is hit
    ///
    /// Override to implement custom hit testing behavior.
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Tests whether any children are hit
    ///
    /// Override to forward hit testing to children.
    fn hit_test_children(&self, _result: &mut dyn BoxHitTestResult, _position: Offset) -> bool {
        false
    }

    // ===== Intrinsic Dimensions =====

    /// Computes the minimum width that this object could have for a given height
    ///
    /// Used for intrinsic sizing calculations.
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum width that this object could have for a given height
    ///
    /// Used for intrinsic sizing calculations.
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum height that this object could have for a given width
    ///
    /// Used for intrinsic sizing calculations.
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum height that this object could have for a given width
    ///
    /// Used for intrinsic sizing calculations.
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    // ===== Baseline =====

    /// Computes the distance from the top of this object to the given baseline
    ///
    /// Returns None if this object has no baseline.
    fn compute_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    // ===== Dry Layout =====

    /// Computes layout without side effects (for optimization)
    ///
    /// Used when only the size is needed without actually performing layout.
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        // Default: return smallest size
        constraints.smallest()
    }
}

/// Baseline type for text layout
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    /// Alphabetic baseline (most common for Latin text)
    Alphabetic,
    /// Ideographic baseline (for CJK text)
    Ideographic,
}

/// Trait for painting context (simplified for now)
pub trait PaintingContext {
    // Painting context methods will be implemented when we create the pipeline module
    // For now, this is a placeholder to allow RenderBox trait to compile
}

/// Trait for box hit test results (simplified for now)
pub trait BoxHitTestResult {
    // Hit test result methods will be implemented later
    // For now, this is a placeholder
}
