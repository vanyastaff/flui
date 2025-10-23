//! RenderFlex - flex layout container (Row/Column)

use flui_types::{Offset, Size, constraints::BoxConstraints, Axis, MainAxisAlignment, CrossAxisAlignment, MainAxisSize};
use flui_types::layout::FlexFit;
use flui_core::DynRenderObject;

/// RenderObject for flex layout (Row/Column)
///
/// After architecture refactoring, RenderObjects now directly implement DynRenderObject
/// without a RenderBox wrapper. State is stored in ElementTree, accessed via RenderContext.
///
/// This is a simplified implementation. A full implementation would include:
/// - FlexParentData for flex factors
/// - Flexible/Expanded child support
/// - Baseline alignment
/// - TextDirection support
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderFlex;
/// use flui_types::Axis;
///
/// let flex = RenderFlex::row();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RenderFlex {
    /// The direction to lay out children (horizontal for Row, vertical for Column)
    pub direction: Axis,
    /// How to align children along the main axis
    pub main_axis_alignment: MainAxisAlignment,
    /// How much space should be occupied on the main axis
    pub main_axis_size: MainAxisSize,
    /// How to align children along the cross axis
    pub cross_axis_alignment: CrossAxisAlignment,
}

impl RenderFlex {
    /// Create new flex data
    pub fn new(direction: Axis) -> Self {
        Self {
            direction,
            main_axis_alignment: MainAxisAlignment::default(),
            main_axis_size: MainAxisSize::default(),
            cross_axis_alignment: CrossAxisAlignment::default(),
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
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderFlex {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints directly in state
        *state.constraints.lock() = Some(constraints);

        let direction = self.direction;
        let main_axis_size = self.main_axis_size;

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();
        let child_count = children_ids.len();

        if children_ids.is_empty() {
            // No children - use smallest size
            let size = constraints.smallest();
            *state.size.lock() = Some(size);
            state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
            return size;
        }

        // ========== FLEX LAYOUT ALGORITHM ==========
        // Proper flex layout with support for Flexible/Expanded widgets
        //
        // Algorithm:
        // 1. Separate inflexible and flexible children
        // 2. Layout inflexible children first
        // 3. Calculate remaining space and total flex
        // 4. Allocate space to flexible children proportionally
        // 5. Layout flexible children with FlexFit constraints

        // Step 1: Collect flex information for each child
        let mut child_info: Vec<(usize, Option<(i32, FlexFit)>)> = Vec::new();
        let mut total_flex = 0;

        for &child_id in children_ids.iter() {
            // Try to read FlexParentData
            let flex_info = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
                if let Some(flex_data) = parent_data.downcast_ref::<crate::parent_data::FlexParentData>() {
                    if flex_data.flex > 0 {
                        total_flex += flex_data.flex;
                        Some((flex_data.flex, flex_data.fit))
                    } else {
                        None // flex = 0 means inflexible
                    }
                } else {
                    None // No FlexParentData = inflexible
                }
            } else {
                None // No parent data = inflexible
            };

            child_info.push((child_id, flex_info));
        }

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

        // Step 2: Layout inflexible children
        let mut allocated_main_size = 0.0f32;
        let mut max_cross_size = 0.0f32;

        for (child_id, flex_info) in &child_info {
            if flex_info.is_none() {
                // Inflexible child - give loose main axis constraints
                let child_constraints = match direction {
                    Axis::Horizontal => BoxConstraints::new(
                        0.0,
                        constraints.max_width,
                        cross_constraints.0,
                        cross_constraints.1,
                    ),
                    Axis::Vertical => BoxConstraints::new(
                        cross_constraints.0,
                        cross_constraints.1,
                        0.0,
                        constraints.max_height,
                    ),
                };

                let child_size = ctx.layout_child_cached(*child_id, child_constraints, Some(child_count));

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
        }

        // Step 3: Calculate space for flexible children
        let max_main_size = match direction {
            Axis::Horizontal => constraints.max_width,
            Axis::Vertical => constraints.max_height,
        };

        let remaining_space = (max_main_size - allocated_main_size).max(0.0);

        // Step 4 & 5: Layout flexible children
        if total_flex > 0 && remaining_space > 0.0 {
            let space_per_flex = remaining_space / total_flex as f32;

            for (child_id, flex_info) in &child_info {
                if let Some((flex, fit)) = flex_info {
                    let allocated_space = space_per_flex * (*flex as f32);

                    // Create constraints based on FlexFit
                    let child_constraints = match direction {
                        Axis::Horizontal => {
                            let (min_main, max_main) = match fit {
                                FlexFit::Tight => (allocated_space, allocated_space),
                                FlexFit::Loose => (0.0, allocated_space),
                            };
                            BoxConstraints::new(
                                min_main,
                                max_main,
                                cross_constraints.0,
                                cross_constraints.1,
                            )
                        }
                        Axis::Vertical => {
                            let (min_main, max_main) = match fit {
                                FlexFit::Tight => (allocated_space, allocated_space),
                                FlexFit::Loose => (0.0, allocated_space),
                            };
                            BoxConstraints::new(
                                cross_constraints.0,
                                cross_constraints.1,
                                min_main,
                                max_main,
                            )
                        }
                    };

                    let child_size = ctx.layout_child_cached(*child_id, child_constraints, Some(child_count));

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
            }
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
                Size::new(width, max_cross_size.clamp(constraints.min_height, constraints.max_height))
            }
            Axis::Vertical => {
                let height = if main_axis_size.is_max() {
                    constraints.max_height
                } else {
                    total_main_size.min(constraints.max_height)
                };
                Size::new(max_cross_size.clamp(constraints.min_width, constraints.max_width), height)
            }
        };

        // Store size directly in state
        *state.size.lock() = Some(size);

        // ========== Calculate and save child offsets in ParentData ==========
        // This avoids recalculating positions in paint() and hit_test()

        // Calculate available space for main axis alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        // Calculate main axis spacing
        let (leading_space, between_space) = self.main_axis_alignment.calculate_spacing(
            available_space.max(0.0),
            children_ids.len(),
        );

        // Calculate and save offset for each child
        let mut current_main_pos = leading_space;

        for &child_id in children_ids {
            // Get child size
            let child_size = ctx.child_size(child_id);

            // Calculate cross-axis offset based on alignment
            let child_offset = match direction {
                Axis::Horizontal => {
                    let cross_offset = match self.cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.height - child_size.height) / 2.0,
                        CrossAxisAlignment::End => size.height - child_size.height,
                        CrossAxisAlignment::Stretch => 0.0,
                        CrossAxisAlignment::Baseline => 0.0, // TODO: Baseline alignment
                    };
                    Offset::new(current_main_pos, cross_offset)
                }
                Axis::Vertical => {
                    let cross_offset = match self.cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.width - child_size.width) / 2.0,
                        CrossAxisAlignment::End => size.width - child_size.width,
                        CrossAxisAlignment::Stretch => 0.0,
                        CrossAxisAlignment::Baseline => 0.0, // TODO: Baseline alignment
                    };
                    Offset::new(cross_offset, current_main_pos)
                }
            };

