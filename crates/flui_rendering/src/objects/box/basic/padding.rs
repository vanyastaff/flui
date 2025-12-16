//! RenderPadding - insets its child by the given padding.
//!
//! When passing layout constraints to its child, padding shrinks the
//! constraints by the given padding, causing the child to layout at a smaller
//! size. Padding then sizes itself to its child's size, inflated by the
//! padding, effectively creating empty space around the child.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `RenderPadding` class in `rendering/shifted_box.dart`.
//!
//! # Architecture
//!
//! Following Flutter's architecture exactly:
//! - `RenderPadding` extends `RenderShiftedBox`
//! - Child offset is stored in `child.parentData.offset` (BoxParentData)
//! - `performLayout()` sets `child.parentData.offset = Offset(padding.left, padding.top)`
//! - `paint()` and `hitTestChildren()` read offset from child.parentData

use std::any::Any;

use flui_types::{EdgeInsets, Offset, Size};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::lifecycle::BaseRenderObject;
use crate::parent_data::BoxParentData;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::{
    set_child_offset, BoxHitTestResult, DiagnosticPropertiesBuilder, RenderBox, RenderObject,
    RenderShiftedBox, SingleChildRenderBox, TextBaseline,
};

/// A render object that insets its child by the given padding.
///
/// # Trait Hierarchy
///
/// `RenderPadding` implements the full render object trait chain:
/// ```text
/// RenderObject → RenderBox → SingleChildRenderBox → RenderShiftedBox
/// ```
///
/// This enables polymorphic usage:
/// - `Box<dyn RenderObject>` - for generic render tree operations
/// - `Box<dyn RenderBox>` - for box layout operations
/// - `Box<dyn SingleChildRenderBox>` - for single-child operations
/// - `Box<dyn RenderShiftedBox>` - for shifted box operations
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderPadding` class:
/// ```dart
/// class RenderPadding extends RenderShiftedBox {
///   EdgeInsetsGeometry _padding;
///   TextDirection? _textDirection;
///   EdgeInsets? _resolvedPaddingCache;
///   // ...
/// }
/// ```
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderPadding;
/// use flui_types::EdgeInsets;
///
/// let mut padding = RenderPadding::new(EdgeInsets::all(16.0));
///
/// // Use as dyn RenderBox
/// let render_box: &dyn RenderBox = &padding;
/// ```
#[derive(Debug)]
pub struct RenderPadding {
    /// Base render object for lifecycle management.
    base: BaseRenderObject,

    /// The child render box (from RenderObjectWithChildMixin).
    child: BoxChild,

    /// The cached size from last layout.
    size: Size,

    /// The amount to pad the child in each dimension.
    padding: EdgeInsets,
}

impl RenderPadding {
    /// Creates a new render padding with the given edge insets.
    ///
    /// The padding must have non-negative insets.
    pub fn new(padding: EdgeInsets) -> Self {
        debug_assert!(
            padding.is_non_negative(),
            "Padding must have non-negative insets"
        );
        Self {
            base: BaseRenderObject::new(),
            child: BoxChild::new(),
            size: Size::ZERO,
            padding,
        }
    }

    /// Creates a new render padding with child.
    pub fn with_child(padding: EdgeInsets, child: Box<dyn RenderBox>) -> Self {
        debug_assert!(
            padding.is_non_negative(),
            "Padding must have non-negative insets"
        );

        // Setup BoxParentData on the child
        let mut child = child;
        Self::setup_child_parent_data(&mut *child);

        Self {
            base: BaseRenderObject::new(),
            child: BoxChild::with(child),
            size: Size::ZERO,
            padding,
        }
    }

    /// Returns the current padding.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Sets the padding.
    ///
    /// The padding must have non-negative insets.
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        debug_assert!(
            padding.is_non_negative(),
            "Padding must have non-negative insets"
        );
        if self.padding != padding {
            self.padding = padding;
            self.mark_needs_layout();
        }
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.deflate(self.padding)
    }

    /// Sets up BoxParentData on a child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `setupParentData`:
    /// ```dart
    /// void setupParentData(RenderObject child) {
    ///   if (child.parentData is! BoxParentData)
    ///     child.parentData = BoxParentData();
    /// }
    /// ```
    fn setup_child_parent_data(child: &mut dyn RenderBox) {
        // Only set if not already BoxParentData
        let needs_setup = child
            .parent_data()
            .map(|pd| pd.as_any().downcast_ref::<BoxParentData>().is_none())
            .unwrap_or(true);

        if needs_setup {
            child.set_parent_data(Box::new(BoxParentData::default()));
        }
    }
}

