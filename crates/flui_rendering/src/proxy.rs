//! Proxy traits for pass-through render objects.
//!
//! This module provides traits for render objects that forward operations to
//! a single child with minimal modification.
//!
//! # Flutter Compatibility
//!
//! Follows Flutter's RenderProxyBox and RenderProxySliver protocols:
//! - **Size forwarding**: Adopts child's size or smallest size if no child
//! - **Paint forwarding**: Paints child at origin (identity transform)
//! - **Hit test forwarding**: Delegates to child
//! - **Baseline forwarding**: Delegates baseline calculations to child
//! - **Intrinsics forwarding**: Delegates intrinsic dimensions to child

use std::fmt;

use flui_foundation::{DiagnosticsProperty, ElementId};
use flui_interaction::HitTestResult;
use flui_types::{prelude::TextBaseline, Matrix4, Offset, Rect, Size, SliverGeometry};

use crate::arity::Single;
use crate::box_render::RenderBox;
use crate::hit_test_context::BoxHitTestContext;
use crate::layout_context::BoxLayoutContext;
use crate::object::RenderObject;
use crate::paint_context::BoxPaintContext;
use crate::sliver::RenderSliver;
use crate::BoxConstraints;

// Forward declarations for sliver contexts
use crate::hit_test_context::SliverHitTestContext;
use crate::layout_context::SliverLayoutContext;
use crate::paint_context::SliverPaintContext;

// ============================================================================
// RENDER PROXY BOX
// ============================================================================

/// Trait for box protocol render objects that delegate to a single child.
///
/// RenderProxyBox is the base for transparent wrappers that mimic most
/// properties of their child. It provides default implementations that
/// forward all operations to the child.
///
/// # Arity
///
/// Proxy boxes always have exactly one child (arity = `Single`).
///
/// # Default Behavior
///
/// All methods forward to child by default:
/// - **Layout**: Returns child size or smallest size if no child
/// - **Paint**: Paints child at origin (0, 0)
/// - **Hit test**: Forwards to child
/// - **Paint transform**: Identity (no transformation)
/// - **Baseline**: Forwards to child
/// - **Intrinsics**: Forwards to child
pub trait RenderProxyBox: RenderObject + fmt::Debug + Send + Sync {
    /// Performs layout by forwarding to the child.
    fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> crate::RenderResult<Size> {
        ctx.layout_single_child()
            .or_else(|_| Ok(self.compute_size_for_no_child(&ctx.constraints)))
    }

    /// Computes size when no child exists.
    fn compute_size_for_no_child(&self, constraints: &BoxConstraints) -> Size {
        constraints.smallest()
    }

    /// Performs painting by forwarding to the child.
    fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        let _ = ctx.paint_single_child(Offset::ZERO);
    }

    /// Performs hit testing by forwarding to the child.
    fn proxy_hit_test(
        &self,
        ctx: &BoxHitTestContext<'_, Single>,
        result: &mut HitTestResult,
    ) -> bool {
        if ctx.hit_test_single_child(ctx.position, result) {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    /// Applies paint transform for coordinate space conversion.
    fn apply_paint_transform(&self, _child_id: ElementId, _transform: &mut Matrix4) {
        // Default: identity transform (no changes)
    }

    /// Computes distance to baseline by forwarding to child.
    fn compute_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    /// Gets the local bounding rectangle.
    fn proxy_local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    /// Fills diagnostic properties for debugging.
    #[cfg(debug_assertions)]
    fn proxy_debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {}
}

impl<T: RenderProxyBox> RenderBox<Single> for T {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> crate::RenderResult<Size> {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        self.proxy_paint(ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        self.proxy_hit_test(ctx, result)
    }

    fn baseline_offset(&self) -> Option<f32> {
        self.compute_distance_to_baseline(TextBaseline::Alphabetic)
    }

    fn local_bounds(&self) -> Rect {
        self.proxy_local_bounds()
    }
}

// ============================================================================
// RENDER PROXY SLIVER
// ============================================================================

/// Trait for sliver protocol render objects that delegate to a single child.
///
/// RenderProxySliver provides default implementations that forward all
/// operations to the child sliver.
///
/// # Arity
///
/// Proxy slivers always have exactly one child (arity = `Single`).
pub trait RenderProxySliver: RenderObject + fmt::Debug + Send + Sync {
    /// Performs layout by forwarding to the child.
    fn proxy_layout(
        &mut self,
        mut ctx: SliverLayoutContext<'_, Single>,
    ) -> crate::RenderResult<SliverGeometry> {
        ctx.layout_single_child()
    }

    /// Performs painting by forwarding to the child.
    fn proxy_paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        let _ = ctx.paint_single_child(Offset::ZERO);
    }

    /// Performs hit testing by forwarding to the child.
    fn proxy_hit_test(
        &self,
        ctx: &SliverHitTestContext<'_, Single>,
        result: &mut HitTestResult,
    ) -> bool {
        if ctx.hit_test_single_child(ctx.position, result) {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    /// Gets the local bounding rectangle.
    fn proxy_local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    /// Fills diagnostic properties for debugging.
    #[cfg(debug_assertions)]
    fn proxy_debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {}
}

impl<T: RenderProxySliver> RenderSliver<Single> for T {
    fn layout(
        &mut self,
        ctx: SliverLayoutContext<'_, Single>,
    ) -> crate::RenderResult<SliverGeometry> {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        self.proxy_paint(ctx)
    }

    fn hit_test(&self, ctx: &SliverHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        self.proxy_hit_test(ctx, result)
    }

    fn local_bounds(&self) -> Rect {
        self.proxy_local_bounds()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestProxyBox {
        _label: String,
    }

    impl RenderProxyBox for TestProxyBox {}
    impl RenderObject for TestProxyBox {}

    #[derive(Debug)]
    struct TestProxySliver {
        _label: String,
    }

    impl RenderProxySliver for TestProxySliver {}
    impl RenderObject for TestProxySliver {}

    #[test]
    fn test_proxy_box_is_render_box() {
        let proxy = TestProxyBox {
            _label: "test".to_string(),
        };
        let _: &dyn RenderBox<Single> = &proxy;
    }

    #[test]
    fn test_proxy_sliver_is_render_sliver() {
        let proxy = TestProxySliver {
            _label: "test".to_string(),
        };
        let _: &dyn RenderSliver<Single> = &proxy;
    }

    #[test]
    fn test_compute_size_for_no_child() {
        let proxy = TestProxyBox {
            _label: "test".to_string(),
        };

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = proxy.compute_size_for_no_child(&constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }
}
