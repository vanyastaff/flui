//! RenderOffstage - hides widget from display

use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_types::Size;

/// RenderObject that hides its child from display
///
/// When offstage is true:
/// - The child is not painted
/// - The child is laid out (to maintain its state)
/// - The size is reported as zero
///
/// This is different from Opacity(0) - the child doesn't take up space.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOffstage;
///
/// let offstage = RenderOffstage::new(true);
/// ```
#[derive(Debug)]
pub struct RenderOffstage {
    /// Whether the child is offstage (hidden)
    pub offstage: bool,
}

impl RenderOffstage {
    /// Create new RenderOffstage
    pub fn new(offstage: bool) -> Self {
        Self { offstage }
    }

    /// Set whether child is offstage
    pub fn set_offstage(&mut self, offstage: bool) {
        self.offstage = offstage;
    }
}

impl Default for RenderOffstage {
    fn default() -> Self {
        Self { offstage: true }
    }
}

impl RenderBox<Single> for RenderOffstage {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Single>) -> Size {
        let child_id = ctx.children.single();
        // Single arity always has exactly one child - layout it to maintain state
        let child_size = ctx.layout_child(child_id, ctx.constraints);

        // Report size as zero if offstage, otherwise use child size
        if self.offstage {
            Size::ZERO
        } else if child_size != Size::ZERO {
            child_size
        } else {
            ctx.constraints.smallest()
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Don't paint if offstage
        if !self.offstage {
            let child_id = ctx.children.single();
            ctx.paint_child(child_id, ctx.offset);
        }
        // When offstage, don't paint anything (empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_offstage_new() {
        let offstage = RenderOffstage::new(true);
        assert!(offstage.offstage);

        let offstage = RenderOffstage::new(false);
        assert!(!offstage.offstage);
    }

    #[test]
    fn test_render_offstage_default() {
        let offstage = RenderOffstage::default();
        assert!(offstage.offstage);
    }

    #[test]
    fn test_render_offstage_set_offstage() {
        let mut offstage = RenderOffstage::new(true);
        offstage.set_offstage(false);
        assert!(!offstage.offstage);
    }
}
