//! Type-erasure wrappers and utility types for render objects.
//!
//! This module provides wrapper types for working with render objects in
//! type-erased contexts, such as storing them in collections or passing
//! them across API boundaries.
//!
//! # Design Philosophy
//!
//! - **Type erasure**: Store concrete render objects as trait objects
//! - **Arity preservation**: Wrappers maintain arity information
//! - **Protocol preservation**: Box/Sliver protocol is maintained
//! - **Two-level API**: Typed (RenderBox<A>) + dyn-compatible (RenderObject)

use std::fmt;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::{Rect, Size, SliverGeometry};

use crate::arity::Arity;
use crate::box_render::RenderBox;
use crate::hit_test_context::{BoxHitTestContext, SliverHitTestContext};
use crate::layout_context::{BoxLayoutContext, SliverLayoutContext};
use crate::object::RenderObject;
use crate::paint_context::{BoxPaintContext, SliverPaintContext};
use crate::sliver::RenderSliver;
use crate::BoxConstraints;
use crate::RenderResult;

// ============================================================================
// BOX RENDER WRAPPER
// ============================================================================

/// Type-erased wrapper for box protocol render objects.
///
/// This wrapper allows storing any concrete `RenderBox<A>` implementation as a
/// trait object while preserving arity information at compile time.
///
/// # Type Parameters
///
/// - `A`: Arity type (preserved at compile time)
pub struct BoxRenderWrapper<A: Arity> {
    inner: Box<dyn RenderBox<A>>,
}

impl<A: Arity> BoxRenderWrapper<A> {
    /// Creates a new wrapper around a render object.
    pub fn new<R: RenderBox<A> + 'static>(render: R) -> Self {
        Self {
            inner: Box::new(render),
        }
    }

    /// Creates a wrapper from a boxed trait object.
    pub fn from_box(inner: Box<dyn RenderBox<A>>) -> Self {
        Self { inner }
    }

    /// Gets a reference to the inner render object.
    pub fn inner(&self) -> &dyn RenderBox<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn RenderBox<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    pub fn downcast_ref<R: RenderBox<A> + 'static>(&self) -> Option<&R> {
        (self.inner.as_ref() as &dyn RenderObject)
            .as_any()
            .downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    pub fn downcast_mut<R: RenderBox<A> + 'static>(&mut self) -> Option<&mut R> {
        (self.inner.as_mut() as &mut dyn RenderObject)
            .as_any_mut()
            .downcast_mut::<R>()
    }

    /// Unwraps the wrapper, returning the inner boxed trait object.
    pub fn into_inner(self) -> Box<dyn RenderBox<A>> {
        self.inner
    }
}

impl<A: Arity> fmt::Debug for BoxRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxRenderWrapper")
            .field("inner", &self.inner.as_ref().debug_name())
            .finish()
    }
}

impl<A: Arity> RenderBox<A> for BoxRenderWrapper<A> {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        RenderBox::paint(&*self.inner, ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        RenderBox::hit_test(&*self.inner, ctx, result)
    }

    fn intrinsic_width(&self, height: f32) -> Option<f32> {
        self.inner.intrinsic_width(height)
    }

    fn intrinsic_height(&self, width: f32) -> Option<f32> {
        self.inner.intrinsic_height(width)
    }

    fn baseline_offset(&self) -> Option<f32> {
        RenderBox::baseline_offset(self.inner.as_ref())
    }

    fn local_bounds(&self) -> Rect {
        RenderBox::local_bounds(self.inner.as_ref())
    }
}

impl<A: Arity> RenderObject for BoxRenderWrapper<A> {
    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }
}

// ============================================================================
// SLIVER RENDER WRAPPER
// ============================================================================

/// Type-erased wrapper for sliver protocol render objects.
///
/// This wrapper allows storing any concrete `RenderSliver<A>` implementation as a
/// trait object while preserving arity information at compile time.
///
/// # Type Parameters
///
/// - `A`: Arity type (preserved at compile time)
pub struct SliverRenderWrapper<A: Arity> {
    inner: Box<dyn RenderSliver<A>>,
}

impl<A: Arity> SliverRenderWrapper<A> {
    /// Creates a new wrapper around a sliver render object.
    pub fn new<R: RenderSliver<A> + 'static>(render: R) -> Self {
        Self {
            inner: Box::new(render),
        }
    }

    /// Creates a wrapper from a boxed trait object.
    pub fn from_box(inner: Box<dyn RenderSliver<A>>) -> Self {
        Self { inner }
    }

    /// Gets a reference to the inner render object.
    pub fn inner(&self) -> &dyn RenderSliver<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn RenderSliver<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    pub fn downcast_ref<R: RenderSliver<A> + 'static>(&self) -> Option<&R> {
        (self.inner.as_ref() as &dyn RenderObject)
            .as_any()
            .downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    pub fn downcast_mut<R: RenderSliver<A> + 'static>(&mut self) -> Option<&mut R> {
        (self.inner.as_mut() as &mut dyn RenderObject)
            .as_any_mut()
            .downcast_mut::<R>()
    }

    /// Unwraps the wrapper, returning the inner boxed trait object.
    pub fn into_inner(self) -> Box<dyn RenderSliver<A>> {
        self.inner
    }
}

impl<A: Arity> fmt::Debug for SliverRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverRenderWrapper")
            .field("inner", &self.inner.as_ref().debug_name())
            .finish()
    }
}

impl<A: Arity> RenderSliver<A> for SliverRenderWrapper<A> {
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> RenderResult<SliverGeometry> {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>) {
        RenderSliver::paint(&*self.inner, ctx)
    }

    fn hit_test(&self, ctx: &SliverHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        RenderSliver::hit_test(&*self.inner, ctx, result)
    }
}

impl<A: Arity> RenderObject for SliverRenderWrapper<A> {
    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }

    fn perform_layout(
        &mut self,
        _element_id: ElementId,
        _constraints: BoxConstraints,
        _layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> RenderResult<Size>,
    ) -> RenderResult<Size> {
        Err(crate::RenderError::UnsupportedProtocol {
            expected: "SliverProtocol",
            found: "BoxProtocol (use perform_sliver_layout instead)",
        })
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arity::Leaf;

    #[derive(Debug)]
    struct MockRenderBox {
        value: i32,
    }

    impl RenderObject for MockRenderBox {}

    impl RenderBox<Leaf> for MockRenderBox {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
            Ok(ctx.constraints.smallest())
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<'_, Leaf>) {}
    }

    #[test]
    fn test_wrapper_creation() {
        let mock = MockRenderBox { value: 42 };
        let wrapper = BoxRenderWrapper::new(mock);

        assert_eq!(
            wrapper.inner().debug_name(),
            "flui_rendering::wrapper::tests::MockRenderBox"
        );
    }

    #[test]
    fn test_wrapper_downcast() {
        let mock = MockRenderBox { value: 42 };
        let mut wrapper = BoxRenderWrapper::new(mock);

        let downcast = wrapper.downcast_ref::<MockRenderBox>();
        assert!(downcast.is_some());
        assert_eq!(downcast.unwrap().value, 42);

        let downcast_mut = wrapper.downcast_mut::<MockRenderBox>();
        assert!(downcast_mut.is_some());
        downcast_mut.unwrap().value = 100;

        assert_eq!(wrapper.downcast_ref::<MockRenderBox>().unwrap().value, 100);
    }
}
