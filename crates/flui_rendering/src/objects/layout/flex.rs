//! RenderFlex - flex layout container (Row/Column)
//!
//! Implements Flutter's flex layout algorithm for arranging children along
//! a main axis (horizontal for Row, vertical for Column).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderFlex` | `RenderFlex` from `package:flutter/src/rendering/flex.dart` |
//! | `direction` | `direction` property |
//! | `main_axis_alignment` | `mainAxisAlignment` property |
//! | `cross_axis_alignment` | `crossAxisAlignment` property |
//! | `main_axis_size` | `mainAxisSize` property |
//! | `text_baseline` | `textBaseline` property |
//!
//! # Layout Protocol
//!
//! 1. **Determine cross-axis constraints**
//!    - If `CrossAxisAlignment::Stretch`: use parent's min/max cross size
//!    - Otherwise: use 0 to parent's max cross size
//!
//! 2. **Layout non-flexible children**
//!    - Each gets unbounded main axis (0 to max)
//!    - Respects cross-axis constraints from step 1
//!
//! 3. **Calculate main axis size**
//!    - Sum child sizes on main axis
//!    - `MainAxisSize::Min`: use sum (respecting parent min)
//!    - `MainAxisSize::Max`: use parent's max constraint
//!
//! 4. **Apply main axis alignment**
//!    - Calculate spacing between children
//!    - `Start/End/Center`: simple offset calculation
//!    - `SpaceBetween/Around/Evenly`: distribute free space
//!
//! 5. **Apply cross axis alignment per child**
//!    - `Start/End/Center`: offset within available space
//!    - `Stretch`: child already sized to fill
//!    - `Baseline`: TODO - align by text baseline
//!
//! # Performance
//!
//! - **Layout**: O(n) - single pass through children
//! - **Paint**: O(n) - paint each child once
//! - **Memory**: O(n) - stores offset per child
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderFlex;
//! use flui_types::{Axis, MainAxisAlignment, CrossAxisAlignment};
//!
//! // Row with space between
//! let row = RenderFlex::row()
//!     .with_main_axis_alignment(MainAxisAlignment::SpaceBetween);
//!
//! // Column centered both axes
//! let column = RenderFlex::column()
//!     .with_main_axis_alignment(MainAxisAlignment::Center)
//!     .with_cross_axis_alignment(CrossAxisAlignment::Center);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, ChildrenAccess, RenderBox, Variable};
use crate::{RenderObject, RenderResult};
use flui_types::{
    constraints::BoxConstraints,
    layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize},
    typography::TextBaseline,
    Axis, Offset, Size,
};

/// RenderObject for flex layout (Row/Column).
///
/// Arranges children along a main axis with flexible space distribution
/// and alignment options.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Row**: Horizontal arrangement (Axis::Horizontal)
/// - **Column**: Vertical arrangement (Axis::Vertical)
/// - **Flexible layouts**: TODO - with Expanded/Flexible children
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderFlex behavior:
/// - Respects MainAxisAlignment for spacing
/// - Respects CrossAxisAlignment per child
/// - Handles MainAxisSize (min vs max)
/// - Detects and warns about overflow (debug mode)
#[derive(Debug)]
pub struct RenderFlex {
    /// The direction to lay out children (horizontal for Row, vertical for Column)
    pub direction: Axis,
    /// How to align children along the main axis
    pub main_axis_alignment: MainAxisAlignment,
    /// How much space should be occupied on the main axis
    pub main_axis_size: MainAxisSize,
    /// How to align children along the cross axis
    pub cross_axis_alignment: CrossAxisAlignment,
    /// Text baseline type for baseline alignment
    pub text_baseline: TextBaseline,

    // Cache for paint
    child_offsets: Vec<Offset>,

    // Debug-only overflow tracking
    #[cfg(debug_assertions)]
    overflow_pixels: f32,
    #[cfg(debug_assertions)]
    container_size: Size,
}

