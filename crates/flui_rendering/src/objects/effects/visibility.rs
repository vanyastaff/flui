//! RenderVisibility - flexible visibility control
//!
//! More advanced than RenderOffstage, supports maintaining size, state, and other properties.

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::Size;

/// RenderObject that controls visibility with fine-grained options
///
/// Unlike RenderOffstage, this supports maintaining size, state, animations, etc.
///
/// # Visibility Modes
///
/// ```text
/// visible=true:  Child is fully visible and interactive
///
/// visible=false combinations:
/// - maintain_size=false (default): Child removed, no space taken
/// - maintain_size=true: Child hidden but space reserved
/// - maintain_state=true: Child hidden but state kept alive
/// - maintain_animation=true: Animations continue running
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderVisibility;
///
/// // Hide but maintain size (useful for fade animations)
/// let visibility = RenderVisibility::new(false, true, true, false, false, false);
/// ```
#[derive(Debug)]
pub struct RenderVisibility {
    /// Whether the child is visible
    pub visible: bool,

    /// Whether to maintain the space occupied by the child when not visible
    pub maintain_size: bool,

    /// Whether to maintain the state of the child when not visible
    pub maintain_state: bool,

    /// Whether to maintain animations when not visible
    pub maintain_animation: bool,

    /// Whether to maintain interactivity when not visible
    pub maintain_interactivity: bool,

    /// Whether to maintain semantics when not visible
    pub maintain_semantics: bool,
}

impl RenderVisibility {
    /// Create new RenderVisibility
    pub fn new(
        visible: bool,
        maintain_size: bool,
        maintain_state: bool,
        maintain_animation: bool,
        maintain_interactivity: bool,
        maintain_semantics: bool,
    ) -> Self {
        Self {
            visible,
            maintain_size,
            maintain_state,
            maintain_animation,
            maintain_interactivity,
            maintain_semantics,
        }
    }

    /// Set whether child is visible
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Set whether to maintain size
    pub fn set_maintain_size(&mut self, maintain_size: bool) {
        self.maintain_size = maintain_size;
    }

    /// Set whether to maintain state
    pub fn set_maintain_state(&mut self, maintain_state: bool) {
        self.maintain_state = maintain_state;
    }
}

impl Default for RenderVisibility {
    fn default() -> Self {
        Self::new(true, false, false, false, false, false)
    }
}

impl RenderObject for RenderVisibility {}

impl RenderBox<Single> for RenderVisibility {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let child_id = *ctx.children.single();

        // Layout child if visible OR if we need to maintain state/size
        let should_layout = self.visible || self.maintain_state || self.maintain_size;

        if should_layout {
            let child_size = ctx.layout_child(child_id, ctx.constraints)?;

            // Return child size if visible or maintaining size
            if self.visible || self.maintain_size {
                if child_size != Size::ZERO {
                    Ok(child_size)
                } else {
                    Ok(ctx.constraints.smallest())
                }
            } else {
                // Not visible, not maintaining size: report zero
                Ok(Size::ZERO)
            }
        } else {
            // Child completely removed: report zero size
            Ok(Size::ZERO)
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Only paint if visible
        if self.visible {
            let child_id = *ctx.children.single();
            ctx.paint_child(child_id, ctx.offset);
        }
        // When not visible, don't paint anything
    }

    // Note: In a full implementation, you would also override:
    // - hit_test() - to control interactivity based on maintain_interactivity
    // - visit_children_for_semantics() - to control semantics based on maintain_semantics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_visibility_new() {
        let visibility = RenderVisibility::new(true, false, false, false, false, false);
        assert!(visibility.visible);
        assert!(!visibility.maintain_size);
        assert!(!visibility.maintain_state);
    }

    #[test]
    fn test_render_visibility_default() {
        let visibility = RenderVisibility::default();
        assert!(visibility.visible);
        assert!(!visibility.maintain_size);
    }

    #[test]
    fn test_render_visibility_set_visible() {
        let mut visibility = RenderVisibility::default();
        visibility.set_visible(false);
        assert!(!visibility.visible);
    }

    #[test]
    fn test_render_visibility_maintain_size() {
        let visibility = RenderVisibility::new(false, true, true, false, false, false);
        assert!(!visibility.visible);
        assert!(visibility.maintain_size);
        assert!(visibility.maintain_state);
    }
}
