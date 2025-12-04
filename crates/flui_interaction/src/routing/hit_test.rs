//! Hit testing infrastructure
//!
//! Hit testing determines which UI elements are under a given point (cursor/touch).
//! This is the foundation for event routing.
//!
//! # Architecture
//!
//! This implementation follows Flutter's hit testing architecture with Rust idioms:
//!
//! - **Transform stack**: Coordinate space management with RAII guards
//! - **Event propagation**: Stop/continue control
//! - **HitTestBehavior**: Controls hit detection semantics
//! - **Dispatch order**: Leaf to root (most specific first)
//!
//! # Type System Features
//!
//! - **Sealed traits**: `HitTestable` cannot be implemented outside this crate
//! - **Newtype pattern**: Type-safe element IDs
//! - **Extension traits**: Convenience methods for pointer events
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::hit_test::*;
//!
//! let mut result = HitTestResult::new();
//!
//! // Use transform guard for automatic cleanup
//! {
//!     let _guard = result.push_offset(Offset::new(10.0, 20.0));
//!     child_layer.hit_test(position, &mut result);
//! } // Transform automatically popped
//!
//! // Dispatch to all hit elements
//! result.dispatch(&pointer_event);
//! ```
//!
//! Flutter references:
//! - HitTestTarget: https://api.flutter.dev/flutter/gestures/HitTestTarget-class.html
//! - HitTestResult: https://api.flutter.dev/flutter/rendering/HitTestResult-class.html
//! - HitTestBehavior: https://api.flutter.dev/flutter/rendering/HitTestBehavior.html

use flui_types::{
    events::{MouseCursor, PointerEvent, ScrollEventData},
    geometry::{Matrix4, Offset, Rect},
};
use std::sync::Arc;

// Re-export ElementId from flui-foundation
pub use flui_foundation::ElementId;

// ============================================================================
// EventPropagation enum
// ============================================================================

/// Event propagation control.
///
/// Determines whether event dispatch should continue to the next handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventPropagation {
    /// Continue dispatching to next handler.
    #[default]
    Continue,
    /// Stop propagation (event handled).
    Stop,
}

impl EventPropagation {
    /// Returns `true` if propagation should continue.
    #[inline]
    pub const fn should_continue(self) -> bool {
        matches!(self, Self::Continue)
    }

    /// Returns `true` if propagation should stop.
    #[inline]
    pub const fn should_stop(self) -> bool {
        matches!(self, Self::Stop)
    }
}

// ============================================================================
// PointerEventHandler type alias
// ============================================================================

/// Handler for pointer events with propagation control.
///
/// Called when a hit-tested element receives a pointer event.
/// Returns `EventPropagation::Stop` to prevent further dispatch.
pub type PointerEventHandler = Arc<dyn Fn(&PointerEvent) -> EventPropagation + Send + Sync>;

/// Handler for scroll events with propagation control.
///
/// Called when a hit-tested element receives a scroll event.
/// Returns `EventPropagation::Stop` to prevent bubbling to parent elements.
///
/// Scroll events bubble from innermost (first hit) to outermost (last hit)
/// until a handler returns `Stop`.
pub type ScrollEventHandler = Arc<dyn Fn(&ScrollEventData) -> EventPropagation + Send + Sync>;

// ============================================================================
// HitTestBehavior enum
// ============================================================================

/// Hit test behavior.
///
/// Controls how hit testing is performed on an element.
/// Follows Flutter's HitTestBehavior pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitTestBehavior {
    /// Only register hit if a descendant is hit.
    #[default]
    DeferToChild,
    /// Always register hit and block events below.
    Opaque,
    /// Register hit but let events pass through.
    Translucent,
}

impl HitTestBehavior {
    /// Returns `true` if this behavior should register a hit even when
    /// no descendants are hit.
    #[inline]
    pub const fn registers_self(self) -> bool {
        matches!(self, Self::Opaque | Self::Translucent)
    }

    /// Returns `true` if this behavior blocks events from reaching
    /// elements below.
    #[inline]
    pub const fn blocks_below(self) -> bool {
        matches!(self, Self::Opaque)
    }
}

// ============================================================================
// TransformGuard for RAII transform management
// ============================================================================

