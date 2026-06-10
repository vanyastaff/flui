//! RenderFlex - lays out children in a row or column.

use flui_tree::Variable;
use flui_types::{Offset, Pixels, Point, Rect, Size, geometry::px};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::{FlexFit, FlexParentData},
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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

/// Re-export of the canonical [`flui_types::layout::MainAxisSize`]:
/// `Max` (Flutter default) fills the incoming max main extent when it
/// is bounded - without it, alignment is dead under loose constraints
/// (the container shrink-wraps, so there is never free space to
/// distribute).
pub use flui_types::layout::MainAxisSize;

/// How children are aligned along the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    /// How much main-axis space the container claims.
    main_axis_size: MainAxisSize,
    /// Cross axis alignment.
    cross_axis_alignment: CrossAxisAlignment,
    /// Spacing between children.
    spacing: f32,
    /// Size after layout.
    size: Size,
    /// Number of children (tracked for hit testing).
    child_count: usize,
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Horizontal,
            main_axis_alignment: MainAxisAlignment::Start,
            main_axis_size: MainAxisSize::Max,
            cross_axis_alignment: CrossAxisAlignment::Start,
            spacing: 0.0,
            size: Size::ZERO,
            child_count: 0,
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

    /// Builder: set the main-axis size policy.
    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
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
    fn main_size(&self, size: Size) -> Pixels {
        match self.direction {
            FlexDirection::Horizontal => size.width,
            FlexDirection::Vertical => size.height,
        }
    }

    /// Extracts cross axis extent from a size.
    fn cross_size(&self, size: Size) -> Pixels {
        match self.direction {
            FlexDirection::Horizontal => size.height,
            FlexDirection::Vertical => size.width,
        }
    }

    /// Creates an offset from main and cross values.
    fn offset(&self, main: Pixels, cross: Pixels) -> Offset {
        match self.direction {
            FlexDirection::Horizontal => Offset::new(main, cross),
            FlexDirection::Vertical => Offset::new(cross, main),
        }
    }

    /// Creates a size from main and cross values.
    fn size_from_main_cross(&self, main: Pixels, cross: Pixels) -> Size {
        match self.direction {
            FlexDirection::Horizontal => Size::new(main, cross),
            FlexDirection::Vertical => Size::new(cross, main),
        }
    }
}

