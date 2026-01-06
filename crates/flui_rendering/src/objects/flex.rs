//! RenderFlex - lays out children in a row or column.

use flui_types::{Offset, Point, Rect, Size};

use crate::arity::Variable;
use crate::constraints::BoxConstraints;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// Direction of the flex layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexDirection {
    /// Children are laid out horizontally (Row).
    #[default]
    Horizontal,
    /// Children are laid out vertically (Column).
    Vertical,
}

/// How children are aligned along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxisAlignment {
    /// Children are placed at the start.
    #[default]
    Start,
    /// Children are placed at the end.
    End,
    /// Children are centered.
    Center,
    /// Space is distributed evenly between children.
    SpaceBetween,
    /// Space is distributed evenly around children.
    SpaceAround,
    /// Space is distributed evenly, including edges.
    SpaceEvenly,
}

/// How children are aligned along the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    /// Children are aligned at the start.
    #[default]
    Start,
    /// Children are aligned at the end.
    End,
    /// Children are centered.
    Center,
    /// Children are stretched to fill the cross axis.
    Stretch,
}

/// A render object that lays out children in a flex layout (row or column).
///
/// This is a simplified Flex implementation without flex factors.
/// Children are laid out sequentially and positioned according to alignment.
///
/// # Example
///
/// ```ignore
/// // Horizontal row
/// let row = RenderFlex::row();
///
/// // Vertical column with center alignment
/// let column = RenderFlex::column()
///     .with_main_axis_alignment(MainAxisAlignment::Center)
///     .with_cross_axis_alignment(CrossAxisAlignment::Center);
/// ```
#[derive(Debug, Clone)]
pub struct RenderFlex {
    /// Direction of layout.
    direction: FlexDirection,
    /// Main axis alignment.
    main_axis_alignment: MainAxisAlignment,
    /// Cross axis alignment.
    cross_axis_alignment: CrossAxisAlignment,
    /// Spacing between children.
    spacing: f32,
    /// Size after layout.
    size: Size,
    /// Number of children (tracked for hit testing).
    child_count: usize,
    /// Child offsets (tracked for hit testing).
    child_offsets: Vec<Offset>,
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Horizontal,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Start,
            spacing: 0.0,
            size: Size::ZERO,
            child_count: 0,
            child_offsets: Vec::new(),
        }
    }
}

impl RenderFlex {
    /// Creates a new flex with default settings (horizontal).
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a horizontal flex (Row).
    pub fn row() -> Self {
        Self {
            direction: FlexDirection::Horizontal,
            ..Default::default()
        }
    }

    /// Creates a vertical flex (Column).
    pub fn column() -> Self {
        Self {
            direction: FlexDirection::Vertical,
            ..Default::default()
        }
    }

    /// Sets the main axis alignment.
    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Sets the cross axis alignment.
    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Sets the spacing between children.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Returns the direction.
    pub fn direction(&self) -> FlexDirection {
        self.direction
    }

    /// Returns true if this is a horizontal layout.
    pub fn is_horizontal(&self) -> bool {
        self.direction == FlexDirection::Horizontal
    }

    /// Returns true if this is a vertical layout.
    pub fn is_vertical(&self) -> bool {
        self.direction == FlexDirection::Vertical
    }

    /// Extracts main axis extent from a size.
    fn main_size(&self, size: Size) -> f32 {
        match self.direction {
            FlexDirection::Horizontal => size.width,
            FlexDirection::Vertical => size.height,
        }
    }

    /// Extracts cross axis extent from a size.
    fn cross_size(&self, size: Size) -> f32 {
        match self.direction {
            FlexDirection::Horizontal => size.height,
            FlexDirection::Vertical => size.width,
        }
    }

    /// Creates an offset from main and cross values.
    fn offset(&self, main: f32, cross: f32) -> Offset {
        match self.direction {
            FlexDirection::Horizontal => Offset::new(main, cross),
            FlexDirection::Vertical => Offset::new(cross, main),
        }
    }

    /// Creates a size from main and cross values.
    fn size_from_main_cross(&self, main: f32, cross: f32) -> Size {
        match self.direction {
            FlexDirection::Horizontal => Size::new(main, cross),
            FlexDirection::Vertical => Size::new(cross, main),
        }
    }
}

impl flui_foundation::Diagnosticable for RenderFlex {}
impl RenderBox for RenderFlex {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) {
        let constraints = ctx.constraints().clone();
        let child_count = ctx.child_count();
        self.child_count = child_count;