/// Depth marker for transform stack management.
///
/// Represents the transform stack depth before a push operation.
/// Use with `HitTestResult::pop_to_depth()` to restore the stack.
///
/// # Example
///
/// ```rust,ignore
/// let mut result = HitTestResult::new();
/// let depth = result.push_offset(Offset::new(10.0, 20.0));
/// child.hit_test(position, &mut result);
/// result.pop_to_depth(depth);
/// ```
#[must_use = "TransformGuard depth must be used to pop transforms with pop_to_depth()"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformGuard {
    /// The transform stack depth after the push.
    depth: usize,
}

impl TransformGuard {
    /// Returns the depth to restore to when popping.
    #[inline]
    pub fn depth(self) -> usize {
        self.depth
    }
}

// ============================================================================
// HitTestResult
// ============================================================================

/// Result of hit testing.
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
/// // Use RAII guard for automatic transform cleanup
/// {
///     let _guard = result.push_offset(Offset::new(10.0, 20.0));
///     child_layer.hit_test(cursor_position, &mut result);
/// }
///
/// // Dispatch event to all hit elements (leaf to root)
/// result.dispatch(&pointer_event);
/// ```
#[derive(Debug, Clone, Default)]
pub struct HitTestResult {
    /// Entries from front to back (topmost first).
    entries: Vec<HitTestEntry>,

    /// Transform stack for coordinate space management.
    /// Each transform converts from parent to child coordinate space.
    transforms: Vec<Matrix4>,
}

