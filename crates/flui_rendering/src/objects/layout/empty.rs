//! RenderEmpty - a render object that does nothing

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::traits::Render;
use flui_core::render::Leaf;
use flui_types::Size;

/// A render object that renders nothing
///
/// This is used as a placeholder for widgets that need a child but
/// don't have meaningful content (e.g., spacers).
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderEmpty;

impl RenderBox<Leaf> for RenderEmpty {
    fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
        // Take minimum space
        let constraints = &ctx.constraints;
        flui_types::Size::new(constraints.min_width, constraints.min_height)
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        // Nothing to paint
    }
}
