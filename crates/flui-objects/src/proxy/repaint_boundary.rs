//! RenderRepaintBoundary — marks a repaint boundary for paint caching.
//!
//! A pure proxy render object (single-child, pass-through layout) whose sole
//! purpose is to return `true` from [`is_repaint_boundary`] so the pipeline
//! allocates a dedicated compositing layer for this subtree.  When the child
//! tree needs repainting, only this subtree's layer is invalidated — siblings
//! and ancestors keep their cached paint results.
//!
//! [`is_repaint_boundary`]: flui_rendering::traits::RenderObject::is_repaint_boundary

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
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
/// This type implements `RenderBox` and overrides `is_repaint_boundary` and
/// `always_needs_compositing` — hooks added to `RenderBox` so the blanket
/// `impl RenderObject<BoxProtocol> for T where T: RenderBox` forwards them
/// to the concrete type without requiring direct protocol-trait access.
#[derive(Debug, Clone, Default)]
pub struct RenderRepaintBoundary {
    /// Debug metric: number of times this subtree was repainted.
    paint_count: u32,
    /// Whether a child was attached at the last layout — gates hit-testing
    /// (a childless boundary must not absorb hits).
    has_child: bool,
}

impl RenderRepaintBoundary {
    /// Creates a new repaint boundary.
    pub fn new() -> Self {
        Self::default()
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

impl flui_foundation::Diagnosticable for RenderRepaintBoundary {}

// ============================================================================
// RenderBox impl
// ============================================================================

impl RenderBox for RenderRepaintBoundary {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            // Pass-through: layout child with same constraints, adopt child size.
            ctx.layout_child(0, constraints)
        } else {
            self.has_child = false;
            // No child — take minimum size.
            constraints.smallest()
        }
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // No visual effect of its own — splice the child in order. The
        // boundary split (OffsetLayer + rebase to ZERO) is the paint
        // walk's job, keyed off `is_repaint_boundary()`.
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Pure pass-through (Flutter RenderProxyBox: `hitTestSelf` is false, so
        // the boundary is hit iff its child is hit). Without this override the
        // trait default `is_within_own_size()` would absorb the hit on the
        // boundary itself and never recurse — blocking the entire subtree from
        // receiving pointer events.
        if !ctx.is_within_own_size() {
            return false;
        }
        self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    // === Optimization boundaries (the KEY overrides) ========================

    fn is_repaint_boundary(&self) -> bool {
        true
    }

    fn always_needs_compositing(&self) -> bool {
        true
    }

    // === Query pass-through =================================================

    flui_rendering::forward_single_child_box_queries!();

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
        use flui_rendering::protocol::RenderObject;
        let rb = RenderRepaintBoundary::new();
        assert!(RenderObject::<flui_rendering::protocol::BoxProtocol>::is_repaint_boundary(&rb));
    }

    #[test]
    fn test_always_needs_compositing() {
        use flui_rendering::protocol::RenderObject;
        let rb = RenderRepaintBoundary::new();
        assert!(
            RenderObject::<flui_rendering::protocol::BoxProtocol>::always_needs_compositing(&rb)
        );
    }

    #[test]
    fn test_debug_name() {
        use flui_rendering::protocol::RenderObject;
        let rb = RenderRepaintBoundary::new();
        assert_eq!(
            RenderObject::<flui_rendering::protocol::BoxProtocol>::debug_name(&rb),
            "RenderRepaintBoundary"
        );
    }

    #[test]
    fn test_paint_effects_none() {
        use flui_rendering::traits::RenderBox;
        let rb = RenderRepaintBoundary::new();
        assert!(RenderBox::paint_alpha(&rb).is_none());
        assert!(RenderBox::paint_transform(&rb, flui_types::Size::ZERO).is_none());
    }
}
