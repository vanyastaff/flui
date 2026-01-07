//! RenderColoredBox - a simple colored rectangle.

use flui_painting::Paint;
use flui_types::{Color, Offset, Point, Rect, Size};

use crate::arity::Leaf;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext, CanvasContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// A render object that paints a colored rectangle.
#[derive(Debug, Clone)]
pub struct RenderColoredBox {
    color: [f32; 4],
    preferred_size: Size,
    size: Size,
}

impl RenderColoredBox {
    /// Creates a new colored box.
    pub fn new(color: [f32; 4], preferred_size: Size) -> Self {
        Self {
            color,
            preferred_size,
            size: Size::ZERO,
        }
    }

    /// Creates a red box.
    pub fn red(width: f32, height: f32) -> Self {
        Self::new([1.0, 0.0, 0.0, 1.0], Size::new(width, height))
    }

    /// Creates a green box.
    pub fn green(width: f32, height: f32) -> Self {
        Self::new([0.0, 1.0, 0.0, 1.0], Size::new(width, height))
    }

    /// Creates a blue box.
    pub fn blue(width: f32, height: f32) -> Self {
        Self::new([0.0, 0.0, 1.0, 1.0], Size::new(width, height))
    }

    /// Returns the color.
    pub fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Returns the preferred size.
    pub fn preferred_size(&self) -> Size {
        self.preferred_size
    }
}

impl flui_foundation::Diagnosticable for RenderColoredBox {}
impl RenderBox for RenderColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        let constrained = ctx.constrain(self.preferred_size);
        self.size = constrained;
        tracing::debug!(
            "RenderColoredBox::perform_layout: preferred={:?}, constrained={:?}",
            self.preferred_size,
            constrained
        );
        ctx.complete_with_size(constrained);
    }

    fn size(&self) -> &Size {
        &self.size
    }
    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Leaf, BoxParentData>) {
        // Painting is done via paint_with_canvas
    }

    fn paint_with_canvas(&self, context: &mut CanvasContext, offset: Offset) {
        let rect = Rect::from_origin_size(Point::new(offset.dx, offset.dy), self.size);
        tracing::debug!(
            "RenderColoredBox::paint_with_canvas: offset=({}, {}), size={:?}, rect={:?}",
            offset.dx,
            offset.dy,
            self.size,
            rect
        );
        let color = Color::from_rgba_f32_array(self.color);
        let paint = Paint::fill(color);
        context.canvas().draw_rect(rect, &paint);
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        ctx.is_within_size(self.size.width, self.size.height)
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

