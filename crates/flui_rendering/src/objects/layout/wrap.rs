//! RenderWrap - Wrapping layout container (like CSS flexbox wrap)
//!
//! Implements Flutter's wrapping layout algorithm for arranging children
//! that automatically wrap to new lines (horizontal) or columns (vertical)
//! when space runs out. Similar to CSS flexbox with flex-wrap enabled.
//! Supports alignment, spacing between items, and spacing between runs.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderWrap` | `RenderWrap` from `package:flutter/src/rendering/wrap.dart` |
//! | `direction` | `direction` property (Axis enum) |
//! | `alignment` | `alignment` property (WrapAlignment) |
//! | `spacing` | `spacing` property (space between items in run) |
//! | `run_spacing` | `runSpacing` property (space between runs) |
//! | `cross_alignment` | `crossAxisAlignment` property |
//! | `horizontal()` | Creates Axis.horizontal configuration |
//! | `vertical()` | Creates Axis.vertical configuration |
//! | `set_direction()` | `direction = value` setter |
//! | `set_spacing()` | `spacing = value` setter |
//! | `WrapAlignment::Start` | `WrapAlignment.start` |
//! | `WrapAlignment::SpaceBetween` | `WrapAlignment.spaceBetween` |
//!
//! # Layout Protocol
//!
//! 1. **Initialize run tracking**
//!    - Track current position in main axis
//!    - Track current run's cross-axis extent (max height/width of run)
//!    - Track total size across all runs
//!
//! 2. **Layout children sequentially**
//!    - Give each child remaining space in current run
//!    - Child gets:
//!      - Main axis: remaining space in run
//!      - Cross axis: parent's max constraint
//!    - Check if child fits in current run
//!
//! 3. **Handle wrapping**
//!    - If child doesn't fit AND not first in run:
//!      - Start new run (new line or column)
//!      - Add run_spacing to cross position
//!      - Reset main-axis position to 0
//!      - Reset run's cross extent
//!
//! 4. **Position children**
//!    - Cache offsets based on current run position
//!    - Add spacing between children in same run
//!    - Track max cross extent of run
//!
//! 5. **Calculate final size**
//!    - Main axis: max extent across all runs
//!    - Cross axis: total of all run heights + run_spacing
//!
//! # Paint Protocol
//!
//! 1. **Paint children in order**
//!    - Use cached offsets from layout phase
//!    - Paint each child at parent offset + child offset
//!    - Children painted in order (same as layout order)
//!
//! # Performance
//!
//! - **Layout**: O(n) - single pass through children with wrap detection
//! - **Paint**: O(n) - paint each child once in order
//! - **Memory**: 48 bytes base + O(n) for cached offsets (16 bytes per child)
//!
//! # Use Cases
//!
//! - **Tag clouds**: Wrapping text tags (hashtags, categories)
//! - **Chip lists**: Material Design chips with automatic wrapping
//! - **Image galleries**: Variable-sized images with wrapping
//! - **Responsive layouts**: Automatic flow based on available space
//! - **Button groups**: Groups of buttons that wrap on narrow screens
//! - **Breadcrumbs**: Navigation breadcrumbs with wrapping
//! - **Word wrapping**: Words in a sentence (with custom widgets)
//!
//! # Wrap Behavior
//!
//! ```text
//! Horizontal direction (wraps to new lines):
//!   Run 1: [Item1][Item2][Item3]
//!   Run 2: [Item4][Item5]
//!   Run 3: [Item6]
//!
//! Vertical direction (wraps to new columns):
//!   Column 1: [Item1]
//!             [Item2]
//!             [Item3]
//!   Column 2: [Item4]
//!             [Item5]
//!   Column 3: [Item6]
//! ```
//!
//! # Spacing Behavior
//!
//! ```text
//! spacing = 8.0 (between items in same run)
//! run_spacing = 12.0 (between runs)
//!
//! [Item1]--8--[Item2]--8--[Item3]
//!     |
//!    12 (run_spacing)
//!     |
//! [Item4]--8--[Item5]
//!     |
//!    12
//!     |
//! [Item6]
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderFlex**: Flex is single-line, Wrap wraps to multiple lines
//! - **vs RenderListBody**: ListBody is simple sequential, Wrap handles wrapping
//! - **vs RenderFlow**: Flow uses delegate for custom logic, Wrap uses standard wrapping
//! - **vs RenderGrid**: Grid has fixed rows/columns, Wrap wraps dynamically
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderWrap;
//! use flui_types::Axis;
//!
//! // Horizontal wrap (like flexbox flex-wrap)
//! let wrap = RenderWrap::horizontal()
//!     .with_spacing(8.0)        // Space between items in run
//!     .with_run_spacing(12.0);  // Space between lines
//!
//! // Vertical wrap (wraps to new columns)
//! let vertical_wrap = RenderWrap::vertical()
//!     .with_spacing(4.0);
//!
//! // Tag cloud with spacing
//! let tags = RenderWrap::horizontal()
//!     .with_spacing(6.0)
//!     .with_run_spacing(10.0);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, ChildrenAccess, RenderBox, Variable};
use crate::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::{Axis, Offset, Size};

