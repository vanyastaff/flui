//! RenderProxySliver trait - single sliver child passthrough.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use super::{RenderSliver, SliverHitTestResult};
use crate::pipeline::PaintingContext;

/// Trait for slivers with a single sliver child.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderProxySliver` in Flutter.
pub trait RenderProxySliver: RenderSliver {
    /// Returns the child sliver, if any.
    fn child(&self) -> Option<&dyn RenderSliver>;

    /// Returns the child sliver mutably, if any.
    fn child_mut(&mut self) -> Option<&mut dyn RenderSliver>;

    /// Sets the child sliver.
    fn set_child(&mut self, child: Option<Box<dyn RenderSliver>>);

    /// Performs layout by delegating to the child.
    fn proxy_perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.child_mut()
            .map(|c| c.perform_layout(constraints))
            .unwrap_or_else(SliverGeometry::zero)
    }

    /// Paints by delegating to the child.
    fn proxy_paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_sliver_child(child, offset);
        }
    }

    /// Hit tests by delegating to the child.
    fn proxy_hit_test_children(
        &self,
        result: &mut SliverHitTestResult,
        main: f32,
        cross: f32,
    ) -> bool {
        self.child()
            .map(|c| c.hit_test(result, main, cross))
            .unwrap_or(false)
    }
}
