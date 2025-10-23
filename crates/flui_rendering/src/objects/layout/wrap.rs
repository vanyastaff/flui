//! RenderWrap - arranges children with wrapping (like flexbox wrap)

use flui_types::{Offset, Size, constraints::BoxConstraints, Axis};
use flui_core::DynRenderObject;
use crate::core::{ContainerRenderBox, RenderBoxMixin};

/// Alignment for runs in wrap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapAlignment {
    /// Place runs at the start
    Start,
    /// Place runs at the end
    End,
    /// Center runs
    Center,
    /// Space runs evenly
    SpaceBetween,
    /// Space runs with space around
    SpaceAround,
    /// Space runs evenly with equal space
    SpaceEvenly,
}

/// Cross-axis alignment for children within a run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapCrossAlignment {
    /// Align to start of cross axis
    Start,
    /// Align to end of cross axis
    End,
    /// Center on cross axis
    Center,
}

/// Data for RenderWrap
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WrapData {
    /// Main axis direction (horizontal or vertical)
    pub direction: Axis,
    /// Alignment of runs along main axis
    pub alignment: WrapAlignment,
    /// Spacing between children in a run
    pub spacing: f32,
    /// Spacing between runs
    pub run_spacing: f32,
    /// Cross-axis alignment within a run
    pub cross_alignment: WrapCrossAlignment,
}

impl WrapData {
    /// Create new wrap data
    pub fn new(direction: Axis) -> Self {
        Self {
            direction,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_spacing: 0.0,
            cross_alignment: WrapCrossAlignment::Start,
        }
    }

    /// Create horizontal wrap
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Create vertical wrap
    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Set spacing
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Set run spacing
    pub fn with_run_spacing(mut self, run_spacing: f32) -> Self {
        self.run_spacing = run_spacing;
        self
    }
}

impl Default for WrapData {
    fn default() -> Self {
        Self::horizontal()
    }
}

/// RenderObject that arranges children with wrapping
///
/// Like Flex (Row/Column), but wraps to the next line when reaching
/// the edge of the container.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{ContainerRenderBox, objects::layout::WrapData};
///
/// // Create horizontal wrap with spacing
/// let mut wrap = ContainerRenderBox::new(
///     WrapData::horizontal().with_spacing(8.0).with_run_spacing(4.0)
/// );
/// ```
pub type RenderWrap = ContainerRenderBox<WrapData>;

// ===== Public API =====

impl RenderWrap {
    /// Get direction
    pub fn direction(&self) -> Axis {
        self.data.direction
    }

    /// Get spacing
    pub fn spacing(&self) -> f32 {
        self.data.spacing
    }

    /// Get run spacing
    pub fn run_spacing(&self) -> f32 {
        self.data.run_spacing
    }