// ============================================================================
// RenderObject trait implementation
// ============================================================================

impl RenderObject for RenderPadding {
    fn base(&self) -> &BaseRenderObject {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        &mut self.base
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        None // TODO: implement when pipeline is connected
    }

    fn attach(&mut self, owner: &PipelineOwner) {
        // Attach children
        if let Some(child) = self.child.get_mut() {
            child.attach(owner);
        }
    }

    fn detach(&mut self) {
        // Detach children
        if let Some(child) = self.child.get_mut() {
            child.detach();
        }
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {
        // TODO: implement proper adopt_child
    }

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {
        // TODO: implement proper drop_child
    }

    fn redepth_child(&mut self, _child: &mut dyn RenderObject) {
        // TODO: implement proper redepth_child
    }

    fn mark_parent_needs_layout(&mut self) {
        // TODO: implement when parent tracking is added
    }

    fn schedule_initial_layout(&mut self) {
        // TODO: implement when pipeline is connected
    }

    fn schedule_initial_paint(&mut self) {
        // TODO: implement when pipeline is connected
    }

    fn paint_bounds(&self) -> flui_types::Rect {
        flui_types::Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height)
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = self.child.get() {
            visitor(child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = self.child.get_mut() {
            visitor(child);
        }
    }

    fn debug_fill_properties(&self, properties: &mut DiagnosticPropertiesBuilder) {
        properties.add_string("padding", format!("{:?}", self.padding));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// RenderBox trait implementation
// ============================================================================

impl RenderBox for RenderPadding {
    /// Performs layout following Flutter's RenderPadding.performLayout exactly.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void performLayout() {
    ///   final BoxConstraints constraints = this.constraints;
    ///   final EdgeInsets padding = _resolvedPadding;
    ///   if (child == null) {
    ///     size = constraints.constrain(Size(padding.horizontal, padding.vertical));
    ///     return;
    ///   }
    ///   final BoxConstraints innerConstraints = constraints.deflate(padding);
    ///   child!.layout(innerConstraints, parentUsesSize: true);
    ///   final BoxParentData childParentData = child!.parentData! as BoxParentData;
    ///   childParentData.offset = Offset(padding.left, padding.top);
    ///   size = constraints.constrain(Size(
    ///     padding.horizontal + child!.size.width,
    ///     padding.vertical + child!.size.height,
    ///   ));
    /// }
    /// ```
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let padding = self.padding;

        if !self.child.has_child() {
            let size = constraints.constrain(Size::new(
                padding.horizontal_total(),
                padding.vertical_total(),
            ));
            self.size = size;
            return size;
        }

        // Layout child with deflated constraints
        let inner_constraints = constraints.deflate(padding);
        let child = self.child.get_mut().unwrap();
        let child_size = child.perform_layout(inner_constraints);

        // Set child offset in child's parentData (Flutter style!)
        set_child_offset(child, Offset::new(padding.left, padding.top));

        // Size is child size + padding
        let size = constraints.constrain(Size::new(
            padding.horizontal_total() + child_size.width,
            padding.vertical_total() + child_size.height,
        ));
        self.size = size;
        size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Paints the child at the offset from child.parentData.
    ///
    /// # Flutter Equivalence
    ///
    /// Uses inherited `RenderShiftedBox.paint` which reads from child.parentData.offset.
    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Use RenderShiftedBox default implementation
        self.shifted_paint(context, offset);
    }

    /// Hit tests children using offset from child.parentData.
    ///
    /// # Flutter Equivalence
    ///
    /// Uses inherited `RenderShiftedBox.hitTestChildren`.
    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.shifted_hit_test_children(result, position)
    }

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        let padding = self.padding;
        let inner_height = (height - padding.vertical_total()).max(0.0);
        let child_width = self
            .child
            .get()
            .map(|c| c.get_min_intrinsic_width(inner_height))
            .unwrap_or(0.0);
        child_width + padding.horizontal_total()
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        let padding = self.padding;
        let inner_height = (height - padding.vertical_total()).max(0.0);
        let child_width = self
            .child
            .get()
            .map(|c| c.get_max_intrinsic_width(inner_height))
            .unwrap_or(0.0);
        child_width + padding.horizontal_total()
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        let padding = self.padding;
        let inner_width = (width - padding.horizontal_total()).max(0.0);
        let child_height = self
            .child
            .get()
            .map(|c| c.get_min_intrinsic_height(inner_width))
            .unwrap_or(0.0);
        child_height + padding.vertical_total()
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        let padding = self.padding;
        let inner_width = (width - padding.horizontal_total()).max(0.0);
        let child_height = self
            .child
            .get()
            .map(|c| c.get_max_intrinsic_height(inner_width))
            .unwrap_or(0.0);
        child_height + padding.vertical_total()
    }

    /// Computes baseline distance following Flutter's RenderPadding.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double? computeDryBaseline(BoxConstraints constraints, TextBaseline baseline) {
    ///   if (child == null) return null;
    ///   final EdgeInsets padding = _resolvedPadding;
    ///   final BoxConstraints innerConstraints = constraints.deflate(padding);
    ///   final double? childBaseline = child!.getDryBaseline(innerConstraints, baseline);
    ///   if (childBaseline == null) return null;
    ///   return childBaseline + padding.top;
    /// }
    /// ```
    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        // Use RenderShiftedBox implementation which reads offset from child.parentData
        self.shifted_compute_distance_to_actual_baseline(baseline)
    }
}

