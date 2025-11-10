//! RenderVisibility - flexible visibility control
//!
//! More advanced than RenderOffstage, supports maintaining size, state, and other properties.

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::{layer::pool, BoxedLayer};
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

impl Render for RenderVisibility {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Layout child if visible OR if we need to maintain state/size
        let should_layout = self.visible || self.maintain_state || self.maintain_size;

        if should_layout {
            let child_size = tree.layout_child(child_id, constraints);

            // Return child size if visible or maintaining size
            if self.visible || self.maintain_size {
                if child_size != Size::ZERO {
                    child_size
                } else {
                    constraints.smallest()
                }
            } else {
                // Not visible, not maintaining size: report zero
                Size::ZERO
            }
        } else {
            // Child completely removed: report zero size
            Size::ZERO
        }
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Only paint if visible
        if self.visible {
            tree.paint_child(child_id, offset)
        } else {
            // Return empty container layer when not visible
            Box::new(pool::acquire_container())
        }
    }

    // Note: In a full implementation, you would also override:
    // - hit_test() - to control interactivity based on maintain_interactivity
    // - visit_children_for_semantics() - to control semantics based on maintain_semantics
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Default - update if needed
    }
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
