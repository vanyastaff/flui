//! RenderFlex - flex layout container (Row/Column)

use flui_types::{Offset, Size, constraints::BoxConstraints, Axis, MainAxisAlignment, CrossAxisAlignment, MainAxisSize};
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

        if children_ids.is_empty() {
            // No children - use smallest size
            let size = constraints.smallest();
            *state.size.lock() = Some(size);
            state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
            return size;
        }

        // Simplified layout algorithm
        // TODO: This is a basic implementation. A full implementation would:
        // 1. Calculate flex factors from FlexParentData
        // 2. Distribute space according to flex factors
        // 3. Handle Flexible/Expanded children properly

        let mut total_main_size = 0.0;
        let mut max_cross_size: f32 = 0.0;

        // Layout all children with constraints based on cross-axis alignment
        for (idx, &child_id) in children_ids.iter().enumerate() {
            let child_constraints = match direction {
                Axis::Horizontal => {
                    // Main axis = horizontal, cross axis = vertical
                    // Main axis is loose (0.0 to max), cross axis depends on alignment
                    let (min_cross, max_cross) = if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
                        (constraints.min_height, constraints.max_height)
                    } else {
                        (0.0, constraints.max_height)
                    };
                    BoxConstraints::new(
                        0.0,
                        constraints.max_width,
                        min_cross,
                        max_cross,
                    )
                }
                Axis::Vertical => {
                    // Main axis = vertical, cross axis = horizontal
                    // Main axis is loose (0.0 to max), cross axis depends on alignment
                    let (min_cross, max_cross) = if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
                        (constraints.min_width, constraints.max_width)
                    } else {
                        (0.0, constraints.max_width)
                    };
                    BoxConstraints::new(
                        min_cross,
                        max_cross,
                        0.0,
                        constraints.max_height,
                    )
                }
            };

            tracing::debug!("RenderFlex: laying out child #{} (id={}) with constraints {:?}", idx, child_id, child_constraints);
            let child_size = ctx.layout_child(child_id, child_constraints);
            tracing::debug!("RenderFlex: child #{} size = {:?}", idx, child_size);

            match direction {
                Axis::Horizontal => {
                    total_main_size += child_size.width;
                    max_cross_size = max_cross_size.max(child_size.height);
                }
                Axis::Vertical => {
                    total_main_size += child_size.height;
                    max_cross_size = max_cross_size.max(child_size.width);
                }
            }
        }

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

        // Store size directly in state and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        let direction = self.direction;
        let size = state.size.lock().expect("Size not set in paint");
        let main_axis_alignment = self.main_axis_alignment;
        let cross_axis_alignment = self.cross_axis_alignment;

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Calculate total size of children
        let mut total_main_size = 0.0;
        for &child_id in children_ids {
            // Get child size (traversing to RenderObject if needed)
            let child_size = ctx.child_size(child_id);
            match direction {
                Axis::Horizontal => total_main_size += child_size.width,
                Axis::Vertical => total_main_size += child_size.height,
            }
        }

        // Calculate available space for main axis alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        // Calculate main axis spacing
        let (leading_space, between_space) = main_axis_alignment.calculate_spacing(
            available_space.max(0.0),
            children_ids.len(),
        );

        tracing::debug!(
            "RenderFlex::paint: direction={:?}, flex_size={:?}, total_main={:.1}, available={:.1}, leading={:.1}, between={:.1}",
            direction, size, total_main_size, available_space, leading_space, between_space
        );

        // Paint children
        let mut current_offset = offset;
        match direction {
            Axis::Horizontal => current_offset.dx += leading_space,
            Axis::Vertical => current_offset.dy += leading_space,
        }

        for (idx, &child_id) in children_ids.iter().enumerate() {
            // Get child size for positioning
            let child_size = ctx.child_size(child_id);

            // Calculate cross-axis offset based on alignment
            let child_offset = match direction {
                Axis::Horizontal => {
                    // Main axis = horizontal, cross axis = vertical
                    let cross_offset = match cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.height - child_size.height) / 2.0,
                        CrossAxisAlignment::End => size.height - child_size.height,
                        CrossAxisAlignment::Stretch => 0.0, // TODO: Stretch handled in layout
                        CrossAxisAlignment::Baseline => 0.0, // TODO: Baseline alignment
                    };
                    Offset::new(current_offset.dx, offset.dy + cross_offset)
                }
                Axis::Vertical => {
                    // Main axis = vertical, cross axis = horizontal
                    let cross_offset = match cross_axis_alignment {
                        CrossAxisAlignment::Start => 0.0,
                        CrossAxisAlignment::Center => (size.width - child_size.width) / 2.0,
                        CrossAxisAlignment::End => size.width - child_size.width,
                        CrossAxisAlignment::Stretch => 0.0, // TODO: Stretch handled in layout
                        CrossAxisAlignment::Baseline => 0.0, // TODO: Baseline alignment
                    };

                    if idx == 3 {
                        tracing::debug!(
                            "RenderFlex: child #{} cross_offset calculation: flex_width={}, child_width={}, cross_offset={}, base_offset.dx={}",
                            idx, size.width, child_size.width, cross_offset, offset.dx
                        );
                    }

                    Offset::new(offset.dx + cross_offset, current_offset.dy)
                }
            };

            tracing::debug!(
                "RenderFlex: painting child #{} (id={}) at offset {:?}, size {:?}, cross_align={:?}",
                idx, child_id, child_offset, child_size, cross_axis_alignment
            );

            // Paint child with calculated offset
            ctx.paint_child(child_id, painter, child_offset);

            // Advance main axis position
            match direction {
                Axis::Horizontal => {
                    current_offset.dx += child_size.width + between_space;
                },
                Axis::Vertical => {
                    current_offset.dy += child_size.height + between_space;
                },
            }
        }
    }

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
