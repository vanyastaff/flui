//! RenderSliverOpacity - applies opacity to a sliver.
//!
//! Renders a sliver with a specified opacity level.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;

/// A sliver that applies opacity to its child.
///
/// The opacity is a value between 0.0 (fully transparent) and 1.0 (fully opaque).
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::sliver::effects::RenderSliverOpacity;
///
/// let opacity = RenderSliverOpacity::new(0.5);
/// ```
#[derive(Debug)]
pub struct RenderSliverOpacity {
    /// The opacity value (0.0 to 1.0).
    opacity: f32,

    /// Whether to always include in hit testing regardless of opacity.
    always_include_semantics: bool,

    /// Cached geometry from last layout.
    geometry: SliverGeometry,

    /// Cached constraints from last layout.
    constraints: SliverConstraints,

    /// Child geometry.
    child_geometry: SliverGeometry,
}

impl RenderSliverOpacity {
    /// Creates a new sliver opacity with the given opacity level.
    pub fn new(opacity: f32) -> Self {
        Self {
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
            geometry: SliverGeometry::zero(),
            constraints: SliverConstraints::default(),
            child_geometry: SliverGeometry::zero(),
        }
    }

    /// Returns the current opacity.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity.
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() > f32::EPSILON {
            self.opacity = clamped;
            // mark_needs_paint (not layout)
        }
    }

    /// Returns whether semantics are always included.
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Sets whether to always include semantics.
    pub fn set_always_include_semantics(&mut self, value: bool) {
        self.always_include_semantics = value;
    }

    /// Returns the current geometry.
    pub fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    /// Returns the current constraints.
    pub fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    /// Returns whether the sliver is fully transparent.
    pub fn is_transparent(&self) -> bool {
        self.opacity == 0.0
    }

    /// Returns whether the sliver is fully opaque.
    pub fn is_opaque(&self) -> bool {
        self.opacity == 1.0
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
    /// If opacity is 0, skips painting entirely.
    /// If opacity is 1, paints child normally.
    /// Otherwise, paints child with opacity layer.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        if self.is_transparent() {
            return;
        }

        // In real implementation:
        // if self.is_opaque() {
        //     paint child normally
        // } else {
        //     push opacity layer
        //     paint child
        //     pop layer
        // }
        let _ = (context, offset);
    }

    /// Hit tests this sliver.
    ///
    /// Returns false if transparent (unless always_include_semantics is true).
    pub fn hit_test(&self, main_axis_position: f32, cross_axis_position: f32) -> bool {
        if self.is_transparent() && !self.always_include_semantics {
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
    fn test_opacity_new() {
        let opacity = RenderSliverOpacity::new(0.5);
        assert_eq!(opacity.opacity(), 0.5);
    }

    #[test]
    fn test_opacity_clamping() {
        let opacity1 = RenderSliverOpacity::new(-0.5);
        assert_eq!(opacity1.opacity(), 0.0);

        let opacity2 = RenderSliverOpacity::new(1.5);
        assert_eq!(opacity2.opacity(), 1.0);
    }

    #[test]
    fn test_opacity_transparent() {
        let opacity = RenderSliverOpacity::new(0.0);
        assert!(opacity.is_transparent());
        assert!(!opacity.is_opaque());
    }

    #[test]
    fn test_opacity_opaque() {
        let opacity = RenderSliverOpacity::new(1.0);
        assert!(!opacity.is_transparent());
        assert!(opacity.is_opaque());
    }

    #[test]
    fn test_opacity_layout() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        let geometry = opacity.perform_layout_with_child(constraints, child_geometry);

        // Opacity passes through child geometry
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }

    #[test]
    fn test_opacity_hit_test_visible() {
        let mut opacity = RenderSliverOpacity::new(0.5);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        opacity.perform_layout_with_child(constraints, child_geometry);

        assert!(opacity.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_opacity_hit_test_transparent() {
        let mut opacity = RenderSliverOpacity::new(0.0);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        opacity.perform_layout_with_child(constraints, child_geometry);

        // Transparent sliver doesn't receive hits
        assert!(!opacity.hit_test(50.0, 200.0));
    }

    #[test]
    fn test_opacity_hit_test_always_include() {
        let mut opacity = RenderSliverOpacity::new(0.0);
        opacity.set_always_include_semantics(true);
        let constraints = make_constraints(0.0, 400.0);
        let child_geometry = SliverGeometry::new(100.0, 100.0, 0.0);

        opacity.perform_layout_with_child(constraints, child_geometry);

        // Always include semantics overrides transparency
        assert!(opacity.hit_test(50.0, 200.0));
    }
}
