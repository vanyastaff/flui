//! RenderSliverOffstage - conditionally hides a sliver.
//!
//! When offstage, the sliver is not painted and takes no space.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that can be hidden from the viewport.
///
/// When `offstage` is true, the sliver is not painted and reports
/// zero geometry, effectively hiding it from the scroll view.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::basic::RenderSliverOffstage;
///
/// let offstage = RenderSliverOffstage::new(false);
/// ```
#[derive(Debug)]
pub struct RenderSliverOffstage {
    /// Whether the sliver is offstage (hidden).
    offstage: bool,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child geometry (when onstage).
    child_geometry: SliverGeometry,
}

impl RenderSliverOffstage {
    /// Creates a new offstage sliver.
    pub fn new(offstage: bool) -> Self {
        Self {
            offstage,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            child_geometry: SliverGeometry::zero(),
        }
    }

    /// Returns whether the sliver is offstage.
    pub fn offstage(&self) -> bool {
        self.offstage
    }

    /// Sets whether the sliver is offstage.
    pub fn set_offstage(&mut self, offstage: bool) {
        if self.offstage != offstage {
            self.offstage = offstage;
            // mark_needs_layout
        }
    }

    /// Returns the current geometry.
    pub fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    /// Returns the current constraints.
    pub fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    /// Returns constraints for the child (same as parent).
    pub fn constraints_for_child(&self, constraints: SliverConstraints) -> SliverConstraints {
        constraints
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        self.constraints = constraints;
        self.child_geometry = SliverGeometry::zero();
        self.geometry = SliverGeometry::zero();
        self.geometry
    }

    /// Performs layout with child geometry.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: SliverConstraints,
        child_geometry: SliverGeometry,
    ) -> SliverGeometry {
        self.constraints = constraints;
        self.child_geometry = child_geometry;

        if self.offstage {
            // When offstage, report zero geometry
            self.geometry = SliverGeometry::zero();
        } else {
            // When onstage, pass through child geometry
            self.geometry = child_geometry;
        }

        self.geometry
    }

    /// Returns the child offset (always zero, passthrough).
    pub fn child_offset(&self) -> Offset {
        Offset::ZERO
    }

    /// Paints this sliver.
    ///
    /// Does nothing when offstage.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.offstage {
            // Don't paint when offstage
            return;
        }
        let _ = (context, offset);
        // In real implementation: paint child
    }

    /// Hit tests this sliver.
    ///
    /// Returns false when offstage.
    pub fn hit_test(&self, main_axis_position: f32, cross_axis_position: f32) -> bool {
        if self.offstage {
            return false;
        }

        let geometry = &self.child_geometry;
        let constraints = &self.constraints;

        main_axis_position >= 0.0
            && main_axis_position < geometry.hit_test_extent()
            && cross_axis_position >= 0.0
            && cross_axis_position < constraints.cross_axis_extent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::GrowthDirection;
    use flui_types::layout::{Axis, AxisDirection};

    fn make_constraints(scroll_offset: f32, remaining: f32) -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            scroll_offset,
            remaining,
            600.0,
            400.0,
        )
    }

    #[test]
    fn test_offstage_new() {
        let offstage = RenderSliverOffstage::new(false);
        assert!(!offstage.offstage());
    }

    #[test]
    fn test_offstage_set() {
        let mut offstage = RenderSliverOffstage::new(false);
        offstage.set_offstage(true);
        assert!(offstage.offstage());
    }

    #[test]
    fn test_offstage_onstage_layout() {
        let mut offstage = RenderSliverOffstage::new(false);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = offstage.perform_layout_with_child(constraints, child_geometry);

        // When onstage, passes through child geometry
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }

    #[test]
    fn test_offstage_hidden_layout() {
        let mut offstage = RenderSliverOffstage::new(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = offstage.perform_layout_with_child(constraints, child_geometry);

        // When offstage, reports zero
        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
    }

    #[test]
    fn test_offstage_hit_test_onstage() {
        let mut offstage = RenderSliverOffstage::new(false);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        offstage.perform_layout_with_child(constraints, child_geometry);

        assert!(offstage.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_offstage_hit_test_hidden() {
        let mut offstage = RenderSliverOffstage::new(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        offstage.perform_layout_with_child(constraints, child_geometry);

        // No hit testing when offstage
        assert!(!offstage.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_offstage_toggle() {
        let mut offstage = RenderSliverOffstage::new(false);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        // Initially onstage
        let g1 = offstage.perform_layout_with_child(constraints, child_geometry);
        assert_eq!(g1.scroll_extent, 100.0);

        // Go offstage
        offstage.set_offstage(true);
        let g2 = offstage.perform_layout_with_child(constraints, child_geometry);
        assert_eq!(g2.scroll_extent, 0.0);

        // Come back onstage
        offstage.set_offstage(false);
        let g3 = offstage.perform_layout_with_child(constraints, child_geometry);
        assert_eq!(g3.scroll_extent, 100.0);
    }
}
