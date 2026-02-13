//! RenderSizedBox - forces specific size constraints.

use flui_types::{Pixels, Point, Rect, Size};

use crate::arity::Leaf;
use crate::context::{BoxHitTestContext, BoxLayoutContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// A render object that forces a specific size.
///
/// If width or height is None, that dimension is unconstrained
/// and will use the incoming constraints.
///
/// # Example
///
/// ```ignore
/// use flui_types::geometry::px;
///
/// // Fixed 100x100 box
/// let sized = RenderSizedBox::new(Some(px(100.0)), Some(px(100.0)));
///
/// // Fixed width, flexible height
/// let wide = RenderSizedBox::new(Some(px(200.0)), None);
///
/// // Expand to fill available space
/// let expand = RenderSizedBox::expand();
/// ```
#[derive(Debug, Clone)]
pub struct RenderSizedBox {
    /// Fixed width, or None for flexible.
    width: Option<Pixels>,
    /// Fixed height, or None for flexible.
    height: Option<Pixels>,
    /// Actual size after layout.
    size: Size,
}

impl RenderSizedBox {
    /// Creates a sized box with optional fixed dimensions.
    pub fn new(width: Option<Pixels>, height: Option<Pixels>) -> Self {
        Self {
            width,
            height,
            size: Size::ZERO,
        }
    }

    /// Creates a sized box with fixed dimensions.
    pub fn fixed(width: Pixels, height: Pixels) -> Self {
        Self::new(Some(width), Some(height))
    }

    /// Creates a sized box that expands to fill available space.
    pub fn expand() -> Self {
        Self::new(None, None)
    }

    /// Creates a sized box that shrinks to zero.
    pub fn shrink() -> Self {
        Self::fixed(Pixels::ZERO, Pixels::ZERO)
    }

    /// Creates a square sized box.
    pub fn square(dimension: Pixels) -> Self {
        Self::fixed(dimension, dimension)
    }

    /// Returns the fixed width, if any.
    pub fn width(&self) -> Option<Pixels> {
        self.width
    }

    /// Returns the fixed height, if any.
    pub fn height(&self) -> Option<Pixels> {
        self.height
    }
}

impl flui_foundation::Diagnosticable for RenderSizedBox {}
impl RenderBox for RenderSizedBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        let constraints = ctx.constraints();

        // Use fixed dimension or constrain to max
        let width = self
            .width
            .map(|w| w.clamp(constraints.min_width, constraints.max_width))
            .unwrap_or(constraints.max_width);

        let height = self
            .height
            .map(|h| h.clamp(constraints.min_height, constraints.max_height))
            .unwrap_or(constraints.max_height);

        self.size = Size::new(width, height);
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    // paint() uses default no-op - SizedBox only affects layout

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        ctx.is_within_size(self.size.width, self.size.height)
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_sized_box_fixed_creation() {
        let sized = RenderSizedBox::fixed(px(100.0), px(50.0));
        assert_eq!(sized.width(), Some(px(100.0)));
        assert_eq!(sized.height(), Some(px(50.0)));
    }

    #[test]
    fn test_sized_box_expand_creation() {
        let sized = RenderSizedBox::expand();
        // expand() uses None which means "expand to fill available space"
        assert_eq!(sized.width(), None);
        assert_eq!(sized.height(), None);
    }

    #[test]
    fn test_sized_box_shrink_creation() {
        let sized = RenderSizedBox::shrink();
        assert_eq!(sized.width(), Some(Pixels::ZERO));
        assert_eq!(sized.height(), Some(Pixels::ZERO));
    }

    #[test]
    fn test_sized_box_partial_creation() {
        // Fixed width, flexible height
        let sized = RenderSizedBox::new(Some(px(100.0)), None);
        assert_eq!(sized.width(), Some(px(100.0)));
        assert_eq!(sized.height(), None);
    }
}
