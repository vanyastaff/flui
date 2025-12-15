//! RenderFlex - linear layout for Row and Column.
//!
//! This render object arranges children in a single row or column,
//! with flexible sizing based on flex factors.

use flui_types::{Offset, Point, Rect, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ChildList;
use crate::parent_data::FlexParentData;
use crate::pipeline::PaintingContext;
use crate::protocol::BoxProtocol;
use crate::traits::TextBaseline;
use flui_tree::arity::Variable;

/// The direction children are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Axis {
    /// Lay out horizontally.
    #[default]
    Horizontal,
    /// Lay out vertically.
    Vertical,
}

impl Axis {
    /// Returns the perpendicular axis.
    pub fn flip(&self) -> Self {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }
}

/// How children are placed along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxisAlignment {
    /// Place children at the start.
    #[default]
    Start,
    /// Place children at the end.
    End,
    /// Place children at the center.
    Center,
    /// Place free space evenly between children.
    SpaceBetween,
    /// Place free space evenly around children.
    SpaceAround,
    /// Place free space evenly, including before first and after last.
    SpaceEvenly,
}

/// How children are sized along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxisSize {
    /// Minimize the amount of free space.
    Min,
    /// Maximize the amount of free space.
    #[default]
    Max,
}

/// How children are placed along the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    /// Place children at the start.
    Start,
    /// Place children at the end.
    End,
    /// Place children at the center.
    #[default]
    Center,
    /// Stretch children to fill the cross axis.
    Stretch,
    /// Align baselines.
    Baseline,
}

/// How to handle children that overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalDirection {
    /// Children start at the top and grow downward.
    #[default]
    Down,
    /// Children start at the bottom and grow upward.
    Up,
}

/// How to lay out children that exceed available space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexFit {
    /// The child is forced to fill available space.
    Tight,
    /// The child can be at most as large as available space.
    #[default]
    Loose,
}

/// A render object that arranges children in a linear layout.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::layout::{RenderFlex, Axis, MainAxisAlignment};
///
/// // Create a row with children spaced evenly
/// let mut flex = RenderFlex::row();
/// flex.set_main_axis_alignment(MainAxisAlignment::SpaceEvenly);
///
/// // Create a column
/// let flex = RenderFlex::column();
/// ```
#[derive(Debug)]
pub struct RenderFlex {
    /// Container for children.
    children: ChildList<BoxProtocol, Variable, FlexParentData>,

    /// The direction to lay out children.
    direction: Axis,

    /// How children are placed along the main axis.
    main_axis_alignment: MainAxisAlignment,

    /// How much space to take along the main axis.
    main_axis_size: MainAxisSize,

    /// How children are placed along the cross axis.
    cross_axis_alignment: CrossAxisAlignment,

    /// The text direction for start/end alignment.
    text_direction: TextDirection,

    /// The vertical direction for up/down orientation.
    vertical_direction: VerticalDirection,

    /// The baseline to use for baseline alignment.
    text_baseline: Option<TextBaseline>,

    /// Cached size.
    size: Size,

    /// Cached overflow amount.
    overflow: f32,
}

/// Text direction for horizontal layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Left to right.
    #[default]
    Ltr,
    /// Right to left.
    Rtl,
}

impl RenderFlex {
    /// Creates a new flex layout with the given direction.
    pub fn new(direction: Axis) -> Self {
        Self {
            children: ChildList::new(),
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            main_axis_size: MainAxisSize::Max,
            cross_axis_alignment: CrossAxisAlignment::Center,
            text_direction: TextDirection::Ltr,
            vertical_direction: VerticalDirection::Down,
            text_baseline: None,
            size: Size::ZERO,
            overflow: 0.0,
        }
    }

    /// Creates a horizontal flex (Row).
    pub fn row() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Creates a vertical flex (Column).
    pub fn column() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Returns the direction.
    pub fn direction(&self) -> Axis {
        self.direction
    }

    /// Sets the direction.
    pub fn set_direction(&mut self, direction: Axis) {
        if self.direction != direction {
            self.direction = direction;
        }
    }

    /// Returns main axis alignment.
    pub fn main_axis_alignment(&self) -> MainAxisAlignment {
        self.main_axis_alignment
    }

    /// Sets main axis alignment.
    pub fn set_main_axis_alignment(&mut self, alignment: MainAxisAlignment) {
        if self.main_axis_alignment != alignment {
            self.main_axis_alignment = alignment;
        }
    }

    /// Returns main axis size.
    pub fn main_axis_size(&self) -> MainAxisSize {
        self.main_axis_size
    }

