//! RenderShiftedBox - Shifts child painted position by fixed offset
//!
//! Implements a simple positioning utility that shifts a child's painted position
//! by a fixed pixel offset without affecting layout. Similar to absolute positioning
//! in CSS but for the child's paint position only.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderShiftedBox` | Similar to `RenderTransform.translate()` from `package:flutter/src/widgets/basic.dart` |
//! | `offset` | Translation offset (Offset) |
//! | `set_offset()` | `offset = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!    - Shift doesn't affect layout
//!
//! 2. **Cache size**
//!    - Store child size (not used in current impl, but available)
//!
//! 3. **Return child size**
//!    - Container size = child size (shift doesn't change size)
//!
//! # Paint Protocol
//!
//! 1. **Calculate shifted offset**
//!    - Shifted offset = parent offset + shift offset
//!    - Child painted at shifted position
//!
//! 2. **Paint child**
//!    - Child painted at shifted offset
//!    - No clipping applied (child can overflow if shifted)
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child + size cache
//! - **Paint**: O(1) - simple offset addition + child paint
//! - **Memory**: 24 bytes (Offset + Size cache)
//!
//! # Use Cases
//!
//! - **Fixed offsets**: Position child at specific pixel offset
//! - **Stacking layers**: Overlay elements with pixel-perfect positioning
//! - **Animation**: Animate position with absolute pixel offsets
//! - **Manual layout**: Fine-tune positioning by exact pixels
//! - **Tooltip positioning**: Position tooltips at fixed offsets
//! - **Debugging**: Temporarily shift elements to see overlaps
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderShiftedBox;
//! use flui_types::Offset;
//!
//! // Shift right 10px, down 20px
//! let shifted = RenderShiftedBox::new(Offset::new(10.0, 20.0));
//!
//! // Shift horizontally only
//! let right = RenderShiftedBox::shift_x(15.0);
//!
//! // Shift vertically only
//! let down = RenderShiftedBox::shift_y(25.0);
//!
//! // No shift (identity)
//! let identity = RenderShiftedBox::zero();
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::{Offset, Size};

/// RenderObject that shifts its child by a fixed pixel offset.
///
/// Positions child at a specific offset from container's origin. Affects only
/// painting, not layout. Child can overflow container if shifted.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged, only affects painting position.
///
/// # Use Cases
///
/// - **Fixed positioning**: Shift child by exact pixels
/// - **Layer stacking**: Overlay elements with precise positioning
/// - **Position animation**: Animate with absolute pixel offsets
/// - **Manual layout**: Fine-tune positioning manually
/// - **Tooltip offsets**: Position tooltips precisely
/// - **Debugging**: Temporarily shift to reveal overlaps
///
/// # Flutter Compliance
///
/// Similar to Flutter's Transform.translate behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child (shift doesn't affect size)
/// - Child painted at shifted position
/// - No clipping applied (can overflow)
/// - Affects only painting, not layout or hit testing
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderShiftedBox;
/// use flui_types::Offset;
///
/// // Shift down-right
/// let shifted = RenderShiftedBox::new(Offset::new(10.0, 20.0));
///
/// // Shift left-up (negative offsets)
/// let shifted_back = RenderShiftedBox::new(Offset::new(-5.0, -10.0));
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
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        let size = ctx.layout_child(child_id, ctx.constraints)?;

        // Store size for paint (available but not currently used)
        self.size = size;

        // Return child's size unchanged (shift doesn't affect layout)
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Paint child at shifted position
        // shifted_offset = parent_offset + shift_offset
        let child_offset = ctx.offset + self.offset;

        ctx.paint_child(child_id, child_offset);
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
