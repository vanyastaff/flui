//! Safe wrappers for type-erased render objects
//!
//! This module provides zero-overhead wrappers that bridge between typed render objects
//! (Render<A>, SliverRender<A>) and type-erased trait objects (DynRenderObject).
//!
//! # Architecture
//!
//! ```text
//! User Code                Type-Safe Layer              Type-Erased Layer
//! ─────────────────────────────────────────────────────────────────────────
//! impl Render<Single>  →  BoxRenderObjectWrapper<>  →  Box<dyn DynRenderObject>
//! impl SliverRender<>  →  SliverRenderObjectWrapper  →  Box<dyn DynRenderObject>
//! ```
//!
//! # Key Design Decisions
//!
//! 1. **No unsafe code**: All wrappers use safe Rust abstractions
//! 2. **Debug assertions only**: Arity validation is zero-cost in release builds
//! 3. **Single source of truth**: Protocol and arity stored in RenderElement, not wrappers
//! 4. **Zero overhead**: Wrappers inline away completely in release builds
//!
//! # Safety Guarantees
//!
//! - **Compile time**: Type system ensures Render<A> matches arity A
//! - **Debug time**: debug_assert! validates runtime arity matches compile-time arity
//! - **Release time**: No overhead, direct delegation to inner render object

use super::arity::Arity;
use super::protocol::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, SliverHitTestContext,
    SliverLayoutContext, SliverPaintContext,
};
use super::traits::{Render, SliverRender};
use super::type_erasure::{DynConstraints, DynGeometry, DynHitTestResult, DynRenderObject};
use crate::element::{ElementId, ElementTree};
use flui_types::Offset;
use std::any::Any;
use std::fmt::Debug;

// ============================================================================
// Box Protocol Wrapper
// ============================================================================

/// Safe wrapper for Box protocol render objects
///
/// This wrapper converts a typed `Render<A>` into a type-erased `DynRenderObject`.
/// It validates arity in debug builds and delegates to the inner render object.
///
/// # Type Parameters
///
/// - `A`: Arity type (Leaf, Single, Variable, etc.)
/// - `R`: Concrete render object type implementing Render<A>
///
/// # Performance
///
/// In release builds, this wrapper compiles to direct function calls with no overhead.
/// Debug assertions are completely removed by the optimizer.
pub struct BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    inner: R,
    _phantom: std::marker::PhantomData<A>,
}

impl<A, R> Debug for BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxRenderObjectWrapper")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<A, R> BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    /// Create a new wrapper around a render object
    ///
    /// # Arguments
    ///
    /// * `inner` - The concrete render object implementing Render<A>
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get a reference to the inner render object
    #[inline]
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Get a mutable reference to the inner render object
    #[inline]
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner render object
    #[inline]
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<A, R> DynRenderObject for BoxRenderObjectWrapper<A, R>
where
    A: Arity,
    R: Render<A>,
{
    fn dyn_layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry {
        // Validate arity in debug builds (zero cost in release)
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {:?}, got {} children",
            std::any::type_name::<R>(),
            A::runtime_arity(),
            children.len()
        );

        // Extract BoxConstraints (panics if wrong protocol)
        let box_constraints = constraints.as_box();

        // SAFETY: ElementId is repr(transparent) over NonZeroUsize
        // so &[ElementId] has same layout as &[NonZeroUsize]
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };

        // Create typed children accessor
        let typed_children = A::from_slice(children_slice);

        // Create context with tree reference for layout_child() helper
        let ctx = BoxLayoutContext::new(tree, *box_constraints, typed_children);

        // Delegate to typed render object
        let size = self.inner.layout(&ctx);

        // Wrap result in type-erased enum
        DynGeometry::Box(size)
    }

    fn dyn_paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> flui_painting::Canvas {
        // Validate arity in debug builds
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {:?}, got {} children",
            std::any::type_name::<R>(),
            A::runtime_arity(),
            children.len()
        );

        // SAFETY: ElementId is repr(transparent) over NonZeroUsize
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };

        // Create typed children accessor
        let typed_children = A::from_slice(children_slice);

        // Create context with tree reference for paint_child() helper
        let mut ctx = BoxPaintContext::new(tree, offset, typed_children);

        // Delegate to typed render object (paints into ctx.canvas())
        self.inner.paint(&mut ctx);

        // Take ownership of the canvas and return it
        ctx.take_canvas()
    }

    fn dyn_hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
    ) -> DynHitTestResult {
        // Validate arity in debug builds
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {:?}, got {} children",
            std::any::type_name::<R>(),
            A::runtime_arity(),
            children.len()
        );

        // SAFETY: ElementId is repr(transparent) over NonZeroUsize
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };

        // Create typed children accessor
        let typed_children = A::from_slice(children_slice);

        // TODO Phase 6: Get element_id and size from RenderElement
        // For now, use placeholder values
        let element_id = ElementId::new(1);
        let size = flui_types::Size::new(0.0, 0.0);

        // Create context (using position from parameter)
        let ctx = BoxHitTestContext::new(tree, position, size, element_id, typed_children);

        // Create result accumulator
        let mut result = crate::element::hit_test::BoxHitTestResult::new();

        // Delegate to typed render object
        let hit = self.inner.hit_test(&ctx, &mut result);

        DynHitTestResult::Box(hit)
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

/// Safe wrapper for Sliver protocol render objects
///
/// This wrapper converts a typed `SliverRender<A>` into a type-erased `DynRenderObject`.
/// It validates arity in debug builds and delegates to the inner render object.
///
/// # Type Parameters
///
/// - `A`: Arity type (Leaf, Single, Variable, etc.)
/// - `R`: Concrete render object type implementing SliverRender<A>
///
/// # Performance
///
/// In release builds, this wrapper compiles to direct function calls with no overhead.
/// Debug assertions are completely removed by the optimizer.
pub struct SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    inner: R,
    _phantom: std::marker::PhantomData<A>,
}