            // Save offset in FlexParentData
            if let Some(mut parent_data) = ctx.tree().parent_data_mut(child_id) {
                if let Some(flex_data) = parent_data.downcast_mut::<crate::parent_data::FlexParentData>() {
                    flex_data.offset = child_offset;
                }
            }

            // Advance main axis position
            current_main_pos += match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            } + between_space;
        }

        // Clear needs_layout flag
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, _state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Paint children using offsets saved in ParentData during layout
        for &child_id in children_ids {
            // Read offset from FlexParentData (set during layout)
            let local_offset = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
                if let Some(flex_data) = parent_data.downcast_ref::<crate::parent_data::FlexParentData>() {
                    flex_data.offset
                } else {
                    Offset::ZERO
                }
            } else {
                Offset::ZERO
            };

            // Add parent offset to local offset
            let child_offset = Offset::new(
                offset.dx + local_offset.dx,
                offset.dy + local_offset.dy,
            );

            // Paint child
            ctx.paint_child(child_id, painter, child_offset);
        }
    }

    // hit_test_children() now uses the default implementation from DynRenderObject,
    // which automatically reads offsets from FlexParentData via ParentDataWithOffset trait.
    // This eliminates ~30 lines of duplicate code!

    // All other methods (size, mark_needs_layout, etc.) use default implementations
    // from DynRenderObject trait, which delegate to RenderContext/ElementTree.
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
    fn test_render_flex_layout_no_children() {
        use flui_core::testing::mock_render_context;

        let flex = RenderFlex::row();
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = flex.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_flex_with_main_axis_alignment() {
        let flex = RenderFlex::row();
        let flex = flex.with_main_axis_alignment(MainAxisAlignment::Center);
        assert_eq!(flex.main_axis_alignment(), MainAxisAlignment::Center);
    }
}
