//! RenderMouseRegion - tracks mouse enter/exit/hover events
//!
//! Used by MouseRegion widget to detect when the mouse enters, exits, or moves within a region.

use crate::render_object::RenderObject;
use flui_core::BoxConstraints;
use flui_types::events::{HitTestEntry, HitTestResult, PointerEvent, PointerEventHandler};
use flui_types::{Offset, Size};

/// Callbacks for mouse region events
#[derive(Clone)]
pub struct MouseRegionCallbacks {
    /// Called when mouse enters the region
    pub on_enter: Option<PointerEventHandler>,
    /// Called when mouse exits the region
    pub on_exit: Option<PointerEventHandler>,
    /// Called when mouse moves within the region (hover)
    pub on_hover: Option<PointerEventHandler>,
}

impl Default for MouseRegionCallbacks {
    fn default() -> Self {
        Self {
            on_enter: None,
            on_exit: None,
            on_hover: None,
        }
    }
}

impl std::fmt::Debug for MouseRegionCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseRegionCallbacks")
            .field("has_on_enter", &self.on_enter.is_some())
            .field("has_on_exit", &self.on_exit.is_some())
            .field("has_on_hover", &self.on_hover.is_some())
            .finish()
    }
}

/// RenderMouseRegion tracks mouse enter/exit/hover events
///
/// This render object registers callbacks for mouse events and participates in hit testing
/// to ensure it receives pointer events even when opaque.
///
/// # Layout Algorithm
///
/// Simply passes constraints to child and adopts child size (like RenderProxyBox).
///
/// # Hit Testing
///
/// Always participates in hit testing (even if transparent) to track mouse enter/exit.
/// Registers a handler that dispatches to the appropriate callback based on event type.
///
/// # Mouse Tracking
///
/// - **Enter**: Called when mouse first enters the region bounds
/// - **Exit**: Called when mouse leaves the region bounds
/// - **Hover**: Called when mouse moves within the region (Move events)
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::RenderMouseRegion;
/// use std::sync::Arc;
///
/// let callbacks = MouseRegionCallbacks {
///     on_enter: Some(Arc::new(|e| println!("Mouse entered!"))),
///     on_exit: Some(Arc::new(|e| println!("Mouse left!"))),
///     on_hover: Some(Arc::new(|e| println!("Mouse moved!"))),
/// };
///
/// let mut render = RenderMouseRegion::new(callbacks);
/// ```
pub struct RenderMouseRegion {
    /// Event callbacks
    callbacks: MouseRegionCallbacks,
    /// Child render object
    child: Option<Box<dyn RenderObject>>,
    /// Current size
    size: Size,
    /// Layout dirty flag
    needs_layout_flag: bool,
    /// Paint dirty flag
    needs_paint_flag: bool,
    /// Whether mouse is currently inside (for tracking enter/exit)
    is_mouse_inside: bool,
}

impl RenderMouseRegion {
    /// Creates a new RenderMouseRegion with callbacks
    ///
    /// # Parameters
    ///
    /// - `callbacks`: Mouse event callbacks (enter, exit, hover)
    pub fn new(callbacks: MouseRegionCallbacks) -> Self {
        Self {
            callbacks,
            child: None,
            size: Size::zero(),
            needs_layout_flag: true,
            needs_paint_flag: true,
            is_mouse_inside: false,
        }
    }

    /// Sets the child
    pub fn set_child(&mut self, child: Option<Box<dyn RenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Returns a reference to the child
    pub fn child(&self) -> Option<&dyn RenderObject> {
        self.child.as_deref()
    }

    /// Sets the callbacks
    pub fn set_callbacks(&mut self, callbacks: MouseRegionCallbacks) {
        self.callbacks = callbacks;
    }

    /// Returns a reference to the callbacks
    pub fn callbacks(&self) -> &MouseRegionCallbacks {
        &self.callbacks
    }

    /// Create a handler that dispatches to appropriate callback based on event type
    fn create_handler(&self) -> PointerEventHandler {
        let callbacks = self.callbacks.clone();
        std::sync::Arc::new(move |event: &PointerEvent| {
            match event {
                PointerEvent::Enter(_) => {
                    if let Some(handler) = &callbacks.on_enter {
                        handler(event);
                    }
                }
                PointerEvent::Exit(_) => {
                    if let Some(handler) = &callbacks.on_exit {
                        handler(event);
                    }
                }
                PointerEvent::Move(_) => {
                    // Move events within region are hover events
                    if let Some(handler) = &callbacks.on_hover {
                        handler(event);
                    }
                }
                _ => {
                    // Other events (Down, Up, Cancel) are not handled by MouseRegion
                    // They might be handled by nested GestureDetectors
                }
            }
        })
    }
}

impl RenderObject for RenderMouseRegion {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        if let Some(child) = &mut self.child {
            // Pass constraints through to child
            self.size = child.layout(constraints);
        } else {
            // Without child, use smallest size
            self.size = constraints.smallest();
        }

        self.needs_layout_flag = false;
        self.size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            // Simply paint child - mouse tracking doesn't affect rendering
            child.paint(painter, offset);
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        // Always respond to hit tests to track mouse enter/exit
        // Even if we're transparent, we need to know when mouse enters/exits
        true
    }

