//! RenderFlex - flex layout container (Row/Column)
//!
//! Flutter equivalent: `RenderFlex`
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderFlex-class.html>

use std::collections::HashMap;

use crate::core::{BoxProtocol, FlexParentData, LayoutContext, PaintContext, FullRenderTree, RenderBox, Variable};
use flui_foundation::ElementId;
use flui_types::{
    constraints::BoxConstraints,
    layout::{CrossAxisAlignment, FlexFit, MainAxisAlignment, MainAxisSize},
    typography::TextBaseline,
    Axis, Offset, Size,
};

/// RenderObject for flex layout (Row/Column)
///
/// Flex layout arranges children along a main axis (horizontal for Row, vertical for Column)
/// with support for flexible children that expand to fill available space.
///
/// # Layout Algorithm (Flutter-compatible)
///
/// The flex layout uses a two-pass algorithm:
///
/// **Pass 1 - Non-flexible children:**
/// Layout children with `flex == None` or `flex == 0` using unbounded main axis constraints.
/// They take their natural size.
///
/// **Pass 2 - Flexible children:**
/// Calculate remaining space after non-flexible children.
/// Distribute remaining space among flexible children proportionally based on their flex factors.
/// A child with `flex=2` gets twice the space of a child with `flex=1`.
///
/// # FlexFit
///
/// - `FlexFit::Tight` (Expanded): Child must fill exactly the allocated space
/// - `FlexFit::Loose` (Flexible): Child can be smaller than allocated space
///
/// # Features
///
/// - Main axis alignment (start, end, center, space between/around/evenly)
/// - Cross axis alignment (start, end, center, stretch, baseline)
/// - Main axis sizing (min or max)
/// - Flexible/Expanded child support via FlexParentData
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderFlex;
/// use flui_rendering::core::FlexParentData;
/// use flui_types::Axis;
///
/// let mut flex = RenderFlex::row();
///
/// // Set up flexible children
/// flex.set_child_flex(child1_id, FlexParentData::non_flexible());  // Fixed size
/// flex.set_child_flex(child2_id, FlexParentData::expanded(1));      // Takes 1/3 of remaining
/// flex.set_child_flex(child3_id, FlexParentData::expanded(2));      // Takes 2/3 of remaining
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

    /// Per-child flex parent data (flex factor and fit)
    child_parent_data: HashMap<ElementId, FlexParentData>,

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
            child_parent_data: HashMap::new(),
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

    // ========== FLEX PARENT DATA MANAGEMENT ==========

    /// Set flex parent data for a child.
    ///
    /// This determines how the child participates in the flex layout:
    /// - Non-flexible: Takes its natural size
    /// - Flexible: Can be smaller than allocated space
    /// - Expanded: Must fill allocated space
    pub fn set_child_flex(&mut self, child_id: ElementId, parent_data: FlexParentData) {
        self.child_parent_data.insert(child_id, parent_data);
    }

    /// Get flex parent data for a child.
    ///
    /// Returns `None` if no parent data was set (child is treated as non-flexible).
    pub fn get_child_flex(&self, child_id: ElementId) -> Option<&FlexParentData> {
        self.child_parent_data.get(&child_id)
    }

    /// Remove flex parent data for a child.
    pub fn remove_child_flex(&mut self, child_id: ElementId) -> Option<FlexParentData> {
        self.child_parent_data.remove(&child_id)
    }

    /// Clear all flex parent data.
    pub fn clear_child_flex(&mut self) {
        self.child_parent_data.clear();
    }

    /// Check if a child is flexible (has a non-zero flex factor).
    #[allow(dead_code)]
    fn is_child_flexible(&self, child_id: ElementId) -> bool {
        self.child_parent_data
            .get(&child_id)
            .map(|pd| pd.is_flexible())
            .unwrap_or(false)
    }

    /// Get the flex factor for a child (0 if non-flexible).
    fn get_child_flex_factor(&self, child_id: ElementId) -> i32 {
        self.child_parent_data
            .get(&child_id)
            .map(|pd| pd.flex_factor())
            .unwrap_or(0)
    }

    /// Get the flex fit for a child.
    fn get_child_flex_fit(&self, child_id: ElementId) -> FlexFit {
        self.child_parent_data
            .get(&child_id)
            .map(|pd| pd.fit)
            .unwrap_or(FlexFit::Loose)
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

impl<T: FullRenderTree> RenderBox<T, Variable> for RenderFlex {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Clear cache
        self.child_offsets.clear();

        // Collect child IDs for layout_child calls and parent data lookup
        let child_ids: Vec<ElementId> = children.iter().collect();
        let child_count = child_ids.len();

        if child_count == 0 {
            return constraints.smallest();
        }

        // ========== FLUTTER-COMPATIBLE TWO-PASS FLEX LAYOUT ==========
        let direction = self.direction;
        let main_axis_size = self.main_axis_size;

        // Get max main axis extent from constraints
        let max_main_extent = match direction {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        };

        // Cross-axis constraints
        // Flutter: If crossAxisAlignment is stretch, use tight cross constraints
        // Otherwise, use loose cross constraints (0 to max)
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

        // ========== PASS 1: Layout non-flexible children ==========
        // Non-flexible children get UNBOUNDED main axis constraints
        // and take their natural size.
        let mut child_sizes: Vec<Option<Size>> = vec![None; child_count];
        let mut total_flex = 0i32;
        let mut inflexible_main_size = 0.0f32;
        let mut max_cross_size = 0.0f32;

        for (i, &child_id) in child_ids.iter().enumerate() {
            let flex_factor = self.get_child_flex_factor(child_id);

            if flex_factor == 0 {
                // Non-flexible child: layout with unbounded main axis
                let child_constraints = match direction {
                    Axis::Horizontal => BoxConstraints::new(
                        0.0,
                        f32::INFINITY,
                        cross_constraints.0,
                        cross_constraints.1,
                    ),
                    Axis::Vertical => BoxConstraints::new(
                        cross_constraints.0,
                        cross_constraints.1,
                        0.0,
                        f32::INFINITY,
                    ),
                };

                let child_size = ctx.layout_child(child_id, child_constraints);
                child_sizes[i] = Some(child_size);

                let child_main = match direction {
                    Axis::Horizontal => child_size.width,
                    Axis::Vertical => child_size.height,
                };
                let child_cross = match direction {
                    Axis::Horizontal => child_size.height,
                    Axis::Vertical => child_size.width,
                };

                inflexible_main_size += child_main;
                max_cross_size = max_cross_size.max(child_cross);
            } else {
                // Flexible child: defer layout to pass 2
                total_flex += flex_factor;
            }
        }

        // ========== PASS 2: Layout flexible children ==========
        // Calculate remaining space and distribute among flexible children
        let remaining_space = (max_main_extent - inflexible_main_size).max(0.0);
        let space_per_flex = if total_flex > 0 {
            remaining_space / total_flex as f32
        } else {
            0.0
        };

        let mut total_main_size = inflexible_main_size;

        for (i, &child_id) in child_ids.iter().enumerate() {
            if child_sizes[i].is_some() {
                // Already laid out in pass 1
                continue;
            }

            let flex_factor = self.get_child_flex_factor(child_id);
            let flex_fit = self.get_child_flex_fit(child_id);

            // Calculate allocated space for this flexible child
            let allocated_space = space_per_flex * flex_factor as f32;

            // Build constraints based on FlexFit
            let child_constraints = match (direction, flex_fit) {
                (Axis::Horizontal, FlexFit::Tight) => BoxConstraints::new(
                    allocated_space,
                    allocated_space, // Tight: exactly allocated space
                    cross_constraints.0,
                    cross_constraints.1,
                ),
                (Axis::Horizontal, FlexFit::Loose) => BoxConstraints::new(
                    0.0,
                    allocated_space, // Loose: 0 to allocated space
                    cross_constraints.0,
                    cross_constraints.1,
                ),
                (Axis::Vertical, FlexFit::Tight) => BoxConstraints::new(
                    cross_constraints.0,
                    cross_constraints.1,
                    allocated_space,
                    allocated_space, // Tight: exactly allocated space
                ),
                (Axis::Vertical, FlexFit::Loose) => BoxConstraints::new(
                    cross_constraints.0,
                    cross_constraints.1,
                    0.0,
                    allocated_space, // Loose: 0 to allocated space
                ),
            };

            let child_size = ctx.layout_child(child_id, child_constraints);
            child_sizes[i] = Some(child_size);

            let child_main = match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            };
            let child_cross = match direction {
                Axis::Horizontal => child_size.height,
                Axis::Vertical => child_size.width,
            };

            total_main_size += child_main;
            max_cross_size = max_cross_size.max(child_cross);
        }

        // ========== CALCULATE FINAL SIZE ==========
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

        // ========== POSITION CHILDREN ==========
        // Calculate available space for main axis alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        let (leading_space, between_space) = self
            .main_axis_alignment
            .calculate_spacing(available_space.max(0.0), child_count);

        // Collect actual sizes (unwrap Option<Size>)
        let actual_sizes: Vec<Size> = child_sizes
            .into_iter()
            .map(|s| s.unwrap_or(Size::ZERO))
            .collect();

        // For baseline alignment
        let child_baselines: Vec<f32> = if self.cross_axis_alignment == CrossAxisAlignment::Baseline
        {
            actual_sizes
                .iter()
                .map(|&s| self.estimate_baseline(s))
                .collect()
        } else {
            Vec::new()
        };
        let max_baseline = child_baselines.iter().copied().fold(0.0f32, f32::max);

        // Calculate offset for each child
        let mut current_main_pos = leading_space;

        for (i, child_size) in actual_sizes.iter().enumerate() {
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

    #[test]
    fn test_flex_parent_data_management() {
        let mut flex = RenderFlex::row();
        let child_id = ElementId::new(1);

        // Initially no parent data
        assert!(flex.get_child_flex(child_id).is_none());
        assert!(!flex.is_child_flexible(child_id));
        assert_eq!(flex.get_child_flex_factor(child_id), 0);

        // Set flexible parent data
        flex.set_child_flex(child_id, FlexParentData::flexible(2));
        assert!(flex.get_child_flex(child_id).is_some());
        assert!(flex.is_child_flexible(child_id));
        assert_eq!(flex.get_child_flex_factor(child_id), 2);
        assert_eq!(flex.get_child_flex_fit(child_id), FlexFit::Loose);

        // Update to expanded
        flex.set_child_flex(child_id, FlexParentData::expanded(3));
        assert_eq!(flex.get_child_flex_factor(child_id), 3);
        assert_eq!(flex.get_child_flex_fit(child_id), FlexFit::Tight);

        // Remove parent data
        let removed = flex.remove_child_flex(child_id);
        assert!(removed.is_some());
        assert!(flex.get_child_flex(child_id).is_none());
    }

    #[test]
    fn test_flex_parent_data_clear() {
        let mut flex = RenderFlex::row();
        let child1 = ElementId::new(1);
        let child2 = ElementId::new(2);

        flex.set_child_flex(child1, FlexParentData::expanded(1));
        flex.set_child_flex(child2, FlexParentData::flexible(2));

        assert!(flex.get_child_flex(child1).is_some());
        assert!(flex.get_child_flex(child2).is_some());

        flex.clear_child_flex();

        assert!(flex.get_child_flex(child1).is_none());
        assert!(flex.get_child_flex(child2).is_none());
    }

    #[test]
    fn test_flex_non_flexible_defaults() {
        let flex = RenderFlex::row();
        let child_id = ElementId::new(1);

        // Without parent data, child is non-flexible
        assert!(!flex.is_child_flexible(child_id));
        assert_eq!(flex.get_child_flex_factor(child_id), 0);
        assert_eq!(flex.get_child_flex_fit(child_id), FlexFit::Loose);
    }
}
