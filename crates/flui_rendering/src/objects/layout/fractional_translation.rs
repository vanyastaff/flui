//! RenderFractionalTranslation - Translates child by fraction of its size
//!
//! Applies a translation transformation before painting, where the translation
//! is specified as a fraction of the child's size rather than absolute pixels.

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::{Offset, Size};

/// RenderObject that translates its child by a fraction of the child's size
///
/// RenderFractionalTranslation shifts a child's painted position without
/// affecting layout. The translation is specified as fractions of the
/// child's dimensions:
///
/// - `translation.dx` = fraction of child width
/// - `translation.dy` = fraction of child height
///
/// # Translation Calculation
///
/// ```text
/// actual_offset.dx = child_size.width * translation.dx
/// actual_offset.dy = child_size.height * translation.dy
/// ```
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderFractionalTranslation;
/// use flui_types::Offset;
///
/// // Shift right by 25% of child width
/// let translate = RenderFractionalTranslation::new(Offset::new(0.25, 0.0));
///
/// // Shift down by 50% of child height
/// let translate = RenderFractionalTranslation::new(Offset::new(0.0, 0.5));
///
/// // Shift diagonally
/// let translate = RenderFractionalTranslation::new(Offset::new(0.5, 0.5));
/// ```
///
/// # Layout Behavior
///
/// - Child is laid out with parent's constraints unchanged
/// - Returns child's size (translation doesn't affect layout size)
/// - Translation only affects painting, not layout
///
/// # Paint Behavior
///
/// - Calculates actual offset: `child_size * translation`
/// - Paints child at offset position
/// - No clipping applied (child can overflow if translated)
///
/// # Hit Testing
///
/// Hit tests are confined to the original layout bounds, even if
/// painted content overflows due to translation. This matches
/// Flutter's behavior.
///
/// # Use Cases
///
/// - **Responsive positioning**: Offsets that scale with content size
/// - **Subtle animations**: Position shifts relative to dimensions
/// - **Alignment tweaks**: Fine-tune positioning as percentage
/// - **Parallax effects**: Different layers move at different rates
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

impl RenderBox<Single> for RenderFractionalTranslation {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Layout child with same constraints
        let child_size = ctx.layout_child(child_id, ctx.constraints);

        // Cache child size for paint phase
        self.last_child_size = child_size;

        // Return child size (translation doesn't affect layout)
        child_size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Calculate actual pixel offset from fractional translation
        let pixel_offset = self.calculate_pixel_offset(self.last_child_size);

        // Paint child at translated position
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
