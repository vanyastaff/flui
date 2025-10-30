//! RenderListBody - simple scrollable list layout

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::MultiRender;
use flui_engine::{BoxedLayer, Transform, TransformLayer, layer::pool};
use flui_types::{Axis, Offset, Size, constraints::BoxConstraints};

/// RenderObject that arranges children in a simple scrollable list
///
/// Unlike Flex, ListBody doesn't support flex factors. All children
/// are sized to their intrinsic size along the main axis.
/// Useful for simple scrollable lists.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderListBody;
/// use flui_types::Axis;
///
/// // Create vertical list with spacing
/// let mut list = RenderListBody::vertical().with_spacing(8.0);
/// ```
#[derive(Debug)]
pub struct RenderListBody {
    /// Main axis direction (horizontal or vertical)
    pub main_axis: Axis,
    /// Spacing between children
    pub spacing: f32,

    // Cache for paint
    child_sizes: Vec<Size>,
}

impl RenderListBody {
    /// Create new list body
    pub fn new(main_axis: Axis) -> Self {
        Self {
            main_axis,
            spacing: 0.0,
            child_sizes: Vec::new(),
        }
    }

    /// Create vertical list
    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Create horizontal list
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Set spacing between children
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set main axis
    pub fn set_main_axis(&mut self, main_axis: Axis) {
        self.main_axis = main_axis;
    }

    /// Set spacing
    pub fn set_spacing(&mut self, spacing: f32) {
        self.spacing = spacing;
    }
}

impl Default for RenderListBody {
    fn default() -> Self {
        Self::vertical()
    }
}

impl MultiRender for RenderListBody {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        if child_ids.is_empty() {
            self.child_sizes.clear();
            return constraints.smallest();
        }

        // Layout children based on axis
        self.child_sizes.clear();

        match self.main_axis {
            Axis::Vertical => {
                let mut total_height = 0.0_f32;
                let mut max_width = 0.0_f32;

                for child in child_ids.iter().copied() {
                    // Child gets parent's width constraints, infinite height
                    let child_constraints = BoxConstraints::new(
                        constraints.min_width,
                        constraints.max_width,
                        0.0,
                        f32::INFINITY,
                    );

                    let child_size = tree.layout_child(child, child_constraints);
                    self.child_sizes.push(child_size);

                    total_height += child_size.height;
                    max_width = max_width.max(child_size.width);
                }

                // Add spacing between children
                if !child_ids.is_empty() {
                    total_height += self.spacing * (child_ids.len() - 1) as f32;
                }

                constraints.constrain(Size::new(max_width, total_height))
            }
            Axis::Horizontal => {
                let mut total_width = 0.0_f32;
                let mut max_height = 0.0_f32;

                for child in child_ids.iter().copied() {
                    // Child gets infinite width, parent's height constraints
                    let child_constraints = BoxConstraints::new(
                        0.0,
                        f32::INFINITY,
                        constraints.min_height,
                        constraints.max_height,
                    );

                    let child_size = tree.layout_child(child, child_constraints);
                    self.child_sizes.push(child_size);

                    total_width += child_size.width;
                    max_height = max_height.max(child_size.height);
                }

                // Add spacing between children
                if !child_ids.is_empty() {
                    total_width += self.spacing * (child_ids.len() - 1) as f32;
                }

                constraints.constrain(Size::new(total_width, max_height))
            }
        }
    }

    fn paint(&self, tree: &ElementTree, child_ids: &[ElementId], offset: Offset) -> BoxedLayer {
        let mut container = pool::acquire_container();

        let mut current_offset = 0.0_f32;

        for (i, &child_id) in child_ids.iter().enumerate() {
            let child_size = self.child_sizes.get(i).copied().unwrap_or(Size::ZERO);

            let child_offset = match self.main_axis {
                Axis::Vertical => Offset::new(0.0, current_offset),
                Axis::Horizontal => Offset::new(current_offset, 0.0),
            };

            // Paint child with combined offset
            let child_layer = tree.paint_child(child_id, offset + child_offset);
            container.add_child(child_layer);

            current_offset += match self.main_axis {
                Axis::Vertical => child_size.height + self.spacing,
                Axis::Horizontal => child_size.width + self.spacing,
            };
        }

        Box::new(container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_body_new() {
        let data = RenderListBody::new(Axis::Vertical);
        assert_eq!(data.main_axis, Axis::Vertical);
        assert_eq!(data.spacing, 0.0);
    }

    #[test]
    fn test_list_body_vertical() {
        let data = RenderListBody::vertical();
        assert_eq!(data.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_list_body_horizontal() {
        let data = RenderListBody::horizontal();
        assert_eq!(data.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_list_body_with_spacing() {
        let data = RenderListBody::vertical().with_spacing(10.0);
        assert_eq!(data.spacing, 10.0);
    }

    #[test]
    fn test_list_body_default() {
        let data = RenderListBody::default();
        assert_eq!(data.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_render_list_body_set_main_axis() {
        let mut list = RenderListBody::vertical();
        list.set_main_axis(Axis::Horizontal);
        assert_eq!(list.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_render_list_body_set_spacing() {
        let mut list = RenderListBody::default();
        list.set_spacing(8.0);
        assert_eq!(list.spacing, 8.0);
    }
}
