//! Proxy traits for pass-through render objects (Flutter Model).
//!
//! This module provides traits for render objects that forward operations to
//! a single child with minimal modification, following Flutter's exact model.
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

use crate::box_render::BoxHitTestResult;
use crate::hit_test::SliverHitTestResult;
use flui_foundation::{DiagnosticsProperty, ElementId};
use flui_types::{prelude::TextBaseline, Matrix4, Offset, Rect, Size, SliverGeometry};

use super::box_render::RenderBox;
use super::object::RenderObject;
use super::painting_context::PaintingContext;
use super::sliver::RenderSliver;
use super::BoxConstraints;
use flui_tree::arity::Single;

// ============================================================================
// RENDER PROXY BOX (Flutter Model)
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
///     if (child != null) {
///       child!.layout(constraints, parentUsesSize: true);
///       size = child!.size;
///     } else {
///       size = computeSizeForNoChild(constraints);
///     }
///   }
///
///   @override
///   void paint(PaintingContext context, Offset offset) {
///     if (child != null) context.paintChild(child!, offset);
///   }
///
///   @override
///   bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
///     return child?.hitTest(result, position: position) ?? false;
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
/// - **Layout**: Returns child size or smallest size if no child
/// - **Paint**: Paints child at origin (0, 0)
/// - **Hit test**: Forwards to child
/// - **Paint transform**: Identity (no transformation)
/// - **Baseline**: Forwards to child
/// - **Intrinsics**: Forwards to child
///
/// # Examples
///
/// ## Minimal Proxy (Metadata)
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSemantics {
///     label: String,
///     size: Size,
/// }
///
/// impl RenderProxyBox for RenderSemantics {
///     fn size(&self) -> Size { self.size }
/// }
///
/// impl RenderObject for RenderSemantics {}
/// ```
///
/// ## Opacity Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderOpacity {
///     opacity: f32,
///     size: Size,
///     child_id: RenderId,
/// }
///
/// impl RenderProxyBox for RenderOpacity {
///     fn size(&self) -> Size { self.size }
///
///     fn proxy_paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///         ctx.push_opacity((self.opacity * 255.0) as u8, |ctx| {
///             ctx.paint_child(self.child_id, offset);
///         });
///     }
/// }
///
/// impl RenderObject for RenderOpacity {}
/// ```
pub trait RenderProxyBox: RenderObject + fmt::Debug + Send + Sync {
    // ========================================================================
    // SIZE (Required)
    // ========================================================================

    /// Returns the computed size from layout.
    ///
    /// Implementations must store and return the size computed during layout.
    fn size(&self) -> Size;

    // ========================================================================
    // LAYOUT (Optional override)
    // ========================================================================

    /// Performs layout by forwarding to the child.
    ///
    /// Default returns the previously computed size. Override to implement
    /// custom layout that interacts with children.
    ///
    /// # Note
    ///
    /// In the Flutter model, proxy boxes layout their child and adopt the
    /// child's size. Since we don't have direct child access in the trait,
    /// implementations should:
    /// 1. Store child reference during construction
    /// 2. Layout child in `proxy_perform_layout`
    /// 3. Store and return the computed size
    fn proxy_perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Default: return smallest size (implementations should override)
        constraints.smallest()
    }

    /// Computes size when no child exists (Flutter computeSizeForNoChild).
    ///
    /// Default: returns `constraints.smallest()`.
    fn compute_size_for_no_child(&self, constraints: &BoxConstraints) -> Size {
        constraints.smallest()
    }

    // ========================================================================
    // PAINT (Optional override)
    // ========================================================================

    /// Performs painting by forwarding to the child.
    ///
    /// Default: no-op. Override to paint child or add effects.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn proxy_paint(&self, ctx: &mut PaintingContext, offset: Offset) {
    ///     ctx.paint_child(self.child_id, offset);
    /// }
    /// ```
    fn proxy_paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // Default: no-op (implementations should override to paint child)
    }

    // ========================================================================
    // HIT TESTING (Optional override)
    // ========================================================================

    /// Performs hit testing by forwarding to the child.
    ///
    /// Default: rectangular bounds check and returns true if within bounds.
    fn proxy_hit_test(&self, _result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Default: check if within bounds
        self.proxy_local_bounds().contains(position)
    }

    /// Hit tests children.
    ///
    /// Default: returns false (no children accessible from trait).
    /// Override to forward to child.
    fn proxy_hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        false
    }

    // ========================================================================
    // PAINT TRANSFORM (Optional override)
    // ========================================================================

    /// Applies paint transform for coordinate space conversion.
    ///
    /// Default: identity transform (no changes).
    fn apply_paint_transform(&self, _child_id: ElementId, _transform: &mut Matrix4) {
        // Default: identity transform
    }

    // ========================================================================
    // BASELINE (Optional override)
    // ========================================================================

    /// Computes distance to baseline by forwarding to child.
    ///
    /// Default: None (no baseline).
    fn compute_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }

    // ========================================================================
    // BOUNDS (Optional override)
    // ========================================================================

    /// Gets the local bounding rectangle.
    ///
    /// Default: rectangle from origin to size.
    fn proxy_local_bounds(&self) -> Rect {
        Rect::from_min_size(Offset::ZERO, self.size())
    }

    // ========================================================================
    // DEBUG (Optional override)
    // ========================================================================

    /// Fills diagnostic properties for debugging.
    #[cfg(debug_assertions)]
    fn proxy_debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Default: no custom properties
    }
}

