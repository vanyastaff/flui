//! DirtyTracker - Composable dirty tracking for pipeline phases.
//!
//! Provides common dirty tracking functionality used by both
//! LayoutPipeline and PaintPipeline.

use flui_foundation::ElementId;
use flui_pipeline::LockFreeDirtySet;

/// Composable dirty tracking for pipeline phases.
///
/// Wraps `LockFreeDirtySet` with a clean API. Used by both
/// LayoutPipeline and PaintPipeline to avoid code duplication.
#[derive(Debug, Default)]
pub struct DirtyTracker {
    dirty: LockFreeDirtySet,
}

impl DirtyTracker {
    /// Creates a new dirty tracker.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks an element as dirty.
    #[inline]
    pub fn mark_dirty(&self, id: ElementId) {
        self.dirty.mark_dirty(id);
    }

    /// Checks if any elements are dirty.
    #[inline]
    pub fn has_dirty(&self) -> bool {
        self.dirty.has_dirty()
    }

    /// Checks if a specific element is dirty.
    #[inline]
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.dirty.is_dirty(id)
    }

    /// Returns the number of dirty elements.
    #[inline]
    pub fn len(&self) -> usize {
        self.dirty.len()
    }

    /// Returns true if there are no dirty elements.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.dirty.len() == 0
    }

    /// Clears all dirty elements without processing.
    #[inline]
    pub fn clear(&mut self) {
        self.dirty.clear();
    }

    /// Drains all dirty element IDs for processing.
    #[inline]
    pub fn drain(&self) -> Vec<ElementId> {
        self.dirty.drain()
    }

    /// Marks all elements as dirty.
    #[inline]
    pub fn mark_all_dirty(&self) {
        self.dirty.mark_all_dirty();
    }

    /// Returns a reference to the underlying dirty set.
    #[inline]
    pub fn inner(&self) -> &LockFreeDirtySet {
        &self.dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirty_tracking() {
        let tracker = DirtyTracker::new();

        assert!(!tracker.has_dirty());
        assert_eq!(tracker.len(), 0);

        tracker.mark_dirty(ElementId::new(1));
        tracker.mark_dirty(ElementId::new(2));

        assert!(tracker.has_dirty());
        assert_eq!(tracker.len(), 2);
        assert!(tracker.is_dirty(ElementId::new(1)));
        assert!(tracker.is_dirty(ElementId::new(2)));
        assert!(!tracker.is_dirty(ElementId::new(3)));
    }

    #[test]
    fn test_drain() {
        let tracker = DirtyTracker::new();

        tracker.mark_dirty(ElementId::new(1));
        tracker.mark_dirty(ElementId::new(2));

        let drained = tracker.drain();
        assert_eq!(drained.len(), 2);
        assert!(!tracker.has_dirty());
    }
}
