//! RenderPadding - adds padding around a single child.

use flui_tree::Single;
use flui_types::{EdgeInsets, Offset, Pixels, Point, Rect, Size, geometry::px};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

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
        Self::new(EdgeInsets::all(px(value)))
    }

    /// Creates symmetric padding.
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(px(vertical), px(horizontal)))
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
        let horizontal = self.padding.horizontal_total();
        let vertical = self.padding.vertical_total();

        BoxConstraints::new(
            (constraints.min_width - horizontal).max(Pixels::ZERO),
            (constraints.max_width - horizontal).max(Pixels::ZERO),
            (constraints.min_height - vertical).max(Pixels::ZERO),
            (constraints.max_height - vertical).max(Pixels::ZERO),
        )
    }
}

impl flui_foundation::Diagnosticable for RenderPadding {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("padding", self.padding);
    }
}
impl RenderBox for RenderPadding {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Deflate constraints for child
            let child_constraints = self.deflate_constraints(&constraints);
            let child_size = ctx.layout_child(0, child_constraints);

            // Position child with top-left padding offset
            self.child_offset = Offset::new(self.padding.left, self.padding.top);
            ctx.position_child(0, self.child_offset);

            // Our size is child size + padding
            self.size = Size::new(
                child_size.width + self.padding.horizontal_total(),
                child_size.height + self.padding.vertical_total(),
            );
        } else {
            self.has_child = false;
            // No child - just the padding itself
            self.size = Size::new(
                self.padding.horizontal_total(),
                self.padding.vertical_total(),
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

    fn compute_min_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let deflated_height = (height - self.padding.vertical_total().get()).max(0.0);
        if ctx.child_count() == 0 {
            return self.padding.horizontal_total().get();
        }
        ctx.child_min_intrinsic_width(0, deflated_height) + self.padding.horizontal_total().get()
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let deflated_height = (height - self.padding.vertical_total().get()).max(0.0);
        if ctx.child_count() == 0 {
            return self.padding.horizontal_total().get();
        }
        ctx.child_max_intrinsic_width(0, deflated_height) + self.padding.horizontal_total().get()
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let deflated_width = (width - self.padding.horizontal_total().get()).max(0.0);
        if ctx.child_count() == 0 {
            return self.padding.vertical_total().get();
        }
        ctx.child_min_intrinsic_height(0, deflated_width) + self.padding.vertical_total().get()
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let deflated_width = (width - self.padding.horizontal_total().get()).max(0.0);
        if ctx.child_count() == 0 {
            return self.padding.vertical_total().get();
        }
        ctx.child_max_intrinsic_height(0, deflated_width) + self.padding.vertical_total().get()
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return constraints.constrain(Size::new(
                self.padding.horizontal_total(),
                self.padding.vertical_total(),
            ));
        }
        let child_constraints = self.deflate_constraints(&constraints);
        let child_size = ctx.child_dry_layout(0, child_constraints);
        constraints.constrain(Size::new(
            child_size.width + self.padding.horizontal_total(),
            child_size.height + self.padding.vertical_total(),
        ))
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: crate::traits::TextBaseline,
        ctx: &mut crate::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_constraints = self.deflate_constraints(&constraints);
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        Some(child_baseline + self.padding.top.get())
    }

    // paint() uses default no-op - Padding just positions children

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }

        if self.has_child {
            ctx.hit_test_child_at_layout_offset(0)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderPadding {}
impl SemanticsCapability for RenderPadding {}
impl HotReloadCapability for RenderPadding {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::BoxConstraints;

    #[test]
    fn test_edge_insets() {
        let insets = EdgeInsets::all(px(10.0));
        assert_eq!(insets.horizontal_total(), px(20.0));
        assert_eq!(insets.vertical_total(), px(20.0));
        // insets.left/top are Pixels, so build the expected offset directly.
        let top_left = Offset::new(insets.left, insets.top);
        assert_eq!(top_left, Offset::new(px(10.0), px(10.0)));
    }

    #[test]
    fn test_padding_creation() {
        let padding = RenderPadding::all(16.0);
        assert_eq!(padding.padding(), EdgeInsets::all(px(16.0)));
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
        let insets = EdgeInsets::symmetric(px(10.0), px(20.0));
        // symmetric(vertical=10.0, horizontal=20.0)
        assert_eq!(insets.horizontal_total(), px(40.0)); // left + right = 20.0 + 20.0
        assert_eq!(insets.vertical_total(), px(20.0)); // top + bottom = 10.0 + 10.0
    }
}
