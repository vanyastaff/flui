//! RenderStack - stacks children on the Z-axis.
//!
//! Children can be positioned absolutely or sized to fit the stack.

use flui_types::{Offset, Point, Rect, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ChildList;
use crate::objects::r#box::effects::fitted_box::FittedAlignment;
use crate::parent_data::StackParentData;
use crate::pipeline::PaintingContext;
use crate::protocol::BoxProtocol;
use crate::traits::TextBaseline;
use flui_tree::arity::Variable;

/// How to size the non-positioned children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StackFit {
    /// Non-positioned children get loose constraints (0 to max).
    #[default]
    Loose,
    /// Non-positioned children get tight constraints (expand to fill).
    Expand,
    /// Non-positioned children get passed-through constraints.
    Passthrough,
}

/// How to handle children that overflow the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StackClipBehavior {
    /// Don't clip overflowing children.
    None,
    /// Clip overflowing children without anti-aliasing.
    HardEdge,
    /// Clip overflowing children with anti-aliasing.
    #[default]
    AntiAlias,
}

/// A render object that stacks children on top of each other.
///
/// Children are painted in order, with later children appearing on top.
/// Children can be positioned absolutely using StackParentData, or they
/// can be aligned using the stack's alignment.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::layout::{RenderStack, StackFit};
///
/// let mut stack = RenderStack::new();
/// stack.set_fit(StackFit::Expand);
/// ```
#[derive(Debug)]
pub struct RenderStack {
    /// Container for children.
    children: ChildList<BoxProtocol, Variable, StackParentData>,

    /// Alignment for non-positioned children.
    alignment: FittedAlignment,

    /// How to size non-positioned children.
    fit: StackFit,

    /// How to clip overflowing children.
    clip_behavior: StackClipBehavior,

    /// Cached size.
    size: Size,

    /// Whether there's overflow.
    has_overflow: bool,
}

impl RenderStack {
    /// Creates a new stack.
    pub fn new() -> Self {
        Self {
            children: ChildList::new(),
            alignment: FittedAlignment::TOP_LEFT,
            fit: StackFit::Loose,
            clip_behavior: StackClipBehavior::AntiAlias,
            size: Size::ZERO,
            has_overflow: false,
        }
    }

    /// Creates with custom alignment.
    pub fn with_alignment(alignment: FittedAlignment) -> Self {
        Self {
            children: ChildList::new(),
            alignment,
            fit: StackFit::Loose,
            clip_behavior: StackClipBehavior::AntiAlias,
            size: Size::ZERO,
            has_overflow: false,
        }
    }

    /// Returns the alignment.
    pub fn alignment(&self) -> FittedAlignment {
        self.alignment
    }

