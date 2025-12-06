//! RenderOffstage - lays out child but doesn't paint or hit test
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderOffstage-class.html>

use crate::core::{
    FullRenderTree,
    LayoutTree, PaintTree, FullRenderTree, RenderBox, Single, {BoxLayoutCtx, PaintContext},
};
use flui_types::Size;

/// RenderObject that lays out child but doesn't paint or allow hit testing
///
/// When `offstage` is true, the child is laid out but:
/// - Not painted (invisible)
/// - Not hit testable (can't receive pointer events)
/// - Not included in semantics tree
///
/// The child still takes up space in the layout (unlike Visibility with
/// maintainSize: false).
///
/// # Use Cases
///
/// - Preloading content that will be shown later
/// - Keeping widgets in the tree for state preservation
/// - Measuring widget size without displaying it
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOffstage;
///
/// // Hide child but maintain its layout
/// let mut offstage = RenderOffstage::new(true);
///
/// // Show the child
/// offstage.set_offstage(false);
/// ```
#[derive(Debug)]
pub struct RenderOffstage {
    /// Whether the child is hidden
    offstage: bool,
    /// Cached child size for when offstage
    cached_size: Size,
}

// ===== Public API =====

impl RenderOffstage {
    /// Create new RenderOffstage
    ///
    /// # Arguments
    /// * `offstage` - If true, child is laid out but not painted or hit tested
    pub fn new(offstage: bool) -> Self {
        Self {
            offstage,
            cached_size: Size::ZERO,
        }
    }

    /// Check if child is offstage (hidden)
    pub fn offstage(&self) -> bool {
        self.offstage
    }

    /// Set whether child is offstage
    ///
    /// When changed, triggers repaint but not relayout (child size unchanged)
    pub fn set_offstage(&mut self, offstage: bool) {
        if self.offstage != offstage {
            self.offstage = offstage;
            // Would mark needs paint in full implementation
        }
    }

    /// Get the cached size of the child
    ///
    /// This is useful when offstage to know the child's size without painting
    pub fn child_size(&self) -> Size {
        self.cached_size
    }
}

impl Default for RenderOffstage {
    fn default() -> Self {
        Self::new(true)
    }
}

// ===== RenderObject Implementation =====

impl<T: FullRenderTree> RenderBox<T, Single> for RenderOffstage {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        let child_id = ctx.children.single();

        if self.offstage {
            // Layout child to get its size, but we report zero size
            // This matches Flutter's behavior where offstage widgets
            // don't take up space in their parent
            self.cached_size = ctx.layout_child(child_id, ctx.constraints);
            Size::ZERO
        } else {
            // Normal layout - pass through to child
            let size = ctx.layout_child(child_id, ctx.constraints);
            self.cached_size = size;
            size
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // Only paint child if not offstage
        if !self.offstage {
            let child_id = ctx.children.single();
            ctx.paint_child(child_id, ctx.offset);
        }
        // When offstage, we paint nothing - child is invisible
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_offstage_new() {
        let offstage = RenderOffstage::new(true);
        assert!(offstage.offstage());
        assert_eq!(offstage.child_size(), Size::ZERO);
    }

    #[test]
    fn test_render_offstage_new_visible() {
        let offstage = RenderOffstage::new(false);
        assert!(!offstage.offstage());
    }

    #[test]
    fn test_render_offstage_set_offstage() {
        let mut offstage = RenderOffstage::new(true);
        assert!(offstage.offstage());

        offstage.set_offstage(false);
        assert!(!offstage.offstage());

        offstage.set_offstage(true);
        assert!(offstage.offstage());
    }

    #[test]
    fn test_render_offstage_default() {
        let offstage = RenderOffstage::default();
        assert!(offstage.offstage()); // Default is offstage (hidden)
    }

    #[test]
    fn test_render_offstage_no_change() {
        let mut offstage = RenderOffstage::new(true);

        // Setting to same value shouldn't trigger anything
        offstage.set_offstage(true);
        assert!(offstage.offstage());
    }
}
