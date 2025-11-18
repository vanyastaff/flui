//! Hit testing infrastructure
//!
//! Hit testing determines which UI elements are under a given point (cursor/touch).
//! This is the foundation for event routing.
//!
//! This implementation follows Flutter's hit testing architecture:
//! - Transform stack for coordinate space management
//! - Event propagation control (stop/continue)
//! - HitTestBehavior for controlling hit detection
//! - Dispatch order from leaf to root (most specific first)

use flui_types::{
    events::PointerEvent,
    geometry::{Matrix4, Offset, Rect},
};
use std::sync::Arc;

/// Element ID type (placeholder until we have proper element tree integration)
///
/// In a full implementation, this would be imported from flui_core.
/// For now, we use usize as a simple ID.
pub type ElementId = usize;

/// Event propagation control
///
/// Determines whether event dispatch should continue to the next handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPropagation {
    /// Continue dispatching to next handler
    Continue,
    /// Stop propagation (event handled)
    Stop,
}

/// Handler for pointer events with propagation control
///
/// Called when a hit-tested element receives a pointer event.
/// Returns `EventPropagation::Stop` to prevent further dispatch.
pub type PointerEventHandler = Arc<dyn Fn(&PointerEvent) -> EventPropagation + Send + Sync>;

/// Hit test behavior
///
/// Controls how hit testing is performed on an element.
/// Follows Flutter's HitTestBehavior pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Only register hit if a descendant is hit
    #[default]
    DeferToChild,
    /// Always register hit and block events below
    Opaque,
    /// Register hit but let events pass through
    Translucent,
}

/// Result of hit testing
///
/// Contains all UI elements that were "hit" by a point, ordered from
/// front to back (topmost element first).
///
/// Supports coordinate space transformations via a transform stack,
/// following Flutter's pattern of `pushTransform`/`popTransform`.
///
/// # Example
///
/// ```rust,ignore
/// let mut result = HitTestResult::new();
///
/// // Use transforms for nested coordinate spaces
/// result.push_offset(Offset::new(10.0, 20.0));
/// child_layer.hit_test(cursor_position, &mut result);
/// result.pop_transform();
///
/// // Dispatch event to all hit elements (leaf to root)
/// result.dispatch(&pointer_event);
/// ```
#[derive(Debug, Clone, Default)]
pub struct HitTestResult {
    /// Entries from front to back (topmost first)
    entries: Vec<HitTestEntry>,

    /// Transform stack for coordinate space management
    /// Each transform converts from parent to child coordinate space
    transforms: Vec<Matrix4>,
}

