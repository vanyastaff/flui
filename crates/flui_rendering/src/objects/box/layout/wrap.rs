//! RenderWrap - wraps children to multiple rows or columns.
//!
//! When children exceed the main axis extent, they wrap to a new run.

use flui_types::{Offset, Point, Rect, Size};

use crate::constraints::BoxConstraints;

use crate::containers::ChildList;
use crate::objects::r#box::layout::flex::{Axis, VerticalDirection};
use crate::parent_data::WrapParentData;
use crate::pipeline::PaintingContext;
use crate::protocol::BoxProtocol;
use crate::traits::TextBaseline;
use flui_tree::arity::Variable;

/// How runs are aligned within the wrap on the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapAlignment {
    /// Place runs at the start.
    #[default]
    Start,
    /// Place runs at the end.
    End,
    /// Place runs at the center.
    Center,
    /// Place free space evenly between runs.
    SpaceBetween,
    /// Place free space evenly around runs.
    SpaceAround,
    /// Place free space evenly, including before first and after last.
    SpaceEvenly,
}

/// How children are aligned within a run on the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapCrossAlignment {
    /// Place children at the start.
    #[default]
    Start,
    /// Place children at the end.
    End,
    /// Place children at the center.
    Center,
}

/// A render object that wraps children to multiple runs.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::layout::{RenderWrap, WrapAlignment};
///
/// let mut wrap = RenderWrap::new();
/// wrap.set_spacing(8.0);
/// wrap.set_run_spacing(8.0);
/// ```
#[derive(Debug)]
pub struct RenderWrap {
    /// Container for children.
    children: ChildList<BoxProtocol, Variable, WrapParentData>,

    /// The direction to lay out runs.
    direction: Axis,

    /// Alignment of children within a run.
    alignment: WrapAlignment,

    /// Spacing between children within a run.
    spacing: f32,

    /// How runs are aligned on the cross axis.
    run_alignment: WrapAlignment,

    /// Spacing between runs.
    run_spacing: f32,

    /// Alignment of children within a run on the cross axis.
    cross_axis_alignment: WrapCrossAlignment,

    /// Text direction for horizontal wraps.
    text_direction: TextDirection,

    /// Vertical direction for vertical wraps.
    vertical_direction: VerticalDirection,

    /// Cached size.
    size: Size,
}

// Re-use TextDirection from flex module
use super::flex::TextDirection;

/// A single run of children in wrap layout.
#[derive(Debug, Default)]
struct WrapRun {
    /// Indices of children in this run.
    children: Vec<usize>,
    /// Total main axis extent.
    main_extent: f32,
    /// Max cross axis extent.
    cross_extent: f32,
}

impl RenderWrap {
    /// Creates a new wrap layout.
    pub fn new() -> Self {
        Self {
            children: ChildList::new(),
            direction: Axis::Horizontal,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_alignment: WrapAlignment::Start,
            run_spacing: 0.0,
            cross_axis_alignment: WrapCrossAlignment::Start,
            text_direction: TextDirection::Ltr,
            vertical_direction: VerticalDirection::Down,
            size: Size::ZERO,
        }
    }

    /// Creates a horizontal wrap.
    pub fn horizontal() -> Self {
        Self::new()
    }

    /// Creates a vertical wrap.
    pub fn vertical() -> Self {
        let mut wrap = Self::new();
        wrap.direction = Axis::Vertical;
        wrap
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

    /// Returns the alignment.
    pub fn alignment(&self) -> WrapAlignment {
        self.alignment
    }

    /// Sets the alignment.
    pub fn set_alignment(&mut self, alignment: WrapAlignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
        }
    }

    /// Returns the spacing.
    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    /// Sets the spacing between children.
    pub fn set_spacing(&mut self, spacing: f32) {
        if (self.spacing - spacing).abs() > f32::EPSILON {
            self.spacing = spacing;
        }
    }

    /// Returns the run alignment.
    pub fn run_alignment(&self) -> WrapAlignment {
        self.run_alignment
    }

    /// Sets the run alignment.
    pub fn set_run_alignment(&mut self, alignment: WrapAlignment) {
        if self.run_alignment != alignment {
            self.run_alignment = alignment;
        }
    }

