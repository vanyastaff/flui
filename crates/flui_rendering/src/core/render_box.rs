//! RenderBox trait for box protocol render objects.
//!
//! - [`RenderBox<A>`] - 2D box layout with arity validation
//! - Arities: `Leaf` (0 children), `Single` (1), `Variable` (0+)

use std::fmt;

use flui_interaction::HitTestResult;
use flui_types::{Offset, Rect, Size};

use super::arity::Arity;
use super::contexts::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use super::geometry::BoxConstraints;
use super::render_object::RenderObject;

// ============================================================================
// CORE RENDER BOX TRAIT
// ============================================================================

/// Render trait for box protocol with arity validation.
pub trait RenderBox<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    /// Computes size given constraints.
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> Size;

    /// Paints to canvas.
    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>);

    /// Hit testing (default: bounds check + children in reverse z-order).
    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        if !ctx.contains_position() {
            return false;
        }
        for child_id in ctx.children_reverse() {
            if ctx.hit_test_child(child_id, ctx.position, result) {
                return true;
            }
        }
        ctx.hit_test_self(result)
    }

    /// Minimum intrinsic width for a given height.
    fn intrinsic_width(&self, _height: f32) -> Option<f32> {
        None
    }

    /// Minimum intrinsic height for a given width.
    fn intrinsic_height(&self, _width: f32) -> Option<f32> {
        None
    }

    /// Baseline offset for text alignment.
    fn baseline_offset(&self) -> Option<f32> {
        None
    }

    /// Dry layout without side effects (for intrinsic sizing).
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }

    /// Local bounding rectangle.
    fn local_bounds(&self) -> Rect {
        (self as &dyn RenderObject).local_bounds()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};
    use std::marker::PhantomData;

    // Simple test render box
    #[derive(Debug)]
    struct TestRenderBox<A: Arity> {
        size: Size,
        _phantom: PhantomData<A>,
    }

    impl<A: Arity> TestRenderBox<A> {
        fn new(size: Size) -> Self {
            Self {
                size,
                _phantom: PhantomData,
            }
        }
    }

    impl<A: Arity> RenderBox<A> for TestRenderBox<A> {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> Size {
            ctx.constraints.constrain(self.size)
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
            // Do nothing
        }
    }

    impl<A: Arity> RenderObject for TestRenderBox<A> {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_render_box_arity() {
        let _leaf: TestRenderBox<Leaf> = TestRenderBox::new(Size::new(100.0, 50.0));
        let _single: TestRenderBox<Single> = TestRenderBox::new(Size::new(100.0, 50.0));
        let _variable: TestRenderBox<Variable> = TestRenderBox::new(Size::new(100.0, 50.0));
        // Compiles = arity system works
    }

    #[test]
    fn test_default_intrinsic_size() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        assert_eq!(render.intrinsic_width(50.0), None);
        assert_eq!(render.intrinsic_height(100.0), None);
    }

    #[test]
    fn test_default_baseline() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        assert_eq!(render.baseline_offset(), None);
    }

    #[test]
    fn test_compute_dry_layout() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        let constraints = BoxConstraints::tight(Size::new(80.0, 40.0));
        let size = render.compute_dry_layout(constraints);
        assert_eq!(size, Size::new(80.0, 40.0));
    }
}