impl HitTestResult {
    /// Create a new empty hit test result
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            transforms: Vec::new(),
        }
    }

    /// Push a transformation matrix onto the transform stack
    ///
    /// This transform will be applied to all subsequent entries added.
    /// Must be matched with a `pop_transform()` call.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// result.push_transform(rotation_matrix);
    /// child.hit_test(position, result);
    /// result.pop_transform();
    /// ```
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.transforms.push(transform);
    }

    /// Push an offset translation onto the transform stack
    ///
    /// Convenience method for simple translations.
    /// Equivalent to `push_transform(Matrix4::translation(offset.dx, offset.dy, 0.0))`.
    pub fn push_offset(&mut self, offset: Offset) {
        self.transforms
            .push(Matrix4::translation(offset.dx, offset.dy, 0.0));
    }

    /// Pop the most recent transform from the stack
    ///
    /// Must be called once for each `push_transform()` or `push_offset()`.
    ///
    /// # Panics
    ///
    /// Panics if the transform stack is empty (unbalanced push/pop).
    pub fn pop_transform(&mut self) {
        self.transforms
            .pop()
            .expect("Unbalanced push/pop on HitTestResult transform stack");
    }

    /// Get the current composed transform (all transforms multiplied)
    ///
    /// Returns `None` if no transforms are active.
    fn current_transform(&self) -> Option<Matrix4> {
        if self.transforms.is_empty() {
            return None;
        }

        // Compose all transforms: child = T1 * T2 * T3 * ... * Tn * parent
        let mut result = Matrix4::identity();
        for transform in &self.transforms {
            result = *transform * result;
        }
        Some(result)
    }

    /// Add an entry to the result
    ///
    /// Entries should be added from back to front during tree traversal,
    /// but will be stored front to back for dispatch.
    ///
    /// Automatically captures the current transform from the transform stack.
    pub fn add(&mut self, entry: HitTestEntry) {
        // Capture current transform
        let mut entry = entry;
        entry.transform = self.current_transform();

        // Insert at front (reverse order from traversal)
        self.entries.insert(0, entry);
    }

    /// Get all entries
    pub fn entries(&self) -> &[HitTestEntry] {
        &self.entries
    }

    /// Check if any entries were found
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Dispatch event to all hit entries
    ///
    /// Calls handlers in order (front to back / leaf to root) until one stops propagation.
    /// Follows Flutter's event dispatch pattern.
    ///
    /// # Event Transformation
    ///
    /// If an entry has a transform, the event position is transformed to the entry's
    /// local coordinate space before dispatch.
    pub fn dispatch(&self, event: &PointerEvent) {
        for entry in &self.entries {
            if let Some(handler) = &entry.handler {
                // Transform event to local coordinate space if needed
                let local_event = if let Some(ref transform) = entry.transform {
                    // Try to invert transform (global -> local)
                    if let Some(inverse) = transform.try_inverse() {
                        transform_pointer_event(event, &inverse)
                    } else {
                        // Transform can't be inverted (degenerate), skip this entry
                        tracing::warn!(
                            element_id = entry.element_id,
                            "Failed to invert transform for event dispatch"
                        );
                        continue;
                    }
                } else {
                    event.clone()
                };

                // Dispatch and check if propagation should stop
                match handler(&local_event) {
                    EventPropagation::Stop => break,
                    EventPropagation::Continue => continue,
                }
            }
        }
    }
}

/// Single entry in a hit test result
///
/// Represents one UI element that was hit, with its local coordinates,
/// transform, and optional event handler.
///
/// Follows Flutter's HitTestEntry pattern with transform support.
#[derive(Clone)]
pub struct HitTestEntry {
    /// Element ID (for mouse tracking and region identification)
    pub element_id: ElementId,

    /// Local position (relative to this element's coordinate space)
    pub local_position: Offset,

    /// Bounds of this element (for debugging)
    pub bounds: Rect,

    /// Optional handler for pointer events with propagation control
    pub handler: Option<PointerEventHandler>,

    /// Transform from global to local coordinate space
    ///
    /// Captured automatically when the entry is added to HitTestResult.
    /// Used to transform events to local coordinates during dispatch.
    pub transform: Option<Matrix4>,
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("element_id", &self.element_id)
            .field("local_position", &self.local_position)
            .field("bounds", &self.bounds)
            .field("has_handler", &self.handler.is_some())
            .field("has_transform", &self.transform.is_some())
            .finish()
    }
}

impl HitTestEntry {
    /// Create a new hit test entry
    ///
    /// The transform will be captured automatically when added to HitTestResult.
    pub fn new(element_id: ElementId, local_position: Offset, bounds: Rect) -> Self {
        Self {
            element_id,
            local_position,
            bounds,
            handler: None,
            transform: None,
        }
    }

    /// Create entry with a handler
    ///
    /// The transform will be captured automatically when added to HitTestResult.
    pub fn with_handler(
        element_id: ElementId,
        local_position: Offset,
        bounds: Rect,
        handler: PointerEventHandler,
    ) -> Self {
        Self {
            element_id,
            local_position,
            bounds,
            handler: Some(handler),
            transform: None,
        }
    }
}

