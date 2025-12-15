//! RenderBox trait for 2D box layout.

use flui_types::{Offset, Point, Size};

use crate::constraints::BoxConstraints;
use crate::pipeline::PaintingContext;
use crate::traits::RenderObject;

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
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.performLayout` method.
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Returns the current size of this render object.
    ///
    /// Only valid after `perform_layout` has been called.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.size` getter.
    fn size(&self) -> Size;

    /// Sets the size of this render object.
    ///
    /// This should only be called during layout. The size must satisfy
    /// the constraints that were passed to `perform_layout`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.size` setter.
    fn set_size(&mut self, size: Size);

    /// Returns whether this render object has undergone layout and has a size.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.hasSize` getter.
    fn has_size(&self) -> bool {
        true
    }

    /// Returns the box constraints most recently supplied by the parent.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.constraints` getter.
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Paints this render object.
    ///
    /// Called after layout is complete. Should paint this object and
    /// then paint children at their appropriate offsets.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.paint` method.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Hit tests this render object.
    ///
    /// Returns true if the given position hits this render object or
    /// any of its children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.hitTest` method.
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
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.hitTestSelf` method.
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Hit tests children of this render object.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.hitTestChildren` method.
    fn hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        false
    }

    // ========================================================================
    // Intrinsic Dimensions
    // ========================================================================

    /// Returns the minimum intrinsic width for a given height.
    ///
    /// This is a public getter that may use caching.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMinIntrinsicWidth` method.
    fn get_min_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_min_intrinsic_width(height)
    }

    /// Returns the maximum intrinsic width for a given height.
    ///
    /// This is a public getter that may use caching.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMaxIntrinsicWidth` method.
    fn get_max_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_max_intrinsic_width(height)
    }

    /// Returns the minimum intrinsic height for a given width.
    ///
    /// This is a public getter that may use caching.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMinIntrinsicHeight` method.
    fn get_min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_min_intrinsic_height(width)
    }

    /// Returns the maximum intrinsic height for a given width.
    ///
    /// This is a public getter that may use caching.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getMaxIntrinsicHeight` method.
    fn get_max_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }

    /// Computes the minimum intrinsic width for a given height.
    ///
    /// Override this method to provide custom intrinsic width calculation.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeMinIntrinsicWidth` method.
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width for a given height.
    ///
    /// Override this method to provide custom intrinsic width calculation.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeMaxIntrinsicWidth` method.
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height for a given width.
    ///
    /// Override this method to provide custom intrinsic height calculation.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeMinIntrinsicHeight` method.
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height for a given width.
    ///
    /// Override this method to provide custom intrinsic height calculation.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeMaxIntrinsicHeight` method.
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    // ========================================================================
    // Dry Layout
    // ========================================================================

    /// Returns the size this render box would have given the constraints.
    ///
    /// This is a public getter that may use caching.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDryLayout` method.
    fn get_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(constraints)
    }

    /// Computes the size without actually laying out.
    ///
    /// Override this method to provide dry layout support.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeDryLayout` method.
    fn compute_dry_layout(&self, _constraints: BoxConstraints) -> Size {
        Size::ZERO
    }

    // ========================================================================
    // Baseline
    // ========================================================================

    /// Returns the distance from the top of the box to the first baseline.
    ///
    /// This is a public getter that may use caching.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDistanceToBaseline` method.
    fn get_distance_to_baseline(&self, baseline: TextBaseline, only_real: bool) -> Option<f32> {
        let result = self.get_distance_to_actual_baseline(baseline);
        if result.is_none() && !only_real {
            Some(self.size().height)
        } else {
            result
        }
    }

    /// Returns the distance from the top of the box to the actual baseline.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDistanceToActualBaseline` method.
    fn get_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_actual_baseline(baseline)
    }

    /// Computes the distance from top to the actual baseline.
    ///
    /// Override this method to provide baseline support.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeDistanceToActualBaseline` method.
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Returns the baseline offset for dry layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.getDryBaseline` method.
    fn get_dry_baseline(&self, constraints: BoxConstraints, baseline: TextBaseline) -> Option<f32> {
        self.compute_dry_baseline(constraints, baseline)
    }

    /// Computes the baseline offset for dry layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.computeDryBaseline` method.
    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        _baseline: TextBaseline,
    ) -> Option<f32> {
        None
    }

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Converts a point from global coordinates to local coordinates.
    ///
    /// # Arguments
    ///
    /// * `point` - The point in global coordinates
    /// * `ancestor` - Optional ancestor to stop at (None = root)
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.globalToLocal` method.
    fn global_to_local(&self, point: Point, ancestor: Option<&dyn RenderObject>) -> Point {
        let transform = self.get_transform_to(ancestor);
        // Invert transform and apply to point
        // For now, simple implementation assuming identity or translation only
        let _ = transform;
        point
    }

    /// Converts a point from local coordinates to global coordinates.
    ///
    /// # Arguments
    ///
    /// * `point` - The point in local coordinates
    /// * `ancestor` - Optional ancestor to stop at (None = root)
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.localToGlobal` method.
    fn local_to_global(&self, point: Point, ancestor: Option<&dyn RenderObject>) -> Point {
        let transform = self.get_transform_to(ancestor);
        // Apply transform to point
        // For now, simple implementation assuming identity or translation only
        let _ = transform;
        point
    }

    // ========================================================================
    // Default Helpers
    // ========================================================================

    /// Default implementation for painting children.
    ///
    /// Paints each child at its offset from parent data.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBoxContainerDefaultsMixin.defaultPaint` method.
    fn default_paint(&self, _context: &mut PaintingContext, _offset: Offset) {
        // Default: do nothing
        // Implementations should iterate over children and paint them
    }

    /// Default implementation for hit testing children.
    ///
    /// Tests each child in reverse paint order.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBoxContainerDefaultsMixin.defaultHitTestChildren` method.
    fn default_hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        // Default: no children hit
        false
    }

    /// Computes the distance to the first baseline of any child.
    ///
    /// Returns the minimum baseline distance among all children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBoxContainerDefaultsMixin.defaultComputeDistanceToFirstActualBaseline` method.
    fn default_compute_distance_to_first_actual_baseline(
        &self,
        _baseline: TextBaseline,
    ) -> Option<f32> {
        // Default implementation: return None
        // Implementations should iterate over children and find the first baseline
        None
    }

    /// Computes the distance to the highest baseline of any child.
    ///
    /// Returns the minimum baseline distance (highest on screen) among all children.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBoxContainerDefaultsMixin.defaultComputeDistanceToHighestActualBaseline` method.
    fn default_compute_distance_to_highest_actual_baseline(
        &self,
        _baseline: TextBaseline,
    ) -> Option<f32> {
        // Default implementation: return None
        // Implementations should iterate over children and find the highest baseline
        None
    }

    // ========================================================================
    // Debug Methods
    // ========================================================================

    /// Paints debugging visuals for the size of this render box.
    ///
    /// This is called by `debugPaint` when debugging layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugPaintSize` method.
    fn debug_paint_size(&self, _context: &mut PaintingContext, _offset: Offset) {
        // Default: do nothing
        // In debug mode, could paint a colored border around the box
    }

    /// Paints debugging visuals for baselines.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugPaintBaselines` method.
    fn debug_paint_baselines(&self, _context: &mut PaintingContext, _offset: Offset) {
        // Default: do nothing
        // In debug mode, could paint lines at baseline positions
    }

    /// Paints debugging visuals for pointer positions.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugPaintPointers` method.
    fn debug_paint_pointers(&self, _context: &mut PaintingContext, _offset: Offset) {
        // Default: do nothing
        // In debug mode, could paint indicators at hit points
    }

    /// Called when dry layout cannot be computed.
    ///
    /// Used for debugging to identify render objects that don't support dry layout.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugCannotComputeDryLayout` method.
    fn debug_cannot_compute_dry_layout(&self, reason: Option<&str>) -> bool {
        let _ = reason;
        // Default: return true to indicate dry layout not supported
        // Subclasses that support dry layout should override to return false
        true
    }

    /// Debug assertion that this render box meets its constraints.
    ///
    /// Called after layout to verify the size satisfies constraints.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugAssertDoesMeetConstraints` method.
    fn debug_assert_does_meet_constraints(&self) {
        if let Some(constraints) = self.constraints() {
            let size = self.size();
            debug_assert!(
                constraints.is_satisfied_by(size),
                "RenderBox size {:?} does not meet constraints {:?}",
                size,
                constraints
            );
        }
    }

    /// Marks this render box as having failed an intrinsic size check.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugAdoptSize` method.
    fn debug_adopt_size(&self, size: Size) -> Size {
        // In debug mode, could track and validate the size
        size
    }

    /// Resets the size for debug purposes.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderBox.debugResetSize` method.
    fn debug_reset_size(&mut self) {
        // Default: do nothing
        // Subclasses may use this to clear cached size information
    }
}

/// Result of a box hit test.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
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

    /// Adds an entry with just a local position.
    pub fn add_with_position(&mut self, local_position: Offset) {
        self.entries.push(BoxHitTestEntry::new(local_position));
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[BoxHitTestEntry] {
        &self.entries
    }

    /// Returns whether this result has any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Executes hit test with a paint offset.
    ///
    /// Transforms the position by subtracting the offset, calls the hit test
    /// closure, and returns the result.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `BoxHitTestResult.addWithPaintOffset`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// result.add_with_paint_offset(
    ///     Some(child_offset),
    ///     position,
    ///     |result, transformed| child.hit_test(result, transformed),
    /// )
    /// ```
    pub fn add_with_paint_offset<F>(
        &mut self,
        offset: Option<Offset>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut Self, Offset) -> bool,
    {
        let effective_offset = offset.unwrap_or(Offset::ZERO);
        let transformed_position = Offset::new(
            position.dx - effective_offset.dx,
            position.dy - effective_offset.dy,
        );
        hit_test(self, transformed_position)
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
    /// The alphabetic baseline.
    Alphabetic,
    /// The ideographic baseline.
    Ideographic,
}