impl RenderFlex {
    /// Create new flex data
    pub fn new(direction: Axis) -> Self {
        Self {
            direction,
            main_axis_alignment: MainAxisAlignment::default(),
            main_axis_size: MainAxisSize::default(),
            cross_axis_alignment: CrossAxisAlignment::default(),
            text_baseline: TextBaseline::default(),
            child_offsets: Vec::new(),
            #[cfg(debug_assertions)]
            overflow_pixels: 0.0,
            #[cfg(debug_assertions)]
            container_size: Size::ZERO,
        }
    }

    /// Create a Row configuration (horizontal)
    pub fn row() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Create a Column configuration (vertical)
    pub fn column() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Get the direction
    pub fn direction(&self) -> Axis {
        self.direction
    }

    /// Set new direction (returns new instance)
    pub fn with_direction(mut self, direction: Axis) -> Self {
        self.direction = direction;
        self
    }

    /// Get main axis alignment
    pub fn main_axis_alignment(&self) -> MainAxisAlignment {
        self.main_axis_alignment
    }

    /// Set main axis alignment (returns new instance)
    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Get main axis size
    pub fn main_axis_size(&self) -> MainAxisSize {
        self.main_axis_size
    }

    /// Set main axis size (returns new instance)
    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
        self
    }

    /// Get cross axis alignment
    pub fn cross_axis_alignment(&self) -> CrossAxisAlignment {
        self.cross_axis_alignment
    }

    /// Set cross axis alignment (returns new instance)
    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Get text baseline
    pub fn text_baseline(&self) -> TextBaseline {
        self.text_baseline
    }

    /// Set text baseline (returns new instance)
    pub fn with_text_baseline(mut self, baseline: TextBaseline) -> Self {
        self.text_baseline = baseline;
        self
    }

    /// Helper: Estimate baseline distance from top for a given size
    fn estimate_baseline(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.height * 0.75,
            Axis::Vertical => size.width * 0.75,
        }
    }

    /// Get overflow information (debug only)
    #[cfg(debug_assertions)]
    pub fn get_overflow(&self) -> (f32, f32) {
        match self.direction {
            Axis::Horizontal => (self.overflow_pixels, 0.0),
            Axis::Vertical => (0.0, self.overflow_pixels),
        }
    }
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self::row()
    }
}

impl RenderObject for RenderFlex {}

impl RenderBox<Variable> for RenderFlex {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Clear cache
        self.child_offsets.clear();

        let child_count = ctx.children.len();
        if child_count == 0 {
            return Ok(constraints.smallest());
        }

        // ========== SIMPLE FLEX LAYOUT (no flexible children yet) ==========
        // TODO: Add FlexItemMetadata support for Flexible/Expanded widgets

        let direction = self.direction;
        let main_axis_size = self.main_axis_size;

