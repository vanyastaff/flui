//! RenderBox trait for 2D box layout with Arity-based child management.

use std::marker::PhantomData;

use flui_tree::{Arity, ChildrenAccess, Leaf, Optional, Variable};
use flui_types::{Offset, Point, Rect, Size};

use super::RenderObject;
use crate::constraints::BoxConstraints;
use crate::parent_data::{BoxParentData, ParentData};
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
pub trait RenderBox: RenderObject {
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
    /// The context provides type-safe access to children with the correct
    /// ParentData type based on this render object's associated type.
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Self::Arity, Self::ParentData>)
        -> Size;

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
    fn paint(&self, ctx: &mut BoxPaintContext<Self::Arity, Self::ParentData>, offset: Offset);

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Returns the hit test behavior for this render object.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }

    /// Hit tests this render object.
    fn hit_test(
        &self,
        ctx: &mut BoxHitTestContext<Self::Arity, Self::ParentData>,
        position: Offset,
    ) -> bool;

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
// BoxChild - Type-safe child wrapper with ParentData
// ============================================================================

/// A child in the box layout context with type-safe parent data.
///
/// The `P` type parameter ensures compile-time type safety for parent data access.
pub struct BoxChild<P: ParentData + Default> {
    /// The child render box (type-erased for storage).
    render_box: Box<dyn RenderBoxDyn>,
    /// Computed size after layout.
    size: Size,
    /// Position offset set by parent.
    offset: Offset,
    /// Parent data stored by parent on this child (type-safe).
    parent_data: P,
}

impl<P: ParentData + Default> BoxChild<P> {
    /// Creates a new BoxChild with default parent data.
    pub fn new(render_box: Box<dyn RenderBoxDyn>) -> Self {
        Self {
            render_box,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data: P::default(),
        }
    }

    /// Creates a new BoxChild with specific parent data.
    pub fn with_parent_data(render_box: Box<dyn RenderBoxDyn>, parent_data: P) -> Self {
        Self {
            render_box,
            size: Size::ZERO,
            offset: Offset::ZERO,
            parent_data,
        }
    }

    /// Returns the size of this child (valid after layout).
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the offset of this child.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Sets the offset of this child.
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Returns a reference to the typed parent data.
    pub fn parent_data(&self) -> &P {
        &self.parent_data
    }

    /// Returns a mutable reference to the typed parent data.
    pub fn parent_data_mut(&mut self) -> &mut P {
        &mut self.parent_data
    }

    /// Returns a reference to the underlying render box.
    pub fn render_box(&self) -> &dyn RenderBoxDyn {
        self.render_box.as_ref()
    }

    /// Returns a mutable reference to the underlying render box.
    pub fn render_box_mut(&mut self) -> &mut dyn RenderBoxDyn {
        self.render_box.as_mut()
    }

    /// Layout this child with the given constraints.
    pub fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.render_box.perform_layout_dyn(constraints);
        self.size
    }
}

// Marker to allow BoxChild in Arity accessors
unsafe impl<P: ParentData + Default + Send> Send for BoxChild<P> {}
unsafe impl<P: ParentData + Default + Sync> Sync for BoxChild<P> {}

// ============================================================================
// Layout Context with Arity and ParentData
// ============================================================================

/// Layout context for RenderBox, generic over Arity and ParentData.
///
/// Provides type-safe access to children with properly typed parent data.
///
/// ## Examples
///
/// ```ignore
/// // For a Flex container:
/// fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, FlexParentData>) -> Size {
///     let total_flex: f32 = ctx.iter_children()
///         .map(|child| child.parent_data().flex)
///         .sum();
///     // ...
/// }
///
/// // For a Stack container:
/// fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Variable, StackParentData>) -> Size {
///     for child in ctx.iter_children() {
///         let pos = child.parent_data();
///         if let Some(left) = pos.left {
///             // Position from left
///         }
///     }
/// }
/// ```
pub struct BoxLayoutContext<'a, A: Arity, P: ParentData + Default> {
    constraints: BoxConstraints,
    children: A::Accessor<'a, BoxChild<P>>,
    _marker: PhantomData<P>,
}

impl<'a, A: Arity, P: ParentData + Default> BoxLayoutContext<'a, A, P> {
    /// Creates a new layout context.
    pub fn new(constraints: BoxConstraints, children: A::Accessor<'a, BoxChild<P>>) -> Self {
        Self {
            constraints,
            children,
            _marker: PhantomData,
        }
    }

    /// Returns the constraints for this layout.
    pub fn constraints(&self) -> BoxConstraints {
        self.constraints
    }

    /// Returns the children accessor.
    pub fn children(&self) -> &A::Accessor<'a, BoxChild<P>> {
        &self.children
    }

    /// Returns a mutable reference to the children accessor.
    pub fn children_mut(&mut self) -> &mut A::Accessor<'a, BoxChild<P>> {
        &mut self.children
    }
}

