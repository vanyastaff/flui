//! RenderBox - Box Protocol Render Trait (Flutter Model)
//!
//! This module provides the complete box protocol implementation following Flutter's
//! exact architecture from `rendering/box.dart`:
//!
//! - **`RenderBox<A>`** - Core trait for box protocol render objects
//! - **`BoxConstraints`** - Size constraints for box layout (re-exported)
//! - **`BoxHitTestResult`** - Hit testing with paint transform support
//! - **`BoxHitTestEntry`** - Hit entry with local position
//!
//! # Flutter Equivalence
//!
//! This module mirrors Flutter's `rendering/box.dart` which defines:
//!
//! ```dart
//! // Flutter rendering/box.dart contains:
//! class BoxConstraints extends Constraints { ... }
//! class BoxHitTestResult extends HitTestResult { ... }
//! class BoxHitTestEntry extends HitTestEntry<RenderBox> { ... }
//! typedef BoxHitTest = bool Function(BoxHitTestResult result, Offset position);
//! abstract class RenderBox extends RenderObject { ... }
//! ```
//!
//! # Architecture
//!
//! ```text
//! RenderObject (base)
//!       ↓
//! RenderBox<A: Arity>  ← Protocol-specific + arity validation
//!       ↓
//! Concrete implementations:
//!  ├─ RenderPadding: RenderBox<Single>
//!  ├─ RenderText: RenderBox<Leaf>
//!  ├─ RenderFlex: RenderBox<Variable>
//!  └─ RenderContainer: RenderBox<Optional>
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::box_render::{RenderBox, BoxConstraints, BoxHitTestResult};
//! use flui_tree::arity::Leaf;
//!
//! struct MyBox { size: Size }
//!
//! impl RenderBox<Leaf> for MyBox {
//!     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
//!         self.size = constraints.constrain(Size::new(100.0, 50.0));
//!         self.size
//!     }
//!     // ... other methods
//! }
//! ```

use std::fmt;

use flui_types::layout::TextBaseline;
use flui_types::{Matrix4, Offset, Rect, Size};

use super::object::RenderObject;
use super::painting_context::PaintingContext;
use flui_tree::arity::Arity;

// ============================================================================
// RE-EXPORTS (Flutter box.dart style)
// ============================================================================

/// Box constraints for layout.
///
/// Re-exported from `flui_types` to match Flutter's box.dart organization.
///
/// # Flutter Equivalence
///
/// ```dart
/// class BoxConstraints extends Constraints {
///   final double minWidth;
///   final double maxWidth;
///   final double minHeight;
///   final double maxHeight;
///   // ...
/// }
/// ```
pub use flui_types::BoxConstraints;

/// Hit test result for box protocol.
///
/// Defined in `flui_rendering` (like Flutter's `rendering/box.dart`). Provides:
/// - `add_with_paint_offset()` - Test child with paint offset
/// - `add_with_paint_transform()` - Test child with paint transform
/// - `add_with_raw_transform()` - Test child with already-inverted transform
///
/// # Flutter Equivalence
///
/// ```dart
/// class BoxHitTestResult extends HitTestResult {
///   bool addWithPaintOffset({Offset? offset, required Offset position, required BoxHitTest hitTest});
///   bool addWithPaintTransform({Matrix4? transform, required Offset position, required BoxHitTest hitTest});
///   bool addWithRawTransform({Matrix4? transform, required Offset position, required BoxHitTest hitTest});
/// }
/// ```
pub use crate::hit_test::BoxHitTestResult;

/// Hit test entry for box protocol.
///
/// Defined in `flui_rendering`. Contains:
/// - `target` - The RenderId of the hit render object
/// - `local_position` - Position in target's local coordinates
/// - `bounds` - Bounding rectangle of the target
///
/// # Flutter Equivalence
///
/// ```dart
/// class BoxHitTestEntry extends HitTestEntry<RenderBox> {
///   final Offset localPosition;
/// }
/// ```
pub use crate::hit_test::BoxHitTestEntry;

/// Hit test callback signature for box protocol.
///
/// Defined in `flui_rendering`.
///
/// # Flutter Equivalence
///
/// ```dart
/// typedef BoxHitTest = bool Function(BoxHitTestResult result, Offset position);
/// ```
#[allow(unused_imports)]
pub use crate::hit_test::BoxHitTest;

