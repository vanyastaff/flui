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
use flui_types::{prelude::TextBaseline, Matrix4, Offset, Rect, Size, SliverGeometry};

use super::arity::Single;
use super::box_render::RenderBox;
use super::context::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, SliverHitTestContext,
    SliverLayoutContext, SliverPaintContext,
};
use super::object::RenderObject;
use super::sliver::RenderSliver;
use super::BoxConstraints;
use crate::RenderResult;
use flui_interaction::HitTestResult;

// ============================================================================
// RENDER PROXY BOX
// ============================================================================

/// Trait for box protocol render objects that delegate to a single child.
///
/// RenderProxyBox is the base for transparent wrappers that mimic most
/// properties of their child. It provides default implementations that
/// forward all operations to the child.
///
/// # Flutter Protocol
///
/// ```dart
/// // Flutter equivalent:
/// class RenderProxyBox extends RenderBox {
///   @override
///   void performLayout() {
///     size = child?.size ?? computeSizeForNoChild(constraints);
///   }
///
///   @override
///   void paint(PaintingContext context, Offset offset) {
///     if (child != null) context.paintChild(child, offset);
///   }
///
///   @override
///   void applyPaintTransform(RenderObject child, Matrix4 transform) {
///     // Identity transform - no changes
///   }
/// }
/// ```
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
///
/// # Override Points
///
/// Override only what you need to customize:
/// - [`proxy_layout`] - Modify constraints or size
/// - [`proxy_paint`] - Add visual effects (opacity, clipping, etc)
/// - [`proxy_hit_test`] - Custom hit testing
/// - [`apply_paint_transform`] - Coordinate space transformations
/// - [`compute_size_for_no_child`] - Size when no child exists
/// - [`compute_distance_to_baseline`] - Custom baseline calculation
///
/// # Examples
///
/// ## Minimal Proxy (Metadata)
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSemantics {
///     label: String,
/// }
///
/// // That's it! Full RenderBox for free
/// impl RenderProxyBox for RenderSemantics {}
///
/// impl RenderObject for RenderSemantics {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
///
/// ## Opacity Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderOpacity {
///     opacity: f32,
/// }
///
/// impl RenderProxyBox for RenderOpacity {
///     fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
///         ctx.canvas_mut().save();
///         ctx.canvas_mut().set_opacity(self.opacity);
///         ctx.paint_single_child(Offset::ZERO);
///         ctx.canvas_mut().restore();
///     }
/// }
///
/// impl RenderObject for RenderOpacity {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
///
/// ## Transform Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderTransform {
///     transform: Matrix4,
/// }
///
/// impl RenderProxyBox for RenderTransform {
///     fn apply_paint_transform(&self, _child_id: ElementId, transform: &mut Matrix4) {
///         // Apply transformation to child coordinates
///         transform.multiply(&self.transform);
///     }
///
///     fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
///         ctx.canvas_mut().save();
///         ctx.canvas_mut().transform(&self.transform);
///         ctx.paint_single_child(Offset::ZERO);
///         ctx.canvas_mut().restore();
///     }
/// }
///
/// impl RenderObject for RenderTransform {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
pub trait RenderProxyBox: RenderObject + fmt::Debug + Send + Sync {
    // ============================================================================
    // LAYOUT (Optional override)
    // ============================================================================

    /// Performs layout by forwarding to the child.
    ///
    /// Default: passes constraints to child, returns child size or
    /// `compute_size_for_no_child()` if no child.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// @override
    /// void performLayout() {
    ///   size = child?.size ?? computeSizeForNoChild(constraints);
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: forward to child
    /// fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
    ///     ctx.layout_single_child()
    ///         .or_else(|_| Ok(self.compute_size_for_no_child(&ctx.constraints)))
    /// }
    ///
    /// // Custom: modify constraints
    /// fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
    ///     let loosened = ctx.constraints.loosen();
    ///     ctx.layout_single_child_with(|_| loosened)
    /// }
    /// ```
    fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Try to layout child, fallback to no-child size
        ctx.layout_single_child()
            .or_else(|_| Ok(self.compute_size_for_no_child(&ctx.constraints)))
    }

    /// Computes size when no child exists (Flutter computeSizeForNoChild).
    ///
    /// Default: returns `constraints.smallest()`.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// Size computeSizeForNoChild(BoxConstraints constraints) {
    ///   return constraints.smallest;
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Custom: prefer specific size
    /// fn compute_size_for_no_child(&self, constraints: &BoxConstraints) -> Size {
    ///     constraints.constrain(Size::new(100.0, 100.0))
    /// }
    /// ```
    fn compute_size_for_no_child(&self, constraints: &BoxConstraints) -> Size {
        constraints.smallest()
    }

    // ============================================================================
    // PAINT (Optional override)
    // ============================================================================

    /// Performs painting by forwarding to the child.
    ///
    /// Default: paints child at offset (0, 0).
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// @override
    /// void paint(PaintingContext context, Offset offset) {
    ///   if (child != null) {
    ///     context.paintChild(child, offset);
    ///   }
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Custom: add opacity
    /// fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
    ///     ctx.canvas_mut().save();
    ///     ctx.canvas_mut().set_opacity(0.5);
    ///     ctx.paint_single_child(Offset::ZERO);
    ///     ctx.canvas_mut().restore();
    /// }
    /// ```
    fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        // Paint child at origin
        let _ = ctx.paint_single_child(Offset::ZERO);
    }

    // ============================================================================
    // HIT TESTING (Optional override)
    // ============================================================================

    /// Performs hit testing by forwarding to the child.
    ///
    /// Default: forwards to child, adds self if child hit.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// @override
    /// bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
    ///   return child?.hitTest(result, position: position) ?? false;
    /// }
    /// ```
    fn proxy_hit_test(
        &self,
        ctx: &BoxHitTestContext<'_, Single>,
        result: &mut HitTestResult,
    ) -> bool {
        // Forward to child
        if ctx.hit_test_single_child(ctx.position, result) {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    // ============================================================================
    // PAINT TRANSFORM (Optional override)
    // ============================================================================

    /// Applies paint transform for coordinate space conversion.
    ///
    /// Default: identity transform (no changes).
    ///
    /// Override this when paint applies transformations. The transform must
    /// match what was applied during painting for hit testing to work correctly.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// @override
    /// void applyPaintTransform(RenderObject child, Matrix4 transform) {
    ///   // Default: no transform (identity)
    /// }
    /// ```
    ///
    /// # When to override
    ///
    /// Override when:
    /// - Applying rotation, scale, or translation
    /// - Changing coordinate space during paint
    /// - Using canvas transforms
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Rotation transform
    /// fn apply_paint_transform(&self, _child_id: ElementId, transform: &mut Matrix4) {
    ///     transform.rotate_z(self.angle);
    /// }
    ///
    /// // Translation transform
    /// fn apply_paint_transform(&self, _child_id: ElementId, transform: &mut Matrix4) {
    ///     transform.translate(self.offset.dx, self.offset.dy);
    /// }
    /// ```
    fn apply_paint_transform(&self, _child_id: ElementId, _transform: &mut Matrix4) {
        // Default: identity transform (no changes)
    }

    // ============================================================================
    // BASELINE (Optional override)
    // ============================================================================

    /// Computes distance to baseline by forwarding to child.
    ///
    /// Default: forwards to child's baseline.
    ///
    /// # Flutter Protocol
    ///
    /// ```dart
    /// @override
    /// double? computeDistanceToActualBaseline(TextBaseline baseline) {
    ///   return child?.getDistanceToActualBaseline(baseline);
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Ignore baseline (return None)
    /// fn compute_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
    ///     None
    /// }
    ///
    /// // Offset child baseline
    /// fn compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
    ///     // Get child baseline and add offset
    ///     self.get_child_baseline(baseline).map(|b| b + self.offset.dy)
    /// }
    /// ```
    fn compute_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None // Default: forward to child (handled by RenderBox trait)
    }

    // ============================================================================
    // BOUNDS (Optional override)
    // ============================================================================

    /// Gets the local bounding rectangle.
    ///
    /// Default: empty rectangle.
    fn proxy_local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    // ============================================================================
    // DEBUG (Optional override)
    // ============================================================================

    /// Fills diagnostic properties for debugging.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn proxy_debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
    ///     properties.push(DiagnosticsProperty::new("opacity", self.opacity));
    /// }
    /// ```
    #[cfg(debug_assertions)]
    fn proxy_debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Default: no custom properties
    }
}

