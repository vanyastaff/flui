//! RenderPadding - adds padding around a single child.

use flui_types::geometry::px;
use flui_types::{EdgeInsets, Offset, Pixels, Point, Rect, Size};

use crate::arity::Single;
use crate::constraints::BoxConstraints;
use crate::context::{BoxHitTestContext, BoxLayoutContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// A render object that adds padding around its child.
///
/// # Example
///
/// ```ignore
/// let padding = RenderPadding::new(EdgeInsets::all(16.0));
/// let mut wrapper = BoxWrapper::new(padding);
/// // Add child, then layout...
/// ```
#[derive(Debug, Clone)]
pub struct RenderPadding {
    /// The padding to apply.
    padding: EdgeInsets,
    /// Size after layout.
    size: Size,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
    /// Child offset for hit testing.
    child_offset: Offset,
}

impl RenderPadding {
    /// Creates a new padding render object.
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            size: Size::ZERO,
            has_child: false,
            child_offset: Offset::ZERO,
        }
    }

    /// Creates padding with all sides equal.
    pub fn all(value: f32) -> Self {
        Self::new(EdgeInsets::all(value))
    }

    /// Creates symmetric padding.
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(vertical, horizontal))
    }

    /// Returns the padding.
    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }

    /// Sets the padding.
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    /// Deflates constraints by padding amount.
    fn deflate_constraints(&self, constraints: &BoxConstraints) -> BoxConstraints {
        let horizontal = px(self.padding.horizontal_total());
        let vertical = px(self.padding.vertical_total());

        BoxConstraints::new(
            (constraints.min_width - horizontal).max(Pixels::ZERO),
            (constraints.max_width - horizontal).max(Pixels::ZERO),
            (constraints.min_height - vertical).max(Pixels::ZERO),
            (constraints.max_height - vertical).max(Pixels::ZERO),
        )
    }
}

impl flui_foundation::Diagnosticable for RenderPadding {}
impl RenderBox for RenderPadding {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = ctx.constraints().clone();

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Deflate constraints for child
            let child_constraints = self.deflate_constraints(&constraints);
            let child_size = ctx.layout_child(0, child_constraints);

            // Position child with top-left padding offset
            self.child_offset = Offset::new(px(self.padding.left), px(self.padding.top));
            ctx.position_child(0, self.child_offset);

            // Our size is child size + padding
            self.size = Size::new(
                child_size.width + px(self.padding.horizontal_total()),
                child_size.height + px(self.padding.vertical_total()),
            );
        } else {
            self.has_child = false;
            // No child - just the padding itself
            self.size = Size::new(
                px(self.padding.horizontal_total()),
                px(self.padding.vertical_total()),
            );
        }

        // Constrain to parent's constraints
        self.size = constraints.constrain(self.size);
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    // paint() uses default no-op - Padding just positions children

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // First check if we're in bounds
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        // Then test child at its offset
        if self.has_child {
            ctx.hit_test_child_at_offset(0, self.child_offset)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::BoxConstraints;

    #[test]
    fn test_edge_insets() {
        let insets = EdgeInsets::all(10.0);
        assert_eq!(insets.horizontal_total(), 20.0);
        assert_eq!(insets.vertical_total(), 20.0);
        // EdgeInsets is Edges<f32>; top_left() is only on Edges<Pixels>,
        // so construct the expected offset manually.
        let top_left = Offset::new(px(insets.left), px(insets.top));
        assert_eq!(top_left, Offset::new(px(10.0), px(10.0)));
    }

    #[test]
    fn test_padding_creation() {
        let padding = RenderPadding::all(16.0);
        assert_eq!(padding.padding(), EdgeInsets::all(16.0));
    }

    #[test]
    fn test_deflate_constraints() {
        let padding = RenderPadding::symmetric(20.0, 10.0);
        let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(100.0));
        let deflated = padding.deflate_constraints(&constraints);

        assert_eq!(deflated.max_width, px(160.0)); // 200 - 40
        assert_eq!(deflated.max_height, px(80.0)); // 100 - 20
    }

    #[test]
    fn test_edge_insets_symmetric() {
        let insets = EdgeInsets::symmetric(10.0, 20.0);
        // symmetric(vertical=10.0, horizontal=20.0)
        assert_eq!(insets.horizontal_total(), 40.0); // left + right = 20.0 + 20.0
        assert_eq!(insets.vertical_total(), 20.0); // top + bottom = 10.0 + 10.0
    }
}
