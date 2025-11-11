//! RenderSliverOffstage - Conditionally hides sliver without removing from tree

use flui_core::render::{Arity, RenderSliver, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::SliverGeometry;

/// RenderObject that conditionally hides a sliver child
///
/// When offstage=true, the child is not painted and reports zero geometry,
/// but remains in the element tree (unlike conditionally removing it).
/// This is useful for:
/// - Animated show/hide transitions
/// - Maintaining scroll position when toggling visibility
/// - Preloading content before showing it
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverOffstage;
///
/// // Child is hidden
/// let offstage = RenderSliverOffstage::new(true);
/// ```
#[derive(Debug)]
pub struct RenderSliverOffstage {
    /// Whether child is offstage (hidden)
    pub offstage: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverOffstage {
    /// Create new sliver offstage
    ///
    /// # Arguments
    /// * `offstage` - Whether to hide the child
    pub fn new(offstage: bool) -> Self {
        Self {
            offstage,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set offstage state
    pub fn set_offstage(&mut self, offstage: bool) {
        self.offstage = offstage;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Check if child should be painted
    pub fn should_paint(&self) -> bool {
        !self.offstage
    }

    /// Check if child should participate in hit testing
    pub fn should_hit_test(&self) -> bool {
        !self.offstage
    }
}

impl Default for RenderSliverOffstage {
    fn default() -> Self {
        Self::new(false) // Default to visible
    }
}

impl RenderSliver for RenderSliverOffstage {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        if self.offstage {
            // When offstage, report zero geometry
            self.sliver_geometry = SliverGeometry::default();
        } else {
            // Pass through to child when visible
            if let Some(child_id) = ctx.children.try_single() {
                self.sliver_geometry = ctx.tree.layout_sliver_child(child_id, ctx.constraints);
            } else {
                self.sliver_geometry = SliverGeometry::default();
            }
        }

        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Only paint if not offstage
        if self.should_paint() {
            if let Some(child_id) = ctx.children.try_single() {
                if self.sliver_geometry.visible {
                    return ctx.tree.paint_child(child_id, ctx.offset);
                }
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sliver_offstage_new() {
        let offstage = RenderSliverOffstage::new(true);

        assert!(offstage.offstage);
    }

    #[test]
    fn test_render_sliver_offstage_new_visible() {
        let offstage = RenderSliverOffstage::new(false);

        assert!(!offstage.offstage);
    }

    #[test]
    fn test_set_offstage() {
        let mut offstage = RenderSliverOffstage::new(false);
        offstage.set_offstage(true);

        assert!(offstage.offstage);
    }

    #[test]
    fn test_should_paint() {
        let offstage_hidden = RenderSliverOffstage::new(true);
        let offstage_visible = RenderSliverOffstage::new(false);

        assert!(!offstage_hidden.should_paint());
        assert!(offstage_visible.should_paint());
    }

    #[test]
    fn test_should_hit_test() {
        let offstage_hidden = RenderSliverOffstage::new(true);
        let offstage_visible = RenderSliverOffstage::new(false);

        assert!(!offstage_hidden.should_hit_test());
        assert!(offstage_visible.should_hit_test());
    }

    #[test]
    fn test_default_is_visible() {
        let offstage = RenderSliverOffstage::default();

        assert!(!offstage.offstage);
    }

    #[test]
    fn test_arity_is_single_child() {
        let offstage = RenderSliverOffstage::new(true);
        assert_eq!(offstage.arity(), Arity::Exact(1));
    }
}
