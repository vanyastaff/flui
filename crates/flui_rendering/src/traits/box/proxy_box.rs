//! RenderProxyBox trait - single child where size equals child size.

use flui_types::{BoxConstraints, Offset, Size};

use super::{BoxHitTestResult, SingleChildRenderBox, TextBaseline};
use crate::pipeline::PaintingContext;

/// Trait for render boxes that pass through to a single child.
///
/// RenderProxyBox is used for render objects that:
/// - Apply visual effects (opacity, color filters)
/// - Apply transformations
/// - Simply wrap a child without changing size
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderProxyBox` and `RenderProxyBoxMixin` in Flutter.
///
/// # Default Behavior
///
/// All methods delegate to the child by default:
/// - `perform_layout`: Returns child's size or `constraints.smallest()`
/// - `paint`: Paints the child at the given offset
/// - `hit_test_children`: Delegates to child
/// - Intrinsic dimensions: Delegates to child
pub trait RenderProxyBox: SingleChildRenderBox {
    /// Performs layout by delegating to the child.
    fn proxy_perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.child_mut()
            .map(|c| c.perform_layout(constraints))
            .unwrap_or_else(|| constraints.smallest())
    }

    /// Paints by delegating to the child.
    fn proxy_paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset);
        }
    }

    /// Hit tests by delegating to the child.
    fn proxy_hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        self.child()
            .map(|c| c.hit_test(result, position))
            .unwrap_or(false)
    }

    /// Computes min intrinsic width by delegating to child.
    fn proxy_compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Computes max intrinsic width by delegating to child.
    fn proxy_compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Computes min intrinsic height by delegating to child.
    fn proxy_compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_min_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Computes max intrinsic height by delegating to child.
    fn proxy_compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.compute_max_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Computes dry layout by delegating to child.
    fn proxy_compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        self.child()
            .map(|c| c.compute_dry_layout(constraints))
            .unwrap_or_else(|| constraints.smallest())
    }

    /// Computes baseline by delegating to child.
    fn proxy_compute_distance_to_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.child()
            .and_then(|c| c.compute_distance_to_baseline(baseline))
    }
}

/// How to behave during hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Defer hit testing to children.
    #[default]
    DeferToChild,

    /// Absorb hits even if no child is hit.
    Opaque,

    /// Pass through if no child is hit, but record the hit.
    Translucent,
}