// Blanket implementation: RenderProxyBox -> RenderBox<Single>
impl<T: RenderProxyBox> RenderBox<Single> for T {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.proxy_perform_layout(constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.proxy_paint(ctx, offset)
    }

    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Check bounds first
        if !self.proxy_local_bounds().contains(position) {
            return false;
        }

        // Test children, then self
        // Note: The tree layer is responsible for adding entries with IDs
        let children_hit = self.proxy_hit_test_children(result, position);
        children_hit || self.proxy_hit_test(result, position)
    }

    fn size(&self) -> Size {
        RenderProxyBox::size(self)
    }

    fn local_bounds(&self) -> Rect {
        self.proxy_local_bounds()
    }

    fn baseline_offset(&self) -> Option<f32> {
        self.compute_distance_to_baseline(TextBaseline::Alphabetic)
    }
}

// ============================================================================
// RENDER PROXY SLIVER (Flutter Model)
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
pub trait RenderProxySliver: RenderObject + fmt::Debug + Send + Sync {
    // ========================================================================
    // GEOMETRY (Required)
    // ========================================================================

    /// Returns the computed geometry from layout.
    ///
    /// Implementations must store and return the geometry computed during layout.
    fn geometry(&self) -> SliverGeometry;

    // ========================================================================
    // LAYOUT (Optional override)
    // ========================================================================

    /// Performs layout by forwarding to the child.
    ///
    /// Default returns zero geometry. Override to implement
    /// custom layout that interacts with children.
    fn proxy_perform_layout(
        &mut self,
        _constraints: flui_types::SliverConstraints,
    ) -> SliverGeometry {
        SliverGeometry::zero()
    }

    // ========================================================================
    // PAINT (Optional override)
    // ========================================================================

    /// Performs painting by forwarding to the child.
    ///
    /// Default: no-op. Override to paint child.
    fn proxy_paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // Default: no-op
    }

    // ========================================================================
    // HIT TESTING (Optional override)
    // ========================================================================

    /// Performs hit testing.
    ///
    /// Default: returns false.
    fn proxy_hit_test(
        &self,
        _result: &mut SliverHitTestResult,
        _main_axis_position: f32,
        _cross_axis_position: f32,
    ) -> bool {
        false
    }

    /// Hit tests children.
    ///
    /// Default: returns false.
    fn proxy_hit_test_children(
        &self,
        _result: &mut SliverHitTestResult,
        _main_axis_position: f32,
        _cross_axis_position: f32,
    ) -> bool {
        false
    }

    // ========================================================================
    // BOUNDS (Optional override)
    // ========================================================================

    /// Gets the local bounding rectangle.
    fn proxy_local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    // ========================================================================
    // DEBUG (Optional override)
    // ========================================================================

    /// Fills diagnostic properties for debugging.
    #[cfg(debug_assertions)]
    fn proxy_debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Default: no custom properties
    }
}