    /// Sets main axis size.
    pub fn set_main_axis_size(&mut self, size: MainAxisSize) {
        if self.main_axis_size != size {
            self.main_axis_size = size;
        }
    }

    /// Returns cross axis alignment.
    pub fn cross_axis_alignment(&self) -> CrossAxisAlignment {
        self.cross_axis_alignment
    }

    /// Sets cross axis alignment.
    pub fn set_cross_axis_alignment(&mut self, alignment: CrossAxisAlignment) {
        if self.cross_axis_alignment != alignment {
            self.cross_axis_alignment = alignment;
        }
    }

    /// Returns the text direction.
    pub fn text_direction(&self) -> TextDirection {
        self.text_direction
    }

    /// Sets the text direction.
    pub fn set_text_direction(&mut self, direction: TextDirection) {
        if self.text_direction != direction {
            self.text_direction = direction;
        }
    }

    /// Returns the vertical direction.
    pub fn vertical_direction(&self) -> VerticalDirection {
        self.vertical_direction
    }

    /// Sets the vertical direction.
    pub fn set_vertical_direction(&mut self, direction: VerticalDirection) {
        if self.vertical_direction != direction {
            self.vertical_direction = direction;
        }
    }

    /// Returns the text baseline.
    pub fn text_baseline(&self) -> Option<TextBaseline> {
        self.text_baseline
    }

    /// Sets the text baseline.
    pub fn set_text_baseline(&mut self, baseline: Option<TextBaseline>) {
        if self.text_baseline != baseline {
            self.text_baseline = baseline;
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the overflow amount (if children exceed available space).
    pub fn overflow(&self) -> f32 {
        self.overflow
    }

    /// Returns whether there is overflow.
    pub fn has_overflow(&self) -> bool {
        self.overflow > 0.0
    }

    /// Gets the main axis extent from a size.
    fn get_main_size(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        }
    }

    /// Gets the cross axis extent from a size.
    fn get_cross_size(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.height,
            Axis::Vertical => size.width,
        }
    }

    /// Creates a size from main and cross extents.
    fn create_size(&self, main: f32, cross: f32) -> Size {
        match self.direction {
            Axis::Horizontal => Size::new(main, cross),
            Axis::Vertical => Size::new(cross, main),
        }
    }

    /// Gets the main axis constraint.
    fn get_main_constraint(&self, constraints: BoxConstraints) -> (f32, f32) {
        match self.direction {
            Axis::Horizontal => (constraints.min_width, constraints.max_width),
            Axis::Vertical => (constraints.min_height, constraints.max_height),
        }
    }

    /// Gets the cross axis constraint.
    fn get_cross_constraint(&self, constraints: BoxConstraints) -> (f32, f32) {
        match self.direction {
            Axis::Horizontal => (constraints.min_height, constraints.max_height),
            Axis::Vertical => (constraints.min_width, constraints.max_width),
        }
    }

    /// Performs layout with provided child sizes and flex factors.
    ///
    /// This is a simplified layout that takes pre-computed child data.
    pub fn perform_layout_with_children(
        &mut self,
        constraints: BoxConstraints,
        child_data: &[(Size, FlexParentData)],
    ) -> (Size, Vec<Offset>) {
        let (min_main, max_main) = self.get_main_constraint(constraints);
        let (min_cross, max_cross) = self.get_cross_constraint(constraints);

        // Calculate total flex and fixed size
        let mut total_flex: f32 = 0.0;
        let mut allocated_size: f32 = 0.0;
        let mut cross_size: f32 = 0.0;

        for (size, parent_data) in child_data {
            if let Some(flex) = parent_data.flex {
                if flex > 0 {
                    total_flex += flex as f32;
                }
            } else {
                allocated_size += self.get_main_size(*size);
            }
            cross_size = cross_size.max(self.get_cross_size(*size));
        }

        // Calculate remaining space for flex children
        let free_space = (max_main - allocated_size).max(0.0);
        let space_per_flex = if total_flex > 0.0 {
            free_space / total_flex
        } else {
            0.0
        };

        // Calculate actual sizes including flex
        let mut actual_sizes: Vec<f32> = Vec::with_capacity(child_data.len());
        let mut total_main: f32 = 0.0;

        for (size, parent_data) in child_data {
            let main_size = if let Some(flex) = parent_data.flex {
                if flex > 0 {
                    space_per_flex * flex as f32
                } else {
                    self.get_main_size(*size)
                }
            } else {
                self.get_main_size(*size)
            };
            actual_sizes.push(main_size);
            total_main += main_size;
        }

        // Calculate our size
        let ideal_main = match self.main_axis_size {
            MainAxisSize::Max => max_main,
            MainAxisSize::Min => total_main,
        };
        let actual_main = ideal_main.clamp(min_main, max_main);

        let cross = if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
            max_cross
        } else {
            cross_size.clamp(min_cross, max_cross)
        };

