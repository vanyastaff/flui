//! RenderSizedBox - forces specific size constraints.

use flui_tree::Leaf;
use flui_types::{Pixels, Point, Rect, Size};

use crate::{
    constraints::BoxConstraints,
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

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

    fn resolved_size(&self, constraints: &BoxConstraints) -> Size {
        let width = self
            .width
            .map(|w| w.clamp(constraints.min_width, constraints.max_width))
            .unwrap_or(constraints.max_width);
        let height = self
            .height
            .map(|h| h.clamp(constraints.min_height, constraints.max_height))
            .unwrap_or(constraints.max_height);
        Size::new(width, height)
    }
}

impl flui_foundation::Diagnosticable for RenderSizedBox {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_optional("width", self.width.map(|w| format!("{w:?}")));
        properties.add_optional("height", self.height.map(|h| format!("{h:?}")));
    }
}
impl RenderBox for RenderSizedBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
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
        self.size
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn compute_min_intrinsic_width(
        &self,
        _height: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        self.width.map(|w| w.get()).unwrap_or(0.0)
    }

    fn compute_max_intrinsic_width(
        &self,
        _height: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        self.width.map(|w| w.get()).unwrap_or(0.0)
    }

    fn compute_min_intrinsic_height(
        &self,
        _width: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        self.height.map(|h| h.get()).unwrap_or(0.0)
    }

    fn compute_max_intrinsic_height(
        &self,
        _width: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        self.height.map(|h| h.get()).unwrap_or(0.0)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.resolved_size(&constraints)
    }

    // paint() uses default no-op - SizedBox only affects layout

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderSizedBox {}
impl SemanticsCapability for RenderSizedBox {}
impl HotReloadCapability for RenderSizedBox {}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

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