// Blanket implementation: RenderProxySliver -> RenderSliver<Single>
impl<T: RenderProxySliver> RenderSliver<Single> for T {
    fn perform_layout(&mut self, constraints: flui_types::SliverConstraints) -> SliverGeometry {
        self.proxy_perform_layout(constraints)
    }

    fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
        self.proxy_paint(ctx, offset)
    }

    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        // Check geometry first
        let geom = self.geometry();
        let hit_test_extent = geom.hit_test_extent.unwrap_or(0.0);
        if hit_test_extent <= 0.0 {
            return false;
        }

        if main_axis_position < 0.0 || main_axis_position >= hit_test_extent {
            return false;
        }

        // Test children
        self.proxy_hit_test_children(result, main_axis_position, cross_axis_position)
            || self.proxy_hit_test(result, main_axis_position, cross_axis_position)
    }

    fn geometry(&self) -> SliverGeometry {
        RenderProxySliver::geometry(self)
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
        cached_size: Size,
    }

    impl RenderProxyBox for TestProxyBox {
        fn size(&self) -> Size {
            self.cached_size
        }

        fn proxy_perform_layout(&mut self, constraints: BoxConstraints) -> Size {
            self.cached_size = constraints.constrain(Size::new(100.0, 50.0));
            self.cached_size
        }
    }

    impl flui_foundation::Diagnosticable for TestProxyBox {}

    impl flui_interaction::HitTestTarget for TestProxyBox {
        fn handle_event(
            &self,
            _event: &flui_types::events::PointerEvent,
            _entry: &flui_interaction::HitTestEntry,
        ) {
        }
    }

    impl RenderObject for TestProxyBox {}

    // Simple test proxy sliver
    #[derive(Debug)]
    struct TestProxySliver {
        cached_geometry: SliverGeometry,
    }

    impl RenderProxySliver for TestProxySliver {
        fn geometry(&self) -> SliverGeometry {
            self.cached_geometry
        }

        fn proxy_perform_layout(
            &mut self,
            _constraints: flui_types::SliverConstraints,
        ) -> SliverGeometry {
            self.cached_geometry = SliverGeometry {
                scroll_extent: 100.0,
                paint_extent: 50.0,
                ..Default::default()
            };
            self.cached_geometry
        }
    }

    impl flui_foundation::Diagnosticable for TestProxySliver {}

    impl flui_interaction::HitTestTarget for TestProxySliver {
        fn handle_event(
            &self,
            _event: &flui_types::events::PointerEvent,
            _entry: &flui_interaction::HitTestEntry,
        ) {
        }
    }

    impl RenderObject for TestProxySliver {}

    #[test]
    fn test_proxy_box_is_render_box() {
        let proxy = TestProxyBox {
            cached_size: Size::ZERO,
        };

        // Should compile - TestProxyBox implements RenderBox<Single>
        let _: &dyn RenderBox<Single> = &proxy;
    }

    #[test]
    fn test_proxy_sliver_is_render_sliver() {
        let proxy = TestProxySliver {
            cached_geometry: SliverGeometry::zero(),
        };

        // Should compile - TestProxySliver implements RenderSliver<Single>
        let _: &dyn RenderSliver<Single> = &proxy;
    }

    #[test]
    fn test_proxy_box_layout() {
        let mut proxy = TestProxyBox {
            cached_size: Size::ZERO,
        };

        // Tight constraints force the size to be exactly the constraint
        let constraints = BoxConstraints::tight(Size::new(200.0, 100.0));
        let size = RenderBox::<Single>::perform_layout(&mut proxy, constraints);

        // With tight constraints, constrain(100.0, 50.0) returns the constraint size (200.0, 100.0)
        assert_eq!(size, Size::new(200.0, 100.0));
        assert_eq!(RenderProxyBox::size(&proxy), Size::new(200.0, 100.0));
    }

    #[test]
    fn test_compute_size_for_no_child() {
        let proxy = TestProxyBox {
            cached_size: Size::ZERO,
        };

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let size = proxy.compute_size_for_no_child(&constraints);

        // Should return smallest size (which is tight size)
        assert_eq!(size, Size::new(100.0, 50.0));
    }
}
