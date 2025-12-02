//! RenderRotatedBox - rotates child by quarter turns (90°, 180°, 270°)

use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
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

impl RenderBox<Single> for RenderRotatedBox {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Single>) -> Size {
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

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        let child_id = ctx.children.single();

        // If no rotation, just paint child directly
        if self.quarter_turns == QuarterTurns::Zero {
            ctx.paint_child(child_id, ctx.offset);
            return;
        }

        // Read offset before taking mutable borrow
        let offset = ctx.offset;

        // Save canvas state
        ctx.canvas().save();

        // Move to rotation origin (our top-left)
        ctx.canvas().translate(offset.dx, offset.dy);

        // Apply rotation transform
        let angle_radians = self.quarter_turns.radians();
        ctx.canvas().rotate(angle_radians);

        // Calculate child offset in rotated space
        let child_offset = match self.quarter_turns {
            QuarterTurns::Zero => Offset::ZERO,
            QuarterTurns::One => Offset::new(0.0, -self.size.width), // 90° CW
            QuarterTurns::Two => Offset::new(-self.size.width, -self.size.height), // 180°
            QuarterTurns::Three => Offset::new(-self.size.height, 0.0), // 270° CW
        };

        // Paint child with rotated offset
        ctx.paint_child(child_id, child_offset);

        // Restore canvas state
        ctx.canvas().restore();
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
