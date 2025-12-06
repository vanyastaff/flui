//! RenderFractionalTranslation - Translates child by fraction of its size
//!
//! Implements Flutter's FractionalTranslation that shifts a child's painted
//! position using translation values specified as fractions of the child's size
//! rather than absolute pixels.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderFractionalTranslation` | `RenderFractionalTranslation` from `package:flutter/src/rendering/shifted_box.dart` |
//! | `translation` | `translation` property (Offset with fractions) |
//! | `set_translation()` | `translation = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!    - Translation doesn't affect layout
//!
//! 2. **Cache child size**
//!    - Store child size for calculating pixel offset during paint
//!    - Pixel offset = child size × translation fraction
//!
//! 3. **Return child size**
//!    - Container size = child size (translation doesn't change size)
//!
//! # Paint Protocol
//!
//! 1. **Calculate pixel offset**
//!    - dx_pixels = child_width × translation.dx
//!    - dy_pixels = child_height × translation.dy
//!    - Translation is relative to child's dimensions
//!
//! 2. **Paint child at translated position**
//!    - Child painted at parent offset + pixel offset
//!    - No clipping applied (child can overflow)
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child + size cache
//! - **Paint**: O(1) - simple offset calculation + child paint
//! - **Memory**: 24 bytes (Offset + Size cache)
//!
//! # Use Cases
//!
//! - **Responsive positioning**: Offsets that scale with content size
//! - **Slide animations**: Position shifts relative to dimensions (slide 50%)
//! - **Alignment tweaks**: Fine-tune positioning as percentage
//! - **Parallax scrolling**: Different layers move at different fractional rates
//! - **Drag indicators**: Show drag amount as fraction of widget size
//! - **Ripple effects**: Center effects at fractional positions
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderFractionalTranslation;
//! use flui_types::Offset;
//!
//! // Shift right by 25% of child width
//! let translate = RenderFractionalTranslation::new(Offset::new(0.25, 0.0));
//!
//! // Shift down by 50% of child height
//! let translate = RenderFractionalTranslation::new(Offset::new(0.0, 0.5));
//!
//! // Center alignment tweak (shift by -50% of size)
//! let center = RenderFractionalTranslation::new(Offset::new(-0.5, -0.5));
//!
//! // Convenience constructors
//! let horizontal = RenderFractionalTranslation::horizontal(0.25);
//! let vertical = RenderFractionalTranslation::vertical(0.5);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::{Offset, Size};

