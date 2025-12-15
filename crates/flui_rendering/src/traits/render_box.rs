//! RenderBox trait for 2D box layout.

use flui_types::{BoxConstraints, Offset, Point, Rect, Size};

use super::RenderObject;
use crate::pipeline::PaintingContext;

// ============================================================================
// RenderBox Trait
// ============================================================================

/// Trait for render objects that use 2D cartesian coordinates.
///
/// RenderBox is the primary layout protocol for most UI widgets. It:
/// - Receives [`BoxConstraints`] from its parent (min/max width/height)
/// - Computes its own [`Size`] within those constraints
/// - Positions children using [`Offset`] values
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderBox` abstract class in
/// `rendering/box.dart`.
///
/// # Layout Protocol
///
/// 1. Parent calls `perform_layout()` with constraints
/// 2. Child computes its size within constraints
/// 3. Child returns its size
/// 4. Parent positions child by setting offset in parent data
///
/// # Example
///
/// ```ignore
/// impl RenderBox for MyRenderObject {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Layout children first
///         let child_size = if let Some(child) = self.child_mut() {
///             child.perform_layout(constraints)
///         } else {
///             Size::ZERO
///         };
///
///         // Compute own size based on child
///         constraints.constrain(child_size)
///     }
///
///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
///         // Paint self, then children
///         if let Some(child) = self.child() {
///             context.paint_child(child, offset);
///         }
///     }
///
///     // ... other required methods
/// }
/// ```
pub trait RenderBox: RenderObject {
    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this render object.
    ///
    /// Called by the parent with constraints that specify the allowed
    /// size range. Must return a size within those constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The computed size of this render object
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Returns the current size of this render object.
    ///
    /// Only valid after `perform_layout` has been called.
    fn size(&self) -> Size;

    /// Returns whether this render object has undergone layout and has a size.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.hasSize` getter.
    fn has_size(&self) -> bool {
        // Default implementation - subclasses should override
        // to check their internal size state
        true
    }

    /// Returns the constraints most recently supplied by the parent.
    ///
    /// Returns `None` if layout has not yet happened.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.constraints` getter.
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    /// Sets the size of this render object.
    ///
    /// This should only be called during layout.
    fn set_size(&mut self, size: Size);

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Converts a point from global coordinates to local coordinates.
    ///
    /// If `ancestor` is non-null, this method converts the given point from
    /// the coordinate space of the ancestor to the local coordinate space.
    /// If `ancestor` is null, it converts from the global coordinate space.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.globalToLocal` method.
    fn global_to_local(&self, point: Point, ancestor: Option<&dyn RenderObject>) -> Point {
        let _ = ancestor;
        // Default implementation - just return the point as-is
        // Subclasses should implement proper transform chain traversal
        point
    }

    /// Converts a point from local coordinates to global coordinates.
    ///
    /// If `ancestor` is non-null, this method converts the given point from
    /// the local coordinate space to the coordinate space of the ancestor.
    /// If `ancestor` is null, it converts to the global coordinate space.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.localToGlobal` method.
    fn local_to_global(&self, point: Point, ancestor: Option<&dyn RenderObject>) -> Point {
        let _ = ancestor;
        // Default implementation - just return the point as-is
        // Subclasses should implement proper transform chain traversal
        point
    }

    // ========================================================================
    // Paint
    // ========================================================================