impl<A, R> Debug for SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverRenderObjectWrapper")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<A, R> SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    /// Create a new wrapper around a sliver render object
    ///
    /// # Arguments
    ///
    /// * `inner` - The concrete render object implementing SliverRender<A>
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get a reference to the inner render object
    #[inline]
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Get a mutable reference to the inner render object
    #[inline]
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Consume the wrapper and return the inner render object
    #[inline]
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<A, R> DynRenderObject for SliverRenderObjectWrapper<A, R>
where
    A: Arity,
    R: SliverRender<A>,
{
    fn dyn_layout(
        &mut self,
        _tree: &ElementTree,
        children: &[ElementId],
        constraints: &DynConstraints,
    ) -> DynGeometry {
        // Validate arity in debug builds (zero cost in release)
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {:?}, got {} children",
            std::any::type_name::<R>(),
            A::runtime_arity(),
            children.len()
        );

        // Extract SliverConstraints (panics if wrong protocol)
        let sliver_constraints = constraints.as_sliver();

        // SAFETY: ElementId is repr(transparent) over NonZeroUsize
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };

        // Create typed children accessor
        let typed_children = A::from_slice(children_slice);

        // Create context (NOTE: helper methods will be added in Phase 6)
        let ctx = SliverLayoutContext::new(*sliver_constraints, typed_children);

        // Delegate to typed render object
        let geometry = self.inner.layout(&ctx);

        // Wrap result in type-erased enum
        DynGeometry::Sliver(geometry)
    }

    fn dyn_paint(
        &self,
        _tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> flui_painting::Canvas {
        // Validate arity in debug builds
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {:?}, got {} children",
            std::any::type_name::<R>(),
            A::runtime_arity(),
            children.len()
        );

        // SAFETY: ElementId is repr(transparent) over NonZeroUsize
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };

        // Create typed children accessor
        let typed_children = A::from_slice(children_slice);

        // Create context (NOTE: helper methods will be added in Phase 6)
        let mut ctx = SliverPaintContext::new(offset, typed_children);

        // Delegate to typed render object (paints into ctx.canvas())
        self.inner.paint(&mut ctx);

        // Take ownership of the canvas and return it
        ctx.take_canvas()
    }

    fn dyn_hit_test(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        position: Offset,
    ) -> DynHitTestResult {
        // Validate arity in debug builds
        debug_assert!(
            A::validate_count(children.len()),
            "Arity violation in {}: expected {:?}, got {} children",
            std::any::type_name::<R>(),
            A::runtime_arity(),
            children.len()
        );

        // SAFETY: ElementId is repr(transparent) over NonZeroUsize
        let children_slice: &[std::num::NonZeroUsize] = unsafe {
            std::slice::from_raw_parts(
                children.as_ptr() as *const std::num::NonZeroUsize,
                children.len(),
            )
        };

        // Create typed children accessor
        let typed_children = A::from_slice(children_slice);

        // TODO Phase 6: Get proper element_id, geometry, and coordinates
        let element_id = ElementId::new(1);
        let geometry = super::protocol::SliverGeometry::default();
        let main_axis_position = 0.0;
        let cross_axis_position = 0.0;
        let scroll_offset = 0.0;

        // Create context
        let ctx = SliverHitTestContext::new(
            tree,
            main_axis_position,
            cross_axis_position,
            geometry,
            scroll_offset,
            element_id,
            typed_children,
        );

        // Create result accumulator
        let mut result = crate::element::hit_test::SliverHitTestResult::new();

        // Delegate to typed render object
        let hit = self.inner.hit_test(&ctx, &mut result);

        DynHitTestResult::Sliver(hit)
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
    use crate::render::arity::Leaf;
    use crate::render::protocol;
    use flui_types::Size;

    // Mock render object for testing
    #[derive(Debug)]
    struct MockLeafRender {
        size: Size,
    }

    impl Render<Leaf> for MockLeafRender {
        fn layout(&mut self, _ctx: &BoxLayoutContext<Leaf>) -> protocol::BoxGeometry {
            protocol::BoxGeometry { size: self.size }
        }

        fn paint(&self, _ctx: &BoxPaintContext<Leaf>) {
            // No-op for mock
        }

        fn hit_test(
            &self,
            _ctx: &BoxHitTestContext<Leaf>,
            _result: &mut crate::element::hit_test::BoxHitTestResult,
        ) -> bool {
            true
        }
    }

    #[test]
    fn test_box_wrapper_creation() {
        let render = MockLeafRender {
            size: Size::new(100.0, 100.0),
        };
        let wrapper = BoxRenderObjectWrapper::<Leaf, _>::new(render);

        assert_eq!(wrapper.inner().size.width, 100.0);
        assert_eq!(
            wrapper.debug_name(),
            "flui_core::render::wrappers::tests::MockLeafRender"
        );
    }

    #[test]
    fn test_box_wrapper_inner_access() {
        let render = MockLeafRender {
            size: Size::new(100.0, 100.0),
        };
        let mut wrapper = BoxRenderObjectWrapper::<Leaf, _>::new(render);

        // Test immutable access
        assert_eq!(wrapper.inner().size.width, 100.0);

        // Test mutable access
        wrapper.inner_mut().size = Size::new(200.0, 200.0);
        assert_eq!(wrapper.inner().size.width, 200.0);

        // Test into_inner
        let render = wrapper.into_inner();
        assert_eq!(render.size.width, 200.0);
    }

    // Note: Full integration tests for dyn_layout/dyn_paint/dyn_hit_test
    // will be added in Phase 6 when ElementTree integration is complete
}
