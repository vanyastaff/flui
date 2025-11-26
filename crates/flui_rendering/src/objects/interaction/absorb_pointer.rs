//! RenderAbsorbPointer - prevents pointer events from reaching children



use crate::core::{
    HitTestTree, RenderBox, Single,
    {BoxProtocol, HitTestContext, LayoutContext, PaintContext},
};
use flui_interaction::HitTestResult;
use flui_types::Size;

/// RenderObject that prevents pointer events from reaching its child
///
/// When absorbing is true, this widget consumes all pointer events,
/// preventing them from reaching the child. The child is still painted
/// but doesn't receive events.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAbsorbPointer;
///
/// let mut absorb = RenderAbsorbPointer::new(true);
/// ```
#[derive(Debug)]
pub struct RenderAbsorbPointer {
    /// Whether to absorb pointer events
    pub absorbing: bool,
}

impl RenderAbsorbPointer {
    /// Create new RenderAbsorbPointer
    pub fn new(absorbing: bool) -> Self {
        Self { absorbing }
    }

    /// Check if absorbing pointer events
    pub fn absorbing(&self) -> bool {
        self.absorbing
    }

    /// Set whether to absorb pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_absorbing(&mut self, absorbing: bool) {
        self.absorbing = absorbing;
        // Note: In a full implementation, this would mark needs hit test update
    }
}

impl Default for RenderAbsorbPointer {
    fn default() -> Self {
        Self { absorbing: true }
    }
}

impl RenderBox<Single> for RenderAbsorbPointer {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        // Paint child normally - absorbing only affects hit testing
        ctx.paint_child(child_id, ctx.offset);
    }

    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        if self.absorbing {
            // Absorb pointer events - add self to result but DON'T test children
            // This prevents events from reaching the child
            ctx.add_to_result(result);
            true // Event absorbed!
        } else {
            // Not absorbing - use default behavior (test children)
            self.hit_test_children(ctx, result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_absorb_pointer_new() {
        let absorb = RenderAbsorbPointer::new(true);
        assert!(absorb.absorbing());

        let absorb = RenderAbsorbPointer::new(false);
        assert!(!absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_default() {
        let absorb = RenderAbsorbPointer::default();
        assert!(absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_set_absorbing() {
        let mut absorb = RenderAbsorbPointer::new(true);

        absorb.set_absorbing(false);
        assert!(!absorb.absorbing());

        absorb.set_absorbing(true);
        assert!(absorb.absorbing());
    }
}
