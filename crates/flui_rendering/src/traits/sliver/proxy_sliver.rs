//! Proxy sliver trait for pass-through sliver objects

use crate::traits::{RenderSliver, SliverHitTestResult, SliverPaintingContext};
use crate::constraints::SliverConstraints;
use crate::geometry::SliverGeometry;
use flui_types::Offset;

/// Trait for slivers where geometry equals child geometry
///
/// RenderProxySliver is used for sliver objects that simply pass constraints
/// to their child and adopt the child's geometry. This is common for effects
/// like opacity or decorations that don't change scroll behavior.
///
/// # Key Characteristic
///
/// ```text
/// parent.geometry == child.geometry
/// ```
///
/// # Ambassador Support
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(RenderProxySliver, target = "proxy")]
/// struct RenderSliverOpacity {
///     proxy: SliverProxy,
///     opacity: f32,
/// }
///
/// impl RenderProxySliver for RenderSliverOpacity {}
/// ```
#[ambassador::delegatable_trait]
pub trait RenderProxySliver: RenderSliver {
    /// Returns a reference to the child, if any
    fn child(&self) -> Option<&dyn RenderSliver>;

    /// Returns a mutable reference to the child, if any
    fn child_mut(&mut self) -> Option<&mut dyn RenderSliver>;

    // Default implementations

    /// Layout by passing constraints to child
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        if let Some(child) = self.child_mut() {
            child.perform_layout(constraints)
        } else {
            SliverGeometry::zero()
        }
    }

    /// Hit testing delegates to child
    fn hit_test_children(
        &self,
        result: &mut dyn SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        if let Some(child) = self.child() {
            child.hit_test(result, main_axis_position, cross_axis_position)
        } else {
            false
        }
    }

    /// Paint child at the same offset
    fn paint(&self, context: &mut dyn SliverPaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            child.paint(context, offset);
        }
    }
}