// Specialized methods for Leaf arity (no children)
impl<'a, P: ParentData + Default> BoxLayoutContext<'a, Leaf, P> {
    /// Leaf has no children - this is a no-op convenience.
    pub fn no_children(&self) {}
}

// Specialized methods for Optional arity (0 or 1 child)
impl<'a, P: ParentData + Default> BoxLayoutContext<'a, Optional, P> {
    /// Returns the single child if present.
    pub fn child(&self) -> Option<&BoxChild<P>> {
        self.children.get()
    }

    /// Returns a mutable reference to the child if present.
    pub fn child_mut(&mut self) -> Option<&mut BoxChild<P>> {
        self.children.get_mut()
    }

    /// Layout the child with given constraints and return its size.
    pub fn layout_child(&mut self, constraints: BoxConstraints) -> Option<Size> {
        self.children
            .get_mut()
            .map(|child| child.layout(constraints))
    }

    /// Position the child at the given offset.
    pub fn position_child(&mut self, offset: Offset) {
        if let Some(child) = self.children.get_mut() {
            child.set_offset(offset);
        }
    }
}

// Specialized methods for Variable arity (N children)
impl<'a, P: ParentData + Default> BoxLayoutContext<'a, Variable, P> {
    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns true if there are no children.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns an iterator over children.
    pub fn iter_children(&self) -> impl Iterator<Item = &BoxChild<P>> {
        self.children.iter()
    }

    /// Returns a mutable iterator over children.
    pub fn iter_children_mut(&mut self) -> impl Iterator<Item = &mut BoxChild<P>> {
        self.children.iter_mut()
    }

    /// Returns a child at the given index.
    pub fn child_at(&self, index: usize) -> Option<&BoxChild<P>> {
        self.children.get(index)
    }

    /// Returns a mutable reference to a child at the given index.
    pub fn child_at_mut(&mut self, index: usize) -> Option<&mut BoxChild<P>> {
        self.children.get_mut(index)
    }

    /// Layout a child at index with given constraints.
    pub fn layout_child_at(&mut self, index: usize, constraints: BoxConstraints) -> Option<Size> {
        self.children
            .get_mut(index)
            .map(|child| child.layout(constraints))
    }

    /// Position a child at index.
    pub fn position_child_at(&mut self, index: usize, offset: Offset) {
        if let Some(child) = self.children.get_mut(index) {
            child.set_offset(offset);
        }
    }
}

// ============================================================================
// Paint Context with Arity and ParentData
// ============================================================================

/// Paint context for RenderBox, generic over Arity and ParentData.
pub struct BoxPaintContext<'a, A: Arity, P: ParentData + Default> {
    painting_context: &'a mut PaintingContext,
    children: A::Accessor<'a, BoxChild<P>>,
    _marker: PhantomData<P>,
}

impl<'a, A: Arity, P: ParentData + Default> BoxPaintContext<'a, A, P> {
    /// Creates a new paint context.
    pub fn new(
        painting_context: &'a mut PaintingContext,
        children: A::Accessor<'a, BoxChild<P>>,
    ) -> Self {
        Self {
            painting_context,
            children,
            _marker: PhantomData,
        }
    }

    /// Returns the underlying painting context.
    pub fn canvas(&mut self) -> &mut PaintingContext {
        self.painting_context
    }
}

// Specialized methods for Leaf arity
impl<'a, P: ParentData + Default> BoxPaintContext<'a, Leaf, P> {
    /// Leaf has no children to paint.
    pub fn no_children(&self) {}
}

// Specialized methods for Optional arity
impl<'a, P: ParentData + Default> BoxPaintContext<'a, Optional, P> {
    /// Returns the child if present.
    pub fn child(&self) -> Option<&BoxChild<P>> {
        self.children.get()
    }

    /// Paint the child if present at its stored offset.
    pub fn paint_child(&mut self) {
        if let Some(child) = self.children.get() {
            let offset = child.offset();
            child.render_box().paint_dyn(self.painting_context, offset);
        }
    }

    /// Paint the child if present at the given offset.
    pub fn paint_child_at(&mut self, offset: Offset) {
        if let Some(child) = self.children.get() {
            child.render_box().paint_dyn(self.painting_context, offset);
        }
    }
}

// Specialized methods for Variable arity
impl<'a, P: ParentData + Default> BoxPaintContext<'a, Variable, P> {
    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns an iterator over children.
    pub fn iter_children(&self) -> impl Iterator<Item = &BoxChild<P>> {
        self.children.iter()
    }

    /// Paint a child at index using its stored offset.
    pub fn paint_child_at(&mut self, index: usize) {
        if let Some(child) = self.children.get(index) {
            let offset = child.offset();
            child.render_box().paint_dyn(self.painting_context, offset);
        }
    }

