//! RenderProxySliver trait - single sliver child passthrough.

use flui_types::{Offset, Rect};

use crate::constraints::{SliverConstraints, SliverGeometry};
use crate::parent_data::ParentData;
use crate::traits::RenderObject;

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

    /// Returns the semantic bounds by delegating to the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to the `semanticBounds` getter override in Flutter's
    /// `RenderProxySliver`.
    fn proxy_semantic_bounds(&self) -> Rect {
        self.child()
            .map(|c| c.semantic_bounds())
            .unwrap_or_else(|| Rect::from_ltwh(0.0, 0.0, 0.0, 0.0))
    }

    /// Sets up parent data for a child render object.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `setupParentData` in Flutter's `RenderProxySliver`,
    /// which sets up `SliverPhysicalParentData` for children.
    fn proxy_setup_parent_data(&self, child: &mut dyn RenderObject) {
        // Default implementation sets up sliver physical parent data
        // Subclasses can override to set up different parent data types
        if child.parent_data().is_none() {
            child.set_parent_data(Box::new(SliverPhysicalParentData::default()));
        }
    }

    /// Returns the main axis position of the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `childMainAxisPosition` in Flutter's `RenderProxySliver`,
    /// which always returns 0.0 for proxy slivers.
    fn proxy_child_main_axis_position(&self, _child: &dyn RenderSliver) -> f32 {
        0.0
    }

    /// Applies paint transform for a child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `applyPaintTransform` in Flutter's `RenderProxySliver`.
    /// The default implementation applies the paint offset from `SliverPhysicalParentData`.
    fn proxy_apply_paint_transform(&self, child: &dyn RenderObject, transform: &mut [f32; 16]) {
        if let Some(parent_data) = child.parent_data() {
            if let Some(sliver_data) = parent_data
                .as_any()
                .downcast_ref::<SliverPhysicalParentData>()
            {
                sliver_data.apply_paint_transform(transform);
            }
        }
    }
}

/// Physical parent data for sliver children.
///
/// # Flutter Equivalence
///
/// This corresponds to `SliverPhysicalParentData` in Flutter.
#[derive(Debug, Default)]
pub struct SliverPhysicalParentData {
    /// The paint offset for this child.
    pub paint_offset: Offset,
}

impl SliverPhysicalParentData {
    /// Creates new sliver physical parent data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates sliver physical parent data with the given paint offset.
    pub fn with_offset(paint_offset: Offset) -> Self {
        Self { paint_offset }
    }

    /// Applies the paint transform to the given matrix.
    ///
    /// This translates the matrix by the paint offset.
    pub fn apply_paint_transform(&self, transform: &mut [f32; 16]) {
        // Apply translation to the 4x4 matrix
        // Matrix layout (column-major):
        // [0]  [4]  [8]  [12]
        // [1]  [5]  [9]  [13]
        // [2]  [6]  [10] [14]
        // [3]  [7]  [11] [15]
        transform[12] += self.paint_offset.dx;
        transform[13] += self.paint_offset.dy;
    }
}

impl ParentData for SliverPhysicalParentData {
    fn detach(&mut self) {
        self.paint_offset = Offset::ZERO;
    }
}