impl flui_foundation::Diagnosticable for RenderFlex {}
impl RenderBox for RenderFlex {
    type Arity = Variable;
    type ParentData = FlexParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, FlexParentData>) {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        self.child_count = child_count;

        if child_count == 0 {
            // No children - use minimum size
            self.size = constraints.smallest();
            ctx.complete_with_size(self.size);
            return;
        }

        // ====================================================================
        // Two-pass flex layout (Flutter's RenderFlex.performLayout algorithm)
        // ====================================================================

        // Collect flex factors from parent data
        let mut flex_factors: Vec<Option<i32>> = Vec::with_capacity(child_count);
        let mut flex_fits: Vec<FlexFit> = Vec::with_capacity(child_count);
        let mut total_flex: i32 = 0;

        for i in 0..child_count {
            let (flex, fit) = ctx
                .child_parent_data(i)
                .map(|pd| (pd.flex, pd.fit))
                .unwrap_or((None, FlexFit::Loose));
            if let Some(f) = flex {
                total_flex += f;
            }
            flex_factors.push(flex);
            flex_fits.push(fit);
        }

        // Cross-axis policy (Flutter flex.dart:889-898): non-stretch
        // children get a LOOSE cross (an incoming tight cross must not
        // force every child to the container height); Stretch tightens
        // cross to the max when it is bounded - pre-fix, Stretch only
        // changed the cross OFFSET and children never actually
        // stretched.
        let stretch = self.cross_axis_alignment == CrossAxisAlignment::Stretch;
        let cross_max = match self.direction {
            FlexDirection::Horizontal => constraints.max_height,
            FlexDirection::Vertical => constraints.max_width,
        };
        let (child_cross_min, child_cross_max) = if stretch && cross_max.is_finite() {
            (cross_max, cross_max)
        } else {
            (Pixels::ZERO, cross_max)
        };

        // Non-flex child constraints: unbounded on main axis.
        let non_flex_constraints = match self.direction {
            FlexDirection::Horizontal => BoxConstraints::new(
                Pixels::ZERO,
                Pixels::INFINITY,
                child_cross_min,
                child_cross_max,
            ),
            FlexDirection::Vertical => BoxConstraints::new(
                child_cross_min,
                child_cross_max,
                Pixels::ZERO,
                Pixels::INFINITY,
            ),
        };

        // Pass 1: Layout non-flex children, sum their main-axis sizes
        let mut child_sizes: Vec<Option<Size>> = vec![None; child_count];
        let mut inflexible_main = Pixels::ZERO;
        let mut max_cross = Pixels::ZERO;

        for i in 0..child_count {
            if flex_factors[i].is_none() || flex_factors[i] == Some(0) {
                let child_size = ctx.layout_child(i, non_flex_constraints);
                child_sizes[i] = Some(child_size);
                inflexible_main += self.main_size(child_size);
                max_cross = max_cross.max(self.cross_size(child_size));
            }
        }

        // Add spacing to inflexible total
        let total_spacing = px(self.spacing * (child_count - 1) as f32);
        inflexible_main += total_spacing;

        // Calculate available main-axis extent.
        let max_main = match self.direction {
            FlexDirection::Horizontal => constraints.max_width,
            FlexDirection::Vertical => constraints.max_height,
        };

        // Flutter flex.dart:1232 - flex factors only mean something
        // when the main axis is bounded. Under an unbounded main, flex
        // children are DEMOTED to inflexible (pre-fix they received
        // zero-size allocations: a Tight fit collapsed them to 0x0).
        let can_flex = max_main.is_finite();
        if !can_flex && total_flex > 0 {
            for i in 0..child_count {
                if matches!(flex_factors[i], Some(f) if f > 0) {
                    let child_size = ctx.layout_child(i, non_flex_constraints);
                    child_sizes[i] = Some(child_size);
                    inflexible_main += self.main_size(child_size);
                    max_cross = max_cross.max(self.cross_size(child_size));
                }
            }
        }

        // Remaining space for flex children.
        let remaining = if can_flex {
            (max_main - inflexible_main).max(Pixels::ZERO)
        } else {
            Pixels::ZERO
        };

        // Pass 2: Layout flex children, distributing remaining space
        if can_flex && total_flex > 0 {
            for i in 0..child_count {
                if let Some(flex) = flex_factors[i]
                    && flex > 0
                {
                    let allocated = remaining * (flex as f32 / total_flex as f32);

                    let child_constraints = match (self.direction, flex_fits[i]) {
                        (FlexDirection::Horizontal, FlexFit::Tight) => BoxConstraints::new(
                            allocated,
                            allocated,
                            child_cross_min,
                            child_cross_max,
                        ),
                        (FlexDirection::Horizontal, FlexFit::Loose) => BoxConstraints::new(
                            Pixels::ZERO,
                            allocated,
                            child_cross_min,
                            child_cross_max,
                        ),
                        (FlexDirection::Vertical, FlexFit::Tight) => BoxConstraints::new(
                            child_cross_min,
                            child_cross_max,
                            allocated,
                            allocated,
                        ),
                        (FlexDirection::Vertical, FlexFit::Loose) => BoxConstraints::new(
                            child_cross_min,
                            child_cross_max,
                            Pixels::ZERO,
                            allocated,
                        ),
                    };

                    let child_size = ctx.layout_child(i, child_constraints);
                    child_sizes[i] = Some(child_size);
                    max_cross = max_cross.max(self.cross_size(child_size));
                }
            }
        }

        // Calculate total main from all laid-out children
        let mut total_main = Pixels::ZERO;
        for s in child_sizes.iter().flatten() {
            total_main += self.main_size(*s);
        }
        total_main += total_spacing;

        // Calculate our size. Flutter flex.dart:1298 - MainAxisSize::Max
        // claims the full bounded main extent (pre-fix the container
        // always shrink-wrapped, so Center/End/Space* alignment had no
        // free space to distribute under loose constraints).
        let ideal_main = if can_flex && self.main_axis_size == MainAxisSize::Max {
            max_main
        } else {
            total_main
        };
        let main_extent = match self.direction {
            FlexDirection::Horizontal => constraints.constrain_width(ideal_main),
            FlexDirection::Vertical => constraints.constrain_height(ideal_main),
        };
        let cross_extent = match self.direction {
            FlexDirection::Horizontal => constraints.constrain_height(max_cross),
            FlexDirection::Vertical => constraints.constrain_width(max_cross),
        };

        self.size = self.size_from_main_cross(main_extent, cross_extent);

        // Flutter flex.dart:1339 - clamp: an overflowing row must not
        // shift children by NEGATIVE space under End/Center/Space*.
        let free_space = (main_extent - total_main).max(Pixels::ZERO);
        let (mut main_offset, between_space) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (Pixels::ZERO, Pixels::ZERO),
            MainAxisAlignment::End => (free_space, Pixels::ZERO),
            MainAxisAlignment::Center => (free_space / 2.0, Pixels::ZERO),
            MainAxisAlignment::SpaceBetween => {
                if child_count > 1 {
                    (Pixels::ZERO, free_space / (child_count - 1) as f32)
                } else {
                    (Pixels::ZERO, Pixels::ZERO)
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

        for (i, slot) in child_sizes.iter().enumerate().take(child_count) {
            let child_size = slot.unwrap_or(Size::ZERO);

            // Calculate cross axis offset based on alignment
            let cross_offset = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => Pixels::ZERO,
                CrossAxisAlignment::End => cross_extent - self.cross_size(child_size),
                CrossAxisAlignment::Center => (cross_extent - self.cross_size(child_size)) / 2.0,
                CrossAxisAlignment::Stretch => Pixels::ZERO,
            };

            let offset = self.offset(main_offset, cross_offset);
            ctx.position_child(i, offset);

            main_offset += self.main_size(child_size) + px(self.spacing) + between_space;
        }

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    // paint() uses default no-op - Flex just positions children

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, FlexParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        // Test children in reverse order (top-most first)
        for i in (0..self.child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }

        false
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderFlex {}
impl SemanticsCapability for RenderFlex {}
impl HotReloadCapability for RenderFlex {}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_flex_default_values() {
        let flex = RenderFlex::row();
        assert_eq!(flex.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(flex.cross_axis_alignment, CrossAxisAlignment::Start);
        assert_eq!(flex.spacing, 0.0);
    }
}
