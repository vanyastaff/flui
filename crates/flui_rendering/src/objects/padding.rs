//! RenderPadding - adds padding around a single child.

use flui_types::{Offset, Point, Rect, Size};

use crate::arity::Single;
use crate::constraints::BoxConstraints;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// Edge insets for padding.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EdgeInsets {
    /// Left padding.
    pub left: f32,
    /// Top padding.
    pub top: f32,
    /// Right padding.
    pub right: f32,
    /// Bottom padding.
    pub bottom: f32,
}

impl EdgeInsets {
    /// Creates edge insets with all sides equal.
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// Creates edge insets with symmetric horizontal and vertical values.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    /// Creates edge insets with only specific sides.
    pub const fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Returns the total horizontal padding.
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Returns the total vertical padding.
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }

    /// Returns the top-left offset.
    pub fn top_left(&self) -> Offset {
        Offset::new(self.left, self.top)
    }
}

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
        Self::new(EdgeInsets::symmetric(horizontal, vertical))
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
        let horizontal = self.padding.horizontal();
        let vertical = self.padding.vertical();

        BoxConstraints::new(
            (constraints.min_width - horizontal).max(0.0),
            (constraints.max_width - horizontal).max(0.0),
            (constraints.min_height - vertical).max(0.0),
            (constraints.max_height - vertical).max(0.0),
        )
    }
}

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
            self.child_offset = self.padding.top_left();
            ctx.position_child(0, self.child_offset);

            // Our size is child size + padding
            self.size = Size::new(
                child_size.width + self.padding.horizontal(),
                child_size.height + self.padding.vertical(),
            );
        } else {
            self.has_child = false;
            // No child - just the padding itself
            self.size = Size::new(self.padding.horizontal(), self.padding.vertical());
        }

        // Constrain to parent's constraints
        self.size = constraints.constrain(self.size);
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
        // Children are painted automatically by the wrapper
    }

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
    use crate::traits::RenderObject;
    use crate::wrapper::BoxWrapper;

    #[test]
    fn test_edge_insets() {
        let insets = EdgeInsets::all(10.0);
        assert_eq!(insets.horizontal(), 20.0);
        assert_eq!(insets.vertical(), 20.0);
        assert_eq!(insets.top_left(), Offset::new(10.0, 10.0));
    }

    #[test]
    fn test_padding_creation() {
        let padding = RenderPadding::all(16.0);
        assert_eq!(padding.padding(), EdgeInsets::all(16.0));
    }

    #[test]
    fn test_padding_no_child() {
        let padding = RenderPadding::all(10.0);
        let mut wrapper = BoxWrapper::new(padding);

        wrapper.layout(BoxConstraints::loose(Size::new(100.0, 100.0)), true);

        // Just padding, no child
        assert_eq!(wrapper.inner().size(), Size::new(20.0, 20.0));
    }

    #[test]
    fn test_deflate_constraints() {
        let padding = RenderPadding::symmetric(20.0, 10.0);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
        let deflated = padding.deflate_constraints(&constraints);

        assert_eq!(deflated.max_width, 160.0); // 200 - 40
        assert_eq!(deflated.max_height, 80.0); // 100 - 20
    }
}
