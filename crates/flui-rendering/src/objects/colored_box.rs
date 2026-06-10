//! RenderColoredBox - a simple colored rectangle.

use flui_painting::Paint;
use flui_tree::Leaf;
use flui_types::{Color, Point, Rect, Size, geometry::px};

use crate::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

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
        Self::new([1.0, 0.0, 0.0, 1.0], Size::new(px(width), px(height)))
    }

    /// Creates a green box.
    pub fn green(width: f32, height: f32) -> Self {
        Self::new([0.0, 1.0, 0.0, 1.0], Size::new(px(width), px(height)))
    }

    /// Creates a blue box.
    pub fn blue(width: f32, height: f32) -> Self {
        Self::new([0.0, 0.0, 1.0, 1.0], Size::new(px(width), px(height)))
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

impl flui_foundation::Diagnosticable for RenderColoredBox {}
impl RenderBox for RenderColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        let constrained = ctx.constrain(self.preferred_size);
        self.size = constrained;
        tracing::debug!(
            "RenderColoredBox::perform_layout: preferred={:?}, constrained={:?}",
            self.preferred_size,
            constrained
        );
        ctx.complete_with_size(constrained);
    }

    fn size(&self) -> &Size {
        &self.size
    }
    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn paint(&self, ctx: &mut crate::context::PaintCx<'_, Leaf>) {
        // Local coordinates — the recorder pre-translates to this
        // node's origin.
        let rect = Rect::from_origin_size(Point::ZERO, self.size);
        let color = Color::from_rgba_f32_array(self.color);
        ctx.canvas().draw_rect(rect, &Paint::fill(color));
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        ctx.is_within_size(self.size.width, self.size.height)
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderColoredBox {}
impl SemanticsCapability for RenderColoredBox {}
impl HotReloadCapability for RenderColoredBox {}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_colored_box_creation() {
        let box_obj = RenderColoredBox::red(100.0, 50.0);
        // Size starts at ZERO before layout, preferred_size is set
        assert_eq!(box_obj.preferred_size(), Size::new(px(100.0), px(50.0)));
        assert_eq!(*box_obj.size(), Size::ZERO);
    }

    #[test]
    fn test_colored_box_factory_methods() {
        let red = RenderColoredBox::red(10.0, 20.0);
        let green = RenderColoredBox::green(30.0, 40.0);
        let blue = RenderColoredBox::blue(50.0, 60.0);

        // Check preferred sizes (size is ZERO before layout)
        assert_eq!(red.preferred_size(), Size::new(px(10.0), px(20.0)));
        assert_eq!(green.preferred_size(), Size::new(px(30.0), px(40.0)));
        assert_eq!(blue.preferred_size(), Size::new(px(50.0), px(60.0)));
    }
}