impl HitTestResult {
    /// Creates a new empty hit test result.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a hit test result with pre-allocated capacity.
    ///
    /// Use this when you know approximately how many entries to expect.
    #[inline]
    pub fn with_capacity(entry_capacity: usize, transform_capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(entry_capacity),
            transforms: Vec::with_capacity(transform_capacity),
        }
    }

    /// Push a transformation matrix onto the transform stack.
    ///
    /// Returns a depth marker to use with `pop_to_depth()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let depth = result.push_transform(rotation_matrix);
    /// child.hit_test(position, result);
    /// result.pop_to_depth(depth);
    /// ```
    #[must_use = "Use pop_to_depth() with the returned guard to restore the transform stack"]
    pub fn push_transform(&mut self, transform: Matrix4) -> TransformGuard {
        self.transforms.push(transform);
        TransformGuard {
            depth: self.transforms.len(),
        }
    }

    /// Push an offset translation onto the transform stack.
    ///
    /// Returns a depth marker to use with `pop_to_depth()`.
    /// Convenience method for simple translations.
    #[must_use = "Use pop_to_depth() with the returned guard to restore the transform stack"]
    pub fn push_offset(&mut self, offset: Offset) -> TransformGuard {
        self.push_transform(Matrix4::translation(offset.dx, offset.dy, 0.0))
    }

    /// Pop transforms until the stack reaches the given depth.
    ///
    /// Use this with the `TransformGuard` returned from `push_transform()` or `push_offset()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let depth = result.push_offset(Offset::new(10.0, 20.0));
    /// child.hit_test(position, &mut result);
    /// result.pop_to_depth(depth);
    /// ```
    pub fn pop_to_depth(&mut self, guard: TransformGuard) {
        // Pop transforms until we reach the depth before the push
        while self.transforms.len() >= guard.depth && !self.transforms.is_empty() {
            self.transforms.pop();
        }
    }

    /// Returns the current transform stack depth.
    ///
    /// Useful for debugging or manual stack management.
    #[inline]
    pub fn transform_depth(&self) -> usize {
        self.transforms.len()
    }

    /// Pop the most recent transform from the stack.
    ///
    /// **Prefer using `push_transform()` which returns a guard for automatic cleanup.**
    ///
    /// # Panics
    ///
    /// Panics if the transform stack is empty (unbalanced push/pop).
    #[deprecated(
        since = "0.2.0",
        note = "Use push_transform() which returns a guard for automatic cleanup"
    )]
    pub fn pop_transform(&mut self) {
        self.transforms
            .pop()
            .expect("Unbalanced push/pop on HitTestResult transform stack");
    }

    /// Get the current composed transform (all transforms multiplied).
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

    /// Add an entry to the result.
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

    /// Get all entries.
    #[inline]
    pub fn entries(&self) -> &[HitTestEntry] {
        &self.entries
    }

    /// Check if any entries were found.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Clears all entries and transforms for reuse.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.transforms.clear();
    }

    /// Dispatch event to all hit entries.
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
                            element_id = entry.element_id.get(),
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

    /// Returns an iterator over entries that have handlers.
    pub fn entries_with_handlers(&self) -> impl Iterator<Item = &HitTestEntry> {
        self.entries.iter().filter(|e| e.handler.is_some())
    }

    /// Dispatch a scroll event to all entries with scroll handlers.
    ///
    /// Scroll events bubble from innermost (first entry) to outermost (last entry)
    /// until a handler returns `EventPropagation::Stop`.
    ///
    /// If an entry has a transform, the event position is transformed to the entry's
    /// local coordinate space before dispatch.
    ///
    /// Returns `true` if the event was handled (propagation stopped).
    pub fn dispatch_scroll(&self, event: &ScrollEventData) -> bool {
        for entry in &self.entries {
            if let Some(handler) = &entry.scroll_handler {
                // Transform event to local coordinate space if needed
                let local_event = if let Some(ref transform) = entry.transform {
                    // Try to invert transform (global -> local)
                    if let Some(inverse) = transform.try_inverse() {
                        transform_scroll_event(event, &inverse)
                    } else {
                        // Transform can't be inverted (degenerate), skip this entry
                        tracing::warn!(
                            element_id = entry.element_id.get(),
                            "Failed to invert transform for scroll event dispatch"
                        );
                        continue;
                    }
                } else {
                    event.clone()
                };

                // Dispatch and check if propagation should stop
                match handler(&local_event) {
                    EventPropagation::Stop => {
                        tracing::trace!(
                            element_id = entry.element_id.get(),
                            "Scroll event handled, stopping propagation"
                        );
                        return true;
                    }
                    EventPropagation::Continue => continue,
                }
            }
        }
        false
    }

    /// Returns an iterator over entries that have scroll handlers.
    pub fn entries_with_scroll_handlers(&self) -> impl Iterator<Item = &HitTestEntry> {
        self.entries.iter().filter(|e| e.scroll_handler.is_some())
    }

    /// Resolves the active mouse cursor from hit test entries.
    ///
    /// Iterates through entries (front to back) and returns the first
    /// non-`Defer` cursor found. If all cursors are `Defer` or no entries
    /// exist, returns `MouseCursor::BASIC` (the default arrow cursor).
    ///
    /// This follows Flutter's cursor resolution pattern where the front-most
    /// (topmost in z-order) element with a non-defer cursor wins.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let hit_result = perform_hit_test(cursor_position);
    /// let cursor = hit_result.resolve_cursor();
    /// window.set_cursor(cursor);
    /// ```
    pub fn resolve_cursor(&self) -> MouseCursor {
        for entry in &self.entries {
            if !entry.cursor.is_defer() {
                return entry.cursor;
            }
        }
        // Default to basic arrow if no cursor specified
        MouseCursor::BASIC
    }

    /// Returns an iterator over entries that have non-defer cursors.
    pub fn entries_with_cursors(&self) -> impl Iterator<Item = &HitTestEntry> {
        self.entries.iter().filter(|e| e.has_cursor())
    }
}

// ============================================================================
// HitTestEntry
// ============================================================================

/// Single entry in a hit test result.
///
/// Represents one UI element that was hit, with its local coordinates,
/// transform, and optional event handler.
///
/// Follows Flutter's HitTestEntry pattern with transform support.
#[derive(Clone)]
pub struct HitTestEntry {
    /// Element ID (for mouse tracking and region identification).
    pub element_id: ElementId,

    /// Local position (relative to this element's coordinate space).
    pub local_position: Offset,

    /// Bounds of this element (for debugging).
    pub bounds: Rect,

    /// Optional handler for pointer events with propagation control.
    pub handler: Option<PointerEventHandler>,

    /// Optional handler for scroll events with propagation control.
    ///
    /// Scroll events bubble from innermost to outermost element.
    pub scroll_handler: Option<ScrollEventHandler>,