// ============================================================================
// SingleChildRenderBox trait implementation
// ============================================================================

impl SingleChildRenderBox for RenderPadding {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.child.get()
    }

    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.get_mut()
    }

    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        // Clear old child
        self.child.clear();

        // Setup new child
        if let Some(mut new_child) = child {
            Self::setup_child_parent_data(&mut *new_child);
            self.child.set(new_child);
        }

        self.mark_needs_layout();
    }

    fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.take()
    }
}

// ============================================================================
// RenderShiftedBox trait implementation
// ============================================================================

impl RenderShiftedBox for RenderPadding {
    // All methods use default implementations from the trait which read
    // offset from child.parentData.offset
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_new() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        assert_eq!(padding.padding(), EdgeInsets::all(10.0));
    }

    #[test]
    fn test_padding_layout_no_child() {
        let mut padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let size = padding.perform_layout(constraints);

        // Without child, size is just padding
        assert_eq!(size.width, 40.0); // 10 + 30
        assert_eq!(size.height, 60.0); // 20 + 40
    }

    #[test]
    fn test_padding_child_offset_in_parent_data() {
        let mut padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        padding.perform_layout(constraints);

        // Child offset should be read from child.parentData
        // Since no child, offset is zero
        let offset = padding.child_parent_data_offset();
        assert_eq!(offset, Offset::ZERO);
    }

    #[test]
    fn test_constraints_for_child() {
        let padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        let child_constraints = padding.constraints_for_child(constraints);

        assert_eq!(child_constraints.max_width, 160.0); // 200 - 10 - 30
        assert_eq!(child_constraints.max_height, 140.0); // 200 - 20 - 40
    }

    // ========================================================================
    // Polymorphism tests
    // ========================================================================

    #[test]
    fn test_render_padding_as_render_box() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        // Should compile - RenderPadding implements RenderBox
        let _: &dyn RenderBox = &padding;
    }

    #[test]
    fn test_render_padding_as_single_child() {
        let padding = RenderPadding::new(EdgeInsets::all(10.0));
        // Should compile - RenderPadding implements SingleChildRenderBox
        let single: &dyn SingleChildRenderBox = &padding;
        assert!(single.child().is_none());
    }

    #[test]
    fn test_render_padding_as_shifted_box() {
        let mut padding = RenderPadding::new(EdgeInsets::all(10.0));
        padding.perform_layout(BoxConstraints::tight(Size::new(100.0, 100.0)));

        // Should compile - RenderPadding implements RenderShiftedBox
        let _: &dyn RenderShiftedBox = &padding;
    }

    #[test]
    fn test_render_padding_boxed() {
        let padding: Box<dyn RenderBox> = Box::new(RenderPadding::new(EdgeInsets::all(10.0)));
        // Should work - can be stored as Box<dyn RenderBox>
        assert_eq!(padding.size(), Size::ZERO);
    }
}
