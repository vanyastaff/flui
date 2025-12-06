//! RenderRotatedBox - Rotates child by quarter turns (90°, 180°, 270°)
//!
//! Implements Flutter's RotatedBox that rotates its child by multiples of 90
//! degrees. Unlike arbitrary rotation transforms, quarter-turn rotations properly
//! swap layout constraints (width ↔ height) for odd rotations.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderRotatedBox` | `RenderRotatedBox` from `package:flutter/src/rendering/shifted_box.dart` |
//! | `quarter_turns` | `quarterTurns` property (int, clockwise) |
//! | `set_quarter_turns()` | `quarterTurns = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Check rotation amount**
//!    - Odd turns (90°, 270°): Swap width ↔ height constraints
//!    - Even turns (0°, 180°): Pass constraints unchanged
//!
//! 2. **Layout child**
//!    - Child laid out with potentially swapped constraints
//!    - Child determines its size in rotated coordinate space
//!
//! 3. **Calculate container size**
//!    - Odd turns: Swap child dimensions (width ↔ height)
//!    - Even turns: Use child size unchanged
//!
//! # Paint Protocol
//!
//! 1. **Apply rotation transform**
//!    - Save canvas state
//!    - Rotate canvas by quarter_turns × 90°
//!    - Translate to correct rotated position
//!
//! 2. **Paint child**
//!    - Child painted in rotated coordinate space
//!    - Canvas automatically handles rotation
//!
//! 3. **Restore canvas**
//!    - Restore canvas to original state
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constraint swap
//! - **Paint**: O(1) - canvas rotation + child paint
//! - **Memory**: 24 bytes (QuarterTurns + Size cache)
//!
//! # Use Cases
//!
//! - **Portrait/Landscape**: Rotate UI for different orientations
//! - **Vertical text**: Rotate text 90° for vertical labels
//! - **Icons**: Rotate icons (arrows, triangles) by 90° increments
//! - **Layout rotation**: Rotate entire layout sections
//! - **Responsive design**: Different rotations for different screen sizes
//! - **Games**: Rotate game elements by 90° increments
//!
//! # Difference from RenderTransform
//!
//! **RenderRotatedBox (this):**
//! - Only supports 90° increments (quarter turns)
//! - Swaps layout constraints for odd rotations
//! - Affects both layout AND painting
//! - Better performance for 90° rotations
//!
//! **RenderTransform:**
//! - Supports arbitrary rotation angles
//! - Doesn't affect layout (transform-only)
//! - Only affects painting, not layout
//! - More flexible but doesn't swap constraints
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderRotatedBox;
//! use flui_types::geometry::QuarterTurns;
//!
//! // Rotate 90° clockwise
//! let rotated_90 = RenderRotatedBox::rotate_90();
//!
//! // Rotate 180° (upside down)
//! let rotated_180 = RenderRotatedBox::rotate_180();
//!
//! // Rotate 270° clockwise (90° counter-clockwise)
//! let rotated_270 = RenderRotatedBox::rotate_270();
//!
//! // Custom quarter turns
//! let rotated = RenderRotatedBox::new(QuarterTurns::Two);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::{geometry::QuarterTurns, Offset, Size};

/// RenderObject that rotates its child by quarter turns (90° increments).
///
/// Supports rotation by multiples of 90 degrees with proper layout constraint
/// swapping for odd rotations. Unlike arbitrary transforms, quarter-turn rotations
/// affect both layout and painting.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Orientation changes**: Rotate UI for portrait/landscape
/// - **Vertical text**: Rotate text 90° for vertical labels
/// - **Icon rotation**: Rotate arrows, triangles by 90° increments
/// - **Layout sections**: Rotate entire sections of UI
/// - **Responsive rotation**: Different rotations per screen size
/// - **Game elements**: Rotate sprites by 90° increments
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderRotatedBox behavior:
/// - Rotates by quarter_turns × 90° (clockwise)
/// - Odd turns (1, 3): Swaps width ↔ height constraints AND dimensions
/// - Even turns (0, 2, 4): No constraint swapping
/// - Affects both layout (constraint swap) and painting (rotation)
/// - Uses canvas rotation for painting
///
/// # Constraint Swapping Example
///
/// ```text
/// Input (90° rotation):
///   Parent constraints: min=0×0, max=400×600
///   quarter_turns = 1 (90°)
///
/// Step 1 - Swap constraints:
///   child_constraints = max=600×400 (swapped!)
///
/// Step 2 - Layout child:
///   child_size = 200×300 (in rotated space)
///
/// Step 3 - Swap final size:
///   container_size = 300×200 (swapped back!)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderRotatedBox;
///
/// // Rotate 90° clockwise (vertical)
/// let vertical = RenderRotatedBox::rotate_90();
///
/// // Rotate 180° (upside down)
/// let upside_down = RenderRotatedBox::rotate_180();
///
/// // Rotate 270° (90° counter-clockwise)
/// let rotated = RenderRotatedBox::rotate_270();
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

impl RenderObject for RenderRotatedBox {}

impl RenderBox<Single> for RenderRotatedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // For odd quarter turns (90°, 270°), swap width and height constraints
        // This ensures child is laid out in rotated coordinate space
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

        // Layout child with potentially swapped constraints
        let child_size = ctx.layout_child(child_id, child_constraints)?;

        // Our size is child size with potentially swapped dimensions
        // Odd turns: swap dimensions back to parent coordinate space
        // Even turns: keep dimensions unchanged
        let size = if self.quarter_turns.swaps_dimensions() {
            Size::new(child_size.height, child_size.width)
        } else {
            child_size
        };

        // Store size for paint phase (rotation transform calculations)
        self.size = size;
        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // If no rotation, just paint child directly
        if self.quarter_turns == QuarterTurns::Zero {
            ctx.paint_child(child_id, ctx.offset);
            return;
        }

        // Read offset before taking mutable borrow
        let offset = ctx.offset;

        // Save canvas state
        ctx.canvas_mut().save();

        // Move to rotation origin (our top-left)
        ctx.canvas_mut().translate(offset.dx, offset.dy);

        // Apply rotation transform
        let angle_radians = self.quarter_turns.radians();
        ctx.canvas_mut().rotate(angle_radians);

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
        ctx.canvas_mut().restore();
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
