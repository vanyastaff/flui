//! RenderPadding - insets its child by the given padding.
//!
//! When passing layout constraints to its child, padding shrinks the
//! constraints by the given padding, causing the child to layout at a smaller
//! size. Padding then sizes itself to its child's size, inflated by the
//! padding, effectively creating empty space around the child.

use std::any::Any;

use flui_types::{EdgeInsets, Offset, Size};

use crate::constraints::BoxConstraints;
use crate::containers::{ShiftedBox, SingleChildContainer};
use crate::lifecycle::BaseRenderObject;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::{
    BoxHitTestResult, DiagnosticPropertiesBuilder, RenderBox, RenderObject, RenderShiftedBox,
    SingleChildRenderBox, TextBaseline,
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
/// This corresponds to Flutter's `RenderPadding` class which extends
/// `RenderShiftedBox`.
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
    /// Container holding the child, geometry, offset, and lifecycle state.
    shifted: ShiftedBox,

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
            shifted: ShiftedBox::new(),
            padding,
        }
    }

    /// Creates a new render padding with child.
    pub fn with_child(padding: EdgeInsets, child: Box<dyn RenderBox>) -> Self {
        debug_assert!(
            padding.is_non_negative(),
            "Padding must have non-negative insets"
        );
        Self {
            shifted: ShiftedBox::with_child(child),
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
            self.shifted
                .inner_mut()
                .base_mut()
                .state_mut()
                .mark_needs_layout();
        }
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.deflate(self.padding)
    }
}

// ============================================================================
// RenderObject trait implementation
// ============================================================================

impl RenderObject for RenderPadding {
    fn base(&self) -> &BaseRenderObject {
        // Note: We don't have BaseRenderObject in Shifted, we have RenderObjectState
        // This is a design mismatch - for now return a stub
        // TODO: Refactor to store BaseRenderObject or adapt the trait
        unimplemented!("RenderPadding::base() - need to adapt architecture")
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        unimplemented!("RenderPadding::base_mut() - need to adapt architecture")
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        None // TODO: implement when pipeline is connected
    }

    fn attach(&mut self, _owner: &PipelineOwner) {
        // TODO: implement proper attach
    }

    fn detach(&mut self) {
        // TODO: implement proper detach
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
        flui_types::Rect::from_ltwh(0.0, 0.0, self.size().width, self.size().height)
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = SingleChildContainer::child(&self.shifted) {
            // child is &Box<dyn RenderBox>, need &dyn RenderObject
            // RenderBox: RenderObject, so we can upcast
            let child_ref: &dyn RenderBox = child.as_ref();
            visitor(child_ref);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = SingleChildContainer::child_mut(&mut self.shifted) {
            let child_ref: &mut dyn RenderBox = child.as_mut();
            visitor(child_ref);
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
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let padding = self.padding;

        // Layout child if present
        let child_size = if let Some(child) = self.shifted.child_mut() {
            let child_constraints = constraints.deflate(padding);
            child.perform_layout(child_constraints)
        } else {
            Size::ZERO
        };

        // Position child at padding offset
        self.shifted
            .set_offset(Offset::new(padding.left, padding.top));

        // Size is child size plus padding
        let size = Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        );
        let size = constraints.constrain(size);
        self.shifted.set_geometry(size);
        size
    }

    fn size(&self) -> Size {
        self.shifted.size()
    }

    fn set_size(&mut self, size: Size) {
        self.shifted.set_size(size);
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        self.shifted.paint_child(offset, |child, child_offset| {
            child.paint(context, child_offset);
        });
    }

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

    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.shifted.hit_test_child(result, position)
    }

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        let padding = self.padding;
        let inner_height = (height - padding.vertical_total()).max(0.0);
        let child_width = self
            .shifted
            .child()
            .map(|c| c.get_min_intrinsic_width(inner_height))
            .unwrap_or(0.0);
        child_width + padding.horizontal_total()
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        let padding = self.padding;
        let inner_height = (height - padding.vertical_total()).max(0.0);
        let child_width = self
            .shifted
            .child()
            .map(|c| c.get_max_intrinsic_width(inner_height))
            .unwrap_or(0.0);
        child_width + padding.horizontal_total()
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        let padding = self.padding;
        let inner_width = (width - padding.horizontal_total()).max(0.0);
        let child_height = self
            .shifted
            .child()
            .map(|c| c.get_min_intrinsic_height(inner_width))
            .unwrap_or(0.0);
        child_height + padding.vertical_total()
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        let padding = self.padding;
        let inner_width = (width - padding.horizontal_total()).max(0.0);
        let child_height = self
            .shifted
            .child()
            .map(|c| c.get_max_intrinsic_height(inner_width))
            .unwrap_or(0.0);
        child_height + padding.vertical_total()
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.shifted
            .child()
            .and_then(|c| c.get_distance_to_actual_baseline(baseline))
            .map(|distance| distance + self.padding.top)
    }
}

// ============================================================================
// SingleChildRenderBox trait implementation
// ============================================================================

impl SingleChildRenderBox for RenderPadding {
    fn child(&self) -> Option<&dyn RenderBox> {
        SingleChildContainer::child(&self.shifted).map(|b| b.as_ref())
    }

    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        SingleChildContainer::child_mut(&mut self.shifted).map(|b| b.as_mut())
    }

    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        if let Some(c) = child {
            self.shifted.set_child(c);
        } else {
            self.shifted.take_child();
        }
    }
}

// ============================================================================
// RenderShiftedBox trait implementation
// ============================================================================

impl RenderShiftedBox for RenderPadding {
    fn child_offset(&self) -> Offset {
        self.shifted.offset()
    }
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
    fn test_padding_child_offset() {
        let mut padding = RenderPadding::new(EdgeInsets::new(10.0, 20.0, 30.0, 40.0));
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);

        padding.perform_layout(constraints);

        let offset = padding.child_offset();
        assert_eq!(offset.dx, 10.0);
        assert_eq!(offset.dy, 20.0);
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
        let shifted: &dyn RenderShiftedBox = &padding;
        assert_eq!(shifted.child_offset(), Offset::new(10.0, 10.0));
    }

    #[test]
    fn test_render_padding_boxed() {
        let padding: Box<dyn RenderBox> = Box::new(RenderPadding::new(EdgeInsets::all(10.0)));
        // Should work - can be stored as Box<dyn RenderBox>
        assert_eq!(padding.size(), Size::ZERO);
    }
}
