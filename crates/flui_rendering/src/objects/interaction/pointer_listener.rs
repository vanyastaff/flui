//! RenderPointerListener - handles pointer events
//!
//! This RenderObject wraps a child and listens for pointer events,
//! calling the appropriate callbacks when events occur.

use crate::core::{
    FullRenderTree,
    HitTestTree, FullRenderTree, RenderBox, Single, {BoxProtocol, HitTestContext, LayoutContext, PaintContext},
};
use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::events::{PointerEvent, PointerEventHandler};
use flui_types::{Offset, Size};
use std::sync::Arc;

/// Pointer event callbacks
///
/// These callbacks are called when pointer events occur within the widget's bounds.
#[derive(Clone)]
pub struct PointerCallbacks {
    /// Called when pointer is pressed down
    pub on_pointer_down: Option<PointerEventHandler>,

    /// Called when pointer is released
    pub on_pointer_up: Option<PointerEventHandler>,

    /// Called when pointer moves
    pub on_pointer_move: Option<PointerEventHandler>,

    /// Called when pointer is cancelled
    pub on_pointer_cancel: Option<PointerEventHandler>,
}

impl PointerCallbacks {
    /// Create new empty callbacks
    pub fn new() -> Self {
        Self {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
            on_pointer_cancel: None,
        }
    }

    /// Set on_pointer_down callback
    pub fn with_on_pointer_down<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_down = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_up callback
    pub fn with_on_pointer_up<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_up = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_move callback
    pub fn with_on_pointer_move<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_move = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_cancel callback
    pub fn with_on_pointer_cancel<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_cancel = Some(Arc::new(callback));
        self
    }
}

impl Default for PointerCallbacks {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PointerCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerCallbacks")
            .field("on_pointer_down", &self.on_pointer_down.is_some())
            .field("on_pointer_up", &self.on_pointer_up.is_some())
            .field("on_pointer_move", &self.on_pointer_move.is_some())
            .field("on_pointer_cancel", &self.on_pointer_cancel.is_some())
            .finish()
    }
}

/// RenderObject that listens for pointer events
///
/// This widget detects pointer events (mouse clicks, touches) and
/// calls the appropriate callbacks. It wraps a child and doesn't affect
/// layout, but creates a PointerListenerLayer for hit testing.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderPointerListener, PointerCallbacks};
/// use std::sync::Arc;
///
/// let callbacks = PointerCallbacks::new()
///     .with_on_pointer_down(|event| println!("Pointer down: {:?}", event.position()));
///
/// let mut listener = RenderPointerListener::new(callbacks);
/// ```
#[derive(Debug)]
pub struct RenderPointerListener {
    /// Event callbacks
    pub callbacks: PointerCallbacks,

    /// Cached size from last layout
    size: Size,
}

impl RenderPointerListener {
    /// Create new RenderPointerListener
    pub fn new(callbacks: PointerCallbacks) -> Self {
        Self {
            callbacks,
            size: Size::ZERO,
        }
    }

    /// Get the callbacks
    pub fn callbacks(&self) -> &PointerCallbacks {
        &self.callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: PointerCallbacks) {
        self.callbacks = callbacks;
        // No need to mark needs_layout or needs_paint - callbacks don't affect rendering
    }