        // Cross-axis constraints
        let cross_constraints = match direction {
            Axis::Horizontal => {
                if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
                    (constraints.min_height, constraints.max_height)
                } else {
                    (0.0, constraints.max_height)
                }
            }
            Axis::Vertical => {
                if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
                    (constraints.min_width, constraints.max_width)
                } else {
                    (0.0, constraints.max_width)
                }
            }
        };

        // Layout all children
        let mut child_sizes: Vec<Size> = Vec::with_capacity(child_count);
        let mut total_main_size = 0.0f32;
        let mut max_cross_size = 0.0f32;

        let max_main_size = match direction {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        };

        // Use ctx.children() which returns Iterator<Item = ElementId>
        for child_id in ctx.children() {
            let child_constraints = match direction {
                Axis::Horizontal => BoxConstraints::new(
                    0.0,
                    max_main_size,
                    cross_constraints.0,
                    cross_constraints.1,
                ),
                Axis::Vertical => BoxConstraints::new(
                    cross_constraints.0,
                    cross_constraints.1,
                    0.0,
                    max_main_size,
                ),
            };

            let child_size = ctx.layout_child(child_id, child_constraints)?;
            child_sizes.push(child_size);

            let (child_main, child_cross) = match direction {
                Axis::Horizontal => (child_size.width, child_size.height),
                Axis::Vertical => (child_size.height, child_size.width),
            };

            total_main_size = (total_main_size + child_main).min(f32::MAX);
            max_cross_size = max_cross_size.max(child_cross);
        }

        // Calculate final size
        let size = match direction {
            Axis::Horizontal => {
                let width = if main_axis_size.is_max() {
                    constraints.max_width
                } else {
                    total_main_size.min(constraints.max_width)
                };
                Size::new(
                    width,
                    max_cross_size.clamp(constraints.min_height, constraints.max_height),
                )
            }
            Axis::Vertical => {
                let height = if main_axis_size.is_max() {
                    constraints.max_height
                } else {
                    total_main_size.min(constraints.max_height)
                };
                Size::new(
                    max_cross_size.clamp(constraints.min_width, constraints.max_width),
                    height,
                )
            }
        };

        // Debug: Track overflow
        #[cfg(debug_assertions)]
        {
            let container_main_size = match direction {
                Axis::Horizontal => size.width,
                Axis::Vertical => size.height,
            };

            self.overflow_pixels = (total_main_size - container_main_size).max(0.0);
            self.container_size = size;

            if self.overflow_pixels > 0.0 {
                tracing::warn!(
                    direction = ?direction,
                    content_size_px = total_main_size,
                    container_size_px = container_main_size,
                    overflow_px = self.overflow_pixels,
                    "RenderFlex overflow detected!"
                );
            }
        }

        // Calculate child offsets for main axis alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        let (leading_space, between_space) = self
            .main_axis_alignment
            .calculate_spacing(available_space.max(0.0), child_count);

        // For baseline alignment
        let child_baselines: Vec<f32> = if self.cross_axis_alignment == CrossAxisAlignment::Baseline
        {
            child_sizes
                .iter()
                .map(|&s| self.estimate_baseline(s))
                .collect()
        } else {
            Vec::new()
        };
        let max_baseline = child_baselines.iter().copied().fold(0.0f32, f32::max);

        // Calculate offset for each child
        let mut current_main_pos = leading_space;

        for (i, child_size) in child_sizes.iter().enumerate() {
            let child_offset = match direction {
                Axis::Horizontal => {
                    let cross_offset = match self.cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.height - child_size.height) / 2.0,
                        CrossAxisAlignment::End => size.height - child_size.height,
                        CrossAxisAlignment::Stretch => 0.0,
                        CrossAxisAlignment::Baseline => {
                            let child_baseline = child_baselines.get(i).copied().unwrap_or(0.0);
                            max_baseline - child_baseline
                        }
                    };
                    Offset::new(current_main_pos, cross_offset)
                }
                Axis::Vertical => {
                    let cross_offset = match self.cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.width - child_size.width) / 2.0,
                        CrossAxisAlignment::End => size.width - child_size.width,
                        CrossAxisAlignment::Stretch => 0.0,
                        CrossAxisAlignment::Baseline => {
                            let child_baseline = child_baselines.get(i).copied().unwrap_or(0.0);
                            max_baseline - child_baseline
                        }
                    };
                    Offset::new(cross_offset, current_main_pos)
                }
            };

            self.child_offsets.push(child_offset);

            current_main_pos += match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            } + between_space;
        }

        Ok(size)
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
    fn test_flex_data_row() {
        let data = RenderFlex::row();
        assert_eq!(data.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_data_column() {
        let data = RenderFlex::column();
        assert_eq!(data.direction, Axis::Vertical);
    }

    #[test]
    fn test_render_flex_new() {
        let flex = RenderFlex::row();
        assert_eq!(flex.direction(), Axis::Horizontal);
    }

    #[test]
    fn test_render_flex_with_direction() {
        let flex = RenderFlex::row();
        let flex = flex.with_direction(Axis::Vertical);
        assert_eq!(flex.direction(), Axis::Vertical);
    }

    #[test]
    fn test_render_flex_with_main_axis_alignment() {
        let flex = RenderFlex::row();
        let flex = flex.with_main_axis_alignment(MainAxisAlignment::Center);
        assert_eq!(flex.main_axis_alignment(), MainAxisAlignment::Center);
    }
}
