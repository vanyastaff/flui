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

use crate::{context::BoxLayoutContext, parent_data::BoxParentData};

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
    /// Debug metric: number of times this subtree was repainted.
    paint_count: u32,
}

impl RenderRepaintBoundary {
    /// Creates a new repaint boundary.
    pub fn new() -> Self {
        Self { paint_count: 0 }
    }

    /// Returns the number of times this subtree has been repainted.
    pub fn paint_count(&self) -> u32 {
        self.paint_count
    }

    /// Increments the repaint counter (saturating at `u32::MAX`).
    pub fn increment_paint_count(&mut self) {
        self.paint_count = self.paint_count.saturating_add(1);
    }
}

impl Default for RenderRepaintBoundary {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::Diagnosticable for RenderRepaintBoundary {}

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

        let size = if layout_ctx.child_count() > 0 {
            // Pass-through: layout child with same constraints, adopt child size.
            layout_ctx.layout_child(0, constraints)
        } else {
            // No child — take minimum size.
            constraints.smallest()
        };

        Ok(size)
    }

    fn paint_raw(
        &self,
        recorder: &mut crate::context::FragmentRecorder,
        child_count: usize,
        size: flui_types::Size,
    ) {
        // No visual effect of its own — splice the child in order. The
        // boundary split (OffsetLayer + rebase to ZERO) is the paint
        // walk's job, keyed off `is_repaint_boundary()`.
        let mut cx = crate::context::PaintCx::<Single>::new(recorder, child_count, size);
        cx.paint_child();
    }

    fn hit_test_raw(
        &self,
        _position: crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>,
        child_count: usize,
        _size: flui_types::Size,
        hit_child: &mut (
                 dyn FnMut(
            usize,
            Option<crate::protocol::ProtocolPosition<crate::protocol::BoxProtocol>>,
        ) -> bool
                     + Send
                     + Sync
             ),
    ) -> bool {
        // Transparent to hits — the boundary affects repaint
        // scheduling only. Forward to the child at its laid-out
        // position.
        child_count > 0 && hit_child(0, None)
    }

    fn intrinsic_raw(
        &self,
        dimension: crate::storage::IntrinsicDimension,
        extent: f32,
        child_count: usize,
        child_query: &mut (
                 dyn FnMut(usize, crate::storage::IntrinsicDimension, f32) -> f32 + Send + Sync
             ),
        _child_flex: &mut (dyn FnMut(usize) -> i32 + Send + Sync),
    ) -> f32 {
        if child_count == 0 {
            0.0
        } else {
            child_query(0, dimension, extent)
        }
    }

    fn dry_layout_raw(
        &self,
        constraints: crate::protocol::ProtocolConstraints<crate::protocol::BoxProtocol>,
        child_count: usize,
        child_dry: &mut (
                 dyn FnMut(
            usize,
            crate::protocol::ProtocolConstraints<crate::protocol::BoxProtocol>,
        )
            -> crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol>
                     + Send
                     + Sync
             ),
    ) -> crate::protocol::ProtocolGeometry<crate::protocol::BoxProtocol> {
        if child_count == 0 {
            constraints.smallest()
        } else {
            child_dry(0, constraints)
        }
    }

    fn dry_baseline_raw(
        &self,
        constraints: crate::protocol::ProtocolConstraints<crate::protocol::BoxProtocol>,
        baseline: crate::traits::TextBaseline,
        child_count: usize,
        child_query: &mut (
                 dyn FnMut(
            usize,
            crate::context::DryBaselineChildRequest,
        ) -> crate::context::DryBaselineChildResponse
                     + Send
                     + Sync
             ),
    ) -> Option<f32> {
        if child_count == 0 {
            None
        } else {
            match child_query(
                0,
                crate::context::DryBaselineChildRequest::Baseline(constraints, baseline),
            ) {
                crate::context::DryBaselineChildResponse::Baseline(v) => v,
                crate::context::DryBaselineChildResponse::DryLayout(_) => None,
            }
        }
    }

    // === Optimization boundaries (the KEY overrides) ========================

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn always_needs_compositing(&self) -> bool {
        true
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
    use crate::traits::RenderObject as _;

    #[test]
    fn test_new_defaults() {
        let rb = RenderRepaintBoundary::new();
        assert_eq!(rb.paint_count(), 0);
    }

    #[test]
    fn test_default_matches_new() {
        let a = RenderRepaintBoundary::new();
        let b = RenderRepaintBoundary::default();
        assert_eq!(a.paint_count(), b.paint_count());
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
        assert!(rb.paint_transform(flui_types::Size::ZERO).is_none());
    }
}