        self.size = self.create_size(actual_main, cross);
        self.overflow = (total_main - actual_main).max(0.0);

        // Calculate child offsets
        let remaining_space = (actual_main - total_main).max(0.0);
        let child_count = child_data.len();

        let (leading_space, between_space) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (0.0, 0.0),
            MainAxisAlignment::End => (remaining_space, 0.0),
            MainAxisAlignment::Center => (remaining_space / 2.0, 0.0),
            MainAxisAlignment::SpaceBetween => {
                if child_count > 1 {
                    (0.0, remaining_space / (child_count - 1) as f32)
                } else {
                    (0.0, 0.0)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = remaining_space / child_count as f32;
                (space / 2.0, space)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = remaining_space / (child_count + 1) as f32;
                (space, space)
            }
        };

        let mut offsets: Vec<Offset> = Vec::with_capacity(child_count);
        let mut main_position = leading_space;

        // Handle direction reversal
        let flip_main = match self.direction {
            Axis::Horizontal => self.text_direction == TextDirection::Rtl,
            Axis::Vertical => self.vertical_direction == VerticalDirection::Up,
        };

        for (i, ((size, _parent_data), &main_extent)) in
            child_data.iter().zip(actual_sizes.iter()).enumerate()
        {
            // Calculate cross position
            let child_cross = self.get_cross_size(*size);
            let cross_position = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => 0.0,
                CrossAxisAlignment::End => cross - child_cross,
                CrossAxisAlignment::Center => (cross - child_cross) / 2.0,
                CrossAxisAlignment::Stretch => 0.0,
                CrossAxisAlignment::Baseline => 0.0, // Simplified - would need baseline info
            };

            let offset = if flip_main {
                let reversed_main = actual_main - main_position - main_extent;
                match self.direction {
                    Axis::Horizontal => Offset::new(reversed_main, cross_position),
                    Axis::Vertical => Offset::new(cross_position, reversed_main),
                }
            } else {
                match self.direction {
                    Axis::Horizontal => Offset::new(main_position, cross_position),
                    Axis::Vertical => Offset::new(cross_position, main_position),
                }
            };

            offsets.push(offset);

