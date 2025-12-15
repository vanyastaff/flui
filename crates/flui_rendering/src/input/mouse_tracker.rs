//! Mouse tracking for hover events.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use flui_types::{Matrix4, Offset};
use parking_lot::RwLock;

use super::mouse_cursor::{MouseCursor, MouseCursorSession};
use crate::hit_testing::HitTestResult;

/// Signature for hit testing at a given position in a specific view.
///
/// This is used by the MouseTracker to fetch annotations for mouse positions.
pub type MouseTrackerHitTest = Arc<dyn Fn(Offset, i32) -> HitTestResult + Send + Sync>;

/// A trait for render objects that want to receive mouse hover events.
///
/// Implement this trait on render objects that need to respond to mouse
/// enter, hover, and exit events.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `MouseTrackerAnnotation` mixin.
pub trait MouseTrackerAnnotation: Debug + Send + Sync {
    /// Called when the mouse enters this annotation.
    fn on_enter(&self, event: &PointerEnterEvent) {
        let _ = event;
    }

    /// Called when the mouse hovers over this annotation.
    fn on_hover(&self, event: &PointerHoverEvent) {
        let _ = event;
    }

    /// Called when the mouse exits this annotation.
    fn on_exit(&self, event: &PointerExitEvent) {
        let _ = event;
    }

    /// The mouse cursor to use when hovering over this annotation.
    fn cursor(&self) -> MouseCursor {
        MouseCursor::Defer
    }

    /// Whether this annotation is still valid for mouse tracking.
    ///
    /// Return `false` if this annotation has been disposed and should
    /// no longer receive events.
    fn valid_for_mouse_tracker(&self) -> bool {
        true
    }
}

/// Event data for mouse enter events.
#[derive(Debug, Clone)]
pub struct PointerEnterEvent {
    /// The position of the pointer.
    pub position: Offset,

    /// The pointer device ID.
    pub device: i32,

    /// The view ID.
    pub view_id: i32,

    /// Transform from global to local coordinates.
    pub transform: Option<Matrix4>,
}

impl PointerEnterEvent {
    /// Creates a new enter event.
    pub fn new(position: Offset, device: i32, view_id: i32) -> Self {
        Self {
            position,
            device,
            view_id,
            transform: None,
        }
    }

    /// Returns the local position using the transform.
    pub fn local_position(&self) -> Offset {
        self.transform
            .as_ref()
            .and_then(|t| t.try_inverse())
            .map(|inv| {
                let (x, y) = inv.transform_point(self.position.dx, self.position.dy);
                Offset::new(x, y)
            })
            .unwrap_or(self.position)
    }

    /// Creates a transformed copy of this event.
    pub fn transformed(&self, transform: Option<Matrix4>) -> Self {
        Self {
            transform,
            ..self.clone()
        }
    }
}

/// Event data for mouse hover events.
#[derive(Debug, Clone)]
pub struct PointerHoverEvent {
    /// The position of the pointer.
    pub position: Offset,

    /// The pointer device ID.
    pub device: i32,

    /// The view ID.
    pub view_id: i32,

    /// The change in position since the last event.
    pub delta: Offset,

    /// Transform from global to local coordinates.
    pub transform: Option<Matrix4>,
}

impl PointerHoverEvent {
    /// Creates a new hover event.
    pub fn new(position: Offset, device: i32, view_id: i32, delta: Offset) -> Self {
        Self {
            position,
            device,
            view_id,
            delta,
            transform: None,
        }
    }

    /// Returns the local position using the transform.
    pub fn local_position(&self) -> Offset {
        self.transform
            .as_ref()
            .and_then(|t| t.try_inverse())
            .map(|inv| {
                let (x, y) = inv.transform_point(self.position.dx, self.position.dy);
                Offset::new(x, y)
            })
            .unwrap_or(self.position)
    }

    /// Creates a transformed copy of this event.
    pub fn transformed(&self, transform: Option<Matrix4>) -> Self {
        Self {
            transform,
            ..self.clone()
        }
    }
}

/// Event data for mouse exit events.
#[derive(Debug, Clone)]
pub struct PointerExitEvent {
    /// The position of the pointer.
    pub position: Offset,

