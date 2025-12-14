//! RenderSliverVisibility - controls visibility of a sliver.
//!
//! Provides fine-grained control over sliver visibility.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that controls visibility of its child.
///
/// Unlike `RenderSliverOffstage`, this provides more control over what
/// aspects of the sliver are visible (painting, hit testing, semantics,
/// layout contribution).
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::effects::RenderSliverVisibility;
///
/// let visibility = RenderSliverVisibility::new()
///     .with_visible(false)
///     .with_maintain_size(true);
/// ```
#[derive(Debug)]
pub struct RenderSliverVisibility {
    /// Whether the sliver is visible.
    visible: bool,

    /// Whether to maintain the size even when not visible.
    maintain_size: bool,

    /// Whether to maintain interactivity when not visible.
    maintain_interactivity: bool,

    /// Whether to maintain semantics when not visible.
    maintain_semantics: bool,

    /// Whether to maintain animation state when not visible.
    maintain_animation: bool,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child geometry.
    child_geometry: SliverGeometry,
}

impl Default for RenderSliverVisibility {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderSliverVisibility {
    /// Creates a new visibility sliver (visible by default).
    pub fn new() -> Self {
        Self {
            visible: true,
            maintain_size: false,
            maintain_interactivity: false,
            maintain_semantics: false,
            maintain_animation: false,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            child_geometry: SliverGeometry::zero(),
        }
    }

    /// Sets visibility.
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets whether to maintain size when not visible.
    pub fn with_maintain_size(mut self, maintain: bool) -> Self {
        self.maintain_size = maintain;
        self
    }

    /// Sets whether to maintain interactivity when not visible.
    pub fn with_maintain_interactivity(mut self, maintain: bool) -> Self {
        self.maintain_interactivity = maintain;
        self
    }

    /// Sets whether to maintain semantics when not visible.
    pub fn with_maintain_semantics(mut self, maintain: bool) -> Self {
        self.maintain_semantics = maintain;
        self
    }

    /// Sets whether to maintain animation when not visible.
    pub fn with_maintain_animation(mut self, maintain: bool) -> Self {
        self.maintain_animation = maintain;
        self
    }

    /// Returns whether the sliver is visible.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Sets visibility.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            // mark_needs_layout or mark_needs_paint depending on maintain_size
        }
    }

    /// Returns whether to maintain size.
    pub fn maintain_size(&self) -> bool {
        self.maintain_size
    }

    /// Returns whether to maintain interactivity.
    pub fn maintain_interactivity(&self) -> bool {
        self.maintain_interactivity
    }

    /// Returns whether to maintain semantics.
    pub fn maintain_semantics(&self) -> bool {
        self.maintain_semantics
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

        if self.visible || self.maintain_size {
            self.geometry = child_geometry;
        } else {
            self.geometry = SliverGeometry::zero();
        }

        self.geometry
    }

    /// Returns the child offset (always zero, passthrough).
    pub fn child_offset(&self) -> Offset {
        Offset::ZERO
    }

    /// Paints this sliver.
    ///
    /// Only paints child if visible.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if !self.visible {
            return;
        }
        let _ = (context, offset);
        // Paint child
    }

    /// Hit tests this sliver.
    ///
    /// Returns false unless visible or maintain_interactivity is true.
    pub fn hit_test(&self, main_axis_position: f32, cross_axis_position: f32) -> bool {
        if !self.visible && !self.maintain_interactivity {
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
    fn test_visibility_new() {
        let visibility = RenderSliverVisibility::new();
        assert!(visibility.visible());
        assert!(!visibility.maintain_size());
    }

    #[test]
    fn test_visibility_builder() {
        let visibility = RenderSliverVisibility::new()
            .with_visible(false)
            .with_maintain_size(true)
            .with_maintain_interactivity(true);

        assert!(!visibility.visible());
        assert!(visibility.maintain_size());
        assert!(visibility.maintain_interactivity());
    }

    #[test]
    fn test_visibility_visible_layout() {
        let mut visibility = RenderSliverVisibility::new();
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = visibility.perform_layout_with_child(constraints, child_geometry);

        assert_eq!(geometry.scroll_extent, 100.0);
    }

    #[test]
    fn test_visibility_hidden_layout() {
        let mut visibility = RenderSliverVisibility::new().with_visible(false);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = visibility.perform_layout_with_child(constraints, child_geometry);

        // Hidden without maintain_size reports zero
        assert_eq!(geometry.scroll_extent, 0.0);
    }

    #[test]
    fn test_visibility_hidden_maintain_size() {
        let mut visibility = RenderSliverVisibility::new()
            .with_visible(false)
            .with_maintain_size(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = visibility.perform_layout_with_child(constraints, child_geometry);

        // Hidden with maintain_size reports child geometry
        assert_eq!(geometry.scroll_extent, 100.0);
    }

    #[test]
    fn test_visibility_hit_test_visible() {
        let mut visibility = RenderSliverVisibility::new();
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        visibility.perform_layout_with_child(constraints, child_geometry);

        assert!(visibility.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_visibility_hit_test_hidden() {
        let mut visibility = RenderSliverVisibility::new().with_visible(false);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        visibility.perform_layout_with_child(constraints, child_geometry);

        assert!(!visibility.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_visibility_hit_test_maintain_interactivity() {
        let mut visibility = RenderSliverVisibility::new()
            .with_visible(false)
            .with_maintain_interactivity(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        visibility.perform_layout_with_child(constraints, child_geometry);

        // Hidden but maintains interactivity
        assert!(visibility.hit_test(50.0, 200.0));
    }
}
