//! Hit testing for elements
//!
//! This module provides hit testing functionality for elements, following the
//! Flutter pattern of RenderObject.hitTest().
//!
//! # Architecture
//!
//! ```text
//! Pointer Event → ElementTree.hit_test(position)
//!      ↓
//! RenderElement.hit_test(position, result)
//!      ↓ Check bounds (offset + size)
//!      ↓ Recursively test children
//!      ↓ Add self if hit
//! ElementHitTestResult → [element_id1, element_id2, ...]
//!      ↓
//! For each hit element:
//!     element.handle_event(Event::Pointer(...))
//! ```

use crate::ElementId;
use flui_types::Offset;

/// Hit test result entry for an element
///
/// Stores which element was hit and the local position where the hit occurred.
#[derive(Debug, Clone)]
pub struct ElementHitTestEntry {
    /// The element that was hit
    pub element_id: ElementId,

    /// Position in element's local coordinate space
    pub local_position: Offset,
}

impl ElementHitTestEntry {
    /// Create a new hit test entry
    pub fn new(element_id: ElementId, local_position: Offset) -> Self {
        Self {
            element_id,
            local_position,
        }
    }
}

/// Hit test result for element tree
///
/// Contains all elements that were hit during hit testing,
/// ordered from front to back (deepest child first, root last).
///
/// This follows the Flutter pattern where hit test results are accumulated
/// in reverse Z-order (children before parents).
#[derive(Debug, Clone, Default)]
pub struct ElementHitTestResult {
    /// Stack of hit entries (front to back)
    ///
    /// **Order matters**: Entries are added from deepest child to root.
    /// When dispatching events, we typically iterate in reverse (root to child)
    /// or forward (child to root) depending on event type.
    entries: Vec<ElementHitTestEntry>,
}

impl ElementHitTestResult {
    /// Create a new empty hit test result
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add an entry to the result
    ///
    /// Entries are added in depth-first order (children before parents).
    pub fn add(&mut self, entry: ElementHitTestEntry) {
        self.entries.push(entry);
    }

    /// Add an element with position
    ///
    /// Convenience method to add entry without creating ElementHitTestEntry manually.
    pub fn add_element(&mut self, element_id: ElementId, local_position: Offset) {
        self.entries.push(ElementHitTestEntry {
            element_id,
            local_position,
        });
    }

    /// Get all entries
    pub fn entries(&self) -> &[ElementHitTestEntry] {
        &self.entries
    }

    /// Check if any element was hit
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if any element was hit (opposite of is_empty)
    pub fn is_hit(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Get the top-most (front, deepest child) entry
    pub fn front(&self) -> Option<&ElementHitTestEntry> {
        self.entries.first()
    }

    /// Get the back-most (root or shallowest parent) entry
    pub fn back(&self) -> Option<&ElementHitTestEntry> {
        self.entries.last()
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get iterator over entries (front to back)
    pub fn iter(&self) -> impl Iterator<Item = &ElementHitTestEntry> {
        self.entries.iter()
    }

    /// Get iterator over entries (back to front - root to child)
    pub fn iter_reverse(&self) -> impl Iterator<Item = &ElementHitTestEntry> {
        self.entries.iter().rev()
    }
}
