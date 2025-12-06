//! RenderShiftedBox - Shifts child position by an offset

use crate::{RenderObject, RenderResult};

use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_types::{Offset, Size};

/// RenderObject that shifts its child by a fixed offset
///
/// This is a simple utility RenderObject that positions its child at a specific
/// offset from its own origin. The child is laid out with the full constraints,
/// and the resulting size is returned unchanged.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderShiftedBox;
/// use flui_types::Offset;
///
/// // Shift child 10px right, 20px down
/// let shifted = RenderShiftedBox::new(Offset::new(10.0, 20.0));
/// ```
#[derive(Debug)]
pub struct RenderShiftedBox {
    /// Offset to shift the child by
    pub offset: Offset,

    // Cache for paint
    size: Size,
}

impl RenderShiftedBox {
    /// Create new RenderShiftedBox with given offset
    pub fn new(offset: Offset) -> Self {
        Self {
            offset,
            size: Size::ZERO,
        }
    }

    /// Create with zero offset (no shift)
    pub fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Create with X shift only
    pub fn shift_x(dx: f32) -> Self {
        Self::new(Offset::new(dx, 0.0))
    }

    /// Create with Y shift only
    pub fn shift_y(dy: f32) -> Self {
        Self::new(Offset::new(0.0, dy))
    }

    /// Set the shift offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Set the X component of the offset
    pub fn set_dx(&mut self, dx: f32) {
        self.offset.dx = dx;
    }

    /// Set the Y component of the offset
    pub fn set_dy(&mut self, dy: f32) {
        self.offset.dy = dy;
    }
}

impl Default for RenderShiftedBox {
    fn default() -> Self {
        Self::zero()
    }
}

impl RenderObject for RenderShiftedBox {}

impl RenderBox<Single> for RenderShiftedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let child_id = *ctx.children.single();

        // Layout child with full constraints
        let size = ctx.layout_child(child_id, ctx.constraints)?;

        // Store size for paint
        self.size = size;

        // Return child's size unchanged
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = *ctx.children.single();

        // Paint child at shifted position
        let child_offset = Offset::new(
            ctx.offset.dx + self.offset.dx,
            ctx.offset.dy + self.offset.dy,
        );

        let _ = ctx.paint_child(child_id, child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_shifted_box_new() {
        let offset = Offset::new(10.0, 20.0);
        let shifted = RenderShiftedBox::new(offset);

        assert_eq!(shifted.offset.dx, 10.0);
        assert_eq!(shifted.offset.dy, 20.0);
    }

    #[test]
    fn test_render_shifted_box_zero() {
        let shifted = RenderShiftedBox::zero();

        assert_eq!(shifted.offset.dx, 0.0);
        assert_eq!(shifted.offset.dy, 0.0);
    }

    #[test]
    fn test_render_shifted_box_shift_x() {
        let shifted = RenderShiftedBox::shift_x(15.0);

        assert_eq!(shifted.offset.dx, 15.0);
        assert_eq!(shifted.offset.dy, 0.0);
    }

    #[test]
    fn test_render_shifted_box_shift_y() {
        let shifted = RenderShiftedBox::shift_y(25.0);

        assert_eq!(shifted.offset.dx, 0.0);
        assert_eq!(shifted.offset.dy, 25.0);
    }

    #[test]
    fn test_render_shifted_box_default() {
        let shifted = RenderShiftedBox::default();

        assert_eq!(shifted.offset.dx, 0.0);
        assert_eq!(shifted.offset.dy, 0.0);
    }

    #[test]
    fn test_render_shifted_box_set_offset() {
        let mut shifted = RenderShiftedBox::zero();
        shifted.set_offset(Offset::new(5.0, 10.0));

        assert_eq!(shifted.offset.dx, 5.0);
        assert_eq!(shifted.offset.dy, 10.0);
    }

    #[test]
    fn test_render_shifted_box_set_dx() {
        let mut shifted = RenderShiftedBox::new(Offset::new(10.0, 20.0));
        shifted.set_dx(30.0);

        assert_eq!(shifted.offset.dx, 30.0);
        assert_eq!(shifted.offset.dy, 20.0); // Unchanged
    }

    #[test]
    fn test_render_shifted_box_set_dy() {
        let mut shifted = RenderShiftedBox::new(Offset::new(10.0, 20.0));
        shifted.set_dy(40.0);

        assert_eq!(shifted.offset.dx, 10.0); // Unchanged
        assert_eq!(shifted.offset.dy, 40.0);
    }
}