/// Alignment for runs in wrap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapAlignment {
    /// Place runs at the start
    Start,
    /// Place runs at the end
    End,
    /// Center runs
    Center,
    /// Space runs evenly
    SpaceBetween,
    /// Space runs with space around
    SpaceAround,
    /// Space runs evenly with equal space
    SpaceEvenly,
}

/// Cross-axis alignment for children within a run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapCrossAlignment {
    /// Align to start of cross axis
    Start,
    /// Align to end of cross axis
    End,
    /// Center on cross axis
    Center,
}

/// RenderObject that arranges children with automatic wrapping.
///
/// Like Flex (Row/Column) but wraps children to new lines (horizontal) or
/// columns (vertical) when running out of space. Similar to CSS flexbox with
/// flex-wrap enabled. Supports spacing between items in runs and spacing
/// between runs themselves.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Wrapping Layout Container** - Arranges children sequentially with automatic
/// wrapping to new lines/columns, configurable spacing between items and runs,
/// sizes to max extent across runs.
///
/// # Use Cases
///
/// - **Tag clouds**: Wrapping text tags, hashtags, categories
/// - **Chip lists**: Material Design chips with automatic wrapping
/// - **Image galleries**: Variable-sized images with wrapping
/// - **Responsive layouts**: Automatic flow based on available space
/// - **Button groups**: Buttons that wrap on narrow screens
/// - **Breadcrumbs**: Navigation breadcrumbs with wrapping
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderWrap behavior:
/// - Wraps children when running out of space
/// - Respects spacing (between items) and run_spacing (between runs)
/// - Handles both horizontal and vertical directions
/// - Size = max extent across all runs
/// - TODO: Support full WrapAlignment and WrapCrossAlignment
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderWrap;
///
/// // Tag cloud
/// let tags = RenderWrap::horizontal()
///     .with_spacing(8.0)
///     .with_run_spacing(12.0);
/// ```
#[derive(Debug)]
pub struct RenderWrap {
    /// Main axis direction (horizontal or vertical)
    pub direction: Axis,
    /// Alignment of runs along main axis
    pub alignment: WrapAlignment,
    /// Spacing between children in a run
    pub spacing: f32,
    /// Spacing between runs
    pub run_spacing: f32,
    /// Cross-axis alignment within a run
    pub cross_alignment: WrapCrossAlignment,

    // Cache for paint
    child_offsets: Vec<Offset>,
}

impl RenderWrap {
    /// Create new wrap
    pub fn new(direction: Axis) -> Self {
        Self {
            direction,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_spacing: 0.0,
            cross_alignment: WrapCrossAlignment::Start,
            child_offsets: Vec::new(),
        }
    }

    /// Create horizontal wrap
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Create vertical wrap
    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Set spacing
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set run spacing
    pub fn with_run_spacing(mut self, run_spacing: f32) -> Self {
        self.run_spacing = run_spacing;
        self
    }

    /// Set direction
    pub fn set_direction(&mut self, direction: Axis) {
        self.direction = direction;
    }

    /// Set spacing
    pub fn set_spacing(&mut self, spacing: f32) {
        self.spacing = spacing;
    }
}

impl Default for RenderWrap {
    fn default() -> Self {
        Self::horizontal()
    }
}

impl RenderObject for RenderWrap {}

impl RenderBox<Variable> for RenderWrap {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        if ctx.children.len() == 0 {
            self.child_offsets.clear();
            return Ok(constraints.smallest());
        }

