//! RenderRepaintBoundary — marks a repaint boundary for paint caching.
//!
//! A pure proxy render object (single-child, pass-through layout) whose sole
//! purpose is to return `true` from [`is_repaint_boundary`] so the pipeline
//! allocates a dedicated compositing layer for this subtree.  When the child
//! tree needs repainting, only this subtree's layer is invalidated — siblings
//! and ancestors keep their cached paint results.
//!
//! [`is_repaint_boundary`]: crate::traits::RenderObject::is_repaint_boundary

use flui_tree::Single;
use flui_types::{Offset, Point, Rect, Size};

use crate::{
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, SemanticsCapability},
};

/// A render object that marks its subtree as a repaint boundary.
///
/// `RenderRepaintBoundary` is a pure proxy: single-child, pass-through
/// layout, no visual effect of its own.  Its sole purpose is the
/// `is_repaint_boundary() → true` override that tells the pipeline to cache
/// the painted subtree on a dedicated compositing layer.
///
/// # Debug Metrics
///
/// A `paint_count` counter is available for diagnostics.  Increment it via
/// [`increment_paint_count`](Self::increment_paint_count) each time the
/// pipeline repaints this boundary.
///
/// # Implementation Note
///
/// This type implements `RenderObject<BoxProtocol>` **directly** rather than
/// going through the `RenderBox` blanket impl.  The blanket impl cannot
/// delegate `is_repaint_boundary` or `always_needs_compositing` to the
/// concrete type (they use the trait defaults), so a direct impl is required
/// to override those methods.  Layout is bridged through the same
/// `BoxLayoutCtx::from_erased` path the blanket impl uses.
#[derive(Debug, Clone)]
pub struct RenderRepaintBoundary {
    /// Size after layout.
    size: Size,
    /// Debug metric: number of times this subtree was repainted.
    paint_count: u32,
}

impl RenderRepaintBoundary {
    /// Creates a new repaint boundary with zero size.
    pub fn new() -> Self {
        Self {
            size: Size::ZERO,
            paint_count: 0,
        }
    }

    /// Returns the number of times this subtree has been repainted.
    pub fn paint_count(&self) -> u32 {
        self.paint_count
    }

    /// Increments the repaint counter (saturating at `u32::MAX`).
    pub fn increment_paint_count(&mut self) {
        self.paint_count = self.paint_count.saturating_add(1);
    }

    /// Returns a reference to the current size.
    pub fn size(&self) -> &Size {
        &self.size
    }
}

impl Default for RenderRepaintBoundary {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Capability impls — all default / no-op
// ============================================================================

impl flui_foundation::Diagnosticable for RenderRepaintBoundary {}
impl PaintEffectsCapability for RenderRepaintBoundary {}
impl SemanticsCapability for RenderRepaintBoundary {}
impl HotReloadCapability for RenderRepaintBoundary {}

// ============================================================================
// Direct RenderObject<BoxProtocol> impl
// ============================================================================
//
// Implements the protocol trait directly so we can override
// `is_repaint_boundary` and `always_needs_compositing`.  The layout bridge
// mirrors the `RenderBox` blanket impl in `traits/render_box.rs`.

impl crate::protocol::RenderObject<crate::protocol::BoxProtocol> for RenderRepaintBoundary {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <crate::protocol::BoxProtocol as crate::protocol::Protocol>::LayoutCtxErased<'_>,
    ) -> crate::error::RenderResult<crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>>
    {
        // Bridge through the same BoxLayoutCtx path the blanket impl uses.
        let typed_inner = crate::protocol::BoxLayoutCtx::<Single, BoxParentData>::from_erased(ctx);
        let mut layout_ctx = BoxLayoutContext::<Single, BoxParentData>::new(typed_inner);

        let constraints = *layout_ctx.constraints();

        if layout_ctx.child_count() > 0 {
            // Pass-through: layout child with same constraints, adopt child size.
            let child_size = layout_ctx.layout_child(0, constraints);
            self.size = child_size;
        } else {
            // No child — take minimum size.
            self.size = constraints.smallest();
        }

        layout_ctx.complete_with_size(self.size);

        layout_ctx.inner().geometry().copied().ok_or_else(|| {
            crate::error::RenderError::contract_violation(
                self.debug_name(),
                "RenderRepaintBoundary layout did not complete",
            )
        })
    }

    fn paint(&self, _context: &mut crate::pipeline::CanvasContext, _offset: Offset) {
        // No-op — repaint boundary applies no visual effect.
        // Child painting is handled by the pipeline.
    }

    fn hit_test_raw(
        &self,
        _result: &mut crate::protocol::ProtocolHitResult<crate::protocol::BoxProtocol>,
        _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
    ) -> bool {
        // Protocol bridge — real hit testing flows through the pipeline.
        false
    }

    // === Optimization boundaries (the KEY overrides) ========================

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn always_needs_compositing(&self) -> bool {
        true
    }

    // === Geometry access ====================================================

    fn geometry(&self) -> &crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
        &self.size
    }

    fn set_geometry(
        &mut self,
        geometry: crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>,
    ) {
        self.size = geometry;
    }

    fn paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    fn debug_name(&self) -> &'static str {
        "RenderRepaintBoundary"
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_defaults() {
        let rb = RenderRepaintBoundary::new();
        assert_eq!(*rb.size(), Size::ZERO);
        assert_eq!(rb.paint_count(), 0);
    }

    #[test]
    fn test_default_matches_new() {
        let a = RenderRepaintBoundary::new();
        let b = RenderRepaintBoundary::default();
        assert_eq!(a.paint_count(), b.paint_count());
        assert_eq!(*a.size(), *b.size());
    }

    #[test]
    fn test_paint_count_increment() {
        let mut rb = RenderRepaintBoundary::new();
        rb.increment_paint_count();
        assert_eq!(rb.paint_count(), 1);
        rb.increment_paint_count();
        assert_eq!(rb.paint_count(), 2);
    }

    #[test]
    fn test_paint_count_saturates() {
        let mut rb = RenderRepaintBoundary::new();
        rb.paint_count = u32::MAX;
        rb.increment_paint_count();
        assert_eq!(rb.paint_count(), u32::MAX);
    }

    #[test]
    fn test_is_repaint_boundary() {
        use crate::protocol::RenderObject;
        let rb = RenderRepaintBoundary::new();
        assert!(RenderObject::<crate::protocol::BoxProtocol>::is_repaint_boundary(&rb));
    }

    #[test]
    fn test_always_needs_compositing() {
        use crate::protocol::RenderObject;
        let rb = RenderRepaintBoundary::new();
        assert!(RenderObject::<crate::protocol::BoxProtocol>::always_needs_compositing(&rb));
    }

    #[test]
    fn test_debug_name() {
        use crate::protocol::RenderObject;
        let rb = RenderRepaintBoundary::new();
        assert_eq!(
            RenderObject::<crate::protocol::BoxProtocol>::debug_name(&rb),
            "RenderRepaintBoundary"
        );
    }

    #[test]
    fn test_paint_effects_none() {
        let rb = RenderRepaintBoundary::new();
        assert!(rb.paint_alpha().is_none());
        assert!(rb.paint_transform().is_none());
    }
}
