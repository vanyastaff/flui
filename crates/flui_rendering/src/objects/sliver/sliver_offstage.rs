//! RenderSliverOffstage - Conditionally hides sliver content from painting

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::prelude::*;

/// RenderObject that conditionally hides a sliver without removing it from the tree
///
/// Unlike removing a widget from the tree, offstage keeps the child element alive
/// but skips painting. This is useful for:
/// - Keeping state alive while hiding content
/// - Implementing visibility toggles without rebuild overhead
/// - Preserving scroll position when toggling visibility
///
/// # Performance Note
///
/// While offstage widgets are not painted, they still participate in layout.
/// For large lists, consider using conditional building instead if layout cost
/// is significant.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverOffstage;
///
/// // Child is laid out but not painted
/// let offstage = RenderSliverOffstage::new(true);
///
/// // Child is both laid out and painted
/// let visible = RenderSliverOffstage::new(false);
/// ```
#[derive(Debug)]
pub struct RenderSliverOffstage {
    /// Whether the child is offstage (hidden)
    pub offstage: bool,

    // Layout cache
    child_size: Size,
}

impl RenderSliverOffstage {
    /// Create new sliver offstage
    ///
    /// # Arguments
    /// * `offstage` - True to hide child, false to show
    pub fn new(offstage: bool) -> Self {
        Self {
            offstage,
            child_size: Size::ZERO,
        }
    }

    /// Set whether child is offstage
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

impl Render for RenderSliverOffstage {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        // Offstage still participates in layout
        // This preserves scroll position and state
        // In real implementation, child would be laid out here
        self.child_size = Size::new(
            constraints.max_width,
            constraints.max_height,
        );

        self.child_size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // Skip painting if offstage
        if !self.should_paint() {
            return canvas;
        }

        // TODO: Paint child when visible

        canvas
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
    fn test_render_sliver_offstage_new_hidden() {
        let offstage = RenderSliverOffstage::new(true);

        assert!(offstage.offstage);
    }

    #[test]
    fn test_render_sliver_offstage_new_visible() {
        let offstage = RenderSliverOffstage::new(false);

        assert!(!offstage.offstage);
    }

    #[test]
    fn test_render_sliver_offstage_default() {
        let offstage = RenderSliverOffstage::default();

        assert!(!offstage.offstage); // Default is visible
    }

    #[test]
    fn test_set_offstage_hide() {
        let mut offstage = RenderSliverOffstage::new(false);
        offstage.set_offstage(true);

        assert!(offstage.offstage);
    }

    #[test]
    fn test_set_offstage_show() {
        let mut offstage = RenderSliverOffstage::new(true);
        offstage.set_offstage(false);

        assert!(!offstage.offstage);
    }

    #[test]
    fn test_should_paint_when_visible() {
        let offstage = RenderSliverOffstage::new(false);

        assert!(offstage.should_paint());
    }

    #[test]
    fn test_should_not_paint_when_offstage() {
        let offstage = RenderSliverOffstage::new(true);

        assert!(!offstage.should_paint());
    }

    #[test]
    fn test_should_hit_test_when_visible() {
        let offstage = RenderSliverOffstage::new(false);

        assert!(offstage.should_hit_test());
    }

    #[test]
    fn test_should_not_hit_test_when_offstage() {
        let offstage = RenderSliverOffstage::new(true);

        assert!(!offstage.should_hit_test());
    }

    #[test]
    fn test_toggle_offstage() {
        let mut offstage = RenderSliverOffstage::new(false);
        assert!(offstage.should_paint());

        offstage.set_offstage(true);
        assert!(!offstage.should_paint());

        offstage.set_offstage(false);
        assert!(offstage.should_paint());
    }

    #[test]
    fn test_paint_and_hit_test_synchronized() {
        let visible = RenderSliverOffstage::new(false);
        assert_eq!(visible.should_paint(), visible.should_hit_test());

        let hidden = RenderSliverOffstage::new(true);
        assert_eq!(hidden.should_paint(), hidden.should_hit_test());
    }

    #[test]
    fn test_arity_is_single_child() {
        let offstage = RenderSliverOffstage::new(true);
        assert_eq!(offstage.arity(), Arity::Exact(1));
    }
}