    /// The pointer device ID.
    pub device: i32,

    /// The view ID.
    pub view_id: i32,

    /// Transform from global to local coordinates.
    pub transform: Option<Matrix4>,
}

impl PointerExitEvent {
    /// Creates a new exit event.
    pub fn new(position: Offset, device: i32, view_id: i32) -> Self {
        Self {
            position,
            device,
            view_id,
            transform: None,
        }
    }

    /// Returns the local position using the transform.
    pub fn local_position(&self) -> Offset {
        self.transform
            .as_ref()
            .and_then(|t| t.try_inverse())
            .map(|inv| {
                let (x, y) = inv.transform_point(self.position.dx, self.position.dy);
                Offset::new(x, y)
            })
            .unwrap_or(self.position)
    }

    /// Creates a transformed copy of this event.
    pub fn transformed(&self, transform: Option<Matrix4>) -> Self {
        Self {
            transform,
            ..self.clone()
        }
    }
}

/// State of a connected mouse device.
#[derive(Debug)]
struct MouseState {
    /// The latest event from this device.
    latest_position: Offset,

    /// The latest view ID.
    view_id: i32,

    /// The annotations that currently contain this device.
    annotations: HashMap<usize, Matrix4>,

    /// The cursor session for this device.
    cursor_session: MouseCursorSession,
}

impl MouseState {
    fn new(device: i32, position: Offset, view_id: i32) -> Self {
        Self {
            latest_position: position,
            view_id,
            annotations: HashMap::new(),
            cursor_session: MouseCursorSession::new(device),
        }
    }
}

/// Tracks mouse devices and dispatches hover events.
///
/// The MouseTracker manages the relationship between mouse devices and
/// annotations, triggering enter/hover/exit events as needed.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `MouseTracker` class.
pub struct MouseTracker {
    /// Hit test function to find annotations at a position.
    hit_test_in_view: MouseTrackerHitTest,

    /// State of connected mouse devices, keyed by device ID.
    mouse_states: RwLock<HashMap<i32, MouseState>>,

    /// Registered annotations, keyed by a unique ID.
    annotations: RwLock<HashMap<usize, Arc<dyn MouseTrackerAnnotation>>>,

    /// Counter for generating annotation IDs.
    next_annotation_id: RwLock<usize>,

    /// Listeners for mouse connection changes.
    listeners: RwLock<Vec<Arc<dyn Fn(bool) + Send + Sync>>>,
}

impl Debug for MouseTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseTracker")
            .field("mouse_count", &self.mouse_states.read().len())
            .field("annotation_count", &self.annotations.read().len())
            .finish()
    }
}

