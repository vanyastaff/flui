//! RenderRotatedBox - rotates child by quarter turns (90°, 180°, 270°)
//!
//! Flutter equivalent: `RenderRotatedBox`
//! Source: https://api.flutter.dev/flutter/rendering/RenderRotatedBox-class.html

use crate::core::{
    FullRenderTree,
    FullRenderTree, RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::constraints::BoxConstraints;
use flui_types::{geometry::QuarterTurns, Offset, Size};

/// RenderObject that rotates its child_id by quarter turns
///
/// Unlike RenderTransform which can do arbitrary rotations,
/// RenderRotatedBox only supports 90° increments and properly
/// adjusts layout constraints (swapping width/height for odd turns).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderRotatedBox;
///
/// // Rotate child_id 90° clockwise
/// let mut rotated = RenderRotatedBox::rotate_90();
/// ```
#[derive(Debug)]
pub struct RenderRotatedBox {
    /// Number of quarter turns clockwise
    pub quarter_turns: QuarterTurns,
    /// Cached size from layout phase
    size: Size,
}

// ===== Public API =====

impl RenderRotatedBox {
    /// Create new RenderRotatedBox
    pub fn new(quarter_turns: QuarterTurns) -> Self {
        Self {
            quarter_turns,
            size: Size::ZERO,
        }
    }

    /// Create with 90° rotation
    pub fn rotate_90() -> Self {
        Self::new(QuarterTurns::One)
    }

    /// Create with 180° rotation
    pub fn rotate_180() -> Self {
        Self::new(QuarterTurns::Two)
    }

    /// Create with 270° rotation
    pub fn rotate_270() -> Self {
        Self::new(QuarterTurns::Three)
    }

    /// Set quarter turns
    pub fn set_quarter_turns(&mut self, quarter_turns: QuarterTurns) {
        self.quarter_turns = quarter_turns;
    }
}

// ===== RenderObject Implementation =====

impl<T: FullRenderTree> RenderBox<T, Single> for RenderRotatedBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // For odd quarter turns (90°, 270°), swap width and height constraints
        let child_constraints = if self.quarter_turns.swaps_dimensions() {
            // Manually flip constraints - swap width and height
            BoxConstraints::new(
                ctx.constraints.min_height,
                ctx.constraints.max_height,
                ctx.constraints.min_width,
                ctx.constraints.max_width,
            )
        } else {
            ctx.constraints
        };

        // Layout child
        let child_size = ctx.layout_child(child_id, child_constraints);

        // Our size is child size with potentially swapped dimensions
        let size = if self.quarter_turns.swaps_dimensions() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
        };

        // Store size for paint phase
        self.size = size;
        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // If no rotation, just paint child directly
        if self.quarter_turns == QuarterTurns::Zero {
            ctx.paint_child(child_id, offset);
            return;
        }

        // Apply rotation transform using chaining API
        ctx.canvas()
            .saved()
            .translated(offset.dx, offset.dy)
            .rotated(self.quarter_turns.radians());

        // Calculate child offset in rotated space
        let child_offset = match self.quarter_turns {
            QuarterTurns::Zero => Offset::ZERO,
            QuarterTurns::One => Offset::new(0.0, -self.size.width), // 90° CW
            QuarterTurns::Two => Offset::new(-self.size.width, -self.size.height), // 180°
            QuarterTurns::Three => Offset::new(-self.size.height, 0.0), // 270° CW
        };

        // Paint child with rotated offset and restore canvas state
        ctx.paint_child(child_id, child_offset);
        ctx.canvas().restored();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_rotated_box_new() {
        let rotated = RenderRotatedBox::rotate_90();
        assert_eq!(rotated.quarter_turns, QuarterTurns::One);
    }

    #[test]
    fn test_render_rotated_box_set_quarter_turns() {
        let mut rotated = RenderRotatedBox::new(QuarterTurns::Zero);
        rotated.set_quarter_turns(QuarterTurns::Two);
        assert_eq!(rotated.quarter_turns, QuarterTurns::Two);
    }

    #[test]
    fn test_render_rotated_box_helpers() {
        let rotated_90 = RenderRotatedBox::rotate_90();
        assert_eq!(rotated_90.quarter_turns, QuarterTurns::One);

        let rotated_180 = RenderRotatedBox::rotate_180();
        assert_eq!(rotated_180.quarter_turns, QuarterTurns::Two);

        let rotated_270 = RenderRotatedBox::rotate_270();
        assert_eq!(rotated_270.quarter_turns, QuarterTurns::Three);
    }
}