    /// Paint a child at index with a specific offset.
    pub fn paint_child_at_offset(&mut self, index: usize, offset: Offset) {
        if let Some(child) = self.children.get(index) {
            child.render_box().paint_dyn(self.painting_context, offset);
        }
    }

    /// Paint all children using their stored offsets.
    pub fn paint_children(&mut self) {
        for child in self.children.iter() {
            let offset = child.offset();
            child.render_box().paint_dyn(self.painting_context, offset);
        }
    }
}

// ============================================================================
// Hit Test Context with Arity and ParentData
// ============================================================================

/// Hit test context for RenderBox, generic over Arity and ParentData.
pub struct BoxHitTestContext<'a, A: Arity, P: ParentData + Default> {
    result: &'a mut BoxHitTestResult,
    children: A::Accessor<'a, BoxChild<P>>,
    _marker: PhantomData<P>,
}

impl<'a, A: Arity, P: ParentData + Default> BoxHitTestContext<'a, A, P> {
    /// Creates a new hit test context.
    pub fn new(result: &'a mut BoxHitTestResult, children: A::Accessor<'a, BoxChild<P>>) -> Self {
        Self {
            result,
            children,
            _marker: PhantomData,
        }
    }

    /// Add a hit to the result.
    pub fn add_hit(&mut self, entry: BoxHitTestEntry) {
        self.result.add(entry);
    }

    /// Returns the hit test result.
    pub fn result(&self) -> &BoxHitTestResult {
        self.result
    }

    /// Returns a mutable reference to the hit test result.
    pub fn result_mut(&mut self) -> &mut BoxHitTestResult {
        self.result
    }
}

// Specialized methods for Leaf arity
impl<'a, P: ParentData + Default> BoxHitTestContext<'a, Leaf, P> {
    /// Leaf has no children to hit test.
    pub fn no_children(&self) {}
}

// Specialized methods for Optional arity
impl<'a, P: ParentData + Default> BoxHitTestContext<'a, Optional, P> {
    /// Returns the child if present.
    pub fn child(&self) -> Option<&BoxChild<P>> {
        self.children.get()
    }

    /// Hit test the child if present.
    pub fn hit_test_child(&mut self, position: Offset) -> bool {
        if let Some(child) = self.children.get() {
            let child_offset = child.offset();
            let local_position =
                Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);
            return child.render_box().hit_test_dyn(self.result, local_position);
        }
        false
    }
}

// Specialized methods for Variable arity
impl<'a, P: ParentData + Default> BoxHitTestContext<'a, Variable, P> {
    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns an iterator over children.
    pub fn iter_children(&self) -> impl Iterator<Item = &BoxChild<P>> {
        self.children.iter()
    }

    /// Hit test a child at index.
    pub fn hit_test_child_at(&mut self, index: usize, position: Offset) -> bool {
        if let Some(child) = self.children.get(index) {
            let child_offset = child.offset();
            let local_position =
                Offset::new(position.dx - child_offset.dx, position.dy - child_offset.dy);
            return child.render_box().hit_test_dyn(self.result, local_position);
        }
        false
    }

    /// Hit test children in reverse order (front to back).
    /// Returns true if any child was hit.
    pub fn hit_test_children(&mut self, position: Offset) -> bool {
        for i in (0..self.children.len()).rev() {
            if self.hit_test_child_at(i, position) {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// Type-erased RenderBox for storage
// ============================================================================

/// Type-erased RenderBox trait for dynamic dispatch.
///
/// This allows storing heterogeneous RenderBox implementations
/// in a type-erased container while still supporting layout/paint/hit-test.
pub trait RenderBoxDyn: RenderObject + Send + Sync {
    /// Perform layout with type-erased constraints.
    fn perform_layout_dyn(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint with type-erased context.
    fn paint_dyn(&self, context: &mut PaintingContext, offset: Offset);

    /// Hit test with type-erased result.
    fn hit_test_dyn(&self, result: &mut BoxHitTestResult, position: Offset) -> bool;

    /// Returns the current size.
    fn size(&self) -> Size;

    /// Sets the size.
    fn set_size(&mut self, size: Size);
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

    /// Clears all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
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

        result.clear();
        assert!(result.is_empty());
    }

    #[test]
    fn test_box_hit_test_result_with_offset() {
        let mut result = BoxHitTestResult::new();
        let position = Offset::new(100.0, 100.0);
        let paint_offset = Some(Offset::new(10.0, 20.0));

        let hit = result.add_with_paint_offset(paint_offset, position, |_result, transformed| {
            // Verify the position was transformed correctly
            assert_eq!(transformed.dx, 90.0);
            assert_eq!(transformed.dy, 80.0);
            true
        });

        assert!(hit);
    }
}
