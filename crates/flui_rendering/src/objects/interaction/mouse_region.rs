//! RenderMouseRegion - handles mouse hover events

use flui_core::render::{
    {BoxProtocol, LayoutContext, PaintContext},
    RenderBox,
    Single,
};
use flui_types::Size;

/// Mouse hover event callbacks
#[derive(Clone)]
pub struct MouseCallbacks {
    // For now, we use Option<fn()> placeholders
    // In a real implementation, these would be proper callback types
    /// Called when mouse enters the region
    pub on_enter: Option<fn()>,

    /// Called when mouse exits the region
    pub on_exit: Option<fn()>,

    /// Called when mouse hovers over the region
    pub on_hover: Option<fn()>,
}

impl std::fmt::Debug for MouseCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseCallbacks")
            .field("on_enter", &self.on_enter.is_some())
            .field("on_exit", &self.on_exit.is_some())
            .field("on_hover", &self.on_hover.is_some())
            .finish()
    }
}

/// RenderObject that tracks mouse hover state
///
/// This widget detects when the mouse enters, hovers over, or exits its bounds.
/// It does not affect layout or painting, only event handling.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderMouseRegion, MouseCallbacks};
///
/// let callbacks = MouseCallbacks {
///     on_enter: Some(|| println!("Mouse entered")),
///     on_exit: Some(|| println!("Mouse exited")),
///     on_hover: None,
/// };
/// let mut region = RenderMouseRegion::new(callbacks);
/// ```
#[derive(Debug)]
pub struct RenderMouseRegion {
    /// Event callbacks
    pub callbacks: MouseCallbacks,
    /// Whether the mouse is currently hovering
    pub is_hovering: bool,
}

impl RenderMouseRegion {
    /// Create new RenderMouseRegion
    pub fn new(callbacks: MouseCallbacks) -> Self {
        Self {
            callbacks,
            is_hovering: false,
        }
    }

    /// Get the callbacks
    pub fn callbacks(&self) -> &MouseCallbacks {
        &self.callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: MouseCallbacks) {
        self.callbacks = callbacks;
        // No need to mark needs_layout or needs_paint - callbacks don't affect rendering
    }

    /// Check if mouse is currently hovering
    pub fn is_hovering(&self) -> bool {
        self.is_hovering
    }

    /// Set hover state (called by hit testing system)
    pub fn set_hovering(&mut self, hovering: bool) {
        self.is_hovering = hovering;
    }
}

impl RenderBox<Single> for RenderMouseRegion {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        let child_id = ctx.children.single();

        // Simply paint child - hover handling happens elsewhere
        ctx.paint_child(child_id, ctx.offset);

        // TODO: In a real implementation, we would:
        // 1. Register hit test area for hover detection
        // 2. Track mouse enter/exit events
        // 3. Call appropriate callbacks when hover state changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_mouse_region_new() {
        let callbacks = MouseCallbacks {
            on_enter: None,
            on_exit: None,
            on_hover: None,
        };
        let region = RenderMouseRegion::new(callbacks);
        assert!(!region.is_hovering());
        assert!(region.callbacks().on_enter.is_none());
    }

    #[test]
    fn test_render_mouse_region_set_callbacks() {
        fn dummy_callback() {}

        let callbacks1 = MouseCallbacks {
            on_enter: None,
            on_exit: None,
            on_hover: None,
        };
        let mut region = RenderMouseRegion::new(callbacks1);

        let callbacks2 = MouseCallbacks {
            on_enter: Some(dummy_callback),
            on_exit: None,
            on_hover: None,
        };
        region.set_callbacks(callbacks2);
        assert!(region.callbacks().on_enter.is_some());
    }

    #[test]
    fn test_render_mouse_region_hovering() {
        let callbacks = MouseCallbacks {
            on_enter: None,
            on_exit: None,
            on_hover: None,
        };
        let mut region = RenderMouseRegion::new(callbacks);

        assert!(!region.is_hovering());

        region.set_hovering(true);
        assert!(region.is_hovering());

        region.set_hovering(false);
        assert!(!region.is_hovering());
    }

    #[test]
    fn test_mouse_callbacks_debug() {
        fn dummy_callback() {}

        let callbacks = MouseCallbacks {
            on_enter: Some(dummy_callback),
            on_exit: None,
            on_hover: None,
        };
        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("MouseCallbacks"));
    }
}
