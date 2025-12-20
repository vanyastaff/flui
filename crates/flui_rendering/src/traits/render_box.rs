//! RenderBox trait for 2D box layout with Arity-based child management.

use flui_tree::{Arity, ChildrenAccess, Leaf, Optional, Variable};
use flui_types::{Offset, Point, Rect, Size};

use super::RenderObject;
use crate::constraints::BoxConstraints;
use crate::pipeline::PaintingContext;

// ============================================================================
// Hit Test Behavior
// ============================================================================

/// How a render object behaves during hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Targets that defer to their children receive events within their bounds
    /// only if one of their children is hit by the hit test.
    #[default]
    DeferToChild,

    /// Opaque targets can be hit even if their children have not been hit.
    Opaque,

    /// Translucent targets both receive events within their bounds and permit
    /// targets visually behind them to also receive events.
    Translucent,
}

// ============================================================================
// RenderBox Trait with Arity
// ============================================================================

/// Trait for render objects that use 2D cartesian coordinates.
///
/// The `Arity` associated type defines child count at compile time:
/// - `Leaf` - 0 children (Text, Image, ColoredBox)
/// - `Optional` - 0 or 1 child (Container, Padding, Center)
/// - `Variable` - N children (Row, Column, Stack)
///
/// # Example
///
/// ```ignore
/// // Leaf render object - no children
/// struct RenderColoredBox {
///     color: Color,
///     size: Size,
/// }
///
/// impl RenderBox for RenderColoredBox {
///     type Arity = Leaf;
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Self::Arity>) -> Size {
///         ctx.constraints().constrain(self.size)
///     }
/// }
///
/// // Single child render object
/// struct RenderPadding {
///     padding: EdgeInsets,
/// }
///
/// impl RenderBox for RenderPadding {
///     type Arity = Optional;
///
///     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Self::Arity>) -> Size {
///         if let Some(child) = ctx.child() {
///             let inner = ctx.constraints().deflate(self.padding);
///             let child_size = ctx.layout_child(child, inner);
///             ctx.position_child(child, self.padding.top_left());
///             child_size + self.padding.size()
///         } else {
///             ctx.constraints().smallest()
///         }
///     }
/// }
/// ```
pub trait RenderBox: RenderObject {
    /// The arity of this render box (Leaf, Optional, Variable, etc.)
    type Arity: Arity;

    // ========================================================================
    // Layout
    // ========================================================================

    /// Computes the layout of this render object.
    ///
    /// Called with a context that provides access to constraints and children
    /// based on the Arity type.
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Self::Arity>) -> Size;

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
    fn paint(&self, ctx: &mut BoxPaintContext<Self::Arity>, offset: Offset);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Returns the hit test behavior for this render object.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }

    /// Hit tests this render object.
    fn hit_test(&self, ctx: &mut BoxHitTestContext<Self::Arity>, position: Offset) -> bool;

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Converts a point from global coordinates to local coordinates.
    fn global_to_local(&self, point: Point, _ancestor: Option<&dyn RenderObject>) -> Point {
        point
    }

    /// Converts a point from local coordinates to global coordinates.
    fn local_to_global(&self, point: Point, _ancestor: Option<&dyn RenderObject>) -> Point {
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

// ============================================================================
// Layout Context with Arity
// ============================================================================

/// Layout context for RenderBox, generic over Arity.
///
/// Provides type-safe access to children based on the Arity type.
pub struct BoxLayoutContext<'a, A: Arity> {
    constraints: BoxConstraints,
    children: A::Accessor<'a, BoxChild>,
}

/// A child in the box layout context.
pub struct BoxChild {
    /// The child render box (type-erased for storage).
    render_box: Box<dyn RenderBoxDyn>,
    /// Computed size after layout.
    size: Size,
    /// Position offset set by parent.
    offset: Offset,
}

impl BoxChild {
    /// Returns the size of this child (valid after layout).
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the offset of this child.
    pub fn offset(&self) -> Offset {
        self.offset
    }
}

// Marker to allow BoxChild in Arity accessors
unsafe impl Send for BoxChild {}
unsafe impl Sync for BoxChild {}

impl<'a, A: Arity> BoxLayoutContext<'a, A> {
    /// Creates a new layout context.
    pub fn new(constraints: BoxConstraints, children: A::Accessor<'a, BoxChild>) -> Self {
        Self {
            constraints,
            children,
        }
    }

    /// Returns the constraints for this layout.
    pub fn constraints(&self) -> BoxConstraints {
        self.constraints
    }

    /// Returns the children accessor.
    pub fn children(&self) -> &A::Accessor<'a, BoxChild> {
        &self.children
    }
}

// Specialized methods for Leaf arity
impl<'a> BoxLayoutContext<'a, Leaf> {
    /// Leaf has no children - this is a no-op convenience.
    pub fn no_children(&self) {}
}

// Specialized methods for Optional arity
impl<'a> BoxLayoutContext<'a, Optional> {
    /// Returns the single child if present.
    pub fn child(&self) -> Option<&BoxChild> {
        self.children.get()
    }