    /// Paints this render object.
    ///
    /// Called after layout is complete. Should paint this object and
    /// then paint children at their appropriate offsets.
    ///
    /// # Arguments
    ///
    /// * `context` - The painting context with canvas access
    /// * `offset` - The offset from the origin to paint at
    fn paint(&self, context: &mut PaintingContext, offset: Offset);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this render object.
    ///
    /// Returns true if the given position hits this render object or
    /// any of its children.
    ///
    /// # Arguments
    ///
    /// * `result` - The hit test result to add entries to
    /// * `position` - The position to test, in local coordinates
    ///
    /// # Default Implementation
    ///
    /// Tests if position is within bounds, then delegates to children.
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        let size = self.size();
        if position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
        {
            self.hit_test_children(result, position) || self.hit_test_self(position)
        } else {
            false
        }
    }

    /// Hit tests just this render object (not children).
    ///
    /// Override to make this object respond to hits.
    /// Default returns `false`.
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Hit tests children of this render object.
    ///
    /// Override to test children. Should iterate children in reverse
    /// paint order (front to back).
    /// Default returns `false`.
    fn hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        false
    }

    // ========================================================================
    // Intrinsic Dimensions
    // ========================================================================

    /// Returns the minimum intrinsic width for a given height.
    ///
    /// This function should only be called on children. Calling this
    /// function couples the child with the parent so that when the child's
    /// layout changes, the parent is notified.
    ///
    /// Calling this function is expensive as it can result in O(N^2) behavior.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMinIntrinsicWidth` method.
    fn get_min_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_min_intrinsic_width(height)
    }

    /// Returns the maximum intrinsic width for a given height.
    ///
    /// This function should only be called on children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMaxIntrinsicWidth` method.
    fn get_max_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_max_intrinsic_width(height)
    }

    /// Returns the minimum intrinsic height for a given width.
    ///
    /// This function should only be called on children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMinIntrinsicHeight` method.
    fn get_min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_min_intrinsic_height(width)
    }

    /// Returns the maximum intrinsic height for a given width.
    ///
    /// This function should only be called on children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMaxIntrinsicHeight` method.
    fn get_max_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }

    /// Computes the minimum intrinsic width for a given height.
    ///
    /// The minimum width that this box could be without failing to
    /// correctly paint its contents within itself.
    ///
    /// Override this in subclasses that implement `perform_layout`.
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width for a given height.
    ///
    /// The smallest width beyond which increasing width has no effect
    /// on the height of the box.
    ///
    /// Override this in subclasses that implement `perform_layout`.
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height for a given width.
    ///
    /// The minimum height that this box could be without failing to
    /// correctly paint its contents within itself.
    ///
    /// Override this in subclasses that implement `perform_layout`.
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height for a given width.
    ///
    /// The smallest height beyond which increasing height has no effect
    /// on the width of the box.
    ///
    /// Override this in subclasses that implement `perform_layout`.
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    // ========================================================================
    // Dry Layout
    // ========================================================================

    /// Returns the size this box would like to be given the constraints.
    ///
    /// The size returned by this method is guaranteed to be the same size
    /// that this RenderBox computes for itself during layout given the
    /// same constraints.
    ///
    /// This function should only be called on children.
    ///
    /// This layout is called "dry" layout as opposed to the regular "wet"
    /// layout run performed by `perform_layout` because it computes the
    /// desired size for the given constraints without changing any internal state.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDryLayout` method.
    fn get_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(constraints)
    }

    /// Computes the size this box would have given the constraints,
    /// without actually laying out.
    ///
    /// Override this in subclasses that implement `perform_layout`.
    /// This should return the Size that this RenderBox would like to be
    /// given the provided BoxConstraints.
    ///
    /// The size returned by this method must match the size that the
    /// RenderBox will compute for itself in `perform_layout`.
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        let _ = constraints;
        Size::ZERO
    }

    // ========================================================================
    // Baseline
    // ========================================================================

    /// Returns the distance from the top of the box to the first baseline.
    ///
    /// Returns `None` if this box has no baseline.
    ///
    /// This function should only be called on children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDistanceToBaseline` method.
    fn get_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_actual_baseline(baseline)
    }

    /// Returns the distance from the top of the box to its first baseline
    /// for the given constraints, or `None` if this RenderBox does not have
    /// any baselines.
    ///
    /// This method calls `compute_dry_baseline` and is for "dry" layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDryBaseline` method.
    fn get_dry_baseline(&self, constraints: BoxConstraints, baseline: TextBaseline) -> Option<f32> {
        self.compute_dry_baseline(constraints, baseline)
    }

    /// Computes the distance from the top of the box to its first baseline.
    ///
    /// Returns `None` if this box has no baseline.
    ///
    /// Override this in subclasses that have baselines.
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Computes the dry baseline for the given constraints.
    ///
    /// Override this in subclasses that have baselines.
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
    ///
    /// This is typically `Rect::from_ltwh(0, 0, size.width, size.height)`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.paintBounds` getter.
    fn box_paint_bounds(&self) -> Rect {
        let size = self.size();
        Rect::new(0.0, 0.0, size.width, size.height)
    }
}

// ============================================================================
// Supporting Types
// ============================================================================

/// Result of a box hit test.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    /// The list of hit test entries.
    entries: Vec<BoxHitTestEntry>,
}

impl BoxHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry to the result.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        self.entries.push(entry);
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[BoxHitTestEntry] {
        &self.entries
    }

    /// Returns whether this result has any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// An entry in a box hit test result.
#[derive(Debug)]
pub struct BoxHitTestEntry {
    /// The local position of the hit.
    pub local_position: Offset,
}

impl BoxHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(local_position: Offset) -> Self {
        Self { local_position }
    }
}

/// Text baseline types for baseline alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    /// The alphabetic baseline (bottom of letters like 'a', 'e', 'o').
    Alphabetic,

    /// The ideographic baseline (bottom of ideographic characters).
    Ideographic,
}

// ============================================================================
// Single Child RenderBox
// ============================================================================

/// Trait for render boxes with at most one child.
pub trait SingleChildRenderBox: RenderBox {
    /// Returns the child, if any.
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns the child mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// Sets the child.
    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>);
}

// ============================================================================
// Multi Child RenderBox
// ============================================================================

/// Trait for render boxes with multiple children.
pub trait MultiChildRenderBox: RenderBox {
    /// Returns an iterator over children.
    fn children(&self) -> Box<dyn Iterator<Item = &dyn RenderBox> + '_>;

    /// Returns a mutable iterator over children.
    fn children_mut(&mut self) -> Box<dyn Iterator<Item = &mut dyn RenderBox> + '_>;

    /// Returns the number of children.
    fn child_count(&self) -> usize;

    /// Adds a child.
    fn add_child(&mut self, child: Box<dyn RenderBox>);

    /// Removes a child at the given index.
    fn remove_child(&mut self, index: usize) -> Option<Box<dyn RenderBox>>;

    /// Removes all children.
    fn clear_children(&mut self);
}
