//! RenderFlex - flex layout container (Row/Column)

use crate::core::{BoxProtocol, ChildrenAccess, LayoutContext, PaintContext, RenderBox, Variable};
use flui_types::{
    constraints::BoxConstraints,
    layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize},
    typography::TextBaseline,
    Axis, Offset, Size,
};

/// RenderObject for flex layout (Row/Column)
///
/// Flex layout arranges children along a main axis (horizontal for Row, vertical for Column)
/// with support for flexible children that expand to fill available space.
///
/// # Features
///
/// - Main axis alignment (start, end, center, space between/around/evenly)
/// - Cross axis alignment (start, end, center, stretch, baseline)
/// - Main axis sizing (min or max)
/// - TODO: Flexible/Expanded child support via parent_data
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderFlex;
/// use flui_types::Axis;
///
/// let mut flex = RenderFlex::row();
/// ```
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

impl RenderBox<Variable> for RenderFlex {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Clear cache
        self.child_offsets.clear();

        let child_count = children.as_slice().len();
        if child_count == 0 {
            return constraints.smallest();
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

        for child_id in children.iter() {
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

            let child_size = ctx.layout_child(child_id, child_constraints);
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

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: crate::core::PaintTree,
    {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

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
