//! RenderSliverOffstage - Conditionally hides sliver without removing from tree

use crate::core::{LayoutContext, LayoutTree, PaintContext, PaintTree, RenderSliverProxy, Single, SliverProtocol};
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
}

impl RenderSliverOffstage {
    /// Create new sliver offstage
    ///
    /// # Arguments
    /// * `offstage` - Whether to hide the child
    pub fn new(offstage: bool) -> Self {
        Self { offstage }
    }

    /// Set offstage state
    pub fn set_offstage(&mut self, offstage: bool) {
        self.offstage = offstage;
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

impl RenderSliverProxy for RenderSliverOffstage {
    // Layout: custom implementation to return zero geometry when offstage
    fn proxy_layout<T>(
        &mut self,
        mut ctx: LayoutContext<'_, T, Single, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        if self.offstage {
            // When offstage, report zero geometry
            SliverGeometry::default()
        } else {
            // Pass through to child when visible
            ctx.proxy()
        }
    }

    // Paint: custom implementation to skip painting when offstage
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // Only paint if not offstage
        if self.should_paint() {
            ctx.proxy();
        }
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

}