    /// Returns the run spacing.
    pub fn run_spacing(&self) -> f32 {
        self.run_spacing
    }

    /// Sets the spacing between runs.
    pub fn set_run_spacing(&mut self, spacing: f32) {
        if (self.run_spacing - spacing).abs() > f32::EPSILON {
            self.run_spacing = spacing;
        }
    }

    /// Returns the cross axis alignment.
    pub fn cross_axis_alignment(&self) -> WrapCrossAlignment {
        self.cross_axis_alignment
    }

    /// Sets the cross axis alignment.
    pub fn set_cross_axis_alignment(&mut self, alignment: WrapCrossAlignment) {
        if self.cross_axis_alignment != alignment {
            self.cross_axis_alignment = alignment;
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
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

    /// Gets the main axis constraint.
    fn get_main_constraint(&self, constraints: BoxConstraints) -> f32 {
        match self.direction {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        }
    }

    /// Creates a size from main and cross extents.
    fn create_size(&self, main: f32, cross: f32) -> Size {
        match self.direction {
            Axis::Horizontal => Size::new(main, cross),
            Axis::Vertical => Size::new(cross, main),
        }
    }

    /// Creates an offset from main and cross positions.
    fn create_offset(&self, main: f32, cross: f32) -> Offset {
        match self.direction {
            Axis::Horizontal => Offset::new(main, cross),
            Axis::Vertical => Offset::new(cross, main),
        }
    }

    /// Performs layout with provided child sizes.
    pub fn perform_layout_with_children(
        &mut self,
        constraints: BoxConstraints,
        child_sizes: &[Size],
    ) -> (Size, Vec<Offset>) {
        let max_main = self.get_main_constraint(constraints);

        // Group children into runs
        let mut runs: Vec<WrapRun> = Vec::new();
        let mut current_run = WrapRun::default();

        for (i, size) in child_sizes.iter().enumerate() {
            let child_main = self.get_main_size(*size);
            let child_cross = self.get_cross_size(*size);

            // Check if we need to start a new run
            let run_main = if current_run.children.is_empty() {
                child_main
            } else {
                current_run.main_extent + self.spacing + child_main
            };

            if !current_run.children.is_empty() && run_main > max_main {
                runs.push(std::mem::take(&mut current_run));
            }

            // Add to current run
            if current_run.children.is_empty() {
                current_run.main_extent = child_main;
            } else {
                current_run.main_extent += self.spacing + child_main;
            }
            current_run.cross_extent = current_run.cross_extent.max(child_cross);
            current_run.children.push(i);
        }

        // Don't forget the last run
        if !current_run.children.is_empty() {
            runs.push(current_run);
        }

        // Calculate total cross extent
        let total_cross_extent: f32 = runs.iter().map(|r| r.cross_extent).sum::<f32>()
            + (runs.len().saturating_sub(1)) as f32 * self.run_spacing;

        // Calculate our size
        let main_extent = runs
            .iter()
            .map(|r| r.main_extent)
            .fold(0.0_f32, f32::max)
            .min(max_main);

        self.size = self.create_size(main_extent, total_cross_extent);
        self.size = constraints.constrain(self.size);

        // Calculate run positions
        let actual_cross = self.get_cross_size(self.size);
        let free_cross = (actual_cross - total_cross_extent).max(0.0);
        let run_count = runs.len();

        let (run_leading, run_between) = match self.run_alignment {
            WrapAlignment::Start => (0.0, 0.0),
            WrapAlignment::End => (free_cross, 0.0),
            WrapAlignment::Center => (free_cross / 2.0, 0.0),
            WrapAlignment::SpaceBetween => {
                if run_count > 1 {
                    (0.0, free_cross / (run_count - 1) as f32)
                } else {
                    (0.0, 0.0)
                }
            }
            WrapAlignment::SpaceAround => {
                let space = free_cross / run_count as f32;
                (space / 2.0, space)
            }
            WrapAlignment::SpaceEvenly => {
                let space = free_cross / (run_count + 1) as f32;
                (space, space)
            }
        };

        // Calculate offsets for each child
        let mut offsets: Vec<Offset> = vec![Offset::ZERO; child_sizes.len()];
        let mut cross_position = run_leading;

        for run in &runs {
            // Calculate child positions within run
            let free_main = (max_main - run.main_extent).max(0.0);
            let child_count = run.children.len();

            let (main_leading, main_between) = match self.alignment {
                WrapAlignment::Start => (0.0, 0.0),
                WrapAlignment::End => (free_main, 0.0),
                WrapAlignment::Center => (free_main / 2.0, 0.0),
                WrapAlignment::SpaceBetween => {
                    if child_count > 1 {
                        (0.0, free_main / (child_count - 1) as f32)
                    } else {
                        (0.0, 0.0)
                    }
                }
                WrapAlignment::SpaceAround => {
                    let space = free_main / child_count as f32;
                    (space / 2.0, space)
                }
                WrapAlignment::SpaceEvenly => {
                    let space = free_main / (child_count + 1) as f32;
                    (space, space)
                }
            };

            let mut main_position = main_leading;

            for (j, &child_index) in run.children.iter().enumerate() {
                let child_size = child_sizes[child_index];
                let child_cross = self.get_cross_size(child_size);

                // Cross axis alignment within run
                let cross_offset = match self.cross_axis_alignment {
                    WrapCrossAlignment::Start => 0.0,
                    WrapCrossAlignment::End => run.cross_extent - child_cross,
                    WrapCrossAlignment::Center => (run.cross_extent - child_cross) / 2.0,
                };

                offsets[child_index] =
                    self.create_offset(main_position, cross_position + cross_offset);

                if j < child_count - 1 {
                    main_position += self.get_main_size(child_size) + self.spacing + main_between;
                }
            }

            cross_position += run.cross_extent + self.run_spacing + run_between;
        }

        (self.size, offsets)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let _ = (context, offset);
    }

    /// Hit tests at the given position.
    pub fn hit_test(&self, position: Offset) -> bool {
        let rect = Rect::from_origin_size(Point::ZERO, self.size);
        rect.contains(Point::new(position.dx, position.dy))
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, _height: f32, child_widths: &[f32]) -> f32 {
        match self.direction {
            Axis::Horizontal => {
                // At minimum, we need to fit the widest child
                child_widths.iter().cloned().fold(0.0_f32, f32::max)
            }
            Axis::Vertical => {
                // Sum of all widths (they'd all be in separate runs)
                child_widths.iter().sum()
            }
        }
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, _height: f32, child_widths: &[f32]) -> f32 {
        match self.direction {
            Axis::Horizontal => {
                // All children in one run
                let total: f32 = child_widths.iter().sum();
                let spacing = if child_widths.len() > 1 {
                    self.spacing * (child_widths.len() - 1) as f32
                } else {
                    0.0
                };
                total + spacing
            }
            Axis::Vertical => {
                // Widest child
                child_widths.iter().cloned().fold(0.0_f32, f32::max)
            }
        }
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, _width: f32, child_heights: &[f32]) -> f32 {
        match self.direction {
            Axis::Horizontal => {
                // Tallest child
                child_heights.iter().cloned().fold(0.0_f32, f32::max)
            }
            Axis::Vertical => {
                // At minimum, we need to fit the tallest child
                child_heights.iter().cloned().fold(0.0_f32, f32::max)
            }
        }
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, _width: f32, child_heights: &[f32]) -> f32 {
        match self.direction {
            Axis::Horizontal => {
                // Tallest child
                child_heights.iter().cloned().fold(0.0_f32, f32::max)
            }
            Axis::Vertical => {
                // All children in one run
                let total: f32 = child_heights.iter().sum();
                let spacing = if child_heights.len() > 1 {
                    self.spacing * (child_heights.len() - 1) as f32
                } else {
                    0.0
                };
                total + spacing
            }
        }
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

impl Default for RenderWrap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_new() {
        let wrap = RenderWrap::new();
        assert_eq!(wrap.direction(), Axis::Horizontal);
        assert_eq!(wrap.spacing(), 0.0);
    }

    #[test]
    fn test_wrap_spacing() {
        let mut wrap = RenderWrap::new();
        wrap.set_spacing(10.0);
        assert_eq!(wrap.spacing(), 10.0);
    }

    #[test]
    fn test_wrap_single_run() {
        let mut wrap = RenderWrap::new();
        let constraints = BoxConstraints::new(0.0, 300.0, 0.0, 300.0);
        let children = vec![Size::new(50.0, 30.0), Size::new(50.0, 30.0)];

        let (size, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // Both fit in one run
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 30.0);
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_wrap_multiple_runs() {
        let mut wrap = RenderWrap::new();
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 300.0);
        let children = vec![
            Size::new(50.0, 30.0),
            Size::new(50.0, 30.0),
            Size::new(50.0, 30.0), // This should wrap
        ];

        let (size, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // Two runs: [50, 50] and [50]
        assert_eq!(size.width, 100.0);
        assert_eq!(size.height, 60.0); // Two runs of 30 each

        // First two on first run
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[0].dy - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 50.0).abs() < f32::EPSILON);
        assert!((offsets[1].dy - 0.0).abs() < f32::EPSILON);

        // Third on second run
        assert!((offsets[2].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[2].dy - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_wrap_with_spacing() {
        let mut wrap = RenderWrap::new();
        wrap.set_spacing(10.0);
        let constraints = BoxConstraints::new(0.0, 110.0, 0.0, 300.0);
        let children = vec![Size::new(50.0, 30.0), Size::new(50.0, 30.0)];

        let (size, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // 50 + 10 + 50 = 110
        assert_eq!(size.width, 110.0);
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 60.0).abs() < f32::EPSILON); // 50 + 10
    }

    #[test]
    fn test_wrap_run_spacing() {
        let mut wrap = RenderWrap::new();
        wrap.set_run_spacing(5.0);
        let constraints = BoxConstraints::new(0.0, 50.0, 0.0, 300.0);
        let children = vec![
            Size::new(50.0, 30.0),
            Size::new(50.0, 20.0), // Wraps to second run
        ];

        let (size, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // Two runs with 5px spacing
        assert_eq!(size.height, 55.0); // 30 + 5 + 20
        assert!((offsets[1].dy - 35.0).abs() < f32::EPSILON); // 30 + 5
    }

    #[test]
    fn test_wrap_alignment_center() {
        let mut wrap = RenderWrap::new();
        wrap.set_alignment(WrapAlignment::Center);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 300.0);
        let children = vec![Size::new(50.0, 30.0)];

        let (_, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // Centered in 200px: (200 - 50) / 2 = 75
        assert!((offsets[0].dx - 75.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_wrap_cross_alignment_center() {
        let mut wrap = RenderWrap::new();
        wrap.set_cross_axis_alignment(WrapCrossAlignment::Center);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 300.0);
        let children = vec![Size::new(50.0, 20.0), Size::new(50.0, 40.0)];

        let (_, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // First child (20h) centered in run of 40h: (40 - 20) / 2 = 10
        assert!((offsets[0].dy - 10.0).abs() < f32::EPSILON);
        // Second child (40h) fills run
        assert!((offsets[1].dy - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_wrap_vertical() {
        let mut wrap = RenderWrap::vertical();
        let constraints = BoxConstraints::new(0.0, 300.0, 0.0, 100.0);
        let children = vec![
            Size::new(30.0, 50.0),
            Size::new(30.0, 50.0),
            Size::new(30.0, 50.0), // Wraps
        ];

        let (size, offsets) = wrap.perform_layout_with_children(constraints, &children);

        // Two runs of height 100 (2x50), width = 30 + 30 = 60
        assert_eq!(size.height, 100.0);
        assert_eq!(size.width, 60.0);

        // First two in first column
        assert!((offsets[0].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dx - 0.0).abs() < f32::EPSILON);
        assert!((offsets[1].dy - 50.0).abs() < f32::EPSILON);

        // Third in second column
        assert!((offsets[2].dx - 30.0).abs() < f32::EPSILON);
        assert!((offsets[2].dy - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_hit_test() {
        let mut wrap = RenderWrap::new();
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        wrap.perform_layout_with_children(constraints, &[]);

        assert!(wrap.hit_test(Offset::new(50.0, 25.0)));
        assert!(!wrap.hit_test(Offset::new(150.0, 25.0)));
    }
}