    /// Layout the child with given constraints and return its size.
    pub fn layout_child(&mut self, constraints: BoxConstraints) -> Option<Size> {
        // In real implementation, this would call perform_layout on child
        self.children.get().map(|c| c.size)
    }

    /// Position the child at the given offset.
    pub fn position_child(&mut self, offset: Offset) {
        // In real implementation, this would set the child's offset
        let _ = offset;
    }
}

// Specialized methods for Variable arity
impl<'a> BoxLayoutContext<'a, Variable> {
    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns an iterator over children.
    pub fn iter_children(&self) -> impl Iterator<Item = &BoxChild> {
        self.children.iter()
    }

    /// Layout a child at index with given constraints.
    pub fn layout_child_at(&mut self, _index: usize, _constraints: BoxConstraints) -> Size {
        // In real implementation, this would call perform_layout on child
        Size::ZERO
    }

    /// Position a child at index.
    pub fn position_child_at(&mut self, _index: usize, _offset: Offset) {
        // In real implementation, this would set the child's offset
    }
}

// ============================================================================
// Paint Context with Arity
// ============================================================================

/// Paint context for RenderBox, generic over Arity.
pub struct BoxPaintContext<'a, A: Arity> {
    painting_context: &'a mut PaintingContext,
    children: A::Accessor<'a, BoxChild>,
}

impl<'a, A: Arity> BoxPaintContext<'a, A> {
    /// Returns the underlying painting context.
    pub fn canvas(&mut self) -> &mut PaintingContext {
        self.painting_context
    }
}

// Specialized methods for Optional arity
impl<'a> BoxPaintContext<'a, Optional> {
    /// Paint the child if present.
    pub fn paint_child(&mut self, offset: Offset) {
        if let Some(child) = self.children.get() {
            // In real implementation, this would paint the child
            let _ = (child, offset);
        }
    }
}

// Specialized methods for Variable arity
impl<'a> BoxPaintContext<'a, Variable> {
    /// Paint a child at index.
    pub fn paint_child_at(&mut self, _index: usize, _offset: Offset) {
        // In real implementation, this would paint the child
    }

    /// Paint all children with their stored offsets.
    pub fn paint_children(&mut self) {
        for child in self.children.iter() {
            // In real implementation, this would paint each child
            let _ = child;
        }
    }
}

// ============================================================================
// Hit Test Context with Arity
// ============================================================================

/// Hit test context for RenderBox, generic over Arity.
pub struct BoxHitTestContext<'a, A: Arity> {
    result: &'a mut BoxHitTestResult,
    children: A::Accessor<'a, BoxChild>,
}

impl<'a, A: Arity> BoxHitTestContext<'a, A> {
    /// Add a hit to the result.
    pub fn add_hit(&mut self, entry: BoxHitTestEntry) {
        self.result.add(entry);
    }
}

// Specialized methods for Optional arity
impl<'a> BoxHitTestContext<'a, Optional> {
    /// Hit test the child if present.
    pub fn hit_test_child(&mut self, position: Offset) -> bool {
        if let Some(child) = self.children.get() {
            // In real implementation, this would hit test the child
            let _ = (child, position);
        }
        false
    }
}

// Specialized methods for Variable arity
impl<'a> BoxHitTestContext<'a, Variable> {
    /// Hit test children in reverse order (front to back).
    pub fn hit_test_children(&mut self, position: Offset) -> bool {
        for child in self.children.iter().rev() {
            // In real implementation, this would hit test each child
            let _ = (child, position);
        }
        false
    }
}

// ============================================================================
// Type-erased RenderBox for storage
// ============================================================================

/// Type-erased RenderBox trait for dynamic dispatch.
pub trait RenderBoxDyn: RenderObject + Send + Sync {
    /// Perform layout with type-erased context.
    fn perform_layout_dyn(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint with type-erased context.
    fn paint_dyn(&self, context: &mut PaintingContext, offset: Offset);

    /// Hit test with type-erased context.
    fn hit_test_dyn(&self, result: &mut BoxHitTestResult, position: Offset) -> bool;

    /// Returns the size.
    fn size(&self) -> Size;
}

// ============================================================================
// Supporting Types
// ============================================================================

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

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[BoxHitTestEntry] {
        &self.entries
    }

    /// Returns whether this result has any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Transforms the position by subtracting the paint offset.
    pub fn add_with_paint_offset<F>(
        &mut self,
        offset: Option<Offset>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut BoxHitTestResult, Offset) -> bool,
    {
        let transformed = match offset {
            Some(off) => Offset::new(position.dx - off.dx, position.dy - off.dy),
            None => position,
        };
        hit_test(self, transformed)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_test_behavior_default() {
        let behavior = HitTestBehavior::default();
        assert_eq!(behavior, HitTestBehavior::DeferToChild);
    }

    #[test]
    fn test_box_hit_test_result() {
        let mut result = BoxHitTestResult::new();
        assert!(result.is_empty());

        result.add(BoxHitTestEntry::new(Offset::new(10.0, 20.0)));
        assert!(!result.is_empty());
        assert_eq!(result.entries().len(), 1);
    }
}