impl MouseTracker {
    /// Creates a new mouse tracker with the given hit test function.
    pub fn new(hit_test_in_view: MouseTrackerHitTest) -> Self {
        Self {
            hit_test_in_view,
            mouse_states: RwLock::new(HashMap::new()),
            annotations: RwLock::new(HashMap::new()),
            next_annotation_id: RwLock::new(1),
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Returns whether at least one mouse is connected.
    pub fn mouse_is_connected(&self) -> bool {
        !self.mouse_states.read().is_empty()
    }

    /// Registers an annotation to receive mouse events.
    ///
    /// Returns an ID that can be used to unregister the annotation.
    pub fn register_annotation(&self, annotation: Arc<dyn MouseTrackerAnnotation>) -> usize {
        let mut id_guard = self.next_annotation_id.write();
        let id = *id_guard;
        *id_guard += 1;
        drop(id_guard);

        self.annotations.write().insert(id, annotation);
        id
    }

    /// Unregisters an annotation.
    pub fn unregister_annotation(&self, id: usize) {
        self.annotations.write().remove(&id);
    }

    /// Adds a listener for mouse connection changes.
    pub fn add_listener(&self, listener: Arc<dyn Fn(bool) + Send + Sync>) {
        self.listeners.write().push(listener);
    }

    /// Updates the tracker with a pointer event.
    ///
    /// Call this for all pointer events to keep the tracker up to date.
    pub fn update_with_event(&self, position: Offset, device: i32, view_id: i32, is_down: bool) {
        let was_connected = self.mouse_is_connected();

        if is_down {
            // Mouse down - possibly add new device
            let mut states = self.mouse_states.write();
            states
                .entry(device)
                .or_insert_with(|| MouseState::new(device, position, view_id));
        }

        // Update position for existing device
        {
            let mut states = self.mouse_states.write();
            if let Some(state) = states.get_mut(&device) {
                let old_position = state.latest_position;
                state.latest_position = position;
                state.view_id = view_id;

                // Calculate delta for hover events
                let _delta =
                    Offset::new(position.dx - old_position.dx, position.dy - old_position.dy);
            }
        }

        // Update hover state
        self.update_device(device, position, view_id);

        // Notify listeners if connection state changed
        let is_connected = self.mouse_is_connected();
        if was_connected != is_connected {
            let listeners = self.listeners.read();
            for listener in listeners.iter() {
                listener(is_connected);
            }
        }
    }

    /// Removes a mouse device.
    pub fn remove_device(&self, device: i32) {
        let was_connected = self.mouse_is_connected();

        // Send exit events to all annotations
        if let Some(state) = self.mouse_states.write().remove(&device) {
            let annotations = self.annotations.read();
            let exit_event = PointerExitEvent::new(state.latest_position, device, state.view_id);

            for (id, transform) in &state.annotations {
                if let Some(annotation) = annotations.get(id) {
                    if annotation.valid_for_mouse_tracker() {
                        annotation.on_exit(&exit_event.transformed(Some(*transform)));
                    }
                }
            }
        }

        // Notify listeners if connection state changed
        let is_connected = self.mouse_is_connected();
        if was_connected != is_connected {
            let listeners = self.listeners.read();
            for listener in listeners.iter() {
                listener(is_connected);
            }
        }
    }

    /// Updates all devices (typically called after a frame).
    ///
    /// This should be called after each frame to update hover states
    /// in case annotations have moved.
    pub fn update_all_devices(&self) {
        let devices: Vec<(i32, Offset, i32)> = {
            let states = self.mouse_states.read();
            states
                .iter()
                .map(|(device, state)| (*device, state.latest_position, state.view_id))
                .collect()
        };

        for (device, position, view_id) in devices {
            self.update_device(device, position, view_id);
        }
    }

    /// Updates hover state for a single device.
    fn update_device(&self, device: i32, position: Offset, view_id: i32) {
        // Find annotations at the current position
        let hit_result = (self.hit_test_in_view)(position, view_id);
        let _new_annotations = self.annotations_from_hit_result(&hit_result);

        // For now, just a placeholder - in a full implementation we would:
        // 1. Compare old annotations with new annotations
        // 2. Send exit events to annotations no longer hovered
        // 3. Send enter events to newly hovered annotations
        // 4. Send hover events to annotations still hovered
        // 5. Update cursor based on top annotation

        let _ = (device, position, view_id);
    }

    /// Extracts annotations from a hit test result.
    fn annotations_from_hit_result(&self, _result: &HitTestResult) -> HashMap<usize, Matrix4> {
        // In a full implementation, this would extract MouseTrackerAnnotation
        // instances from the hit test path.
        HashMap::new()
    }

    /// Returns the active cursor for a device (for debugging).
    pub fn debug_device_active_cursor(&self, device: i32) -> Option<MouseCursor> {
        self.mouse_states
            .read()
            .get(&device)
            .map(|state| state.cursor_session.cursor().clone())
    }

    /// Disposes of the mouse tracker.
    pub fn dispose(&self) {
        self.mouse_states.write().clear();
        self.annotations.write().clear();
        self.listeners.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tracker() -> MouseTracker {
        let hit_test: MouseTrackerHitTest = Arc::new(|_position, _view_id| HitTestResult::new());
        MouseTracker::new(hit_test)
    }

    #[derive(Debug)]
    struct TestAnnotation {
        cursor: MouseCursor,
    }

    impl MouseTrackerAnnotation for TestAnnotation {
        fn cursor(&self) -> MouseCursor {
            self.cursor.clone()
        }
    }

    #[test]
    fn test_mouse_tracker_new() {
        let tracker = create_test_tracker();
        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn test_mouse_tracker_register_annotation() {
        let tracker = create_test_tracker();
        let annotation = Arc::new(TestAnnotation {
            cursor: MouseCursor::CLICK,
        });

        let id = tracker.register_annotation(annotation);
        assert!(id > 0);
        assert_eq!(tracker.annotations.read().len(), 1);
    }

    #[test]
    fn test_mouse_tracker_unregister_annotation() {
        let tracker = create_test_tracker();
        let annotation = Arc::new(TestAnnotation {
            cursor: MouseCursor::CLICK,
        });

        let id = tracker.register_annotation(annotation);
        assert_eq!(tracker.annotations.read().len(), 1);

        tracker.unregister_annotation(id);
        assert_eq!(tracker.annotations.read().len(), 0);
    }

    #[test]
    fn test_mouse_tracker_update_with_event() {
        let tracker = create_test_tracker();

        // Add mouse down event
        tracker.update_with_event(Offset::new(100.0, 200.0), 0, 0, true);
        assert!(tracker.mouse_is_connected());

        // Update position
        tracker.update_with_event(Offset::new(150.0, 250.0), 0, 0, false);
        assert!(tracker.mouse_is_connected());
    }

    #[test]
    fn test_mouse_tracker_remove_device() {
        let tracker = create_test_tracker();

        tracker.update_with_event(Offset::new(100.0, 200.0), 0, 0, true);
        assert!(tracker.mouse_is_connected());

        tracker.remove_device(0);
        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn test_mouse_tracker_multiple_devices() {
        let tracker = create_test_tracker();

        tracker.update_with_event(Offset::new(100.0, 200.0), 0, 0, true);
        tracker.update_with_event(Offset::new(200.0, 300.0), 1, 0, true);
        assert_eq!(tracker.mouse_states.read().len(), 2);

        tracker.remove_device(0);
        assert_eq!(tracker.mouse_states.read().len(), 1);
        assert!(tracker.mouse_is_connected());

        tracker.remove_device(1);
        assert!(!tracker.mouse_is_connected());
    }

    #[test]
    fn test_pointer_enter_event() {
        let event = PointerEnterEvent::new(Offset::new(10.0, 20.0), 0, 0);
        assert_eq!(event.position.dx, 10.0);
        assert_eq!(event.position.dy, 20.0);
        assert_eq!(event.device, 0);
    }

    #[test]
    fn test_pointer_hover_event() {
        let event = PointerHoverEvent::new(Offset::new(10.0, 20.0), 0, 0, Offset::new(5.0, 5.0));
        assert_eq!(event.position.dx, 10.0);
        assert_eq!(event.delta.dx, 5.0);
    }

    #[test]
    fn test_pointer_exit_event() {
        let event = PointerExitEvent::new(Offset::new(10.0, 20.0), 0, 0);
        assert_eq!(event.position.dx, 10.0);
        assert_eq!(event.device, 0);
    }

    #[test]
    fn test_event_local_position_no_transform() {
        let event = PointerEnterEvent::new(Offset::new(100.0, 200.0), 0, 0);
        let local = event.local_position();
        assert_eq!(local.dx, 100.0);
        assert_eq!(local.dy, 200.0);
    }

    #[test]
    fn test_debug_device_active_cursor() {
        let tracker = create_test_tracker();

        tracker.update_with_event(Offset::new(100.0, 200.0), 0, 0, true);
        let cursor = tracker.debug_device_active_cursor(0);
        assert!(cursor.is_some());
        assert_eq!(cursor.unwrap(), MouseCursor::BASIC);
    }

    #[test]
    fn test_dispose() {
        let tracker = create_test_tracker();

        tracker.update_with_event(Offset::new(100.0, 200.0), 0, 0, true);
        let annotation = Arc::new(TestAnnotation {
            cursor: MouseCursor::CLICK,
        });
        tracker.register_annotation(annotation);

        tracker.dispose();
        assert!(!tracker.mouse_is_connected());
        assert_eq!(tracker.annotations.read().len(), 0);
    }
}