    fn hit_test_children(
        &self,
        result: &mut HitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child) = &self.child {
            child.hit_test(result, position)
        } else {
            false
        }
    }

    /// Override hit_test to add our handler
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check bounds
        if position.dx < 0.0
            || position.dx >= self.size().width
            || position.dy < 0.0
            || position.dy >= self.size().height
        {
            return false;
        }

        // Check children first (front to back)
        let hit_child = self.hit_test_children(result, position);

        // Then check self (always true to track mouse)
        let hit_self = self.hit_test_self(position);

        // Add to result if hit, INCLUDING our handler
        if hit_child || hit_self {
            result.add(HitTestEntry::with_handler(
                position,
                self.size(),
                self.create_handler(),
            ));
            return true;
        }

        false
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout_flag
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout_flag = true;
    }

    fn needs_paint(&self) -> bool {
        self.needs_paint_flag
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint_flag = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

impl std::fmt::Debug for RenderMouseRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderMouseRegion")
            .field("callbacks", &self.callbacks)
            .field("has_child", &self.child.is_some())
            .field("size", &self.size)
            .field("is_mouse_inside", &self.is_mouse_inside)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderConstrainedBox;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_mouse_region_new() {
        let callbacks = MouseRegionCallbacks::default();
        let render = RenderMouseRegion::new(callbacks);

        assert!(render.child.is_none());
        assert_eq!(render.size, Size::zero());
        assert!(!render.is_mouse_inside);
    }

    #[test]
    fn test_mouse_region_layout() {
        let callbacks = MouseRegionCallbacks::default();
        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout should pass through child size
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        let size = render.layout(constraints);

        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn test_mouse_region_hit_test() {
        let callbacks = MouseRegionCallbacks::default();
        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should succeed within bounds
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_mouse_region_hit_test_out_of_bounds() {
        let callbacks = MouseRegionCallbacks::default();
        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout first
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should fail outside bounds
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(150.0, 25.0));

        assert!(!hit);
        assert!(result.is_empty());
    }

    #[test]
    fn test_mouse_region_enter_callback() {
        let entered = Arc::new(Mutex::new(false));
        let entered_clone = entered.clone();

        let callbacks = MouseRegionCallbacks {
            on_enter: Some(Arc::new(move |_event| {
                *entered_clone.lock().unwrap() = true;
            })),
            on_exit: None,
            on_hover: None,
        };

        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Perform hit test to register handler
        let mut result = HitTestResult::new();
        render.hit_test(&mut result, Offset::new(50.0, 25.0));

        // Simulate Enter event
        use flui_types::events::{PointerDeviceKind, PointerEventData};
        let event_data = PointerEventData::new(
            Offset::new(50.0, 25.0),
            PointerDeviceKind::Mouse,
        );
        let event = PointerEvent::Enter(event_data);
        result.dispatch(&event);

        // Verify callback was called
        assert!(*entered.lock().unwrap());
    }

    #[test]
    fn test_mouse_region_exit_callback() {
        let exited = Arc::new(Mutex::new(false));
        let exited_clone = exited.clone();

        let callbacks = MouseRegionCallbacks {
            on_enter: None,
            on_exit: Some(Arc::new(move |_event| {
                *exited_clone.lock().unwrap() = true;
            })),
            on_hover: None,
        };

        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Perform hit test
        let mut result = HitTestResult::new();
        render.hit_test(&mut result, Offset::new(50.0, 25.0));

        // Simulate Exit event
        use flui_types::events::{PointerDeviceKind, PointerEventData};
        let event_data = PointerEventData::new(
            Offset::new(50.0, 25.0),
            PointerDeviceKind::Mouse,
        );
        let event = PointerEvent::Exit(event_data);
        result.dispatch(&event);

        // Verify callback was called
        assert!(*exited.lock().unwrap());
    }

    #[test]
    fn test_mouse_region_hover_callback() {
        let hover_count = Arc::new(Mutex::new(0));
        let hover_count_clone = hover_count.clone();

        let callbacks = MouseRegionCallbacks {
            on_enter: None,
            on_exit: None,
            on_hover: Some(Arc::new(move |_event| {
                *hover_count_clone.lock().unwrap() += 1;
            })),
        };

        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Perform hit test
        let mut result = HitTestResult::new();
        render.hit_test(&mut result, Offset::new(50.0, 25.0));

        // Simulate Move (hover) events
        use flui_types::events::{PointerDeviceKind, PointerEventData};

        let event_data1 = PointerEventData::new(
            Offset::new(50.0, 25.0),
            PointerDeviceKind::Mouse,
        );
        result.dispatch(&PointerEvent::Move(event_data1));

        let event_data2 = PointerEventData::new(
            Offset::new(51.0, 26.0),
            PointerDeviceKind::Mouse,
        );
        result.dispatch(&PointerEvent::Move(event_data2));

        // Verify callback was called twice
        assert_eq!(*hover_count.lock().unwrap(), 2);
    }

    #[test]
    fn test_mouse_region_set_callbacks() {
        let callbacks1 = MouseRegionCallbacks::default();
        let mut render = RenderMouseRegion::new(callbacks1);

        let entered = Arc::new(Mutex::new(false));
        let entered_clone = entered.clone();

        let callbacks2 = MouseRegionCallbacks {
            on_enter: Some(Arc::new(move |_event| {
                *entered_clone.lock().unwrap() = true;
            })),
            on_exit: None,
            on_hover: None,
        };

        render.set_callbacks(callbacks2);
        assert!(render.callbacks().on_enter.is_some());
    }

    #[test]
    fn test_mouse_region_always_hit_tests() {
        // Even without callbacks, MouseRegion should participate in hit testing
        let callbacks = MouseRegionCallbacks::default();
        let mut render = RenderMouseRegion::new(callbacks);

        // Add a child
        let child = Box::new(RenderConstrainedBox::new(
            BoxConstraints::tight(Size::new(100.0, 50.0)),
        ));
        render.set_child(Some(child));

        // Layout
        let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
        render.layout(constraints);

        // Hit test should succeed (to track mouse)
        let mut result = HitTestResult::new();
        let hit = render.hit_test(&mut result, Offset::new(50.0, 25.0));

        assert!(hit);
        assert!(!result.is_empty());
    }
}
