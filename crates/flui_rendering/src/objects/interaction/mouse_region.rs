//! RenderMouseRegion - handles mouse hover events
//!
//! Flutter reference: https://api.flutter.dev/flutter/rendering/RenderMouseRegion-class.html

use crate::core::{
    FullRenderTree,
    FullRenderTree, RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::Size;

/// Mouse cursor type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseCursor {
    /// Default system cursor
    #[default]
    Default,
    /// No cursor
    None,
    /// Text selection cursor
    Text,
    /// Pointer/hand cursor (for clickable elements)
    Pointer,
    /// Grab cursor
    Grab,
    /// Grabbing cursor (actively dragging)
    Grabbing,
    /// Crosshair cursor
    Crosshair,
    /// Move cursor
    Move,
    /// Resize north cursor
    ResizeNorth,
    /// Resize south cursor
    ResizeSouth,
    /// Resize east cursor
    ResizeEast,
    /// Resize west cursor
    ResizeWest,
    /// Resize north-east cursor
    ResizeNorthEast,
    /// Resize north-west cursor
    ResizeNorthWest,
    /// Resize south-east cursor
    ResizeSouthEast,
    /// Resize south-west cursor
    ResizeSouthWest,
    /// Not allowed cursor
    NotAllowed,
    /// Wait/loading cursor
    Wait,
    /// Help cursor
    Help,
}

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
    /// Mouse cursor to display when hovering
    pub cursor: MouseCursor,
    /// Whether this region prevents detection by regions behind it
    ///
    /// When true, this region blocks hit testing for RenderMouseRegion
    /// objects visually behind it. When false, events can pass through.
    pub opaque: bool,
}

impl RenderMouseRegion {
    /// Create new RenderMouseRegion
    pub fn new(callbacks: MouseCallbacks) -> Self {
        Self {
            callbacks,
            is_hovering: false,
            cursor: MouseCursor::Default,
            opaque: true,
        }
    }

    /// Create new RenderMouseRegion with cursor
    pub fn with_cursor(callbacks: MouseCallbacks, cursor: MouseCursor) -> Self {
        Self {
            callbacks,
            is_hovering: false,
            cursor,
            opaque: true,
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

    /// Get the cursor type
    pub fn cursor(&self) -> MouseCursor {
        self.cursor
    }

    /// Set the cursor type
    pub fn set_cursor(&mut self, cursor: MouseCursor) {
        self.cursor = cursor;
    }

    /// Check if this region is opaque
    pub fn opaque(&self) -> bool {
        self.opaque
    }

    /// Set whether this region is opaque
    pub fn set_opaque(&mut self, opaque: bool) {
        self.opaque = opaque;
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderMouseRegion {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Simply paint child - hover handling is done via hit testing
        ctx.paint_child(child_id, ctx.offset);

        // Note: Mouse event handling requires integration with the event system:
        // 1. Hit testing provides the region bounds for hover detection
        // 2. The event dispatcher tracks mouse enter/exit events
        // 3. Callbacks are invoked when hover state changes
        // This render object provides the structure; event handling is done by the framework
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
