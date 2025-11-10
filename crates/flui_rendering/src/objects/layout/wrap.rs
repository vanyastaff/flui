//! RenderWrap - arranges children with wrapping (like flexbox wrap)

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;

use flui_engine::{layer::pool, BoxedLayer};
use flui_types::constraints::BoxConstraints;
use flui_types::{Axis, Offset, Size};

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

/// RenderObject that arranges children with wrapping
///
/// Like Flex (Row/Column), but wraps to the next line when reaching
/// the edge of the container.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderWrap;
///
/// // Create horizontal wrap with spacing
/// let mut wrap = RenderWrap::horizontal().with_spacing(8.0).with_run_spacing(4.0);
/// ```
#[derive(Debug)]
pub struct RenderWrap {
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

    // Cache for paint
    child_offsets: Vec<Offset>,
}

impl RenderWrap {
    /// Create new wrap
    pub fn new(direction: Axis) -> Self {
        Self {
            direction,
            alignment: WrapAlignment::Start,
            spacing: 0.0,
            run_spacing: 0.0,
            cross_alignment: WrapCrossAlignment::Start,
            child_offsets: Vec::new(),
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

    /// Set direction
    pub fn set_direction(&mut self, direction: Axis) {
        self.direction = direction;
    }

    /// Set spacing
    pub fn set_spacing(&mut self, spacing: f32) {
        self.spacing = spacing;
    }
}

impl Default for RenderWrap {
    fn default() -> Self {
        Self::horizontal()
    }
}

impl Render for RenderWrap {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_ids = ctx.children.as_slice();
        let constraints = ctx.constraints;
        if child_ids.is_empty() {
            self.child_offsets.clear();
            return constraints.smallest();
        }

        // Layout algorithm depends on direction
        self.child_offsets.clear();

        match self.direction {
            Axis::Horizontal => {
                let max_width = constraints.max_width;
                let mut current_x = 0.0_f32;
                let mut current_y = 0.0_f32;
                let mut max_run_height = 0.0_f32;
                let mut total_width = 0.0_f32;

                for child in child_ids.iter().copied() {
                    // Child gets unconstrained width, constrained height
                    let child_constraints = BoxConstraints::new(
                        0.0,
                        max_width - current_x,
                        0.0,
                        constraints.max_height,
                    );

                    let child_size = tree.layout_child(child, child_constraints);

                    // Check if we need to wrap
                    if current_x + child_size.width > max_width && current_x > 0.0 {
                        // Wrap to next line
                        current_y += max_run_height + self.run_spacing;
                        current_x = 0.0;
                        max_run_height = 0.0;
                    }

                    // Store child offset
                    self.child_offsets.push(Offset::new(current_x, current_y));

                    // Place child
                    current_x += child_size.width + self.spacing;
                    max_run_height = max_run_height.max(child_size.height);
                    total_width = total_width.max(current_x - self.spacing);
                }

                let total_height = current_y + max_run_height;
                Size::new(total_width.max(0.0), total_height.max(0.0))
            }
            Axis::Vertical => {
                let max_height = constraints.max_height;
                let mut current_x = 0.0_f32;
                let mut current_y = 0.0_f32;
                let mut max_run_width = 0.0_f32;
                let mut total_height = 0.0_f32;

                for child in child_ids.iter().copied() {
                    // Child gets constrained width, unconstrained height
                    let child_constraints = BoxConstraints::new(
                        0.0,
                        constraints.max_width,
                        0.0,
                        max_height - current_y,
                    );

                    let child_size = tree.layout_child(child, child_constraints);

                    // Check if we need to wrap
                    if current_y + child_size.height > max_height && current_y > 0.0 {
                        // Wrap to next column
                        current_x += max_run_width + self.run_spacing;
                        current_y = 0.0;
                        max_run_width = 0.0;
                    }

                    // Store child offset
                    self.child_offsets.push(Offset::new(current_x, current_y));

                    // Place child
                    current_y += child_size.height + self.spacing;
                    max_run_width = max_run_width.max(child_size.width);
                    total_height = total_height.max(current_y - self.spacing);
                }

                let total_width = current_x + max_run_width;
                Size::new(total_width.max(0.0), total_height.max(0.0))
            }
        }
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_ids = ctx.children.as_slice();
        let offset = ctx.offset;
        let mut container = pool::acquire_container();

        for (i, &child_id) in child_ids.iter().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);

            // Paint child with combined offset
            let child_layer = tree.paint_child(child_id, offset + child_offset);
            container.add_child(child_layer);
        }

        Box::new(container)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Multi-child container
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
    fn test_wrap_new() {
        let wrap = RenderWrap::new(Axis::Horizontal);
        assert_eq!(wrap.direction, Axis::Horizontal);
        assert_eq!(wrap.spacing, 0.0);
        assert_eq!(wrap.run_spacing, 0.0);
    }

    #[test]
    fn test_wrap_horizontal() {
        let wrap = RenderWrap::horizontal();
        assert_eq!(wrap.direction, Axis::Horizontal);
    }

    #[test]
    fn test_wrap_vertical() {
        let wrap = RenderWrap::vertical();
        assert_eq!(wrap.direction, Axis::Vertical);
    }

    #[test]
    fn test_wrap_with_spacing() {
        let wrap = RenderWrap::horizontal()
            .with_spacing(10.0)
            .with_run_spacing(5.0);
        assert_eq!(wrap.spacing, 10.0);
        assert_eq!(wrap.run_spacing, 5.0);
    }

    #[test]
    fn test_render_wrap_set_direction() {
        let mut wrap = RenderWrap::horizontal();
        wrap.set_direction(Axis::Vertical);
        assert_eq!(wrap.direction, Axis::Vertical);
    }

    #[test]
    fn test_render_wrap_set_spacing() {
        let mut wrap = RenderWrap::default();
        wrap.set_spacing(8.0);
        assert_eq!(wrap.spacing, 8.0);

        
    }
}