    /// Set direction
    pub fn set_direction(&mut self, direction: Axis) {
        if self.data.direction != direction {
            self.data.direction = direction;
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

impl DynRenderObject for RenderWrap {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let data = self.data;
        let spacing = data.spacing;
        let run_spacing = data.run_spacing;
        let children_ids = ctx.children();

        // Early return if no children
        if children_ids.is_empty() {
            let size = constraints.smallest();
            *state.size.lock() = Some(size);
            state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
            return size;
        }

        // Layout algorithm depends on direction
        let (main_size, cross_size) = match data.direction {
            Axis::Horizontal => {
                self.layout_horizontal(constraints, spacing, run_spacing, ctx)
            }
            Axis::Vertical => {
                self.layout_vertical(constraints, spacing, run_spacing, ctx)
            }
        };

        let size = Size::new(main_size, cross_size);
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Paint all children at their positions
        let children_ids = ctx.children();

        for &child_id in children_ids {
            // In a real implementation, we would store child positions
            // during layout and use them here
            // For now, we paint all children at the same offset
            ctx.paint_child(child_id, painter, offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

// ===== Private Layout Methods =====

impl RenderWrap {
    /// Layout children horizontally with wrapping
    fn layout_horizontal(&self, constraints: BoxConstraints, spacing: f32, run_spacing: f32, ctx: &flui_core::RenderContext) -> (f32, f32) {
        let max_width = constraints.max_width;
        let mut current_x = 0.0_f32;
        let mut current_y = 0.0_f32;
        let mut max_run_height = 0.0_f32;
        let mut total_width = 0.0_f32;
        let children_ids = ctx.children();

        // Layout each child
        for &child_id in children_ids {
            // Child gets unconstrained width, constrained height
            let child_constraints = BoxConstraints::new(
                0.0,
                max_width - current_x,
                0.0,
                constraints.max_height,
            );

            let child_size = ctx.layout_child(child_id, child_constraints);

            // Check if we need to wrap
            if current_x + child_size.width > max_width && current_x > 0.0 {
                // Wrap to next line
                current_y += max_run_height + run_spacing;
                current_x = 0.0;
                max_run_height = 0.0;
            }

            // Place child
            current_x += child_size.width + spacing;
            max_run_height = max_run_height.max(child_size.height);
            total_width = total_width.max(current_x - spacing);
        }

        let total_height = current_y + max_run_height;
        (total_width.max(0.0), total_height.max(0.0))
    }

    /// Layout children vertically with wrapping
    fn layout_vertical(&self, constraints: BoxConstraints, spacing: f32, run_spacing: f32, ctx: &flui_core::RenderContext) -> (f32, f32) {
        let max_height = constraints.max_height;
        let mut current_x = 0.0_f32;
        let mut current_y = 0.0_f32;
        let mut max_run_width = 0.0_f32;
        let mut total_height = 0.0_f32;
        let children_ids = ctx.children();

        // Layout each child
        for &child_id in children_ids {
            // Child gets constrained width, unconstrained height
            let child_constraints = BoxConstraints::new(
                0.0,
                constraints.max_width,
                0.0,
                max_height - current_y,
            );

            let child_size = ctx.layout_child(child_id, child_constraints);

            // Check if we need to wrap
            if current_y + child_size.height > max_height && current_y > 0.0 {
                // Wrap to next column
                current_x += max_run_width + run_spacing;
                current_y = 0.0;
                max_run_width = 0.0;
            }

            // Place child
            current_y += child_size.height + spacing;
            max_run_width = max_run_width.max(child_size.width);
            total_height = total_height.max(current_y - spacing);
        }

        let total_width = current_x + max_run_width;
        (total_width.max(0.0), total_height.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_alignment_variants() {
        assert_ne!(WrapAlignment::Start, WrapAlignment::End);
        assert_ne!(WrapAlignment::Center, WrapAlignment::SpaceBetween);
    }

    #[test]
    fn test_wrap_data_new() {
        let data = WrapData::new(Axis::Horizontal);
        assert_eq!(data.direction, Axis::Horizontal);
        assert_eq!(data.spacing, 0.0);
        assert_eq!(data.run_spacing, 0.0);
    }

    #[test]
    fn test_wrap_data_horizontal() {
        let data = WrapData::horizontal();
        assert_eq!(data.direction, Axis::Horizontal);
    }

    #[test]
    fn test_wrap_data_vertical() {
        let data = WrapData::vertical();
        assert_eq!(data.direction, Axis::Vertical);
    }

    #[test]
    fn test_wrap_data_with_spacing() {
        let data = WrapData::horizontal().with_spacing(10.0).with_run_spacing(5.0);
        assert_eq!(data.spacing, 10.0);
        assert_eq!(data.run_spacing, 5.0);
    }

    #[test]
    fn test_render_wrap_new() {
        let wrap = ContainerRenderBox::new(WrapData::horizontal());
        assert_eq!(wrap.direction(), Axis::Horizontal);
        assert_eq!(wrap.spacing(), 0.0);
    }

    #[test]
    fn test_render_wrap_set_direction() {
        let mut wrap = ContainerRenderBox::new(WrapData::horizontal());

        wrap.set_direction(Axis::Vertical);
        assert_eq!(wrap.direction(), Axis::Vertical);
        assert!(wrap.needs_layout());
    }

    #[test]
    fn test_render_wrap_set_spacing() {
        let mut wrap = ContainerRenderBox::new(WrapData::default());

        wrap.set_spacing(8.0);
        assert_eq!(wrap.spacing(), 8.0);
        assert!(wrap.needs_layout());
    }

    #[test]
    #[cfg(disabled_test)] // TODO: Update test to use RenderContext
    fn test_render_wrap_layout_no_children() {
        let mut wrap = ContainerRenderBox::new(WrapData::horizontal());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = wrap.layout(constraints);

        // No children, should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