// Blanket implementation: RenderProxyBox -> RenderBox<Single>
impl<T: RenderProxyBox> RenderBox<Single> for T {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        self.proxy_paint(ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        self.proxy_hit_test(ctx, result)
    }

    fn baseline_offset(&self) -> Option<f32> {
        // Forward to child's baseline
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
///
/// # Default Behavior
///
/// - **Layout**: Forwards constraints to child, returns child geometry
/// - **Paint**: Paints child at origin
/// - **Hit test**: Forwards to child
///
/// # Examples
///
/// ## Minimal Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSliverSemantics {
///     label: String,
/// }
///
/// impl RenderProxySliver for RenderSliverSemantics {}
///
/// impl RenderObject for RenderSliverSemantics {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
///
/// ## Scroll Offset Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSliverPadding {
///     padding: f32,
/// }
///
/// impl RenderProxySliver for RenderSliverPadding {
///     fn proxy_layout(
///         &mut self,
///         mut ctx: SliverLayoutContext<'_, Single>,
///     ) -> RenderResult<SliverGeometry> {
///         // Adjust scroll offset for padding
///         let adjusted = SliverConstraints {
///             scroll_offset: (ctx.constraints.scroll_offset - self.padding).max(0.0),
///             ..ctx.constraints
///         };
///
///         let mut geometry = ctx.layout_single_child_with(|_| adjusted)?;
///         geometry.scroll_extent += self.padding;
///         Ok(geometry)
///     }
/// }
///
/// impl RenderObject for RenderSliverPadding {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
pub trait RenderProxySliver: RenderObject + fmt::Debug + Send + Sync {
    /// Performs layout by forwarding to the child.
    ///
    /// Default: passes constraints directly to child and returns child geometry.
    fn proxy_layout(
        &mut self,
        mut ctx: SliverLayoutContext<'_, Single>,
    ) -> RenderResult<SliverGeometry> {
        ctx.layout_single_child()
    }

    /// Performs painting by forwarding to the child.
    ///
    /// Default: paints child at offset (0, 0).
    fn proxy_paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        let _ = ctx.paint_single_child(Offset::ZERO);
    }

    /// Performs hit testing by forwarding to the child.
    ///
    /// Default: forwards to child, adds self if child hit.
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
    fn proxy_debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Default: no custom properties
    }
}