    /// Sets the alignment.
    pub fn set_alignment(&mut self, alignment: FittedAlignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
        }
    }

    /// Returns the fit mode.
    pub fn fit(&self) -> StackFit {
        self.fit
    }

    /// Sets the fit mode.
    pub fn set_fit(&mut self, fit: StackFit) {
        if self.fit != fit {
            self.fit = fit;
        }
    }

    /// Returns the clip behavior.
    pub fn clip_behavior(&self) -> StackClipBehavior {
        self.clip_behavior
    }

    /// Sets the clip behavior.
    pub fn set_clip_behavior(&mut self, behavior: StackClipBehavior) {
        if self.clip_behavior != behavior {
            self.clip_behavior = behavior;
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns whether children overflow.
    pub fn has_overflow(&self) -> bool {
        self.has_overflow
    }

    /// Computes constraints for non-positioned children.
    fn get_inner_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        match self.fit {
            StackFit::Loose => constraints.loosen(),
            StackFit::Expand => {
                BoxConstraints::tight(Size::new(constraints.max_width, constraints.max_height))
            }
            StackFit::Passthrough => constraints,
        }
    }

    /// Computes the offset for a positioned child.
    fn compute_positioned_offset(
        &self,
        stack_size: Size,
        child_size: Size,
        parent_data: &StackParentData,
    ) -> Offset {
        let mut x = 0.0;
        let mut y = 0.0;

        // Horizontal positioning
        if let Some(left) = parent_data.left {
            x = left;
        } else if let Some(right) = parent_data.right {
            x = stack_size.width - right - child_size.width;
        } else {
            // Use alignment
            x = self.alignment.along_offset(stack_size, child_size).dx;
        }

        // Vertical positioning
        if let Some(top) = parent_data.top {
            y = top;
        } else if let Some(bottom) = parent_data.bottom {
            y = stack_size.height - bottom - child_size.height;
        } else {
            // Use alignment
            y = self.alignment.along_offset(stack_size, child_size).dy;
        }

        Offset::new(x, y)
    }

    /// Computes constraints for a positioned child.
    fn compute_positioned_constraints(
        &self,
        stack_size: Size,
        parent_data: &StackParentData,
    ) -> BoxConstraints {
        let mut min_width = 0.0;
        let mut max_width = stack_size.width;
        let mut min_height = 0.0;
        let mut max_height = stack_size.height;

        // If both left and right are set, width is fixed
        if let (Some(left), Some(right)) = (parent_data.left, parent_data.right) {
            let width = (stack_size.width - left - right).max(0.0);
            min_width = width;
            max_width = width;
        }

        // If both top and bottom are set, height is fixed
        if let (Some(top), Some(bottom)) = (parent_data.top, parent_data.bottom) {
            let height = (stack_size.height - top - bottom).max(0.0);
            min_height = height;
            max_height = height;
        }

        // Apply explicit width/height
        if let Some(w) = parent_data.width {
            min_width = w;
            max_width = w;
        }
        if let Some(h) = parent_data.height {
            min_height = h;
            max_height = h;
        }

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Performs layout with provided child data.
    pub fn perform_layout_with_children(
        &mut self,
        constraints: BoxConstraints,
        child_data: &[(Size, StackParentData)],
    ) -> (Size, Vec<Offset>) {
        // First pass: lay out non-positioned children to determine stack size
        let inner_constraints = self.get_inner_constraints(constraints);
        let mut width: f32 = 0.0;
        let mut height: f32 = 0.0;

        for (size, parent_data) in child_data {
            if !parent_data.is_positioned() {
                width = width.max(size.width);
                height = height.max(size.height);
            }
        }

        // If no non-positioned children, use constraints
        if width == 0.0 && height == 0.0 {
            width = inner_constraints.max_width;
            height = inner_constraints.max_height;
        }

        let stack_size = constraints.constrain(Size::new(width, height));
        self.size = stack_size;

        // Second pass: compute offsets for all children
        let mut offsets: Vec<Offset> = Vec::with_capacity(child_data.len());
        self.has_overflow = false;

        for (size, parent_data) in child_data {
            let offset = if parent_data.is_positioned() {
                self.compute_positioned_offset(stack_size, *size, parent_data)
            } else {
                self.alignment.along_offset(stack_size, *size)
            };

            // Check for overflow
            if offset.dx < 0.0
                || offset.dy < 0.0
                || offset.dx + size.width > stack_size.width
                || offset.dy + size.height > stack_size.height
            {
                self.has_overflow = true;
            }

            offsets.push(offset);
        }

        (self.size, offsets)
    }

    /// Returns constraints for a positioned child.
    pub fn constraints_for_positioned_child(
        &self,
        parent_data: &StackParentData,
    ) -> BoxConstraints {
        self.compute_positioned_constraints(self.size, parent_data)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Would clip if has_overflow and clip_behavior is not None
        // Then paint all children in order
        let _ = (context, offset);
    }

    /// Hit tests at the given position.
    pub fn hit_test(&self, position: Offset) -> bool {
        let rect = Rect::from_origin_size(Point::ZERO, self.size);
        rect.contains(Point::new(position.dx, position.dy))
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_widths: &[f32]) -> f32 {
        let _ = height;
        child_widths.iter().cloned().fold(0.0_f32, f32::max)
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_widths: &[f32]) -> f32 {
        self.compute_min_intrinsic_width(height, child_widths)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, width: f32, child_heights: &[f32]) -> f32 {
        let _ = width;
        child_heights.iter().cloned().fold(0.0_f32, f32::max)
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_heights: &[f32]) -> f32 {
        self.compute_min_intrinsic_height(width, child_heights)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        _first_child_baseline: Option<f32>,
    ) -> Option<f32> {
        None
    }
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_new() {
        let stack = RenderStack::new();
        assert_eq!(stack.fit(), StackFit::Loose);
        assert_eq!(stack.clip_behavior(), StackClipBehavior::AntiAlias);
    }

    #[test]
    fn test_stack_fit() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);
        assert_eq!(stack.fit(), StackFit::Expand);
    }

    #[test]
    fn test_inner_constraints_loose() {
        let stack = RenderStack::new();
        let constraints = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);

        let inner = stack.get_inner_constraints(constraints);

        assert_eq!(inner.min_width, 0.0);
        assert_eq!(inner.max_width, 100.0);
        assert_eq!(inner.min_height, 0.0);
        assert_eq!(inner.max_height, 200.0);
    }

    #[test]
    fn test_inner_constraints_expand() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);
        let constraints = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);

        let inner = stack.get_inner_constraints(constraints);

        assert_eq!(inner.min_width, 100.0);
        assert_eq!(inner.max_width, 100.0);
        assert_eq!(inner.min_height, 200.0);
        assert_eq!(inner.max_height, 200.0);
    }

    #[test]
    fn test_layout_non_positioned() {
        let mut stack = RenderStack::new();
        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        let children = vec![
            (Size::new(50.0, 50.0), StackParentData::default()),
            (Size::new(100.0, 75.0), StackParentData::default()),
        ];

        let (size, offsets) = stack.perform_layout_with_children(constraints, &children);

        // Size should be constrained to 200x200
        assert_eq!(size, Size::new(200.0, 200.0));

        // Both children should be at top-left (default alignment)
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[0].dy - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dy - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_positioned_left_top() {
        let mut stack = RenderStack::new();
        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        let children = vec![(
            Size::new(50.0, 50.0),
            StackParentData {
                left: Some(10.0),
                top: Some(20.0),
                ..Default::default()
            },
        )];

        let (_, offsets) = stack.perform_layout_with_children(constraints, &children);

        assert!((offsets[0].dx - 10.0).abs() < f32::EPSILON);
        assert!((offsets[0].dy - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_positioned_right_bottom() {
        let mut stack = RenderStack::new();
        let constraints = BoxConstraints::tight(Size::new(200.0, 200.0));
        let children = vec![(
            Size::new(50.0, 50.0),
            StackParentData {
                right: Some(10.0),
                bottom: Some(20.0),
                ..Default::default()
            },
        )];

        let (_, offsets) = stack.perform_layout_with_children(constraints, &children);

        // x = 200 - 10 - 50 = 140
        // y = 200 - 20 - 50 = 130
        assert!((offsets[0].dx - 140.0).abs() < f32::EPSILON);
        assert!((offsets[0].dy - 130.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_positioned_constraints() {
        let mut stack = RenderStack::new();
        stack.size = Size::new(200.0, 200.0);

        let parent_data = StackParentData {
            left: Some(10.0),
            right: Some(20.0),
            top: Some(30.0),
            bottom: Some(40.0),
            ..Default::default()
        };

        let constraints = stack.compute_positioned_constraints(stack.size, &parent_data);

        // Width = 200 - 10 - 20 = 170
        // Height = 200 - 30 - 40 = 130
        assert_eq!(constraints.min_width, 170.0);
        assert_eq!(constraints.max_width, 170.0);
        assert_eq!(constraints.min_height, 130.0);
        assert_eq!(constraints.max_height, 130.0);
    }

    #[test]
    fn test_overflow_detection() {
        let mut stack = RenderStack::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children = vec![(
            Size::new(50.0, 50.0),
            StackParentData {
                left: Some(80.0), // Will overflow right edge
                top: Some(0.0),
                ..Default::default()
            },
        )];

        stack.perform_layout_with_children(constraints, &children);

        assert!(stack.has_overflow());
    }

    #[test]
    fn test_no_overflow() {
        let mut stack = RenderStack::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children = vec![(
            Size::new(50.0, 50.0),
            StackParentData {
                left: Some(10.0),
                top: Some(10.0),
                ..Default::default()
            },
        )];

        stack.perform_layout_with_children(constraints, &children);

        assert!(!stack.has_overflow());
    }

    #[test]
    fn test_hit_test() {
        let mut stack = RenderStack::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        stack.perform_layout_with_children(constraints, &[]);

        assert!(stack.hit_test(Offset::new(50.0, 50.0)));
        assert!(!stack.hit_test(Offset::new(150.0, 50.0)));
    }
}