    /// Transform from global to local coordinate space.
    ///
    /// Captured automatically when the entry is added to HitTestResult.
    /// Used to transform events to local coordinates during dispatch.
    pub transform: Option<Matrix4>,

    /// Mouse cursor for this element.
    ///
    /// Used by MouseTracker to determine the active cursor.
    /// `MouseCursor::Defer` means defer to the next element in the hit test chain.
    pub cursor: MouseCursor,
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("element_id", &self.element_id)
            .field("local_position", &self.local_position)
            .field("bounds", &self.bounds)
            .field("has_handler", &self.handler.is_some())
            .field("has_scroll_handler", &self.scroll_handler.is_some())
            .field("has_transform", &self.transform.is_some())
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl HitTestEntry {
    /// Create a new hit test entry.
    ///
    /// The transform will be captured automatically when added to HitTestResult.
    /// Cursor defaults to `MouseCursor::Defer`.
    #[inline]
    pub fn new(element_id: ElementId, local_position: Offset, bounds: Rect) -> Self {
        Self {
            element_id,
            local_position,
            bounds,
            handler: None,
            scroll_handler: None,
            transform: None,
            cursor: MouseCursor::Defer,
        }
    }

    /// Create entry with a pointer event handler.
    ///
    /// The transform will be captured automatically when added to HitTestResult.
    #[inline]
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
            scroll_handler: None,
            transform: None,
            cursor: MouseCursor::Defer,
        }
    }

    /// Create entry with a scroll event handler.
    ///
    /// The transform will be captured automatically when added to HitTestResult.
    #[inline]
    pub fn with_scroll_handler(
        element_id: ElementId,
        local_position: Offset,
        bounds: Rect,
        scroll_handler: ScrollEventHandler,
    ) -> Self {
        Self {
            element_id,
            local_position,
            bounds,
            handler: None,
            scroll_handler: Some(scroll_handler),
            transform: None,
            cursor: MouseCursor::Defer,
        }
    }

    /// Create entry with both pointer and scroll handlers.
    #[inline]
    pub fn with_handlers(
        element_id: ElementId,
        local_position: Offset,
        bounds: Rect,
        handler: PointerEventHandler,
        scroll_handler: ScrollEventHandler,
    ) -> Self {
        Self {
            element_id,
            local_position,
            bounds,
            handler: Some(handler),
            scroll_handler: Some(scroll_handler),
            transform: None,
            cursor: MouseCursor::Defer,
        }
    }

    /// Create entry with a specific mouse cursor.
    ///
    /// The transform will be captured automatically when added to HitTestResult.
    #[inline]
    pub fn with_cursor(
        element_id: ElementId,
        local_position: Offset,
        bounds: Rect,
        cursor: MouseCursor,
    ) -> Self {
        Self {
            element_id,
            local_position,
            bounds,
            handler: None,
            scroll_handler: None,
            transform: None,
            cursor,
        }
    }

    /// Sets the cursor for this entry (builder pattern).
    #[inline]
    pub fn cursor(mut self, cursor: MouseCursor) -> Self {
        self.cursor = cursor;
        self
    }

    /// Returns `true` if this entry has a pointer handler.
    #[inline]
    pub fn has_handler(&self) -> bool {
        self.handler.is_some()
    }

    /// Returns `true` if this entry has a scroll handler.
    #[inline]
    pub fn has_scroll_handler(&self) -> bool {
        self.scroll_handler.is_some()
    }

    /// Returns `true` if this entry has a transform.
    #[inline]
    pub fn has_transform(&self) -> bool {
        self.transform.is_some()
    }

    /// Returns `true` if this entry has a non-defer cursor.
    #[inline]
    pub fn has_cursor(&self) -> bool {
        !self.cursor.is_defer()
    }
}

// ============================================================================
// HitTestable trait
// ============================================================================

