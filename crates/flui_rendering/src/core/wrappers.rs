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

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::{Offset, Size, SliverGeometry};
use std::any::Any;
use std::fmt::Debug;
use std::marker::PhantomData;

use super::arity::Arity;
use super::geometry::{Constraints, Geometry};
use super::protocol::BoxConstraints;
use super::render_box::RenderBox;
use super::render_object::RenderObject;
use super::render_sliver::SliverRender;

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
    _phantom: PhantomData<A>,
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
            _phantom: PhantomData,
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
        children: &[ElementId],
        constraints: &Constraints,
        layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
    ) -> Geometry {
        let box_constraints = constraints.as_box().clone();

        // Adapter that converts type-erased layout_child to box-specific
        let mut box_layout_child = |id: ElementId, c: BoxConstraints| -> Size {
            let geometry = layout_child(id, Constraints::Box(c));
            geometry.as_box()
        };

        let size = self
            .inner
            .layout(box_constraints, children, &mut box_layout_child);
        Geometry::Box(size)
    }

    fn paint(
        &self,
        children: &[ElementId],
        offset: Offset,
        paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
    ) -> Canvas {
        self.inner.paint(offset, children, paint_child)
    }

    fn hit_test(&self, children: &[ElementId], position: Offset, geometry: &Geometry) -> bool {
        let size = geometry.as_box();

        // Adapter for hit test child
        let mut hit_test_child = |_id: ElementId, _pos: Offset| -> bool {
            // In the full implementation, this would delegate to the tree
            false
        };

        self.inner
            .hit_test(position, size, children, &mut hit_test_child)
    }

    fn debug_name(&self) -> &'static str {
        self.inner.debug_name()
    }

    fn as_any(&self) -> &dyn Any {
        self.inner.as_any()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.inner.as_any_mut()
    }
}

// ============================================================================
// Sliver Protocol Wrapper
// ============================================================================

/// Wrapper that adapts `SliverRender<A>` to `RenderObject`.
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
    _phantom: PhantomData<A>,
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
    /// Creates a new wrapper around the given render object.
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
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

impl<A, R> RenderObject for SliverRenderWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn layout(
        &mut self,
        children: &[ElementId],
        constraints: &Constraints,
        layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
    ) -> Geometry {
        let sliver_constraints = constraints.as_sliver().clone();

        // Adapter that converts type-erased layout_child to sliver-specific
        let mut sliver_layout_child =
            |id: ElementId, c: flui_types::SliverConstraints| -> SliverGeometry {
                let geometry = layout_child(id, Constraints::Sliver(c));
                geometry.as_sliver().clone()
            };

        let geometry = self
            .inner
            .layout(sliver_constraints, children, &mut sliver_layout_child);
        Geometry::Sliver(geometry)
    }

    fn paint(
        &self,
        children: &[ElementId],
        offset: Offset,
        paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
    ) -> Canvas {
        self.inner.paint(offset, children, paint_child)
    }

    fn hit_test(&self, children: &[ElementId], position: Offset, geometry: &Geometry) -> bool {
        let sliver_geometry = geometry.as_sliver();

        // Adapter for hit test child
        let mut hit_test_child = |_id: ElementId, _pos: Offset| -> bool {
            // In the full implementation, this would delegate to the tree
            false
        };

        self.inner
            .hit_test(position, sliver_geometry, children, &mut hit_test_child)
    }

    fn debug_name(&self) -> &'static str {
        self.inner.debug_name()
    }

    fn as_any(&self) -> &dyn Any {
        self.inner.as_any()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.inner.as_any_mut()
    }
}

// ============================================================================
// Empty Render (re-exported from render_box)
// ============================================================================

pub use super::render_box::EmptyRender;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::Leaf;

    #[derive(Debug)]
    struct TestBox {
        size: Size,
    }

    impl RenderBox<Leaf> for TestBox {
        fn layout(
            &mut self,
            constraints: BoxConstraints,
            _children: &[ElementId],
            _layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> Size,
        ) -> Size {
            constraints.constrain(self.size)
        }

        fn paint(
            &self,
            _offset: Offset,
            _children: &[ElementId],
            _paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
        ) -> Canvas {
            Canvas::new()
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_box_wrapper_layout() {
        let mut wrapper = BoxRenderWrapper::new(TestBox {
            size: Size::new(100.0, 50.0),
        });

        let constraints = Constraints::Box(BoxConstraints::tight(Size::new(80.0, 40.0)));
        let geometry = wrapper.layout(&[], &constraints, &mut |_, _| Geometry::default());

        assert_eq!(geometry.as_box(), Size::new(80.0, 40.0));
    }

    #[test]
    fn test_box_wrapper_downcast() {
        let wrapper = BoxRenderWrapper::new(TestBox {
            size: Size::new(100.0, 50.0),
        });

        let any = wrapper.as_any();
        assert!(any.downcast_ref::<TestBox>().is_some());
    }
}
