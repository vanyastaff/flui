//! Dirty tracking for pipeline phases.
//!
//! Provides a lock-free dirty set for tracking elements that need
//! layout or paint updates.

use flui_foundation::ElementId;
use parking_lot::RwLock;
use std::collections::HashSet;

/// A thread-safe set for tracking dirty elements.
///
/// Uses `parking_lot::RwLock` for efficient concurrent access.
/// Marking is lock-free for reads, uses write lock for modifications.
#[derive(Debug, Default)]
pub struct DirtySet {
    /// The set of dirty element IDs.
    elements: RwLock<HashSet<ElementId>>,
}

impl DirtySet {
    /// Creates a new empty dirty set.
    pub fn new() -> Self {
        Self {
            elements: RwLock::new(HashSet::new()),
        }
    }

    /// Marks an element as dirty.
    pub fn mark(&self, id: ElementId) {
        self.elements.write().insert(id);
    }

    /// Marks multiple elements as dirty.
    pub fn mark_many(&self, ids: impl IntoIterator<Item = ElementId>) {
        let mut set = self.elements.write();
        for id in ids {
            set.insert(id);
        }
    }

    /// Clears the dirty flag for an element.
    pub fn clear(&self, id: ElementId) {
        self.elements.write().remove(&id);
    }

    /// Checks if an element is dirty.
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.elements.read().contains(&id)
    }

    /// Returns true if any elements are dirty.
    pub fn has_dirty(&self) -> bool {
        !self.elements.read().is_empty()
    }

    /// Returns the number of dirty elements.
    pub fn len(&self) -> usize {
        self.elements.read().len()
    }

    /// Returns true if no elements are dirty.
    pub fn is_empty(&self) -> bool {
        self.elements.read().is_empty()
    }

    /// Takes all dirty elements, clearing the set.
    pub fn drain(&self) -> Vec<ElementId> {
        let mut set = self.elements.write();
        set.drain().collect()
    }

    /// Clears all dirty elements.
    pub fn clear_all(&self) {
        self.elements.write().clear();
    }

    /// Returns a copy of all dirty element IDs.
    pub fn iter(&self) -> Vec<ElementId> {
        self.elements.read().iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_and_check() {
        let set = DirtySet::new();
        let id = ElementId::new(1);

        assert!(!set.is_dirty(id));
        set.mark(id);
        assert!(set.is_dirty(id));
    }

    #[test]
    fn test_clear() {
        let set = DirtySet::new();
        let id = ElementId::new(1);

        set.mark(id);
        assert!(set.is_dirty(id));

        set.clear(id);
        assert!(!set.is_dirty(id));
    }

    #[test]
    fn test_drain() {
        let set = DirtySet::new();
        set.mark(ElementId::new(1));
        set.mark(ElementId::new(2));

        assert_eq!(set.len(), 2);

        let drained = set.drain();
        assert_eq!(drained.len(), 2);
        assert!(set.is_empty());
    }

    #[test]
    fn test_mark_many() {
        let set = DirtySet::new();
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];

        set.mark_many(ids);
        assert_eq!(set.len(), 3);
    }
}