            if i < child_count - 1 {
                main_position += main_extent + between_space;
            }
        }

        (self.size, offsets)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // In real implementation, would iterate children and paint each at its offset
        // If overflow, would clip
        let _ = (context, offset);
    }

    /// Hit tests at the given position.
    pub fn hit_test(&self, position: Offset) -> bool {
        let rect = Rect::from_origin_size(Point::ZERO, self.size);
        rect.contains(Point::new(position.dx, position.dy))
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_widths: &[f32]) -> f32 {
        match self.direction {
            Axis::Horizontal => {
                // Sum of all children's min widths
                child_widths.iter().sum()
            }
            Axis::Vertical => {
                // Max of all children's min widths
                let _ = height;
                child_widths.iter().cloned().fold(0.0_f32, f32::max)
            }
        }
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_widths: &[f32]) -> f32 {
        self.compute_min_intrinsic_width(height, child_widths)
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, width: f32, child_heights: &[f32]) -> f32 {
        match self.direction {
            Axis::Horizontal => {
                // Max of all children's min heights
                let _ = width;
                child_heights.iter().cloned().fold(0.0_f32, f32::max)
            }
            Axis::Vertical => {
                // Sum of all children's min heights
                child_heights.iter().sum()
            }
        }
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_heights: &[f32]) -> f32 {
        self.compute_min_intrinsic_height(width, child_heights)
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        baseline: TextBaseline,
        first_child_baseline: Option<f32>,
    ) -> Option<f32> {
        let _ = baseline;
        first_child_baseline
    }
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self::row()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_row() {
        let flex = RenderFlex::row();
        assert_eq!(flex.direction(), Axis::Horizontal);
    }

    #[test]
    fn test_flex_column() {
        let flex = RenderFlex::column();
        assert_eq!(flex.direction(), Axis::Vertical);
    }

    #[test]
    fn test_axis_flip() {
        assert_eq!(Axis::Horizontal.flip(), Axis::Vertical);
        assert_eq!(Axis::Vertical.flip(), Axis::Horizontal);
    }

    #[test]
    fn test_main_axis_size() {
        let flex = RenderFlex::row();
        assert_eq!(flex.main_axis_size(), MainAxisSize::Max);
    }

    #[test]
    fn test_cross_axis_alignment() {
        let flex = RenderFlex::row();
        assert_eq!(flex.cross_axis_alignment(), CrossAxisAlignment::Center);
    }

    #[test]
    fn test_get_main_size() {
        let row = RenderFlex::row();
        let col = RenderFlex::column();
        let size = Size::new(100.0, 50.0);

        assert_eq!(row.get_main_size(size), 100.0);
        assert_eq!(col.get_main_size(size), 50.0);
    }

    #[test]
    fn test_get_cross_size() {
        let row = RenderFlex::row();
        let col = RenderFlex::column();
        let size = Size::new(100.0, 50.0);

        assert_eq!(row.get_cross_size(size), 50.0);
        assert_eq!(col.get_cross_size(size), 100.0);
    }

    #[test]
    fn test_create_size() {
        let row = RenderFlex::row();
        let col = RenderFlex::column();

        assert_eq!(row.create_size(100.0, 50.0), Size::new(100.0, 50.0));
        assert_eq!(col.create_size(100.0, 50.0), Size::new(50.0, 100.0));
    }

    #[test]
    fn test_layout_start_alignment() {
        let mut flex = RenderFlex::row();
        flex.set_main_axis_alignment(MainAxisAlignment::Start);

        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        let children = vec![
            (Size::new(50.0, 40.0), FlexParentData::default()),
            (Size::new(50.0, 40.0), FlexParentData::default()),
        ];

        let (size, offsets) = flex.perform_layout_with_children(constraints, &children);

        assert_eq!(size, Size::new(300.0, 100.0));
        assert_eq!(offsets.len(), 2);
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_center_alignment() {
        let mut flex = RenderFlex::row();
        flex.set_main_axis_alignment(MainAxisAlignment::Center);

        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        let children = vec![
            (Size::new(50.0, 40.0), FlexParentData::default()),
            (Size::new(50.0, 40.0), FlexParentData::default()),
        ];

        let (_, offsets) = flex.perform_layout_with_children(constraints, &children);

        // Total width 100, remaining 200, so start at 100
        assert!((offsets[0].dx - 100.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_space_between() {
        let mut flex = RenderFlex::row();
        flex.set_main_axis_alignment(MainAxisAlignment::SpaceBetween);

        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        let children = vec![
            (Size::new(50.0, 40.0), FlexParentData::default()),
            (Size::new(50.0, 40.0), FlexParentData::default()),
        ];

        let (_, offsets) = flex.perform_layout_with_children(constraints, &children);

        // First at start, second at end minus its width
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 250.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_with_flex() {
        let mut flex = RenderFlex::row();

        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        let children = vec![
            (Size::new(50.0, 40.0), FlexParentData::default()), // No flex
            (
                Size::new(0.0, 40.0),
                FlexParentData {
                    flex: Some(1),
                    ..Default::default()
                },
            ), // Flex 1
        ];

        let (_, offsets) = flex.perform_layout_with_children(constraints, &children);

        // First child at 0, flex child takes remaining 250
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_column() {
        let mut flex = RenderFlex::column();

        let constraints = BoxConstraints::tight(Size::new(100.0, 300.0));
        let children = vec![
            (Size::new(40.0, 50.0), FlexParentData::default()),
            (Size::new(40.0, 50.0), FlexParentData::default()),
        ];

        let (_, offsets) = flex.perform_layout_with_children(constraints, &children);

        // Check Y positions
        assert!((offsets[0].dy - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dy - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cross_axis_center() {
        let mut flex = RenderFlex::row();
        flex.set_cross_axis_alignment(CrossAxisAlignment::Center);

        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        let children = vec![(Size::new(50.0, 40.0), FlexParentData::default())];

        let (_, offsets) = flex.perform_layout_with_children(constraints, &children);

        // Child centered vertically: (100 - 40) / 2 = 30
        assert!((offsets[0].dy - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_no_overflow() {
        let mut flex = RenderFlex::row();

        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        let children = vec![
            (Size::new(100.0, 40.0), FlexParentData::default()),
            (Size::new(100.0, 40.0), FlexParentData::default()),
        ];

        flex.perform_layout_with_children(constraints, &children);

        assert!(!flex.has_overflow());
    }

    #[test]
    fn test_hit_test() {
        let mut flex = RenderFlex::row();
        let constraints = BoxConstraints::tight(Size::new(300.0, 100.0));
        flex.perform_layout_with_children(constraints, &[]);

        assert!(flex.hit_test(Offset::new(150.0, 50.0)));
        assert!(!flex.hit_test(Offset::new(350.0, 50.0)));
    }
}
