//! RenderAbsorbPointer - prevents pointer events from reaching children

use crate::core::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_types::{Offset, Rect, Size};

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

impl RenderObject for RenderAbsorbPointer {}

impl RenderBox<Single> for RenderAbsorbPointer {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Layout child with same constraints
        Ok(ctx
            .layout_single_child()
            .unwrap_or_else(|_| ctx.constraints.smallest()))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Paint child normally - absorbing only affects hit testing
        let _ = ctx.paint_single_child(Offset::ZERO);
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        if self.absorbing {
            // Absorb pointer events - add self to result but DON'T test children
            // This prevents events from reaching the child
            let bounds = Rect::from_min_size(Offset::ZERO, ctx.size());
            let entry = HitTestEntry::new(ctx.element_id(), ctx.position, bounds);
            result.add(entry);
            true // Event absorbed!
        } else {
            // Not absorbing - use default behavior (test children)
            ctx.hit_test_children(result)
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