        if child_count == 0 {
            // No children - use minimum size
            self.size = constraints.smallest();
            self.child_offsets.clear();
            ctx.complete_with_size(self.size);
            return;
        }

        // Calculate child constraints based on direction
        let child_constraints = match self.direction {
            FlexDirection::Horizontal => BoxConstraints::new(
                0.0,
                f32::INFINITY, // Unbounded main axis
                constraints.min_height,
                constraints.max_height,
            ),
            FlexDirection::Vertical => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f32::INFINITY, // Unbounded main axis
            ),
        };

        // Layout all children and collect sizes
        let mut child_sizes = Vec::with_capacity(child_count);
        let mut total_main = 0.0f32;
        let mut max_cross = 0.0f32;

        for i in 0..child_count {
            let child_size = ctx.layout_child(i, child_constraints.clone());
            child_sizes.push(child_size);
            total_main += self.main_size(child_size);
            max_cross = max_cross.max(self.cross_size(child_size));
        }

        // Add spacing
        let total_spacing = self.spacing * (child_count - 1).max(0) as f32;
        total_main += total_spacing;

        // Calculate our size
        let main_extent = match self.direction {
            FlexDirection::Horizontal => constraints.constrain_width(total_main),
            FlexDirection::Vertical => constraints.constrain_height(total_main),
        };
        let cross_extent = match self.direction {
            FlexDirection::Horizontal => constraints.constrain_height(max_cross),
            FlexDirection::Vertical => constraints.constrain_width(max_cross),
        };

        self.size = self.size_from_main_cross(main_extent, cross_extent);

        // Calculate starting position based on main axis alignment
        let free_space = main_extent - total_main;
        let (mut main_offset, between_space) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (0.0, 0.0),
            MainAxisAlignment::End => (free_space, 0.0),
            MainAxisAlignment::Center => (free_space / 2.0, 0.0),
            MainAxisAlignment::SpaceBetween => {
                if child_count > 1 {
                    (0.0, free_space / (child_count - 1) as f32)
                } else {
                    (0.0, 0.0)
                }
            }
            MainAxisAlignment::SpaceAround => {
                let space = free_space / child_count as f32;
                (space / 2.0, space)
            }
            MainAxisAlignment::SpaceEvenly => {
                let space = free_space / (child_count + 1) as f32;
                (space, space)
            }
        };

        // Position each child and track offsets
        self.child_offsets.clear();
        self.child_offsets.reserve(child_count);

        for i in 0..child_count {
            let child_size = child_sizes[i];

            // Calculate cross axis offset based on alignment
            let cross_offset = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => 0.0,
                CrossAxisAlignment::End => cross_extent - self.cross_size(child_size),
                CrossAxisAlignment::Center => (cross_extent - self.cross_size(child_size)) / 2.0,
                CrossAxisAlignment::Stretch => 0.0, // Child already stretched via constraints
            };

            let offset = self.offset(main_offset, cross_offset);
            self.child_offsets.push(offset);
            ctx.position_child(i, offset);

            main_offset += self.main_size(child_size) + self.spacing + between_space;
        }

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Variable, BoxParentData>) {
        // Children are painted automatically by the wrapper
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        // Test children in reverse order (top-most first)
        for i in (0..self.child_count).rev() {
            if let Some(&offset) = self.child_offsets.get(i) {
                if ctx.hit_test_child_at_offset(i, offset) {
                    return true;
                }
            }
        }

        false
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::RenderObject;
    use crate::wrapper::BoxWrapper;

    #[test]
    fn test_flex_row_creation() {
        let row = RenderFlex::row();
        assert!(row.is_horizontal());
        assert!(!row.is_vertical());
    }

    #[test]
    fn test_flex_column_creation() {
        let column = RenderFlex::column();
        assert!(column.is_vertical());
        assert!(!column.is_horizontal());
    }

    #[test]
    fn test_flex_no_children() {
        let flex = RenderFlex::row();
        let mut wrapper = BoxWrapper::new(flex);

        wrapper.layout(BoxConstraints::loose(Size::new(200.0, 100.0)), true);

        // Minimum size when no children
        assert_eq!(wrapper.inner().size(), Size::ZERO);
    }

    #[test]
    fn test_flex_builder() {
        let flex = RenderFlex::column()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.0);

        assert_eq!(flex.direction(), FlexDirection::Vertical);
        assert_eq!(flex.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(flex.cross_axis_alignment, CrossAxisAlignment::Stretch);
        assert_eq!(flex.spacing, 8.0);
    }
}