// ============================================================================
// RENDER BOX TRAIT (Flutter Model)
// ============================================================================

/// Render trait for box protocol with compile-time arity validation.
///
/// This trait follows Flutter's `RenderBox` protocol:
///
/// 1. **Constraints go down**: Parent passes `BoxConstraints` to `perform_layout()`
/// 2. **Sizes come up**: `perform_layout()` returns `Size` satisfying constraints
/// 3. **Parent sets position**: Parent positions child after layout
///
/// # Type Parameter
///
/// - `A: Arity` - Compile-time child count validation:
///   - `Leaf` - 0 children (Text, Image, ColoredBox)
///   - `Single` - 1 child (Padding, Transform, Opacity)
///   - `Optional` - 0-1 child (Container, SizedBox)
///   - `Variable` - 0+ children (Flex, Stack, Column)
///
/// # Required Methods
///
/// - `perform_layout(constraints) -> Size` - Compute layout
/// - `paint(ctx, offset)` - Paint to canvas
/// - `size() -> Size` - Return cached size
///
/// # Flutter Compliance
///
/// ✅ **MUST** satisfy constraints
/// ✅ **MUST** be idempotent (same constraints → same size)
/// ✅ **MUST NOT** call layout during paint
/// ✅ **MUST** layout children before querying their size
pub trait RenderBox<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    // ========================================================================
    // LAYOUT (Required)
    // ========================================================================

    /// Computes size given constraints.
    ///
    /// This is called during the layout phase. The implementation must:
    ///
    /// 1. Layout any children (using stored child references)
    /// 2. Compute own size based on constraints and child sizes
    /// 3. Return a size that satisfies the constraints
    ///
    /// # Contract
    ///
    /// - **MUST** return size satisfying constraints
    /// - **MUST** be idempotent (same constraints → same size)
    /// - **SHOULD** layout children before using their sizes
    /// - **SHOULD** store computed size for paint/hit_test access
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// void performLayout() {
    ///   size = constraints.constrain(Size(100, 50));
    /// }
    /// ```
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;

    // ========================================================================
    // PAINT (Required)
    // ========================================================================

    /// Paints this render object and its children.
    ///
    /// Called during the paint phase with a `PaintingContext` for canvas access
    /// and layer composition, plus an offset for positioning.
    ///
    /// # Arguments
    ///
    /// - `ctx` - Context for canvas access and child painting
    /// - `offset` - Position of this object in parent coordinates
    ///
    /// # Contract
    ///
    /// - **MUST NOT** call layout (use cached size from layout phase)
    /// - **SHOULD** draw content at `offset`
    /// - **SHOULD** paint children via `ctx.paint_child()`
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// void paint(PaintingContext context, Offset offset) {
    ///   context.canvas.drawRect(rect.shift(offset), paint);
    ///   context.paintChild(child!, childOffset);
    /// }
    /// ```
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);

    // ========================================================================
    // SIZE AND BOUNDS (Required)
    // ========================================================================

    /// Returns the computed size from layout.
    ///
    /// This should return the size computed during `perform_layout()`.
    /// Implementations must store the size during layout.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Size get size => _size!;
    /// ```
    fn size(&self) -> Size;

    /// Returns whether this render box has undergone layout and has a size.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool get hasSize => _size != null;
    /// ```
    fn has_size(&self) -> bool {
        true // Default: assume size is always available after layout
    }

    /// Returns local bounding rectangle.
    ///
    /// Default returns rectangle from origin to size.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Rect get paintBounds => Offset.zero & size;
    /// ```
    fn local_bounds(&self) -> Rect {
        Rect::from_min_size(Offset::ZERO, self.size())
    }

    /// Returns the bounding box for semantics/accessibility.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Rect get semanticBounds => Offset.zero & size;
    /// ```
    fn semantic_bounds(&self) -> Rect {
        self.local_bounds()
    }

    // ========================================================================
    // HIT TESTING
    // ========================================================================

    /// Hit tests this render object at the given position.
    ///
    /// Returns `true` if this object or any descendant was hit.
    ///
    /// # Arguments
    ///
    /// - `result` - Accumulator for hit test entries (BoxHitTestResult)
    /// - `position` - Position to test in local coordinates
    ///
    /// # Default Implementation
    ///
    /// The default implementation:
    /// 1. Checks if position is within bounds
    /// 2. Tests children via `hit_test_children()`
    /// 3. Tests self via `hit_test_self()`
    /// 4. Adds entry if hit
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// bool hitTest(BoxHitTestResult result, {required Offset position}) {
    ///   if (size.contains(position)) {
    ///     if (hitTestChildren(result, position: position) || hitTestSelf(position)) {
    ///       result.add(BoxHitTestEntry(this, position));
    ///       return true;
    ///     }
    ///   }
    ///   return false;
    /// }
    /// ```
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Check bounds first (position must be within 0..size)
        let size = self.size();
        let in_bounds = position.dx >= 0.0
            && position.dx < size.width
            && position.dy >= 0.0
            && position.dy < size.height;

        if !in_bounds {
            return false;
        }

        // Test children first (reverse z-order), then self
        if self.hit_test_children(result, position) || self.hit_test_self(position) {
            // Note: The tree layer is responsible for adding entries with IDs
            return true;
        }

        false
    }

    /// Whether this render object should be considered hit at the given position.
    ///
    /// Override to control when this object registers as hit.
    /// Default returns `false` (defer to children).
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// bool hitTestSelf(Offset position) => false;
    /// ```
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Hit tests children in reverse paint order (front to back).
    ///
    /// Default implementation returns `false` (no children or leaf node).
    /// Override for containers to test children.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// bool hitTestChildren(BoxHitTestResult result, {required Offset position}) => false;
    /// ```
    fn hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        false
    }

    /// Adds this render object to the hit test result.
    ///
    /// Called when hit test succeeds. The `id` parameter is the render ID
    /// assigned by the tree, since render objects don't inherently know their ID.
    fn add_to_hit_test_result(
        &self,
        result: &mut BoxHitTestResult,
        id: flui_foundation::RenderId,
        position: Offset,
    ) {
        let bounds = self.local_bounds();
        result.add_box_entry(BoxHitTestEntry::new(id, position, bounds));
    }

    // ========================================================================
    // INTRINSIC DIMENSIONS (Flutter-style)
    // ========================================================================

    /// Returns the minimum width that this box could be without failing to
    /// correctly paint its contents within itself, without clipping.
    ///
    /// The `height` argument may give a specific height to assume. The given
    /// height can be infinite, meaning that the intrinsic width in an
    /// unconstrained environment is being requested.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double getMinIntrinsicWidth(double height) {
    ///   return _computeIntrinsics(..., computeMinIntrinsicWidth);
    /// }
    /// ```
    fn get_min_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_min_intrinsic_width(height)
    }

    /// Returns the smallest width beyond which increasing the width never
    /// decreases the preferred height.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double getMaxIntrinsicWidth(double height) {
    ///   return _computeIntrinsics(..., computeMaxIntrinsicWidth);
    /// }
    /// ```
    fn get_max_intrinsic_width(&self, height: f32) -> f32 {
        self.compute_max_intrinsic_width(height)
    }

    /// Returns the minimum height that this box could be without failing to
    /// correctly paint its contents within itself, without clipping.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double getMinIntrinsicHeight(double width) {
    ///   return _computeIntrinsics(..., computeMinIntrinsicHeight);
    /// }
    /// ```
    fn get_min_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_min_intrinsic_height(width)
    }

    /// Returns the smallest height beyond which increasing the height never
    /// decreases the preferred width.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double getMaxIntrinsicHeight(double width) {
    ///   return _computeIntrinsics(..., computeMaxIntrinsicHeight);
    /// }
    /// ```
    fn get_max_intrinsic_height(&self, width: f32) -> f32 {
        self.compute_max_intrinsic_height(width)
    }

    /// Computes the minimum intrinsic width.
    ///
    /// Override this in subclasses. The default returns 0.0.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// double computeMinIntrinsicWidth(double height) => 0.0;
    /// ```
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width.
    ///
    /// Override this in subclasses. The default returns 0.0.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// double computeMaxIntrinsicWidth(double height) => 0.0;
    /// ```
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height.
    ///
    /// Override this in subclasses. The default returns 0.0.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// double computeMinIntrinsicHeight(double width) => 0.0;
    /// ```
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height.
    ///
    /// Override this in subclasses. The default returns 0.0.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// double computeMaxIntrinsicHeight(double width) => 0.0;
    /// ```
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    // ========================================================================
    // DRY LAYOUT
    // ========================================================================

    /// Returns the size this box would have if it were laid out with the
    /// given constraints.
    ///
    /// This method does not change the state of the render object. It is
    /// useful for computing intrinsic dimensions.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Size getDryLayout(BoxConstraints constraints) {
    ///   return _computeIntrinsics(..., computeDryLayout);
    /// }
    /// ```
    fn get_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.compute_dry_layout(constraints)
    }

    /// Computes the size this box would have if laid out with the given constraints.
    ///
    /// Override this in subclasses. The default returns the smallest size
    /// satisfying the constraints.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// Size computeDryLayout(BoxConstraints constraints) {
    ///   return Size.zero;
    /// }
    /// ```
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }

    // ========================================================================
    // BASELINES
    // ========================================================================

    /// Returns the distance from the y-coordinate of the position of the box
    /// to the y-coordinate of the first given baseline in the box's contents.
    ///
    /// Used by certain layout models to align adjacent boxes on a common
    /// baseline, regardless of padding, font size differences, etc.
    ///
    /// If there is no baseline, this function returns the distance from the
    /// y-coordinate of the position of the box to the y-coordinate of the
    /// bottom of the box (i.e., the height of the box) unless `only_real`
    /// is true, in which case it returns `None`.
    ///
    /// # Arguments
    ///
    /// - `baseline` - The type of baseline to find
    /// - `only_real` - If true, return None if no actual baseline exists
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double? getDistanceToBaseline(TextBaseline baseline, {bool onlyReal = false}) {
    ///   final double? result = getDistanceToActualBaseline(baseline);
    ///   if (result == null && !onlyReal) {
    ///     return size.height;
    ///   }
    ///   return result;
    /// }
    /// ```
    fn get_distance_to_baseline(&self, baseline: TextBaseline, only_real: bool) -> Option<f32> {
        let result = self.get_distance_to_actual_baseline(baseline);
        if result.is_none() && !only_real {
            return Some(self.size().height);
        }
        result
    }

    /// Calls `compute_distance_to_actual_baseline` and caches the result.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// @mustCallSuper
    /// double? getDistanceToActualBaseline(TextBaseline baseline) {
    ///   return _computeIntrinsics(..., computeDistanceToActualBaseline);
    /// }
    /// ```
    fn get_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.compute_distance_to_actual_baseline(baseline)
    }

    /// Returns the distance from the y-coordinate of the position of the box
    /// to the y-coordinate of the first given baseline in the box's contents,
    /// if any, or `None` otherwise.
    ///
    /// Do not call this function directly. If you need to know the baseline
    /// of a child from an invocation of `perform_layout` or `paint`, call
    /// `get_distance_to_baseline`.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @protected
    /// double? computeDistanceToActualBaseline(TextBaseline baseline) => null;
    /// ```
    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    // ========================================================================
    // COORDINATE TRANSFORMS
    // ========================================================================

    /// Converts the given point from the global coordinate system to the
    /// local coordinate system of this render object.
    ///
    /// # Arguments
    ///
    /// - `point` - The point in global coordinates
    /// - `ancestor` - Optional ancestor to use as the coordinate space origin.
    ///   If `None`, uses the root of the render tree.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Offset globalToLocal(Offset point, {RenderObject? ancestor}) {
    ///   final Matrix4 transform = getTransformTo(ancestor);
    ///   return MatrixUtils.transformPoint(transform, point);
    /// }
    /// ```
    fn global_to_local(&self, point: Offset, transform: Option<&Matrix4>) -> Offset {
        if let Some(t) = transform {
            // Apply inverse transform
            if let Some(inverse) = t.try_inverse() {
                let (x, y) = inverse.transform_point(point.dx, point.dy);
                return Offset::new(x, y);
            }
        }
        point
    }

    /// Converts the given point from the local coordinate system of this
    /// render object to the global coordinate system.
    ///
    /// # Arguments
    ///
    /// - `point` - The point in local coordinates
    /// - `ancestor` - Optional ancestor to use as the coordinate space origin.
    ///   If `None`, uses the root of the render tree.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// Offset localToGlobal(Offset point, {RenderObject? ancestor}) {
    ///   return MatrixUtils.transformPoint(getTransformTo(ancestor), point);
    /// }
    /// ```
    fn local_to_global(&self, point: Offset, transform: Option<&Matrix4>) -> Offset {
        if let Some(t) = transform {
            let (x, y) = t.transform_point(point.dx, point.dy);
            return Offset::new(x, y);
        }
        point
    }

    /// Multiply the transform from the parent's coordinate system to this
    /// box's coordinate system into the given transform.
    ///
    /// This function is used to convert coordinate systems between boxes.
    /// Subclasses that apply transforms during painting should override this
    /// function to factor those transforms into the calculation.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// void applyPaintTransform(RenderObject child, Matrix4 transform) {
    ///   final BoxParentData childParentData = child.parentData! as BoxParentData;
    ///   final Offset offset = childParentData.offset;
    ///   transform.translate(offset.dx, offset.dy);
    /// }
    /// ```
    fn apply_paint_transform(&self, child_offset: Offset, transform: &mut Matrix4) {
        transform.translate(child_offset.dx, child_offset.dy, 0.0);
    }

    // ========================================================================
    // LEGACY COMPATIBILITY
    // ========================================================================

    /// Returns intrinsic width for given height (legacy API).
    ///
    /// Prefer using `get_min_intrinsic_width` or `get_max_intrinsic_width`.
    #[deprecated(note = "Use get_min_intrinsic_width or get_max_intrinsic_width instead")]
    fn intrinsic_width(&self, height: f32) -> Option<f32> {
        let min = self.get_min_intrinsic_width(height);
        if min > 0.0 {
            Some(min)
        } else {
            None
        }
    }

    /// Returns intrinsic height for given width (legacy API).
    ///
    /// Prefer using `get_min_intrinsic_height` or `get_max_intrinsic_height`.
    #[deprecated(note = "Use get_min_intrinsic_height or get_max_intrinsic_height instead")]
    fn intrinsic_height(&self, width: f32) -> Option<f32> {
        let min = self.get_min_intrinsic_height(width);
        if min > 0.0 {
            Some(min)
        } else {
            None
        }
    }

    /// Returns baseline offset for text alignment (legacy API).
    ///
    /// Prefer using `get_distance_to_baseline`.
    #[deprecated(note = "Use get_distance_to_baseline instead")]
    fn baseline_offset(&self) -> Option<f32> {
        self.get_distance_to_actual_baseline(TextBaseline::Alphabetic)
    }
}