/// RenderObject that translates its child by a fraction of the child's size.
///
/// Shifts a child's painted position without affecting layout. Translation
/// is specified as fractions of the child's dimensions, allowing responsive
/// positioning that scales with content size.
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
/// - **Responsive positioning**: Offsets that scale with content size
/// - **Slide animations**: Position shifts relative to dimensions (slide 50%)
/// - **Alignment tweaks**: Fine-tune positioning as percentage
/// - **Parallax scrolling**: Different layers move at fractional rates
/// - **Drag indicators**: Show drag amount as fraction of size
/// - **Centering**: Shift by -50% to center child
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderFractionalTranslation behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child (translation doesn't affect size)
/// - Translation calculated as: offset = child_size × translation
/// - Child painted at translated position
/// - No clipping applied (can overflow)
/// - Hit testing confined to original layout bounds
///
/// # Translation Calculation
///
/// ```text
/// pixel_offset.dx = child_width × translation.dx
/// pixel_offset.dy = child_height × translation.dy
/// ```
///
/// **Examples:**
/// - translation = (0.5, 0.0), child = 100×200 → offset = (50, 0)
/// - translation = (-0.5, -0.5), child = 100×200 → offset = (-50, -100)
/// - translation = (1.0, 1.0), child = 100×200 → offset = (100, 200)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderFractionalTranslation;
/// use flui_types::Offset;
///
/// // Shift right by 25% of child width
/// let translate = RenderFractionalTranslation::new(Offset::new(0.25, 0.0));
///
/// // Center child (shift by -50% of size)
/// let center = RenderFractionalTranslation::new(Offset::new(-0.5, -0.5));
///
/// // Parallax effect (different layers, different fractions)
/// let background = RenderFractionalTranslation::new(Offset::new(0.2, 0.0));
/// let foreground = RenderFractionalTranslation::new(Offset::new(0.8, 0.0));
/// ```
#[derive(Debug)]
pub struct RenderFractionalTranslation {
    /// Translation as fraction of child size (dx = width fraction, dy = height fraction)
    pub translation: Offset,

    /// Last child size (cached for paint offset calculation)
    last_child_size: Size,
}

impl RenderFractionalTranslation {
    /// Create new RenderFractionalTranslation with specified translation
    ///
    /// # Arguments
    ///
    /// * `translation` - Offset where dx and dy are fractions of child size
    ///   - dx: horizontal fraction (positive = right, negative = left)
    ///   - dy: vertical fraction (positive = down, negative = up)
    pub fn new(translation: Offset) -> Self {
        Self {
            translation,
            last_child_size: Size::ZERO,
        }
    }

    /// Create with no translation (identity)
    pub fn identity() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Create with horizontal translation only
    pub fn horizontal(fraction: f32) -> Self {
        Self::new(Offset::new(fraction, 0.0))
    }

    /// Create with vertical translation only
    pub fn vertical(fraction: f32) -> Self {
        Self::new(Offset::new(0.0, fraction))
    }

    /// Set new translation
    pub fn set_translation(&mut self, translation: Offset) {
        self.translation = translation;
    }

    /// Get current translation
    pub fn get_translation(&self) -> Offset {
        self.translation
    }

    /// Calculate actual pixel offset based on child size and translation fraction
    fn calculate_pixel_offset(&self, child_size: Size) -> Offset {
        Offset::new(
            child_size.width * self.translation.dx,
            child_size.height * self.translation.dy,
        )
    }
}

impl RenderObject for RenderFractionalTranslation {}

impl RenderBox<Single> for RenderFractionalTranslation {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        let child_size = ctx.layout_child(child_id, ctx.constraints)?;

        // Cache child size for calculating pixel offset during paint
        self.last_child_size = child_size;

        // Return child size (translation doesn't affect layout)
        Ok(child_size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate actual pixel offset from fractional translation
        // pixel_offset = child_size × translation
        let pixel_offset = self.calculate_pixel_offset(self.last_child_size);

        // Paint child at translated position
        // Final offset = parent offset + fractional offset
        let translated_offset = ctx.offset + pixel_offset;
        ctx.paint_child(child_id, translated_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fractional_translation_new() {
        let translation = Offset::new(0.5, 0.25);
        let render = RenderFractionalTranslation::new(translation);

        assert_eq!(render.translation, translation);
        assert_eq!(render.last_child_size, Size::ZERO);
    }

    #[test]
    fn test_fractional_translation_identity() {
        let render = RenderFractionalTranslation::identity();
        assert_eq!(render.translation, Offset::ZERO);
    }

    #[test]
    fn test_fractional_translation_horizontal() {
        let render = RenderFractionalTranslation::horizontal(0.75);
        assert_eq!(render.translation.dx, 0.75);
        assert_eq!(render.translation.dy, 0.0);
    }

    #[test]
    fn test_fractional_translation_vertical() {
        let render = RenderFractionalTranslation::vertical(0.5);
        assert_eq!(render.translation.dx, 0.0);
        assert_eq!(render.translation.dy, 0.5);
    }

    #[test]
    fn test_set_and_get_translation() {
        let mut render = RenderFractionalTranslation::new(Offset::ZERO);

        let new_translation = Offset::new(0.3, 0.7);
        render.set_translation(new_translation);

        assert_eq!(render.get_translation(), new_translation);
    }

    #[test]
    fn test_calculate_pixel_offset() {
        let render = RenderFractionalTranslation::new(Offset::new(0.5, 0.25));
        let child_size = Size::new(100.0, 200.0);

        let pixel_offset = render.calculate_pixel_offset(child_size);

        // 0.5 * 100 = 50, 0.25 * 200 = 50
        assert_eq!(pixel_offset.dx, 50.0);
        assert_eq!(pixel_offset.dy, 50.0);
    }

    #[test]
    fn test_calculate_pixel_offset_negative() {
        let render = RenderFractionalTranslation::new(Offset::new(-0.5, -0.25));
        let child_size = Size::new(100.0, 200.0);

        let pixel_offset = render.calculate_pixel_offset(child_size);

        // -0.5 * 100 = -50, -0.25 * 200 = -50
        assert_eq!(pixel_offset.dx, -50.0);
        assert_eq!(pixel_offset.dy, -50.0);
    }

    #[test]
    fn test_calculate_pixel_offset_zero() {
        let render = RenderFractionalTranslation::new(Offset::ZERO);
        let child_size = Size::new(100.0, 200.0);

        let pixel_offset = render.calculate_pixel_offset(child_size);

        assert_eq!(pixel_offset, Offset::ZERO);
    }

    #[test]
    fn test_fractional_translation_large_values() {
        // Test with translation > 1.0 (moves child completely outside)
        let render = RenderFractionalTranslation::new(Offset::new(2.0, 1.5));
        let child_size = Size::new(50.0, 100.0);

        let pixel_offset = render.calculate_pixel_offset(child_size);

        // 2.0 * 50 = 100, 1.5 * 100 = 150
        assert_eq!(pixel_offset.dx, 100.0);
        assert_eq!(pixel_offset.dy, 150.0);
    }
}
