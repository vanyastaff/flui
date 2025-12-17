//! RenderAlign - positions its child according to an alignment.
//!
//! This render object aligns its child within its own bounds using
//! `Alignment` coordinates. It can optionally scale the child's size
//! using width and height factors.
//!
//! # Flutter Hierarchy
//!
//! ```text
//! RenderObject
//!     └── RenderBox
//!         └── SingleChildRenderBox
//!             └── RenderShiftedBox
//!                 └── RenderAligningShiftedBox
//!                     └── RenderPositionedBox (this is RenderAlign in FLUI)
//! ```
//!
//! # Architecture
//!
//! Following Flutter's pattern:
//! - Child offset is stored in `child.parentData.offset` (not locally)
//! - Parent sets offset via `set_child_offset(child, offset)` during layout
//! - `paint()` and `hitTestChildren()` read offset from child.parentData

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_types::{Alignment, Offset, Size};

use crate::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::lifecycle::BaseRenderObject;
use crate::parent_data::BoxParentData;
use crate::pipeline::{PaintingContext, PipelineOwner};
use crate::traits::{
    set_child_offset, BoxHitTestResult, RenderAligningShiftedBox, RenderBox, RenderObject,
    RenderShiftedBox, SingleChildRenderBox, TextBaseline,
};
// TextDirection is defined in aligning_shifted_box.rs
pub use crate::traits::r#box::TextDirection;

/// A render object that aligns its child within itself.
///
/// The child is positioned according to the alignment property.
/// If width/height factors are provided, the child is given those
/// factors of the available space.
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderPositionedBox` which extends
/// `RenderAligningShiftedBox`. The key difference from Flutter:
/// - In Flutter: `RenderPositionedBox extends RenderAligningShiftedBox`
/// - In FLUI: `RenderAlign` implements all traits in the chain
///
/// # Trait Chain
///
/// RenderObject → RenderBox → SingleChildRenderBox → RenderShiftedBox → RenderAligningShiftedBox
///
/// # Polymorphism
///
/// `RenderAlign` can be used as:
/// - `Box<dyn RenderObject>` - for generic render tree operations
/// - `Box<dyn RenderBox>` - for box layout operations
/// - `Box<dyn SingleChildRenderBox>` - for single child operations
/// - `Box<dyn RenderShiftedBox>` - for shifted box operations
/// - `Box<dyn RenderAligningShiftedBox>` - for alignment operations
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderAlign;
/// use flui_types::Alignment;
///
/// // Center alignment
/// let mut align = RenderAlign::new(Alignment::CENTER);
///
/// // Top-right with 50% width
/// let mut align = RenderAlign::new(Alignment::TOP_RIGHT)
///     .with_width_factor(0.5);
/// ```
#[derive(Debug)]
pub struct RenderAlign {
    /// Base render object for lifecycle management.
    base: BaseRenderObject,

    /// Single child using type-safe container.
    child: BoxChild,

    /// Cached size from layout.
    size: Size,

    /// Alignment used to position the child.
    alignment: Alignment,

    /// Text direction for resolving directional alignments.
    text_direction: Option<TextDirection>,

    /// Width factor - if set, width = child_width * factor.
    width_factor: Option<f32>,

    /// Height factor - if set, height = child_height * factor.
    height_factor: Option<f32>,
}

impl Default for RenderAlign {
    fn default() -> Self {
        Self::new(Alignment::CENTER)
    }
}

