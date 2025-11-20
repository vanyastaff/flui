//! Wrappers that bridge typed render traits to type-erased `RenderObject`.
//!
//! These wrappers enable storing render objects with different arities and
//! protocols in a uniform `Box<dyn RenderObject>` while preserving type safety
//! at the boundary.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<A> → BoxRenderWrapper<A, R> → Box<dyn RenderObject>
//! SliverRender<A> → SliverRenderWrapper<A, R> → Box<dyn RenderObject>
//! ```
//!
//! # Implementation
//!
//! The wrappers:
//! 1. Convert type-erased `Constraints` to typed constraints
//! 2. Convert `&[ElementId]` to typed children accessor
//! 3. Create the appropriate context
//! 4. Call the typed render method
//! 5. Convert result back to type-erased `Geometry`

use crate::element::{ElementId, ElementTree};
use crate::render::arity::Arity;
use crate::render::contexts::{HitTestContext, LayoutContext, PaintContext};
use crate::render::render_box::RenderBox;
use crate::render::render_object::{
    Constraints as DynConstraints, Geometry as DynGeometry, RenderObject,
};
use crate::render::render_silver::SliverRender;
use flui_types::Offset;
use std::any::Any;
use std::fmt::Debug;

// ============================================================================
// Box Protocol Wrapper
// ============================================================================

/// Wrapper that adapts `RenderBox<A>` to `RenderObject`.
///
/// Stores a typed render object and implements `RenderObject` by converting
/// between type-erased and typed representations.
///
/// # Type Parameters
///
/// - `A`: Arity (child count)
/// - `R`: The concrete render type implementing `RenderBox<A>`
pub struct BoxRenderWrapper<A, R>
where
    A: Arity,
    R: RenderBox<A>,
{
    inner: R,
    _phantom: std::marker::PhantomData<A>,
}

impl<A, R> Debug for BoxRenderWrapper<A, R>
where
    A: Arity,
    R: RenderBox<A>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxRenderWrapper")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<A, R> BoxRenderWrapper<A, R>
where
    A: Arity,
    R: RenderBox<A>,
{
    /// Creates a new wrapper around the given render object.
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns a reference to the inner render object.
    #[inline]
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Returns a mutable reference to the inner render object.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the inner render object.
    #[inline]
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<A, R> RenderObject for BoxRenderWrapper<A, R>
where
    A: Arity,
    R: RenderBox<A>,
{
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry {
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation: expected {:?}, got {}",
            A::runtime_arity(),
            children.len()
        );

        let box_constraints = constraints.as_box();

        // Convert &[ElementId] to typed children
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };
        let typed_children = A::from_slice(children_slice);

        let ctx = LayoutContext::new(tree, *box_constraints, typed_children);
        let size = self.inner.layout(ctx);

        DynGeometry::Box(size)
    }

    fn paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> flui_painting::Canvas {
        debug_assert!(A::validate_count(children.len()));

        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };
        let typed_children = A::from_slice(children_slice);

        let mut ctx = PaintContext::new(tree, offset, typed_children);
        self.inner.paint(&mut ctx);

        ctx.take_canvas()
    }

    fn hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
        geometry: &DynGeometry,
    ) -> bool {
        debug_assert!(A::validate_count(children.len()));

        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };
        let typed_children = A::from_slice(children_slice);

        let size = geometry.as_box();

        // TODO: get element_id from caller
        let element_id = ElementId::new(1);

        let ctx = HitTestContext::new(tree, position, size, element_id, typed_children);
        let mut result = crate::element::hit_test::BoxHitTestResult::new();

        self.inner.hit_test(ctx, &mut result)
    }

    fn debug_name(&self) -> &'static str {
        std::any::type_name::<R>()
    }

    fn as_any(&self) -> &dyn Any {
        &self.inner
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.inner
    }
}

// ============================================================================
// Sliver Protocol Wrapper
// ============================================================================

/// Wrapper that adapts `SliverRender<A>` to `RenderObject`.
///
/// Similar to `BoxRenderWrapper` but for sliver protocol render objects.
///
/// # Type Parameters
///
/// - `A`: Arity (child count)
/// - `R`: The concrete render type implementing `SliverRender<A>`
pub struct SliverRenderWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    inner: R,
    _phantom: std::marker::PhantomData<A>,
}

impl<A, R> Debug for SliverRenderWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverRenderWrapper")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<A, R> SliverRenderWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    /// Creates a new wrapper around the given sliver render object.
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns a reference to the inner sliver render object.
    #[inline]
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Returns a mutable reference to the inner sliver render object.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the inner sliver render object.
    #[inline]
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<A, R> RenderObject for SliverRenderWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry {
        debug_assert!(A::validate_count(children.len()));

        let sliver_constraints = constraints.as_sliver();

        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };
        let typed_children = A::from_slice(children_slice);

        let ctx = LayoutContext::new(tree, *sliver_constraints, typed_children);
        let geometry = self.inner.layout(ctx);

        DynGeometry::Sliver(geometry)
    }

    fn paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> flui_painting::Canvas {
        debug_assert!(A::validate_count(children.len()));

        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };
        let typed_children = A::from_slice(children_slice);

        let mut ctx = PaintContext::new(tree, offset, typed_children);
        self.inner.paint(&mut ctx);

        ctx.take_canvas()
    }

    fn hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
        geometry: &DynGeometry,
    ) -> bool {
        debug_assert!(A::validate_count(children.len()));

        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };
        let typed_children = A::from_slice(children_slice);

        let sliver_geometry = *geometry.as_sliver();

        // TODO: get element_id from caller
        let element_id = ElementId::new(1);

        let ctx = HitTestContext::new(tree, position, sliver_geometry, element_id, typed_children);
        let mut result = crate::element::hit_test::SliverHitTestResult::new();

        self.inner.hit_test(ctx, &mut result)
    }

    fn debug_name(&self) -> &'static str {
        std::any::type_name::<R>()
    }

    fn as_any(&self) -> &dyn Any {
        &self.inner
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{BoxProtocol, Leaf};
    use flui_types::Size;

    #[derive(Debug)]
    struct MockRender {
        size: Size,
    }

    impl RenderBox<Leaf> for MockRender {
        fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
            ctx.constraints.constrain(self.size)
        }

        fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {}
    }

    #[test]
    fn test_box_wrapper() {
        let render = MockRender {
            size: Size::new(100.0, 100.0),
        };
        let wrapper = BoxRenderWrapper::<Leaf, _>::new(render);
        assert_eq!(wrapper.inner().size.width, 100.0);
    }
}