/// Trait for objects that can be hit-tested.
///
/// Implement this on your Layer or UI element type to enable hit testing.
/// Follows Flutter's RenderBox.hitTest pattern.
///
/// # Custom Hit Testable Types
///
/// To create a custom hit-testable type, implement [`CustomHitTestable`]
/// instead of this trait directly. The blanket implementation will automatically
/// provide `HitTestable` for your type.
///
/// ```rust,ignore
/// use flui_interaction::sealed::CustomHitTestable;
/// use flui_interaction::hit_test::{HitTestResult, HitTestBehavior};
///
/// struct MyLayer {
///     bounds: Rect,
///     element_id: ElementId,
/// }
///
/// impl CustomHitTestable for MyLayer {
///     fn perform_hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
///         if !self.bounds.contains(position) {
///             return false;
///         }
///         result.add(HitTestEntry::new(self.element_id, position, self.bounds));
///         true
///     }
///
///     fn get_hit_test_behavior(&self) -> HitTestBehavior {
///         HitTestBehavior::Opaque
///     }
/// }
///
/// // MyLayer now implements HitTestable automatically!
/// ```
///
/// [`CustomHitTestable`]: crate::sealed::CustomHitTestable
///
/// # Example (Built-in Types)
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
///         {
///             let _guard = result.push_offset(self.child_offset);
///             let child_hit = self.child.hit_test(position, result);
///         } // Transform automatically popped
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
pub trait HitTestable: crate::sealed::hit_testable::Sealed {
    /// Perform hit testing at the given position.
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
    /// 3. Use the returned guard for automatic cleanup
    /// 4. Add your own entry to `result` based on `hit_test_behavior()`
    /// 5. Return `true` if hit (self or child), `false` otherwise
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool;

    /// Returns the hit test behavior for this element.
    ///
    /// Controls whether this element registers as hit and blocks events below.
    /// Default is `DeferToChild`.
    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::DeferToChild
    }
}

// ============================================================================
// Blanket implementation for CustomHitTestable
// ============================================================================

/// Blanket implementation: any `CustomHitTestable` automatically
/// implements `HitTestable`.
impl<T: crate::sealed::CustomHitTestable> HitTestable for T {
    #[inline]
    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.perform_hit_test(position, result)
    }

    #[inline]
    fn hit_test_behavior(&self) -> HitTestBehavior {
        self.get_hit_test_behavior()
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Transform a pointer event using the given transformation matrix.
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
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Down(new_data)
        }
        PointerEvent::Up(data) => {
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Up(new_data)
        }
        PointerEvent::Move(data) => {
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Move(new_data)
        }
        PointerEvent::Hover(data) => {
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Hover(new_data)
        }
        PointerEvent::Enter(data) => {
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Enter(new_data)
        }
        PointerEvent::Exit(data) => {
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Exit(new_data)
        }
        PointerEvent::Cancel(data) => {
            let mut new_data =
                PointerEventData::new(transform_offset(data.position), data.device_kind);
            new_data.device = data.device;
            PointerEvent::Cancel(new_data)
        }
        PointerEvent::Scroll {
            device,
            position,
            scroll_delta,
        } => PointerEvent::Scroll {
            device: *device,
            position: transform_offset(*position),
            scroll_delta: *scroll_delta, // Don't transform delta (it's a vector, not a point)
        },
        // Events without position data - return as-is
        other => other.clone(),
    }
}

