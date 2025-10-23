//! RenderListBody - simple scrollable list layout

use flui_types::{Offset, Size, constraints::BoxConstraints, Axis};
use flui_core::DynRenderObject;
use crate::core::{ContainerRenderBox, RenderBoxMixin};

/// Data for RenderListBody
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListBodyData {
    /// Main axis direction (horizontal or vertical)
    pub main_axis: Axis,
    /// Spacing between children
    pub spacing: f32,
}

impl ListBodyData {
    /// Create new list body data
    pub fn new(main_axis: Axis) -> Self {
        Self {
            main_axis,
            spacing: 0.0,
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
}

impl Default for ListBodyData {
    fn default() -> Self {
        Self::vertical()
    }
}

/// RenderObject that arranges children in a simple scrollable list
///
/// Unlike Flex, ListBody doesn't support flex factors. All children
/// are sized to their intrinsic size along the main axis.
/// Useful for simple scrollable lists.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{ContainerRenderBox, objects::layout::ListBodyData};
///
/// // Create vertical list with spacing
/// let mut list = ContainerRenderBox::new(
///     ListBodyData::vertical().with_spacing(8.0)
/// );
/// ```
pub type RenderListBody = ContainerRenderBox<ListBodyData>;

// ===== Public API =====

impl RenderListBody {
    /// Get main axis
    pub fn main_axis(&self) -> Axis {
        self.data.main_axis
    }

    /// Get spacing
    pub fn spacing(&self) -> f32 {
        self.data.spacing
    }

    /// Set main axis
    pub fn set_main_axis(&mut self, main_axis: Axis) {
        if self.data.main_axis != main_axis {
            self.data.main_axis = main_axis;
            self.mark_needs_layout();
        }
    }

    /// Set spacing
    pub fn set_spacing(&mut self, spacing: f32) {
        if self.data.spacing != spacing {
            self.data.spacing = spacing;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderListBody {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let main_axis = self.data.main_axis;
        let spacing = self.data.spacing;
        let children_ids = ctx.children();

        // Early return if no children
        if children_ids.is_empty() {
            let size = constraints.smallest();
            *state.size.lock() = Some(size);
            state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
            return size;
        }

        // Layout children based on axis
        let size = match main_axis {
            Axis::Vertical => self.layout_vertical(constraints, spacing, ctx),
            Axis::Horizontal => self.layout_horizontal(constraints, spacing, ctx),
        };

        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Paint all children at their positions
        // In a real implementation, we would store child positions during layout
        let main_axis = self.data.main_axis;
        let spacing = self.data.spacing;
        let children_ids = ctx.children();

        let mut current_offset = match main_axis {
            Axis::Vertical => 0.0_f32,
            Axis::Horizontal => 0.0_f32,
        };

        for &child_id in children_ids {
            // Get child size
            let child_size = if let Some(child_elem) = ctx.tree().get(child_id) {
                if let Some(child_ro) = child_elem.render_object() {
                    child_ro.size()
                } else {
                    Size::ZERO
                }
            } else {
                Size::ZERO
            };

            let paint_offset = match main_axis {
                Axis::Vertical => Offset::new(offset.dx, offset.dy + current_offset),
                Axis::Horizontal => Offset::new(offset.dx + current_offset, offset.dy),
            };

            ctx.paint_child(child_id, painter, paint_offset);

            current_offset += match main_axis {
                Axis::Vertical => child_size.height + spacing,
                Axis::Horizontal => child_size.width + spacing,
            };
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

// ===== Private Layout Methods =====

impl RenderListBody {
    /// Layout children vertically
    fn layout_vertical(&self, constraints: BoxConstraints, spacing: f32, ctx: &flui_core::RenderContext) -> Size {
        let mut total_height = 0.0_f32;
        let mut max_width = 0.0_f32;
        let children_ids = ctx.children();

        // Layout each child
        for &child_id in children_ids {
            // Child gets parent's width constraints, infinite height
            let child_constraints = BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f32::INFINITY,
            );

            let child_size = ctx.layout_child(child_id, child_constraints);

            total_height += child_size.height;
            max_width = max_width.max(child_size.width);
        }

        // Add spacing between children
        if !children_ids.is_empty() {
            total_height += spacing * (children_ids.len() - 1) as f32;
        }

        // Constrain to parent constraints
        constraints.constrain(Size::new(max_width, total_height))
    }

    /// Layout children horizontally
    fn layout_horizontal(&self, constraints: BoxConstraints, spacing: f32, ctx: &flui_core::RenderContext) -> Size {
        let mut total_width = 0.0_f32;
        let mut max_height = 0.0_f32;
        let children_ids = ctx.children();

        // Layout each child
        for &child_id in children_ids {
            // Child gets infinite width, parent's height constraints
            let child_constraints = BoxConstraints::new(
                0.0,
                f32::INFINITY,
                constraints.min_height,
                constraints.max_height,
            );

            let child_size = ctx.layout_child(child_id, child_constraints);

            total_width += child_size.width;
            max_height = max_height.max(child_size.height);
        }

        // Add spacing between children
        if !children_ids.is_empty() {
            total_width += spacing * (children_ids.len() - 1) as f32;
        }

        // Constrain to parent constraints
        constraints.constrain(Size::new(total_width, max_height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_body_data_new() {
        let data = ListBodyData::new(Axis::Vertical);
        assert_eq!(data.main_axis, Axis::Vertical);
        assert_eq!(data.spacing, 0.0);
    }

    #[test]
    fn test_list_body_data_vertical() {
        let data = ListBodyData::vertical();
        assert_eq!(data.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_list_body_data_horizontal() {
        let data = ListBodyData::horizontal();
        assert_eq!(data.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_list_body_data_with_spacing() {
        let data = ListBodyData::vertical().with_spacing(10.0);
        assert_eq!(data.spacing, 10.0);
    }

    #[test]
    fn test_list_body_data_default() {
        let data = ListBodyData::default();
        assert_eq!(data.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_render_list_body_new() {
        let list = ContainerRenderBox::new(ListBodyData::vertical());
        assert_eq!(list.main_axis(), Axis::Vertical);
        assert_eq!(list.spacing(), 0.0);
    }

    #[test]
    fn test_render_list_body_set_main_axis() {
        let mut list = ContainerRenderBox::new(ListBodyData::vertical());

        list.set_main_axis(Axis::Horizontal);
        assert_eq!(list.main_axis(), Axis::Horizontal);
        assert!(list.needs_layout());
    }

    #[test]
    fn test_render_list_body_set_spacing() {
        let mut list = ContainerRenderBox::new(ListBodyData::default());

        list.set_spacing(8.0);
        assert_eq!(list.spacing(), 8.0);
        assert!(list.needs_layout());
    }

    #[test]
    #[cfg(disabled_test)] // TODO: Update test to use RenderContext
    fn test_render_list_body_layout_no_children() {
        let mut list = ContainerRenderBox::new(ListBodyData::vertical());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = list.layout(constraints);

        // No children, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
