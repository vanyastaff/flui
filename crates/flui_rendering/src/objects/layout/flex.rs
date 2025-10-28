//! RenderFlex - flex layout container (Row/Column)

use flui_types::{Offset, Size, constraints::BoxConstraints, Axis, MainAxisAlignment, CrossAxisAlignment, MainAxisSize};
use flui_core::render::{RenderObject, MultiArity, LayoutCx, PaintCx, MultiChild, MultiChildPaint};
use flui_engine::{BoxedLayer, ContainerLayer, Transform, TransformLayer};

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
}

impl Default for RenderFlex {
    fn default() -> Self {
        Self::row()
    }
}

impl RenderObject for RenderFlex {
    type Arity = MultiArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let children = cx.children();
        let constraints = cx.constraints();

        if children.is_empty() {
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

        // Step 1: Collect flex information for each child
        // Note: In the new architecture, we don't have direct access to parent data
        // during layout. For now, treat all children as inflexible.
        // TODO: Add parent data support when available in LayoutCx

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

        // Layout all children (simplified - treating all as inflexible for now)
        let mut allocated_main_size = 0.0f32;
        let mut max_cross_size = 0.0f32;

        for child in children.iter().copied() {
            // Give loose main axis constraints
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

            let child_size = cx.layout_child(child, child_constraints);
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

        // ========== Calculate child offsets ==========
        // Calculate available space for main axis alignment
        let available_space = match direction {
            Axis::Horizontal => size.width - total_main_size,
            Axis::Vertical => size.height - total_main_size,
        };

        // Calculate main axis spacing
        let (leading_space, between_space) = self.main_axis_alignment.calculate_spacing(
            available_space.max(0.0),
            children.len(),
        );

        // Calculate offset for each child
        let mut current_main_pos = leading_space;

        for child_size in child_sizes {
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

            self.child_offsets.push(child_offset);

            // Advance main axis position
            current_main_pos += match direction {
                Axis::Horizontal => child_size.width,
                Axis::Vertical => child_size.height,
            } + between_space;
        }

        size
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let children = cx.children();
        let mut container = ContainerLayer::new();

        // Paint children with their calculated offsets
        for (i, &child) in children.iter().enumerate() {
            let offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);

            // Capture child layer and apply offset transform
            let child_layer = cx.capture_child_layer(child);

            if offset != Offset::ZERO {
                let transform = Transform::Translate(offset);
                let transform_layer = TransformLayer::new(child_layer, transform);
                container.add_child(Box::new(transform_layer));
            } else {
                container.add_child(child_layer);
            }
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
