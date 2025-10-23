//! RenderMouseRegion - handles mouse hover events

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

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

/// Data for RenderMouseRegion
#[derive(Debug, Clone)]
pub struct MouseRegionData {
    /// Event callbacks
    pub callbacks: MouseCallbacks,
    /// Whether the mouse is currently hovering
    pub is_hovering: bool,
}

impl MouseRegionData {
    /// Create new mouse region data
    pub fn new(callbacks: MouseCallbacks) -> Self {
        Self {
            callbacks,
            is_hovering: false,
        }
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
/// use flui_rendering::{SingleRenderBox, objects::interaction::{MouseRegionData, MouseCallbacks}};
///
/// let callbacks = MouseCallbacks {
///     on_enter: Some(|| println!("Mouse entered")),
///     on_exit: Some(|| println!("Mouse exited")),
///     on_hover: None,
/// };
/// let mut region = SingleRenderBox::new(MouseRegionData::new(callbacks));
/// ```
pub type RenderMouseRegion = SingleRenderBox<MouseRegionData>;

// ===== Public API =====

impl RenderMouseRegion {
    /// Get the callbacks
    pub fn callbacks(&self) -> &MouseCallbacks {
        &self.data().callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: MouseCallbacks) {
        self.data_mut().callbacks = callbacks;
        // No need to mark needs_layout or needs_paint - callbacks don't affect rendering
    }

    /// Check if mouse is currently hovering
    pub fn is_hovering(&self) -> bool {
        self.data().is_hovering
    }

    /// Set hover state (called by hit testing system)
    pub fn set_hovering(&mut self, hovering: bool) {
        self.data_mut().is_hovering = hovering;
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderMouseRegion {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            child.layout(constraints)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Simply paint child - hover handling happens elsewhere
        if let Some(child) = self.child() {
            child.paint(painter, offset);
        }

        // TODO: In a real implementation, we would:
        // 1. Register hit test area for hover detection
        // 2. Track mouse enter/exit events
        // 3. Call appropriate callbacks when hover state changes
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
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
        let region = SingleRenderBox::new(MouseRegionData::new(callbacks));
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
        let mut region = SingleRenderBox::new(MouseRegionData::new(callbacks1));

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
        let mut region = SingleRenderBox::new(MouseRegionData::new(callbacks));

        assert!(!region.is_hovering());

        region.set_hovering(true);
        assert!(region.is_hovering());

        region.set_hovering(false);
        assert!(!region.is_hovering());
    }

    #[test]
    fn test_render_mouse_region_layout_no_child() {
        let callbacks = MouseCallbacks {
            on_enter: None,
            on_exit: None,
            on_hover: None,
        };
        let mut region = SingleRenderBox::new(MouseRegionData::new(callbacks));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = region.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
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
