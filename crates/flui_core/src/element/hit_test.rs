//! Hit testing for elements
//!
//! This module provides hit testing functionality for elements, following the
//! Flutter pattern of Render.hitTest().
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

use super::hit_test_entry::{BoxHitTestEntry, HitTestEntryTrait, SliverHitTestEntry};
use crate::ElementId;
use flui_types::{Matrix4, Offset};

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

// ========== Generic Hit Test Result ==========

/// Generic hit test result with transform tracking
///
/// This provides a unified hit testing system that works for both box-based
/// rendering (BoxConstraints → Size) and sliver-based rendering
/// (SliverConstraints → SliverGeometry).
///
/// # Type Parameter
///
/// - `E`: Hit test entry type implementing `HitTestEntryTrait`
#[derive(Debug, Clone)]
pub struct GenericHitTestResult<E: HitTestEntryTrait> {
    /// Stack of hit entries with element IDs (front to back: deepest child → root)
    entries: Vec<(ElementId, E)>,

    /// Transform matrices for coordinate conversion
    ///
    /// Parallel to entries vector. Used for transforming hit positions
    /// through the element hierarchy (e.g., for RenderTransform).
    transforms: Vec<Matrix4>,
}

impl<E: HitTestEntryTrait> GenericHitTestResult<E> {
    /// Create a new empty hit test result
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            transforms: Vec::new(),
        }
    }

    /// Add entry with element ID
    pub fn add(&mut self, element_id: ElementId, entry: E) {
        self.entries.push((element_id, entry));
    }

    /// Add entry with transform
    ///
    /// Use this when the element has a transform (e.g., RenderTransform)
    /// that needs to be applied for correct coordinate conversion.
    pub fn add_with_transform(&mut self, element_id: ElementId, entry: E, transform: Matrix4) {
        self.entries.push((element_id, entry));
        self.transforms.push(transform);
    }

    /// Get all entries
    pub fn entries(&self) -> &[(ElementId, E)] {
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

    /// Get the top-most (deepest child) entry
    pub fn front(&self) -> Option<&(ElementId, E)> {
        self.entries.first()
    }

    /// Get the back-most (root or shallowest parent) entry
    pub fn back(&self) -> Option<&(ElementId, E)> {
        self.entries.last()
    }

    /// Get transforms
    pub fn transforms(&self) -> &[Matrix4] {
        &self.transforms
    }

    /// Filter out invalid hits (outside bounds or not visible)
    ///
    /// Uses the `is_valid_hit()` method from `HitTestEntryTrait` to
    /// remove entries that are outside bounds or scrolled off-screen.
    pub fn filter_valid(&mut self) {
        self.entries.retain(|(_, entry)| entry.is_valid_hit());
    }

    /// Clear all entries and transforms
    pub fn clear(&mut self) {
        self.entries.clear();
        self.transforms.clear();
    }

    /// Get iterator over entries (front to back)
    pub fn iter(&self) -> impl Iterator<Item = &(ElementId, E)> {
        self.entries.iter()
    }

    /// Get iterator over entries (back to front - root to child)
    pub fn iter_reverse(&self) -> impl Iterator<Item = &(ElementId, E)> {
        self.entries.iter().rev()
    }
}

impl<E: HitTestEntryTrait> Default for GenericHitTestResult<E> {
    fn default() -> Self {
        Self::new()
    }
}

// Type aliases for convenience
/// Hit test result for box rendering
pub type BoxHitTestResult = GenericHitTestResult<BoxHitTestEntry>;

/// Hit test result for sliver rendering
pub type SliverHitTestResult = GenericHitTestResult<SliverHitTestEntry>;