// ============================================================================
// DOCUMENTATION
// ============================================================================

/// Common mistakes when implementing RenderBox
///
/// # ❌ Pitfall 1: Not constraining result
///
/// ```rust,ignore
/// // WRONG:
/// fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///     Size::new(100.0, 50.0)  // Ignores constraints!
/// }
///
/// // CORRECT:
/// fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///     constraints.constrain(Size::new(100.0, 50.0))
/// }
/// ```
///
/// # ❌ Pitfall 2: Calling layout during paint
///
/// ```rust,ignore
/// // WRONG:
/// fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///     let size = child.perform_layout(constraints);  // NO!
/// }
///
/// // CORRECT:
/// fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///     let size = self.size();  // Use cached size
/// }
/// ```
///
/// # ❌ Pitfall 3: Not storing size
///
/// ```rust,ignore
/// // WRONG:
/// fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///     constraints.biggest()  // Returns but doesn't store
/// }
///
/// fn size(&self) -> Size {
///     Size::ZERO  // Wrong!
/// }
///
/// // CORRECT:
/// fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///     self.cached_size = constraints.biggest();
///     self.cached_size
/// }
///
/// fn size(&self) -> Size {
///     self.cached_size
/// }
/// ```
///
/// # ❌ Pitfall 4: Wrong hit test order
///
/// ```rust,ignore
/// // WRONG: Tests back to front
/// fn hit_test_children(&self, result, position) -> bool {
///     for child in self.children.iter() {  // Wrong order!
///         if child.hit_test(result, position) { return true; }
///     }
///     false
/// }
///
/// // CORRECT: Tests front to back (reverse paint order)
/// fn hit_test_children(&self, result, position) -> bool {
///     for child in self.children.iter().rev() {  // Reverse!
///         if child.hit_test(result, position) { return true; }
///     }
///     false
/// }
/// ```
mod _pitfalls {}