// Blanket implementation: RenderProxySliver -> RenderSliver<Single>
impl<T: RenderProxySliver> RenderSliver<Single> for T {
    fn layout(&mut self, ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
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

    // Simple test proxy box
    #[derive(Debug)]
    struct TestProxyBox {
        _label: String,
    }

    impl RenderProxyBox for TestProxyBox {}

    impl RenderObject for TestProxyBox {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    // Simple test proxy sliver
    #[derive(Debug)]
    struct TestProxySliver {
        _label: String,
    }

    impl RenderProxySliver for TestProxySliver {}

    impl RenderObject for TestProxySliver {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_proxy_box_is_render_box() {
        let proxy = TestProxyBox {
            _label: "test".to_string(),
        };

        // Should compile - TestProxyBox implements RenderBox<Single>
        let _: &dyn RenderBox<Single> = &proxy;
    }

    #[test]
    fn test_proxy_sliver_is_render_sliver() {
        let proxy = TestProxySliver {
            _label: "test".to_string(),
        };

        // Should compile - TestProxySliver implements RenderSliver<Single>
        let _: &dyn RenderSliver<Single> = &proxy;
    }

    #[test]
    fn test_compute_size_for_no_child() {
        let proxy = TestProxyBox {
            _label: "test".to_string(),
        };

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = proxy.compute_size_for_no_child(&constraints);

        // Should return smallest size (which is tight size)
        assert_eq!(size, Size::new(100.0, 50.0));
    }
}
