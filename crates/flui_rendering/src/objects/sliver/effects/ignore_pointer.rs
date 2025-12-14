//! RenderSliverIgnorePointer - ignores pointer events on a sliver.
//!
//! Prevents hit testing on a sliver and its children.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that ignores pointer events.
///
/// When `ignoring` is true, this sliver and its child will not receive
/// any pointer events. The child is still visible and painted normally.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::effects::RenderSliverIgnorePointer;
///
/// let ignore = RenderSliverIgnorePointer::new(true);
/// ```
#[derive(Debug)]
pub struct RenderSliverIgnorePointer {
    /// Whether to ignore pointer events.
    ignoring: bool,

    /// Whether to also ignore semantics (accessibility).
    ignoring_semantics: Option<bool>,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child geometry.
    child_geometry: SliverGeometry,
}

impl RenderSliverIgnorePointer {
    /// Creates a new ignore pointer sliver.
    pub fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            ignoring_semantics: None,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            child_geometry: SliverGeometry::zero(),
        }
    }

    /// Returns whether pointer events are being ignored.
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Sets whether to ignore pointer events.
    pub fn set_ignoring(&mut self, ignoring: bool) {
        if self.ignoring != ignoring {
            self.ignoring = ignoring;
            // No need to mark needs layout, just rebuild
        }
    }

    /// Returns whether semantics are being ignored.
    pub fn ignoring_semantics(&self) -> Option<bool> {
        self.ignoring_semantics
    }

    /// Sets whether to ignore semantics.
    pub fn set_ignoring_semantics(&mut self, value: Option<bool>) {
        self.ignoring_semantics = value;
    }

    /// Returns whether semantics should actually be ignored.
    ///
    /// Defaults to `ignoring` if `ignoring_semantics` is not set.
    pub fn effective_ignoring_semantics(&self) -> bool {
        self.ignoring_semantics.unwrap_or(self.ignoring)
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
        self.geometry = child_geometry;
        self.geometry
    }

    /// Returns the child offset (always zero, passthrough).
    pub fn child_offset(&self) -> Offset {
        Offset::ZERO
    }

    /// Paints this sliver.
    ///
    /// Painting is not affected by the ignore pointer state.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        let _ = (context, offset);
        // Paint child normally
    }

    /// Hit tests this sliver.
    ///
    /// Always returns false when `ignoring` is true.
    pub fn hit_test(&self, main_axis_position: f32, cross_axis_position: f32) -> bool {
        if self.ignoring {
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
    fn test_ignore_pointer_new() {
        let ignore = RenderSliverIgnorePointer::new(true);
        assert!(ignore.ignoring());
    }

    #[test]
    fn test_ignore_pointer_set() {
        let mut ignore = RenderSliverIgnorePointer::new(false);
        ignore.set_ignoring(true);
        assert!(ignore.ignoring());
    }

    #[test]
    fn test_ignore_pointer_layout() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = ignore.perform_layout_with_child(constraints, child_geometry);

        // Layout passes through child geometry
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }

    #[test]
    fn test_ignore_pointer_hit_test_ignoring() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        ignore.perform_layout_with_child(constraints, child_geometry);

        // When ignoring, hit test always returns false
        assert!(!ignore.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_ignore_pointer_hit_test_not_ignoring() {
        let mut ignore = RenderSliverIgnorePointer::new(false);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        ignore.perform_layout_with_child(constraints, child_geometry);

        // When not ignoring, hit test works normally
        assert!(ignore.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_ignore_pointer_semantics() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        assert!(ignore.effective_ignoring_semantics());

        ignore.set_ignoring_semantics(Some(false));
        assert!(!ignore.effective_ignoring_semantics());

        ignore.set_ignoring_semantics(None);
        assert!(ignore.effective_ignoring_semantics()); // Defaults to ignoring
    }

    #[test]
    fn test_ignore_pointer_toggle() {
        let mut ignore = RenderSliverIgnorePointer::new(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        ignore.perform_layout_with_child(constraints, child_geometry);

        // Initially ignoring
        assert!(!ignore.hit_test(50.0, 200.0));

        // Stop ignoring
        ignore.set_ignoring(false);
        assert!(ignore.hit_test(50.0, 200.0));

        // Start ignoring again
        ignore.set_ignoring(true);
        assert!(!ignore.hit_test(50.0, 200.0));
    }
}