/// Transform a pointer event using the given transformation matrix
///
/// Transforms the event's position to a different coordinate space.
fn transform_pointer_event(event: &PointerEvent, transform: &Matrix4) -> PointerEvent {
    use flui_types::events::PointerEventData;

    // Helper to transform a position
    let transform_offset = |offset: Offset| -> Offset {
        let point = transform.transform_point(offset.dx, offset.dy);
        Offset::new(point.0, point.1)
    };

    match event {
        PointerEvent::Down(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Down(new_data)
        }
        PointerEvent::Up(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Up(new_data)
        }
        PointerEvent::Move(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Move(new_data)
        }
        PointerEvent::Hover(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Hover(new_data)
        }
        PointerEvent::Enter(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Enter(new_data)
        }
        PointerEvent::Exit(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Exit(new_data)
        }
        PointerEvent::Cancel(data) => {
            let mut new_data = PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Cancel(new_data)
        }
        PointerEvent::Scroll { device, position, scroll_delta } => PointerEvent::Scroll {
            device: *device,
            position: transform_offset(*position),
            scroll_delta: *scroll_delta, // Don't transform delta (it's a vector, not a point)
        },
        // Events without position data - return as-is
        other => other.clone(),
    }
}

/// Trait for objects that can be hit-tested
///
/// Implement this on your Layer or UI element type to enable hit testing.
/// Follows Flutter's RenderBox.hitTest pattern.
///
/// # Example
///
/// ```rust,ignore
/// impl HitTestable for MyLayer {
///     fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
///         // Check if position is within bounds
///         if !self.bounds.contains(position) {
///             return false;
///         }
///
///         // Hit test children first (with transforms if needed)
///         result.push_offset(self.child_offset);
///         let child_hit = self.child.hit_test(position, result);
///         result.pop_transform();
///
///         // Add our own entry if we want events
///         let entry = HitTestEntry::with_handler(
///             self.element_id,
///             position,
///             self.bounds,
///             self.event_handler.clone(),
///         );
///         result.add(entry);
///
///         true // We were hit
///     }
///
///     fn hit_test_behavior(&self) -> HitTestBehavior {
///         HitTestBehavior::Opaque // Block events below us
///     }
/// }
/// ```
pub trait HitTestable {
    /// Perform hit testing at the given position
    ///
    /// Returns `true` if this element (or a child) was hit.
    ///
    /// # Arguments
    ///
    /// * `position` - Point to test, in this element's coordinate space
    /// * `result` - Accumulator for hit test results
    ///
    /// # Implementation Guidelines
    ///
    /// 1. Check if `position` is within your bounds
    /// 2. Use `result.push_offset()`/`push_transform()` before testing children
    /// 3. Always call `result.pop_transform()` after testing children
    /// 4. Add your own entry to `result` based on `hit_test_behavior()`
    /// 5. Return `true` if hit (self or child), `false` otherwise
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool;

    /// Returns the hit test behavior for this element
    ///
    /// Controls whether this element registers as hit and blocks events below.
    /// Default is `DeferToChild`.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::DeferToChild
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_test_result_empty() {
        let result = HitTestResult::new();
        assert!(result.is_empty());
        assert_eq!(result.entries().len(), 0);
    }

    #[test]
    fn test_hit_test_result_add() {
        let mut result = HitTestResult::new();

        let entry1 = HitTestEntry::new(
            1,
            Offset::new(10.0, 10.0),
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        );
        result.add(entry1);

        assert!(!result.is_empty());
        assert_eq!(result.entries().len(), 1);
    }

    #[test]
    fn test_hit_test_entry_order() {
        let mut result = HitTestResult::new();

        // Add back to front (as tree traversal would)
        result.add(HitTestEntry::new(
            1,
            Offset::new(1.0, 1.0),
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        ));
        result.add(HitTestEntry::new(
            2,
            Offset::new(2.0, 2.0),
            Rect::from_xywh(0.0, 0.0, 20.0, 20.0),
        ));

        // Should be stored front to back
        let entries = result.entries();
        assert_eq!(entries[0].local_position.dx, 2.0); // Front
        assert_eq!(entries[1].local_position.dx, 1.0); // Back
    }