        // Layout algorithm depends on direction
        self.child_offsets.clear();

        match self.direction {
            Axis::Horizontal => {
                let max_width = constraints.max_width;
                let mut current_x = 0.0_f32;
                let mut current_y = 0.0_f32;
                let mut max_run_height = 0.0_f32;
                let mut total_width = 0.0_f32;

                for child_id in ctx.children() {
                    // Child gets unconstrained width, constrained height
                    let child_constraints = BoxConstraints::new(
                        0.0,
                        max_width - current_x,
                        0.0,
                        constraints.max_height,
                    );

                    let child_size = ctx.layout_child(child_id, child_constraints)?;

                    // Check if we need to wrap
                    if current_x + child_size.width > max_width && current_x > 0.0 {
                        // Wrap to next line
                        current_y += max_run_height + self.run_spacing;
                        current_x = 0.0;
                        max_run_height = 0.0;
                    }

                    // Store child offset
                    self.child_offsets.push(Offset::new(current_x, current_y));

                    // Place child
                    current_x += child_size.width + self.spacing;
                    max_run_height = max_run_height.max(child_size.height);
                    total_width = total_width.max(current_x - self.spacing);
                }

                let total_height = current_y + max_run_height;
                Ok(Size::new(total_width.max(0.0), total_height.max(0.0)))
            }
            Axis::Vertical => {
                let max_height = constraints.max_height;
                let mut current_x = 0.0_f32;
                let mut current_y = 0.0_f32;
                let mut max_run_width = 0.0_f32;
                let mut total_height = 0.0_f32;

                for child_id in ctx.children() {
                    // Child gets constrained width, unconstrained height
                    let child_constraints = BoxConstraints::new(
                        0.0,
                        constraints.max_width,
                        0.0,
                        max_height - current_y,
                    );

                    let child_size = ctx.layout_child(child_id, child_constraints)?;

                    // Check if we need to wrap
                    if current_y + child_size.height > max_height && current_y > 0.0 {
                        // Wrap to next column
                        current_x += max_run_width + self.run_spacing;
                        current_y = 0.0;
                        max_run_width = 0.0;
                    }

                    // Store child offset
                    self.child_offsets.push(Offset::new(current_x, current_y));

                    // Place child
                    current_y += child_size.height + self.spacing;
                    max_run_width = max_run_width.max(child_size.width);
                    total_height = total_height.max(current_y - self.spacing);
                }

                let total_width = current_x + max_run_width;
                Ok(Size::new(total_width.max(0.0), total_height.max(0.0)))
            }
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children().collect();

        for (i, child_id) in child_ids.into_iter().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);
            ctx.paint_child(child_id, offset + child_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_alignment_variants() {
        assert_ne!(WrapAlignment::Start, WrapAlignment::End);
        assert_ne!(WrapAlignment::Center, WrapAlignment::SpaceBetween);
    }

    #[test]
    fn test_wrap_new() {
        let wrap = RenderWrap::new(Axis::Horizontal);
        assert_eq!(wrap.direction, Axis::Horizontal);
        assert_eq!(wrap.spacing, 0.0);
        assert_eq!(wrap.run_spacing, 0.0);
    }

    #[test]
    fn test_wrap_horizontal() {
        let wrap = RenderWrap::horizontal();
        assert_eq!(wrap.direction, Axis::Horizontal);
    }

    #[test]
    fn test_wrap_vertical() {
        let wrap = RenderWrap::vertical();
        assert_eq!(wrap.direction, Axis::Vertical);
    }

    #[test]
    fn test_wrap_with_spacing() {
        let wrap = RenderWrap::horizontal()
            .with_spacing(10.0)
            .with_run_spacing(5.0);
        assert_eq!(wrap.spacing, 10.0);
        assert_eq!(wrap.run_spacing, 5.0);
    }

    #[test]
    fn test_render_wrap_set_direction() {
        let mut wrap = RenderWrap::horizontal();
        wrap.set_direction(Axis::Vertical);
        assert_eq!(wrap.direction, Axis::Vertical);
    }

    #[test]
    fn test_render_wrap_set_spacing() {
        let mut wrap = RenderWrap::default();
        wrap.set_spacing(8.0);
        assert_eq!(wrap.spacing, 8.0);
    }
}
