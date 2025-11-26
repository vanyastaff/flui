//! RenderEmpty - a render object that does nothing

use crate::core::{BoxProtocol, LayoutContext, LayoutTree, Leaf, PaintContext, PaintTree, RenderBox};
use flui_types::Size;

/// A render object that renders nothing
///
/// This is used as a placeholder for widgets that need a child but
/// don't have meaningful content (e.g., spacers).
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderEmpty;

impl RenderBox<Leaf> for RenderEmpty {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        // Take minimum space
        let constraints = &ctx.constraints;
        flui_types::Size::new(constraints.min_width, constraints.min_height)
    }

    fn paint<T>(&self, _ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: PaintTree,
    {
        // Nothing to paint
    }
}
