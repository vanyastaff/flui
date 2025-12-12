//! Type-erasure wrappers and utility types for render objects (Flutter Model).
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
//! - **Flutter model**: Uses constraints as parameters, PaintingContext for paint

use std::fmt;

use crate::box_render::BoxHitTestResult;
use crate::hit_test::SliverHitTestResult;
use flui_foundation::{Diagnosticable, DiagnosticsProperty};
use flui_interaction::{HitTestEntry, HitTestTarget};
use flui_types::events::PointerEvent;
use flui_types::{BoxConstraints, Offset, Rect, Size, SliverConstraints, SliverGeometry};

use super::box_render::RenderBox;
use super::object::RenderObject;
use super::painting_context::PaintingContext;
use super::sliver::RenderSliver;
use flui_tree::arity::Arity;

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
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::core::{BoxRenderWrapper, Single};
///
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
/// let wrapper: BoxRenderWrapper<Single> = BoxRenderWrapper::new(padding);
///
/// // Use as RenderBox<Single>
/// let size = wrapper.perform_layout(constraints);
/// ```
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

// ============================================================================
// TYPED API (RenderBox<A>)
// ============================================================================

impl<A: Arity> RenderBox<A> for BoxRenderWrapper<A> {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.inner.perform_layout(constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.inner.paint(ctx, offset)
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.inner.hit_test(result, position)
    }

    fn hit_test_self(&self, position: Offset) -> bool {
        self.inner.hit_test_self(position)
    }

    fn hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.inner.hit_test_children(result, position)
    }

    fn size(&self) -> Size {
        self.inner.size()
    }

    fn local_bounds(&self) -> Rect {
        self.inner.local_bounds()
    }

    fn compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.inner.compute_min_intrinsic_width(height)
    }

    fn compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.inner.compute_max_intrinsic_width(height)
    }

    fn compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.inner.compute_min_intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.inner.compute_max_intrinsic_height(width)
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: flui_types::layout::TextBaseline,
    ) -> Option<f32> {
        self.inner.compute_distance_to_actual_baseline(baseline)
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.inner.compute_dry_layout(constraints)
    }
}

// ============================================================================
// SUPERTRAIT IMPLEMENTATIONS
// ============================================================================

impl<A: Arity> Diagnosticable for BoxRenderWrapper<A> {
    fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
        self.inner.as_ref().debug_fill_properties(properties);
    }
}

impl<A: Arity> HitTestTarget for BoxRenderWrapper<A> {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        self.inner.as_ref().handle_event(event, entry);
    }
}

// ============================================================================
// BASE TRAIT (RenderObject)
// ============================================================================

impl<A: Arity> RenderObject for BoxRenderWrapper<A> {
    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }

    // ========================================================================
    // BOX PROTOCOL IMPLEMENTATION
    // ========================================================================

    fn perform_box_layout(&mut self, constraints: BoxConstraints) -> Option<Size> {
        Some(self.inner.perform_layout(constraints))
    }

    fn perform_box_paint(&self, ctx: &mut PaintingContext, offset: Offset) -> bool {
        self.inner.paint(ctx, offset);
        true
    }

    fn perform_box_hit_test(
        &self,
        result: &mut BoxHitTestResult,
        position: Offset,
    ) -> Option<bool> {
        Some(self.inner.hit_test(result, position))
    }

    fn box_size(&self) -> Option<Size> {
        Some(self.inner.size())
    }

    fn supports_box_protocol(&self) -> bool {
        true
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

// ============================================================================
// TYPED API (RenderSliver<A>)
// ============================================================================

impl<A: Arity> RenderSliver<A> for SliverRenderWrapper<A> {
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.inner.perform_layout(constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.inner.paint(ctx, offset)
    }

    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        self.inner
            .hit_test(result, main_axis_position, cross_axis_position)
    }

    fn hit_test_children(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        self.inner
            .hit_test_children(result, main_axis_position, cross_axis_position)
    }

    fn geometry(&self) -> SliverGeometry {
        self.inner.geometry()
    }

    fn local_bounds(&self) -> Rect {
        self.inner.local_bounds()
    }
}

// ============================================================================
// SUPERTRAIT IMPLEMENTATIONS for Sliver
// ============================================================================

impl<A: Arity> Diagnosticable for SliverRenderWrapper<A> {
    fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
        Diagnosticable::debug_fill_properties(self.inner.as_ref(), properties);
    }
}

impl<A: Arity> HitTestTarget for SliverRenderWrapper<A> {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        self.inner.as_ref().handle_event(event, entry);
    }
}

// ============================================================================
// BASE TRAIT (RenderObject) for Sliver
// ============================================================================

impl<A: Arity> RenderObject for SliverRenderWrapper<A> {
    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }

    // ========================================================================
    // SLIVER PROTOCOL IMPLEMENTATION
    // ========================================================================

    fn perform_sliver_layout(
        &mut self,
        constraints: flui_types::SliverConstraints,
    ) -> Option<flui_types::SliverGeometry> {
        Some(self.inner.perform_layout(constraints))
    }

    fn supports_sliver_protocol(&self) -> bool {
        true
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Leaf;

    #[derive(Debug)]
    struct MockRenderBox {
        value: i32,
        cached_size: Size,
    }

    impl flui_foundation::Diagnosticable for MockRenderBox {}

    impl flui_interaction::HitTestTarget for MockRenderBox {
        fn handle_event(
            &self,
            _event: &flui_types::events::PointerEvent,
            _entry: &flui_interaction::HitTestEntry,
        ) {
        }
    }

    impl RenderObject for MockRenderBox {}

    impl RenderBox<Leaf> for MockRenderBox {
        fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
            self.cached_size = constraints.smallest();
            self.cached_size
        }

        fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {}

        fn size(&self) -> Size {
            self.cached_size
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let mock = MockRenderBox {
            value: 42,
            cached_size: Size::ZERO,
        };
        let wrapper = BoxRenderWrapper::new(mock);

        assert_eq!(
            wrapper.inner().debug_name(),
            "flui_rendering::wrapper::tests::MockRenderBox"
        );
    }

    #[test]
    fn test_wrapper_downcast() {
        let mock = MockRenderBox {
            value: 42,
            cached_size: Size::ZERO,
        };
        let mut wrapper = BoxRenderWrapper::new(mock);

        let downcast = wrapper.downcast_ref::<MockRenderBox>();
        assert!(downcast.is_some());
        assert_eq!(downcast.unwrap().value, 42);

        let downcast_mut = wrapper.downcast_mut::<MockRenderBox>();
        assert!(downcast_mut.is_some());
        downcast_mut.unwrap().value = 100;

        assert_eq!(wrapper.downcast_ref::<MockRenderBox>().unwrap().value, 100);
    }

    #[test]
    fn test_wrapper_layout() {
        let mock = MockRenderBox {
            value: 42,
            cached_size: Size::ZERO,
        };
        let mut wrapper = BoxRenderWrapper::new(mock);

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = wrapper.perform_layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }
}
