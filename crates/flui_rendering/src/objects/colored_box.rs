//! RenderColoredBox - a simple colored rectangle.

use flui_types::{Point, Rect, Size};

use crate::arity::Leaf;
use crate::context::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use crate::parent_data::BoxParentData;
use crate::traits::RenderBox;

/// A render object that paints a colored rectangle.
#[derive(Debug, Clone)]
pub struct RenderColoredBox {
    color: [f32; 4],
    preferred_size: Size,
    size: Size,
}

impl RenderColoredBox {
    /// Creates a new colored box.
    pub fn new(color: [f32; 4], preferred_size: Size) -> Self {
        Self {
            color,
            preferred_size,
            size: Size::ZERO,
        }
    }

    /// Creates a red box.
    pub fn red(width: f32, height: f32) -> Self {
        Self::new([1.0, 0.0, 0.0, 1.0], Size::new(width, height))
    }

    /// Creates a green box.
    pub fn green(width: f32, height: f32) -> Self {
        Self::new([0.0, 1.0, 0.0, 1.0], Size::new(width, height))
    }

    /// Creates a blue box.
    pub fn blue(width: f32, height: f32) -> Self {
        Self::new([0.0, 0.0, 1.0, 1.0], Size::new(width, height))
    }

    /// Returns the color.
    pub fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Returns the preferred size.
    pub fn preferred_size(&self) -> Size {
        self.preferred_size
    }
}

impl RenderBox for RenderColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        let constrained = ctx.constrain(self.preferred_size);
        self.size = constrained;
        ctx.complete_with_size(constrained);
    }

    fn size(&self) -> Size {
        self.size
    }
    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Leaf, BoxParentData>) {
        // TODO: actual painting
    }

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
    use crate::constraints::BoxConstraints;
    use crate::traits::RenderObject;
    use crate::wrapper::BoxWrapper;

    #[test]
    fn test_colored_box_layout() {
        let box_obj = RenderColoredBox::red(100.0, 50.0);
        let mut wrapper = BoxWrapper::new(box_obj);
        wrapper.layout(BoxConstraints::tight(Size::new(80.0, 40.0)), true);
        assert_eq!(wrapper.inner().size(), Size::new(80.0, 40.0));
    }
}