    #[test]
    fn test_dispatch_with_handler() {
        use std::sync::{Arc, Mutex};

        let mut result = HitTestResult::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        let handler = Arc::new(move |_event: &PointerEvent| {
            *called_clone.lock().unwrap() = true;
            EventPropagation::Continue
        });

        let entry = HitTestEntry::with_handler(
            1,
            Offset::new(10.0, 10.0),
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            handler,
        );
        result.add(entry);

        // Dispatch event
        let event = PointerEvent::Down(flui_types::events::PointerEventData::new(
            Offset::new(50.0, 50.0),
            flui_types::events::PointerDeviceKind::Mouse,
        ));
        result.dispatch(&event);

        // Handler should have been called
        assert!(*called.lock().unwrap());
    }

    #[test]
    fn test_event_propagation_stop() {
        use std::sync::{Arc, Mutex};

        let mut result = HitTestResult::new();
        let first_called = Arc::new(Mutex::new(false));
        let second_called = Arc::new(Mutex::new(false));

        let first_clone = first_called.clone();
        let second_clone = second_called.clone();

        // First handler stops propagation (added second, will be dispatched first)
        let handler1 = Arc::new(move |_event: &PointerEvent| {
            *first_clone.lock().unwrap() = true;
            EventPropagation::Stop
        });

        // Second handler should not be called (added first, will be dispatched second)
        let handler2 = Arc::new(move |_event: &PointerEvent| {
            *second_clone.lock().unwrap() = true;
            EventPropagation::Continue
        });

        // Add in reverse order (last added = first dispatched due to insert(0))
        result.add(HitTestEntry::with_handler(
            2,
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            handler2,
        ));
        result.add(HitTestEntry::with_handler(
            1,
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            handler1,
        ));

        let event = PointerEvent::Down(flui_types::events::PointerEventData::new(
            Offset::new(50.0, 50.0),
            flui_types::events::PointerDeviceKind::Mouse,
        ));
        result.dispatch(&event);

        // First handler called, second not called
        assert!(*first_called.lock().unwrap());
        assert!(!*second_called.lock().unwrap());
    }

    #[test]
    fn test_transform_stack() {
        let mut result = HitTestResult::new();

        // Push transform
        result.push_offset(Offset::new(10.0, 20.0));

        let entry = HitTestEntry::new(1, Offset::ZERO, Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
        result.add(entry);

        result.pop_transform();

        // Entry should have captured the transform
        assert!(result.entries()[0].transform.is_some());
    }

    #[test]
    fn test_transform_composition() {
        let mut result = HitTestResult::new();

        // Push multiple transforms
        result.push_offset(Offset::new(10.0, 20.0));
        result.push_offset(Offset::new(5.0, 5.0));

        // Should compose: total offset = (15.0, 25.0)
        let transform = result.current_transform().unwrap();
        let point = transform.transform_point(0.0, 0.0);

        assert!((point.0 - 15.0).abs() < 0.001);
        assert!((point.1 - 25.0).abs() < 0.001);

        result.pop_transform();
        result.pop_transform();
    }

    #[test]
    #[should_panic(expected = "Unbalanced push/pop")]
    fn test_unbalanced_pop_panics() {
        let mut result = HitTestResult::new();
        result.pop_transform(); // Should panic
    }

    #[test]
    fn test_hit_test_behavior() {
        struct TestElement;
        impl HitTestable for TestElement {
            fn hit_test(&self, _position: Offset, _result: &mut HitTestResult) -> bool {
                true
            }

            fn hit_test_behavior(&self) -> HitTestBehavior {
                HitTestBehavior::Opaque
            }
        }

        let element = TestElement;
        assert_eq!(element.hit_test_behavior(), HitTestBehavior::Opaque);
    }
}