    /// Create the unified event handler from individual callbacks
    #[allow(dead_code)]
    fn create_handler(&self) -> PointerEventHandler {
        let callbacks = self.callbacks.clone();
        Arc::new(move |event: &PointerEvent| match event {
            PointerEvent::Down(_) => {
                if let Some(callback) = &callbacks.on_pointer_down {
                    callback(event);
                }
            }
            PointerEvent::Up(_) => {
                if let Some(callback) = &callbacks.on_pointer_up {
                    callback(event);
                }
            }
            PointerEvent::Move(_) => {
                if let Some(callback) = &callbacks.on_pointer_move {
                    callback(event);
                }
            }
            PointerEvent::Cancel(_) => {
                if let Some(callback) = &callbacks.on_pointer_cancel {
                    callback(event);
                }
            }
            _ => {}
        })
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderPointerListener {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Layout child with same constraints
        let size = ctx.layout_child(child_id, ctx.constraints);

        // Cache size for use in paint
        self.size = size;

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // Register hit region for pointer event handling
        // This connects the GestureDetector callbacks to the hit test system
        let bounds =
            flui_types::Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        // Create unified handler from our callbacks
        let callbacks = self.callbacks.clone();
        let handler: flui_painting::HitRegionHandler =
            std::sync::Arc::new(move |event| match event {
                flui_types::events::PointerEvent::Down(_) => {
                    if let Some(callback) = &callbacks.on_pointer_down {
                        callback(event);
                    }
                }
                flui_types::events::PointerEvent::Up(_) => {
                    if let Some(callback) = &callbacks.on_pointer_up {
                        callback(event);
                    }
                }
                flui_types::events::PointerEvent::Move(_) => {
                    if let Some(callback) = &callbacks.on_pointer_move {
                        callback(event);
                    }
                }
                flui_types::events::PointerEvent::Cancel(_) => {
                    if let Some(callback) = &callbacks.on_pointer_cancel {
                        callback(event);
                    }
                }
                _ => {}
            });

        // Add hit region to canvas
        ctx.canvas()
            .add_hit_region(flui_painting::HitRegion::new(bounds, handler));

        tracing::trace!(
            bounds = ?bounds,
            has_down = self.callbacks.on_pointer_down.is_some(),
            has_up = self.callbacks.on_pointer_up.is_some(),
            "RenderPointerListener: registered hit region"
        );

        // Paint child
        ctx.paint_child(child_id, offset);
    }

    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // Test children first (following Flutter's convention)
        let hit_children = self.hit_test_children(ctx, result);

        // Always add self to result if within bounds, regardless of children hit
        // This ensures we receive pointer events even if child is transparent
        if hit_children || self.hit_test_self(ctx.position, ctx.size()) {
            ctx.add_to_result(result);
            return true;
        }

        false
    }

    fn hit_test_self(&self, position: Offset, size: Size) -> bool {
        // Hit if position is within bounds
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }

    fn hit_test_children<T>(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // Test single child using the tree's hit_test_child method
        let child_id = ctx.children.single();
        ctx.tree()
            .hit_test_child(ElementId::new(child_id.get()), ctx.position, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_pointer_listener_new() {
        let callbacks = PointerCallbacks::new();
        let listener = RenderPointerListener::new(callbacks);
        assert!(listener.callbacks().on_pointer_down.is_none());
        assert!(listener.callbacks().on_pointer_up.is_none());
        assert!(listener.callbacks().on_pointer_move.is_none());
        assert!(listener.callbacks().on_pointer_cancel.is_none());
    }

    #[test]
    fn test_render_pointer_listener_with_callbacks() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_up(|_| {});

        let listener = RenderPointerListener::new(callbacks);
        assert!(listener.callbacks().on_pointer_down.is_some());
        assert!(listener.callbacks().on_pointer_up.is_some());
        assert!(listener.callbacks().on_pointer_move.is_none());
    }

    #[test]
    fn test_render_pointer_listener_set_callbacks() {
        let callbacks1 = PointerCallbacks::new();
        let mut listener = RenderPointerListener::new(callbacks1);

        let callbacks2 = PointerCallbacks::new().with_on_pointer_down(|_| {});
        listener.set_callbacks(callbacks2);
        assert!(listener.callbacks().on_pointer_down.is_some());
    }

    #[test]
    fn test_pointer_callbacks_debug() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_move(|_| {});

        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("PointerCallbacks"));
        assert!(debug_str.contains("on_pointer_down"));
    }

    #[test]
    fn test_pointer_callbacks_builder() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_up(|_| {})
            .with_on_pointer_move(|_| {})
            .with_on_pointer_cancel(|_| {});

        assert!(callbacks.on_pointer_down.is_some());
        assert!(callbacks.on_pointer_up.is_some());
        assert!(callbacks.on_pointer_move.is_some());
        assert!(callbacks.on_pointer_cancel.is_some());
    }
}
