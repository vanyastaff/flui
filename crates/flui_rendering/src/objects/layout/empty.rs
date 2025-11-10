//! RenderEmpty - a render object that does nothing

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::Size;

/// A render object that renders nothing
///
/// This is used as a placeholder for widgets that need a child but
/// don't have meaningful content (e.g., spacers).
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderEmpty;

impl Render for RenderEmpty {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        // Take minimum space
        let constraints = ctx.constraints;
        Size::new(constraints.min_width, constraints.min_height)
    }

    fn paint(&self, _ctx: &PaintContext) -> Canvas {
        // Return empty canvas - nothing to paint
        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0)
    }
}
