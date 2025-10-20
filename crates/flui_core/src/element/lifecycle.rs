//! Element lifecycle states and inactive element management

use crate::ElementId;

/// Element lifecycle state (Initial → Active → Inactive → Defunct)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementLifecycle {
    /// Element created but not yet mounted
    Initial,
    /// Element is actively mounted in the tree
    Active,
    /// Element removed but can be reactivated (GlobalKey reparenting)
    Inactive,
    /// Element permanently unmounted and defunct
    Defunct,
}

impl ElementLifecycle {
    /// Check if element is active
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, ElementLifecycle::Active)
    }

    /// Check if element can be reactivated
    #[inline]
    pub fn can_reactivate(&self) -> bool {
        matches!(self, ElementLifecycle::Inactive)
    }

    /// Check if element is mounted (Active or Inactive)
    #[inline]
    pub fn is_mounted(&self) -> bool {
        matches!(self, ElementLifecycle::Active | ElementLifecycle::Inactive)
    }
}

/// Manager for inactive elements (supports GlobalKey reparenting)
#[derive(Debug, Default)]
pub struct InactiveElements {
    /// Deactivated elements (can be reactivated within same frame)
    elements: std::collections::HashSet<ElementId>,
}

impl InactiveElements {
    /// Create empty manager
    #[inline]
    pub fn new() -> Self {
        Self {
            elements: std::collections::HashSet::new(),
        }
    }

    /// Add element to inactive set
    #[inline]
    pub fn add(&mut self, element_id: ElementId) {
        self.elements.insert(element_id);
    }

    /// Remove element from inactive set (returns Some if was inactive)
    #[inline]
    pub fn remove(&mut self, element_id: ElementId) -> Option<ElementId> {
        if self.elements.remove(&element_id) {
            Some(element_id)
        } else {
            None
        }
    }

    /// Check if element is inactive
    #[inline]
    pub fn contains(&self, element_id: ElementId) -> bool {
        self.elements.contains(&element_id)
    }

    /// Get the number of inactive elements
    #[inline]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if there are no inactive elements
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Drain all inactive elements (for end-of-frame cleanup)
    #[inline]
    pub fn drain(&mut self) -> impl Iterator<Item = ElementId> + '_ {
        self.elements.drain()
    }

    /// Clear all inactive elements without returning them
    #[inline]
    pub fn clear(&mut self) {
        self.elements.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_lifecycle_states() {
        assert!(!ElementLifecycle::Initial.is_active());
        assert!(ElementLifecycle::Active.is_active());
        assert!(!ElementLifecycle::Inactive.is_active());
        assert!(!ElementLifecycle::Defunct.is_active());
    }

    #[test]
    fn test_element_lifecycle_can_reactivate() {
        assert!(!ElementLifecycle::Initial.can_reactivate());
        assert!(!ElementLifecycle::Active.can_reactivate());
        assert!(ElementLifecycle::Inactive.can_reactivate());
        assert!(!ElementLifecycle::Defunct.can_reactivate());
    }

    #[test]
    fn test_element_lifecycle_is_mounted() {
        assert!(!ElementLifecycle::Initial.is_mounted());
        assert!(ElementLifecycle::Active.is_mounted());
        assert!(ElementLifecycle::Inactive.is_mounted());
        assert!(!ElementLifecycle::Defunct.is_mounted());
    }

    #[test]
    fn test_inactive_elements_new() {
        let inactive = InactiveElements::new();
        assert!(inactive.is_empty());
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_inactive_elements_add_remove() {
        let mut inactive = InactiveElements::new();
        let id = ElementId::new();

        inactive.add(id);
        assert!(inactive.contains(id));
        assert_eq!(inactive.len(), 1);

        let removed = inactive.remove(id);
        assert_eq!(removed, Some(id));
        assert!(!inactive.contains(id));
        assert!(inactive.is_empty());
    }

    #[test]
    fn test_inactive_elements_drain() {
        let mut inactive = InactiveElements::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        inactive.add(id1);
        inactive.add(id2);
        assert_eq!(inactive.len(), 2);

        let drained: Vec<_> = inactive.drain().collect();
        assert_eq!(drained.len(), 2);
        assert!(drained.contains(&id1));
        assert!(drained.contains(&id2));
        assert!(inactive.is_empty());
    }
}
