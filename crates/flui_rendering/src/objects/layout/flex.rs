//! RenderFlex - flex layout container (Row/Column)

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::MultiRender;
use flui_engine::{BoxedLayer, layer::pool};
use flui_types::{
    Axis, Offset, Size,
    constraints::BoxConstraints,
    layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize},
    typography::TextBaseline,
};

/// RenderObject for flex layout (Row/Column)
///
/// Flex layout arranges children along a main axis (horizontal for Row, vertical for Column)
/// with support for flexible children that expand to fill available space.
///
/// # Features
///
/// - FlexParentData for flex factors and positioning
/// - Flexible/Expanded child support
/// - Main axis alignment (start, end, center, space between/around/evenly)
/// - Cross axis alignment (start, end, center, stretch, baseline)
/// - Main axis sizing (min or max)
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
    ///
    /// This is a simplified heuristic. In a full implementation, baseline would be
    /// queried from the child RenderObject or computed from text metrics.
    ///
    /// For now, we use a heuristic: baseline = height * 0.75 (75% down from top)
    fn estimate_baseline(&self, size: Size) -> f32 {
        match self.direction {
            Axis::Horizontal => size.height * 0.75,
            Axis::Vertical => size.width * 0.75,
        }
    }
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self::row()
    }
}

impl MultiRender for RenderFlex {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        if child_ids.is_empty() {
            self.child_offsets.clear();
            return constraints.smallest();
        }

        // Clear cache
        self.child_offsets.clear();

        // ========== FLEX LAYOUT ALGORITHM ==========
        // Proper flex layout with support for Flexible/Expanded widgets
        //
        // Algorithm:
        // 1. Separate inflexible and flexible children
        // 2. Layout inflexible children first
        // 3. Calculate remaining space and total flex
        // 4. Allocate space to flexible children proportionally
        // 5. Layout flexible children with FlexFit constraints

        let mut child_sizes: Vec<Size> = Vec::new();
        let direction = self.direction;
        let main_axis_size = self.main_axis_size;

        // Cross-axis constraints (same for all children)
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

        // Step 1: Separate inflexible and flexible children
        let mut inflexible_children: Vec<(ElementId, Size)> = Vec::new();
        let flexible_children: Vec<(ElementId, i32, flui_types::layout::FlexFit)> = Vec::new();
        let total_flex = 0i32;

        for &child in child_ids.iter() {
            // TODO: Implement tree.parent_data() method to query parent data from elements
            // For now, treat all children as inflexible
            // if let Some(flex_data) = tree.parent_data::<crate::parent_data::FlexParentData>(child) {
            //     if flex_data.flex > 0 {
            //         // Child is flexible
            //         flexible_children.push((child, flex_data.flex, flex_data.fit));
            //         total_flex += flex_data.flex;
            //         continue;
            //     }
            // }
            // Child is inflexible (no FlexParentData or flex == 0)
            inflexible_children.push((child, Size::ZERO)); // Size will be filled in next step
        }

        // Step 2: Layout inflexible children
        let max_main_size = match direction {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        };

        let mut allocated_main_size = 0.0f32;
        let mut max_cross_size = 0.0f32;

        for (child, size_slot) in inflexible_children.iter_mut() {
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

            let child_size = tree.layout_child(*child, child_constraints);
            *size_slot = child_size;
            child_sizes.push(child_size);

            let child_main_size = match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            };
            let child_cross_size = match direction {
                Axis::Horizontal => child_size.height,
                Axis::Vertical => child_size.width,
            };

            allocated_main_size += child_main_size;
            max_cross_size = max_cross_size.max(child_cross_size);
        }

        // Step 3: Calculate space for flexible children
        let remaining_space = (max_main_size - allocated_main_size).max(0.0);
        let space_per_flex = if total_flex > 0 {
            remaining_space / total_flex as f32
        } else {
            0.0
        };

        // Step 4 & 5: Layout flexible children
        for (child, flex, fit) in flexible_children.iter() {
            let allocated_space = space_per_flex * (*flex as f32);

            let child_constraints = match (direction, fit) {
                (Axis::Horizontal, flui_types::layout::FlexFit::Tight) => {
                    // Tight fit: child must fill allocated space
                    BoxConstraints::new(
                        allocated_space,
                        allocated_space,
                        cross_constraints.0,
                        cross_constraints.1,
                    )
                }
                (Axis::Horizontal, flui_types::layout::FlexFit::Loose) => {
                    // Loose fit: child can be smaller
                    BoxConstraints::new(
                        0.0,
                        allocated_space,
                        cross_constraints.0,
                        cross_constraints.1,
                    )
                }
                (Axis::Vertical, flui_types::layout::FlexFit::Tight) => BoxConstraints::new(
                    cross_constraints.0,
                    cross_constraints.1,
                    allocated_space,
                    allocated_space,
                ),
                (Axis::Vertical, flui_types::layout::FlexFit::Loose) => BoxConstraints::new(
                    cross_constraints.0,
                    cross_constraints.1,
                    0.0,
                    allocated_space,
                ),
            };

            let child_size = tree.layout_child(*child, child_constraints);
            child_sizes.push(child_size);

            let child_main_size = match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            };
            let child_cross_size = match direction {
                Axis::Horizontal => child_size.height,
                Axis::Vertical => child_size.width,
            };

            allocated_main_size += child_main_size;
            max_cross_size = max_cross_size.max(child_cross_size);
        }

        let total_main_size = allocated_main_size;

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

        // ========== Calculate child offsets ==========
        // Calculate available space for main axis alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        // Calculate main axis spacing
        let (leading_space, between_space) = self
            .main_axis_alignment
            .calculate_spacing(available_space.max(0.0), child_ids.len());

        // For baseline alignment, calculate baselines for all children
        let child_baselines: Vec<f32> = if self.cross_axis_alignment == CrossAxisAlignment::Baseline
        {
            child_sizes
                .iter()
                .map(|&size| self.estimate_baseline(size))
                .collect()
        } else {
            Vec::new()
        };

        // Find max baseline for baseline alignment
        let max_baseline = child_baselines.iter().copied().fold(0.0f32, f32::max);

        // Calculate offset for each child
        let mut current_main_pos = leading_space;

        for (i, child_size) in child_sizes.iter().enumerate() {
            // Calculate cross-axis offset based on alignment
            let child_offset = match direction {
                Axis::Horizontal => {
                    let cross_offset = match self.cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.height - child_size.height) / 2.0,
                        CrossAxisAlignment::End => size.height - child_size.height,
                        CrossAxisAlignment::Stretch => 0.0,
                        CrossAxisAlignment::Baseline => {
                            // Align by baseline: offset = max_baseline - child_baseline
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
                            // For vertical direction, baseline alignment uses horizontal baseline
                            let child_baseline = child_baselines.get(i).copied().unwrap_or(0.0);
                            max_baseline - child_baseline
                        }
                    };
                    Offset::new(cross_offset, current_main_pos)
                }
            };

            self.child_offsets.push(child_offset);

            // Advance main axis position
            current_main_pos += match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            } + between_space;
        }

        size
    }

    fn paint(&self, tree: &ElementTree, child_ids: &[ElementId], offset: Offset) -> BoxedLayer {
        let mut container = pool::acquire_container();

        // Paint children with their calculated offsets
        for (i, &child_id) in child_ids.iter().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);

            // Paint child and apply offset transform
            let child_layer = tree.paint_child(child_id, offset + child_offset);
            container.add_child(child_layer);
        }

        Box::new(container)
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
