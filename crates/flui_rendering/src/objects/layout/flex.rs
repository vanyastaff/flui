//! RenderFlex - flex layout container (Row/Column)

use flui_types::{Offset, Size, constraints::BoxConstraints, Axis, MainAxisAlignment, CrossAxisAlignment, MainAxisSize};
use flui_core::DynRenderObject;
use crate::core::{ContainerRenderBox, RenderBoxMixin};

/// Data for RenderFlex
#[derive(Debug, Clone, PartialEq)]
pub struct FlexData {
    /// The direction to lay out children (horizontal for Row, vertical for Column)
    pub direction: Axis,
    /// How to align children along the main axis
    pub main_axis_alignment: MainAxisAlignment,
    /// How much space should be occupied on the main axis
    pub main_axis_size: MainAxisSize,
    /// How to align children along the cross axis
    pub cross_axis_alignment: CrossAxisAlignment,
}

impl FlexData {
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
}

/// RenderObject for flex layout (Row/Column)
///
/// This is a simplified implementation that demonstrates the ContainerRenderBox pattern.
/// A full implementation would include:
/// - FlexParentData for flex factors
/// /// - Flexible/Expanded child support
/// - Baseline alignment
/// - TextDirection support
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{ContainerRenderBox, objects::layout::FlexData};
/// use flui_types::Axis;
///
/// let mut flex = ContainerRenderBox::new(FlexData::row());
/// ```
pub type RenderFlex = ContainerRenderBox<FlexData>;

// ===== Public API =====

impl RenderFlex {
    /// Get reference to type-specific data
    pub fn data(&self) -> &FlexData {
        &self.data
    }

    /// Get mutable reference to type-specific data
    pub fn data_mut(&mut self) -> &mut FlexData {
        &mut self.data
    }

    /// Get the direction
    pub fn direction(&self) -> Axis {
        self.data().direction
    }

    /// Set new direction
    pub fn set_direction(&mut self, direction: Axis) {
        if self.data().direction != direction {
            self.data_mut().direction = direction;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }

    /// Get main axis alignment
    pub fn main_axis_alignment(&self) -> MainAxisAlignment {
        self.data().main_axis_alignment
    }

    /// Set main axis alignment
    pub fn set_main_axis_alignment(&mut self, alignment: MainAxisAlignment) {
        if self.data().main_axis_alignment != alignment {
            self.data_mut().main_axis_alignment = alignment;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderFlex {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let direction = self.data().direction;
        let main_axis_size = self.data().main_axis_size;

        if self.children.is_empty() {
            // No children - use smallest size
            let size = constraints.smallest();
            self.state_mut().size = Some(size);
            self.clear_needs_layout();
            return size;
        }

        // Simplified layout algorithm
        // TODO: This is a basic implementation. A full implementation would:
        // 1. Calculate flex factors from FlexParentData
        // 2. Distribute space according to flex factors
        // 3. Handle Flexible/Expanded children properly

        let mut total_main_size = 0.0;
        let mut max_cross_size: f32 = 0.0;

        // Layout all children with loose constraints
        for child in &mut self.children {
            let child_constraints = match direction {
                Axis::Horizontal => BoxConstraints::new(
                    0.0,
                    constraints.max_width,
                    constraints.min_height,
                    constraints.max_height,
                ),
                Axis::Vertical => BoxConstraints::new(
                    constraints.min_width,
                    constraints.max_width,
                    0.0,
                    constraints.max_height,
                ),
            };

            let child_size = child.layout(child_constraints);

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

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        let direction = self.data().direction;
        let size = self.state().size.unwrap_or(Size::ZERO);
        let main_axis_alignment = self.data().main_axis_alignment;

        // Calculate total size of children
        let mut total_main_size = 0.0;
        for child in &self.children {
            let child_size = child.size();
            match direction {
                Axis::Horizontal => total_main_size += child_size.width,
                Axis::Vertical => total_main_size += child_size.height,
            }
        }

        // Calculate available space for alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        // Calculate spacing
        let (leading_space, between_space) = main_axis_alignment.calculate_spacing(
            available_space.max(0.0),
            self.children.len(),
        );

        // Paint children
        let mut current_offset = offset;
        match direction {
            Axis::Horizontal => current_offset.dx += leading_space,
            Axis::Vertical => current_offset.dy += leading_space,
        }

        for child in &self.children {
            child.paint(painter, current_offset);

            let child_size = child.size();
            match direction {
                Axis::Horizontal => current_offset.dx += child_size.width + between_space,
                Axis::Vertical => current_offset.dy += child_size.height + between_space,
            }
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_data_row() {
        let data = FlexData::row();
        assert_eq!(data.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_data_column() {
        let data = FlexData::column();
        assert_eq!(data.direction, Axis::Vertical);
    }

    #[test]
    fn test_render_flex_new() {
        let flex = ContainerRenderBox::new(FlexData::row());
        assert_eq!(flex.direction(), Axis::Horizontal);
        assert_eq!(flex.children().len(), 0);
    }

    #[test]
    fn test_render_flex_set_direction() {
        let mut flex = ContainerRenderBox::new(FlexData::row());

        flex.set_direction(Axis::Vertical);
        assert_eq!(flex.direction(), Axis::Vertical);
        assert!(RenderBoxMixin::needs_layout(&flex));
    }

    #[test]
    fn test_render_flex_layout_no_children() {
        let mut flex = ContainerRenderBox::new(FlexData::row());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = flex.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_render_flex_set_main_axis_alignment() {
        let mut flex = ContainerRenderBox::new(FlexData::row());

        flex.set_main_axis_alignment(MainAxisAlignment::Center);
        assert_eq!(flex.main_axis_alignment(), MainAxisAlignment::Center);
        assert!(RenderBoxMixin::needs_layout(&flex));
    }
}
