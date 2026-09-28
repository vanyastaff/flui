//! RenderColoredBox - a simple colored rectangle.

use flui_foundation::Leaf;
use flui_foundation::geometry::{Point, Rect, Size};
use flui_painting::Paint;
use flui_painting::styling::Color;

use flui_rendering::{
    constraints::BoxConstraints, context::BoxLayoutContext, parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that paints a colored rectangle.
#[derive(Debug, Clone)]
pub struct RenderColoredBox {
    color: [f32; 4],
    preferred_size: Size,
}

impl RenderColoredBox {
    /// Creates a new colored box.
    pub fn new(color: [f32; 4], preferred_size: Size) -> Self {
        Self {
            color,
            preferred_size,
        }
    }

    /// Creates a red box.
    pub fn red(width: f64, height: f64) -> Self {
        Self::new([1.0, 0.0, 0.0, 1.0], Size::new(width, height))
    }

    /// Creates a green box.
    pub fn green(width: f64, height: f64) -> Self {
        Self::new([0.0, 1.0, 0.0, 1.0], Size::new(width, height))
    }

    /// Creates a blue box.
    pub fn blue(width: f64, height: f64) -> Self {
        Self::new([0.0, 0.0, 1.0, 1.0], Size::new(width, height))
    }

    /// Returns the color.
    pub fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Sets the fill color (RGBA, each channel `0.0..=1.0`).
    ///
    pub fn set_color(&mut self, color: [f32; 4]) -> flui_rendering::RenderUpdateImpact {
        if self.color == color {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.color = color;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Returns the preferred size.
    pub fn preferred_size(&self) -> Size {
        self.preferred_size
    }

    /// Sets the preferred size.
    ///
    /// Takes effect on the next layout.
    pub fn set_preferred_size(&mut self, size: Size) -> flui_rendering::RenderUpdateImpact {
        if self.preferred_size == size {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.preferred_size = size;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl flui_foundation::Diagnosticable for RenderColoredBox {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        // `color` is the defining config; the committed `size` is layered on
        // by the test harness from `RenderState`, so it is not repeated here.
        properties.add_color("color", format!("{:?}", self.color));
    }
}
impl RenderBox for RenderColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let constrained = ctx.constrain(self.preferred_size);
        tracing::debug!(
            "RenderColoredBox::perform_layout: preferred={:?}, constrained={:?}",
            self.preferred_size,
            constrained
        );
        constrained
    }

    fn compute_min_intrinsic_width(
        &self,
        _height: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        self.preferred_size.width
    }

    fn compute_max_intrinsic_width(
        &self,
        _height: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        self.preferred_size.width
    }

    fn compute_min_intrinsic_height(
        &self,
        _width: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        self.preferred_size.height
    }

    fn compute_max_intrinsic_height(
        &self,
        _width: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        self.preferred_size.height
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        constraints.constrain(self.preferred_size)
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Leaf>) {
        // Local coordinates — the recorder pre-translates to this
        // node's origin. Size comes from RenderState via `ctx.size()`.
        let rect = Rect::from_origin_size(Point::ZERO, ctx.size());
        let color = Color::from_rgba_f32_array(self.color);
        ctx.canvas().draw_rect(rect, &Paint::fill(color));
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_colored_box_creation() {
        let box_obj = RenderColoredBox::red(100.0, 50.0);
        // The committed size lives on RenderState after layout; the object
        // only carries its preferred size as config.
        assert_eq!(box_obj.preferred_size(), Size::new(100.0, 50.0));
    }

    #[test]
    fn test_colored_box_factory_methods() {
        let red = RenderColoredBox::red(10.0, 20.0);
        let green = RenderColoredBox::green(30.0, 40.0);
        let blue = RenderColoredBox::blue(50.0, 60.0);

        // Check preferred sizes (size is ZERO before layout)
        assert_eq!(red.preferred_size(), Size::new(10.0, 20.0));
        assert_eq!(green.preferred_size(), Size::new(30.0, 40.0));
        assert_eq!(blue.preferred_size(), Size::new(50.0, 60.0));
    }
}
