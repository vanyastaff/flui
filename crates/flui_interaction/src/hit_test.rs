//! Hit testing infrastructure
//!
//! Hit testing determines which UI elements are under a given point (cursor/touch).
//! This is the foundation for event routing.

use flui_types::{
    events::PointerEvent,
    geometry::{Offset, Rect},
};
use std::sync::Arc;

/// Handler for pointer events
///
/// Called when a hit-tested element receives a pointer event.
pub type PointerEventHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// Result of hit testing
///
/// Contains all UI elements that were "hit" by a point, ordered from
/// front to back (topmost element first).
///
/// # Example
///
/// ```rust,ignore
/// let mut result = HitTestResult::new();
/// root_layer.hit_test(cursor_position, &mut result);
///
/// // Dispatch event to all hit elements
/// result.dispatch(&pointer_event);
/// ```
#[derive(Debug, Clone, Default)]
pub struct HitTestResult {
    /// Entries from front to back (topmost first)
    entries: Vec<HitTestEntry>,
}

impl HitTestResult {
    /// Create a new empty hit test result
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add an entry to the result
    ///
    /// Entries should be added from back to front during tree traversal,
    /// but will be stored front to back for dispatch.
    pub fn add(&mut self, entry: HitTestEntry) {
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
    /// Calls handlers in order (front to back) until one handles the event.
    pub fn dispatch(&self, event: &PointerEvent) {
        for entry in &self.entries {
            if let Some(handler) = &entry.handler {
                handler(event);
                // TODO: Add event propagation control (stopPropagation)
            }
        }
    }
}

/// Single entry in a hit test result
///
/// Represents one UI element that was hit, with its local coordinates
/// and optional event handler.
#[derive(Clone)]
pub struct HitTestEntry {
    /// Local position (relative to this element's coordinate space)
    pub local_position: Offset,

    /// Bounds of this element (for debugging)
    pub bounds: Rect,

    /// Optional handler for pointer events
    pub handler: Option<PointerEventHandler>,
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("local_position", &self.local_position)
            .field("bounds", &self.bounds)
            .field("has_handler", &self.handler.is_some())
            .finish()
    }
}

impl HitTestEntry {
    /// Create a new hit test entry
    pub fn new(local_position: Offset, bounds: Rect) -> Self {
        Self {
            local_position,
            bounds,
            handler: None,
        }
    }

    /// Create entry with a handler
    pub fn with_handler(
        local_position: Offset,
        bounds: Rect,
        handler: PointerEventHandler,
    ) -> Self {
        Self {
            local_position,
            bounds,
            handler: Some(handler),
        }
    }
}

/// Trait for objects that can be hit-tested
///
/// Implement this on your Layer or UI element type to enable hit testing.
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
///         // Add entry with handler
///         let entry = HitTestEntry::with_handler(
///             position - self.offset,
///             self.bounds,
///             self.event_handler.clone(),
///         );
///         result.add(entry);
///
///         true
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
    /// 2. If hit, call `hit_test` on children (back to front)
    /// 3. Add your own entry to `result` if you want events
    /// 4. Return `true` if hit (self or child), `false` otherwise
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool;
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
            Offset::new(1.0, 1.0),
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        ));
        result.add(HitTestEntry::new(
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
        });

        let entry = HitTestEntry::with_handler(
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
}