impl RenderAlign {
    /// Creates a new render align with the given alignment.
    pub fn new(alignment: Alignment) -> Self {
        Self {
            base: BaseRenderObject::new(),
            child: BoxChild::new(),
            size: Size::ZERO,
            alignment,
            text_direction: None,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Creates a new render align with a child.
    pub fn with_child(alignment: Alignment, child: Box<dyn RenderBox>) -> Self {
        let mut child = child;
        Self::setup_child_parent_data(&mut *child);
        Self {
            base: BaseRenderObject::new(),
            child: BoxChild::with(child),
            size: Size::ZERO,
            alignment,
            text_direction: None,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Builder method to set width factor.
    pub fn with_width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Builder method to set height factor.
    pub fn with_height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Builder method to set text direction.
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.text_direction = Some(direction);
        self
    }

    /// Returns constraints for the child (loosened).
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.loosen()
    }

    /// Sets up BoxParentData on a child.
    fn setup_child_parent_data(child: &mut dyn RenderBox) {
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

impl RenderObject for RenderAlign {
    fn base(&self) -> &BaseRenderObject {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        &mut self.base
    }

    fn owner(&self) -> Option<&PipelineOwner> {
        None
    }

    fn attach(&mut self, owner: &PipelineOwner) {
        if let Some(child) = self.child.get_mut() {
            child.attach(owner);
        }
    }

    fn detach(&mut self) {
        if let Some(child) = self.child.get_mut() {
            child.detach();
        }
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {}

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {}

    fn redepth_child(&mut self, _child: &mut dyn RenderObject) {}

    fn mark_parent_needs_layout(&mut self) {}

    fn schedule_initial_layout(&mut self) {}

    fn schedule_initial_paint(&mut self) {}

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
}

// ============================================================================
// RenderBox trait implementation
// ============================================================================

impl RenderBox for RenderAlign {
    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Performs layout following Flutter's RenderPositionedBox.performLayout.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void performLayout() {
    ///   final bool shrinkWrapWidth = _widthFactor != null || constraints.maxWidth == double.infinity;
    ///   final bool shrinkWrapHeight = _heightFactor != null || constraints.maxHeight == double.infinity;
    ///   if (child != null) {
    ///     child!.layout(constraints.loosen(), parentUsesSize: true);
    ///     size = constraints.constrain(Size(
    ///       shrinkWrapWidth ? child!.size.width * (_widthFactor ?? 1.0) : double.infinity,
    ///       shrinkWrapHeight ? child!.size.height * (_heightFactor ?? 1.0) : double.infinity,
    ///     ));
    ///     alignChild();
    ///   } else {
    ///     size = constraints.constrain(Size(
    ///       shrinkWrapWidth ? 0.0 : double.infinity,
    ///       shrinkWrapHeight ? 0.0 : double.infinity,
    ///     ));
    ///   }
    /// }
    /// ```
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let shrink_wrap_width =
            self.width_factor.is_some() || constraints.max_width == f32::INFINITY;
        let shrink_wrap_height =
            self.height_factor.is_some() || constraints.max_height == f32::INFINITY;

        // Copy alignment before borrowing child mutably
        let resolved_alignment = self.resolved_alignment();
        let width_factor = self.width_factor;
        let height_factor = self.height_factor;

        if let Some(child) = self.child.get_mut() {
            // Layout child with loosened constraints
            let child_constraints = constraints.loosen();
            let child_size = child.perform_layout(child_constraints);

            // Compute our size
            let my_size = Size::new(
                if shrink_wrap_width {
                    child_size.width * width_factor.unwrap_or(1.0)
                } else {
                    f32::INFINITY
                },
                if shrink_wrap_height {
                    child_size.height * height_factor.unwrap_or(1.0)
                } else {
                    f32::INFINITY
                },
            );

            let constrained_size = constraints.constrain(my_size);
            self.size = constrained_size;

            // alignChild() - computes and sets child offset in child's parentData
            // Use resolved_alignment directly to avoid borrowing self
            let offset = resolved_alignment.along_offset(Offset::new(
                constrained_size.width - child_size.width,
                constrained_size.height - child_size.height,
            ));
            set_child_offset(child, offset);
        } else {
            // No child
            self.size = constraints.constrain(Size::new(
                if shrink_wrap_width {
                    0.0
                } else {
                    f32::INFINITY
                },
                if shrink_wrap_height {
                    0.0
                } else {
                    f32::INFINITY
                },
            ));
        }

        self.size
    }

    fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Use RenderShiftedBox default implementation which reads from child.parentData.offset
        self.shifted_paint(context, offset);
    }

    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Use RenderShiftedBox implementation which reads offset from child.parentData
        self.shifted_hit_test_children(result, position)
    }

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        if let Some(child) = self.child.get() {
            child.compute_min_intrinsic_width(height) * self.width_factor.unwrap_or(1.0)
        } else {
            0.0
        }
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        if let Some(child) = self.child.get() {
            child.compute_max_intrinsic_width(height) * self.width_factor.unwrap_or(1.0)
        } else {
            0.0
        }
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        if let Some(child) = self.child.get() {
            child.compute_min_intrinsic_height(width) * self.height_factor.unwrap_or(1.0)
        } else {
            0.0
        }
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        if let Some(child) = self.child.get() {
            child.compute_max_intrinsic_height(width) * self.height_factor.unwrap_or(1.0)
        } else {
            0.0
        }
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        // Use RenderShiftedBox implementation which reads offset from child.parentData
        self.shifted_compute_distance_to_actual_baseline(baseline)
    }
}

// ============================================================================
// SingleChildRenderBox trait implementation
// ============================================================================

impl SingleChildRenderBox for RenderAlign {
    fn child(&self) -> Option<&dyn RenderBox> {
        self.child.get()
    }

    fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.get_mut()
    }

    fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child.clear();

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

impl RenderShiftedBox for RenderAlign {
    // All methods use default implementations from the trait which read
    // offset from child.parentData.offset
}

// ============================================================================
// RenderAligningShiftedBox trait implementation
// ============================================================================

impl RenderAligningShiftedBox for RenderAlign {
    fn alignment(&self) -> Alignment {
        self.alignment
    }

    fn set_alignment(&mut self, alignment: Alignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
            self.mark_needs_layout();
        }
    }

    fn text_direction(&self) -> Option<TextDirection> {
        self.text_direction
    }

    fn set_text_direction(&mut self, direction: Option<TextDirection>) {
        if self.text_direction != direction {
            self.text_direction = direction;
            self.mark_needs_layout();
        }
    }

    fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_default() {
        let align = RenderAlign::default();
        assert_eq!(align.alignment(), Alignment::CENTER);
        assert!(align.child().is_none());
    }

    #[test]
    fn test_align_center() {
        let mut align = RenderAlign::new(Alignment::CENTER);
        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));

        let size = align.perform_layout(constraints);

        assert_eq!(size, Size::new(200.0, 200.0));
    }

    #[test]
    fn test_align_with_factors() {
        let align = RenderAlign::new(Alignment::CENTER)
            .with_width_factor(0.5)
            .with_height_factor(0.5);

        assert_eq!(align.width_factor(), Some(0.5));
        assert_eq!(align.height_factor(), Some(0.5));
    }

    #[test]
    fn test_align_no_child() {
        let mut align = RenderAlign::new(Alignment::TOP_LEFT);
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = align.perform_layout(constraints);

        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_computed_offset_center() {
        let align = RenderAlign::new(Alignment::CENTER);

        // Center a 50x50 child in a 100x100 parent
        let offset = align.compute_aligned_offset(Size::new(100.0, 100.0), Size::new(50.0, 50.0));

        assert_eq!(offset.dx, 25.0);
        assert_eq!(offset.dy, 25.0);
    }

    #[test]
    fn test_computed_offset_top_left() {
        let align = RenderAlign::new(Alignment::TOP_LEFT);

        let offset = align.compute_aligned_offset(Size::new(100.0, 100.0), Size::new(50.0, 50.0));

        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 0.0);
    }

    #[test]
    fn test_computed_offset_bottom_right() {
        let align = RenderAlign::new(Alignment::BOTTOM_RIGHT);

        let offset = align.compute_aligned_offset(Size::new(100.0, 100.0), Size::new(50.0, 50.0));

        assert_eq!(offset.dx, 50.0);
        assert_eq!(offset.dy, 50.0);
    }

    #[test]
    fn test_trait_polymorphism() {
        let align = RenderAlign::new(Alignment::CENTER);

        // Should compile - RenderAlign implements all these traits
        let _: &dyn RenderObject = &align;
        let _: &dyn RenderBox = &align;
        let _: &dyn SingleChildRenderBox = &align;
        let _: &dyn RenderShiftedBox = &align;
        let _: &dyn RenderAligningShiftedBox = &align;
    }

    #[test]
    fn test_rtl_alignment() {
        let align = RenderAlign::new(Alignment::new(1.0, 0.0)) // Right aligned
            .with_text_direction(TextDirection::Rtl);

        // RTL flips the x alignment
        let resolved = align.resolved_alignment();
        assert_eq!(resolved.x, -1.0); // Flipped to left
        assert_eq!(resolved.y, 0.0);
    }
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl Diagnosticable for RenderAlign {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("alignment", format!("{:?}", self.alignment));
        if let Some(wf) = self.width_factor {
            properties.add("widthFactor", wf);
        }
        if let Some(hf) = self.height_factor {
            properties.add("heightFactor", hf);
        }
    }
}

impl HitTestTarget for RenderAlign {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        RenderObject::handle_event(self, event, entry);
    }
}
