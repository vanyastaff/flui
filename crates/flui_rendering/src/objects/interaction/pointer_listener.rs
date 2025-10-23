//! RenderPointerListener - handles pointer events

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Pointer event callbacks
#[derive(Clone)]
pub struct PointerCallbacks {
    // For now, we use Option<fn()> placeholders
    // In a real implementation, these would be proper callback types

    /// Called when pointer is pressed down
    pub on_pointer_down: Option<fn()>,

    /// Called when pointer is released
    pub on_pointer_up: Option<fn()>,

    /// Called when pointer moves
    pub on_pointer_move: Option<fn()>,
}

impl std::fmt::Debug for PointerCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerCallbacks")
            .field("on_pointer_down", &self.on_pointer_down.is_some())
            .field("on_pointer_up", &self.on_pointer_up.is_some())
            .field("on_pointer_move", &self.on_pointer_move.is_some())
            .finish()
    }
}

/// Data for RenderPointerListener
#[derive(Debug, Clone)]
pub struct PointerListenerData {
    /// Event callbacks
    pub callbacks: PointerCallbacks,
}

impl PointerListenerData {
    /// Create new pointer listener data
    pub fn new(callbacks: PointerCallbacks) -> Self {
        Self { callbacks }
    }
}

/// RenderObject that listens for pointer events
///
/// This widget detects pointer events (mouse clicks, touches) and
/// calls the appropriate callbacks. It does not affect layout or painting.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::interaction::{PointerListenerData, PointerCallbacks}};
///
/// let callbacks = PointerCallbacks {
///     on_pointer_down: Some(|| println!("Pointer down")),
///     on_pointer_up: None,
///     on_pointer_move: None,
/// };
/// let mut listener = SingleRenderBox::new(PointerListenerData::new(callbacks));
/// ```
pub type RenderPointerListener = SingleRenderBox<PointerListenerData>;

// ===== Public API =====

impl RenderPointerListener {
    /// Get the callbacks
    pub fn callbacks(&self) -> &PointerCallbacks {
        &self.data().callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: PointerCallbacks) {
        self.data_mut().callbacks = callbacks;
        // No need to mark needs_layout or needs_paint - callbacks don't affect rendering
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderPointerListener {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        // Get children from ElementTree via RenderContext
        let children_ids = ctx.children();

        // Layout child with same constraints
        let size = if let Some(&child_id) = children_ids.first() {
            ctx.layout_child_cached(child_id, constraints, None)
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
    }

    fn paint(&self, state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        // Simply paint child - event handling happens elsewhere
        let children_ids = ctx.children();
        if let Some(&child_id) = children_ids.first() {
            ctx.paint_child(child_id, painter, offset);
        }

        // TODO: In a real implementation, we would:
        // 1. Register hit test area for pointer events
        // 2. Handle pointer events in hit testing phase
        // 3. Call appropriate callbacks
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_pointer_listener_new() {
        let callbacks = PointerCallbacks {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
        };
        let listener = SingleRenderBox::new(PointerListenerData::new(callbacks));
        assert!(listener.callbacks().on_pointer_down.is_none());
    }

    #[test]
    fn test_render_pointer_listener_set_callbacks() {
        fn dummy_callback() {}

        let callbacks1 = PointerCallbacks {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
        };
        let mut listener = SingleRenderBox::new(PointerListenerData::new(callbacks1));

        let callbacks2 = PointerCallbacks {
            on_pointer_down: Some(dummy_callback),
            on_pointer_up: None,
            on_pointer_move: None,
        };
        listener.set_callbacks(callbacks2);
        assert!(listener.callbacks().on_pointer_down.is_some());
    }

    #[test]
    fn test_render_pointer_listener_layout_no_child() {
        use flui_core::testing::mock_render_context;

        let callbacks = PointerCallbacks {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
        };
        let listener = SingleRenderBox::new(PointerListenerData::new(callbacks));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let (_tree, ctx) = mock_render_context();
        let size = listener.layout(constraints, &ctx);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_pointer_callbacks_debug() {
        fn dummy_callback() {}

        let callbacks = PointerCallbacks {
            on_pointer_down: Some(dummy_callback),
            on_pointer_up: None,
            on_pointer_move: None,
        };
        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("PointerCallbacks"));
    }
}