/// Transform a scroll event using the given transformation matrix.
///
/// Transforms the event's position to a different coordinate space.
/// The scroll delta is not transformed as it represents a relative movement.
fn transform_scroll_event(event: &ScrollEventData, transform: &Matrix4) -> ScrollEventData {
    let point = transform.transform_point(event.position.dx, event.position.dy);

    ScrollEventData {
        position: Offset::new(point.0, point.1),
        delta: event.delta.clone(), // Delta is relative, don't transform
        modifiers: event.modifiers,
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
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_hit_test_result_with_capacity() {
        let result = HitTestResult::with_capacity(10, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn test_hit_test_result_add() {
        let mut result = HitTestResult::new();

        let entry1 = HitTestEntry::new(
            ElementId::new(1),
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
            ElementId::new(1),
            Offset::new(1.0, 1.0),
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
        ));
        result.add(HitTestEntry::new(
            ElementId::new(2),
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
            ElementId::new(1),
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
            ElementId::new(2),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            handler2,
        ));
        result.add(HitTestEntry::with_handler(
            ElementId::new(1),
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
    fn test_transform_guard() {
        let mut result = HitTestResult::new();

        // Push transform and get depth marker
        let depth = result.push_offset(Offset::new(10.0, 20.0));

        let entry = HitTestEntry::new(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        );
        result.add(entry);

        // Entry should have captured the transform
        assert!(result.entries()[0].transform.is_some());

        // Pop using depth marker
        result.pop_to_depth(depth);

        // Transform stack should be empty now
        assert!(result.transforms.is_empty());
    }

    #[test]
    fn test_transform_composition() {
        let mut result = HitTestResult::new();

        // Push multiple transforms
        let depth1 = result.push_offset(Offset::new(10.0, 20.0));
        let _depth2 = result.push_offset(Offset::new(5.0, 5.0));

        // Should compose: total offset = (15.0, 25.0)
        let transform = result.current_transform().unwrap();
        let point = transform.transform_point(0.0, 0.0);

        assert!((point.0 - 15.0).abs() < 0.001);
        assert!((point.1 - 25.0).abs() < 0.001);

        // Pop to first depth should remove both transforms
        result.pop_to_depth(depth1);
        assert!(result.transforms.is_empty());
    }

    #[test]
    fn test_hit_test_behavior() {
        assert_eq!(HitTestBehavior::default(), HitTestBehavior::DeferToChild);

        assert!(!HitTestBehavior::DeferToChild.registers_self());
        assert!(HitTestBehavior::Opaque.registers_self());
        assert!(HitTestBehavior::Translucent.registers_self());

        assert!(!HitTestBehavior::DeferToChild.blocks_below());
        assert!(HitTestBehavior::Opaque.blocks_below());
        assert!(!HitTestBehavior::Translucent.blocks_below());
    }

    #[test]
    fn test_event_propagation() {
        assert!(EventPropagation::Continue.should_continue());
        assert!(!EventPropagation::Continue.should_stop());

        assert!(EventPropagation::Stop.should_stop());
        assert!(!EventPropagation::Stop.should_continue());

        assert_eq!(EventPropagation::default(), EventPropagation::Continue);
    }

    #[test]
    fn test_hit_test_entry_methods() {
        let entry = HitTestEntry::new(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        );

        assert!(!entry.has_handler());
        assert!(!entry.has_transform());

        let handler = Arc::new(|_: &PointerEvent| EventPropagation::Continue);
        let entry_with_handler = HitTestEntry::with_handler(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            handler,
        );

        assert!(entry_with_handler.has_handler());
    }

    #[test]
    fn test_clear() {
        let mut result = HitTestResult::new();

        let _depth = result.push_offset(Offset::new(10.0, 10.0));
        result.add(HitTestEntry::new(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        ));

        // Clear removes all entries and transforms
        result.clear();

        assert!(result.is_empty());
        assert!(result.transforms.is_empty());
    }

    #[test]
    fn test_scroll_handler_dispatch() {
        use flui_types::events::{KeyModifiers, ScrollDelta};
        use std::sync::Mutex;

        let mut result = HitTestResult::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        let scroll_handler = Arc::new(move |_event: &ScrollEventData| {
            *called_clone.lock().unwrap() = true;
            EventPropagation::Stop
        });

        let entry = HitTestEntry::with_scroll_handler(
            ElementId::new(1),
            Offset::new(10.0, 10.0),
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            scroll_handler,
        );
        result.add(entry);

        let scroll_event = ScrollEventData {
            position: Offset::new(50.0, 50.0),
            delta: ScrollDelta::Lines { x: 0.0, y: -3.0 },
            modifiers: KeyModifiers::default(),
        };

        let handled = result.dispatch_scroll(&scroll_event);
        assert!(handled);
        assert!(*called.lock().unwrap());
    }

    #[test]
    fn test_scroll_bubbling() {
        use flui_types::events::{KeyModifiers, ScrollDelta};
        use std::sync::Mutex;

        let mut result = HitTestResult::new();
        let inner_called = Arc::new(Mutex::new(false));
        let outer_called = Arc::new(Mutex::new(false));

        let inner_clone = inner_called.clone();
        let outer_clone = outer_called.clone();

        // Inner handler (added second, dispatched first) - doesn't handle
        let inner_handler = Arc::new(move |_event: &ScrollEventData| {
            *inner_clone.lock().unwrap() = true;
            EventPropagation::Continue // Let it bubble
        });

        // Outer handler (added first, dispatched second) - handles
        let outer_handler = Arc::new(move |_event: &ScrollEventData| {
            *outer_clone.lock().unwrap() = true;
            EventPropagation::Stop
        });

        // Add outer first (will be at end of entries after insert)
        result.add(HitTestEntry::with_scroll_handler(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 200.0, 200.0),
            outer_handler,
        ));

        // Add inner second (will be at start of entries)
        result.add(HitTestEntry::with_scroll_handler(
            ElementId::new(2),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            inner_handler,
        ));

        let scroll_event = ScrollEventData {
            position: Offset::new(50.0, 50.0),
            delta: ScrollDelta::Lines { x: 0.0, y: -1.0 },
            modifiers: KeyModifiers::default(),
        };

        let handled = result.dispatch_scroll(&scroll_event);

        // Both should be called, outer should handle
        assert!(handled);
        assert!(*inner_called.lock().unwrap());
        assert!(*outer_called.lock().unwrap());
    }

    #[test]
    fn test_scroll_stop_propagation() {
        use flui_types::events::{KeyModifiers, ScrollDelta};
        use std::sync::Mutex;

        let mut result = HitTestResult::new();
        let inner_called = Arc::new(Mutex::new(false));
        let outer_called = Arc::new(Mutex::new(false));

        let inner_clone = inner_called.clone();
        let outer_clone = outer_called.clone();

        // Inner handler stops propagation
        let inner_handler = Arc::new(move |_event: &ScrollEventData| {
            *inner_clone.lock().unwrap() = true;
            EventPropagation::Stop // Don't bubble
        });

        // Outer handler should NOT be called
        let outer_handler = Arc::new(move |_event: &ScrollEventData| {
            *outer_clone.lock().unwrap() = true;
            EventPropagation::Continue
        });

        // Add outer first
        result.add(HitTestEntry::with_scroll_handler(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 200.0, 200.0),
            outer_handler,
        ));

        // Add inner second
        result.add(HitTestEntry::with_scroll_handler(
            ElementId::new(2),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            inner_handler,
        ));

        let scroll_event = ScrollEventData {
            position: Offset::new(50.0, 50.0),
            delta: ScrollDelta::Lines { x: 0.0, y: -1.0 },
            modifiers: KeyModifiers::default(),
        };

        let handled = result.dispatch_scroll(&scroll_event);

        // Inner called and handled, outer NOT called
        assert!(handled);
        assert!(*inner_called.lock().unwrap());
        assert!(!*outer_called.lock().unwrap());
    }

    #[test]
    fn test_scroll_not_handled() {
        use flui_types::events::{KeyModifiers, ScrollDelta};

        let result = HitTestResult::new();

        let scroll_event = ScrollEventData {
            position: Offset::new(50.0, 50.0),
            delta: ScrollDelta::Lines { x: 0.0, y: -1.0 },
            modifiers: KeyModifiers::default(),
        };

        // No handlers, should return false
        let handled = result.dispatch_scroll(&scroll_event);
        assert!(!handled);
    }

    #[test]
    fn test_entry_with_both_handlers() {
        let pointer_handler = Arc::new(|_: &PointerEvent| EventPropagation::Continue);
        let scroll_handler = Arc::new(|_: &ScrollEventData| EventPropagation::Continue);

        let entry = HitTestEntry::with_handlers(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            pointer_handler,
            scroll_handler,
        );

        assert!(entry.has_handler());
        assert!(entry.has_scroll_handler());
    }

    #[test]
    fn test_entries_with_scroll_handlers() {
        let mut result = HitTestResult::new();

        // Add entry without scroll handler
        result.add(HitTestEntry::new(
            ElementId::new(1),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
        ));

        // Add entry with scroll handler
        let scroll_handler = Arc::new(|_: &ScrollEventData| EventPropagation::Continue);
        result.add(HitTestEntry::with_scroll_handler(
            ElementId::new(2),
            Offset::ZERO,
            Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            scroll_handler,
        ));

        let count = result.entries_with_scroll_handlers().count();
        assert_eq!(count, 1);
    }
}
