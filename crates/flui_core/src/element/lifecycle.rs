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
    /// Check if an element is active
    #[inline]
    pub const fn is_active(self) -> bool {
        matches!(self, ElementLifecycle::Active)
    }

    /// Check if an element can be reactivated
    #[inline]
    pub const fn can_reactivate(self) -> bool {
        matches!(self, ElementLifecycle::Inactive)
    }

    /// Check if an element is mounted (Active or Inactive)
    #[inline]
    pub const fn is_mounted(self) -> bool {
        matches!(self, ElementLifecycle::Active | ElementLifecycle::Inactive)
    }
}

impl Default for ElementLifecycle {
    fn default() -> Self {
        Self::Initial
    }
}

/// Manager for inactive elements (supports GlobalKey reparenting)
///
/// Tracks elements that have been deactivated but not yet unmounted.
/// These elements can be reactivated within the same frame for GlobalKey reparenting.
///
/// # Examples
///
/// ```rust,ignore
/// let mut inactive = InactiveElements::new();
///
/// // Deactivate element
/// inactive.add(element_id);
///
/// // Check if inactive
/// assert!(inactive.contains(element_id));
///
/// // Reactivate or cleanup at end of frame
/// if let Some(id) = inactive.remove(element_id) {
///     // Reactivate
/// } else {
///     // Cleanup inactive elements
///     for id in inactive.drain() {
///         tree.remove(id);
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct InactiveElements {
    /// Deactivated elements (can be reactivated within the same frame)
    elements: std::collections::HashSet<ElementId>,
}

impl InactiveElements {
    /// Creates an empty inactive elements manager
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            elements: std::collections::HashSet::new(),
        }
    }

    /// Creates a manager with pre-allocated capacity
    #[must_use]
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            elements: std::collections::HashSet::with_capacity(capacity),
        }
    }

    /// Adds an element to the inactive set
    ///
    /// Returns `true` if the element was newly inserted, `false` if it was already present.
    #[inline]
    pub fn add(&mut self, element_id: ElementId) -> bool {
        self.elements.insert(element_id)
    }

    /// Removes an element from the inactive set
    ///
    /// Returns `Some(element_id)` if the element was inactive, `None` otherwise.
    #[inline]
    pub fn remove(&mut self, element_id: ElementId) -> Option<ElementId> {
        if self.elements.remove(&element_id) {
            Some(element_id)
        } else {
            None
        }
    }

    /// Checks if an element is inactive
    #[must_use]
    #[inline]
    pub fn contains(&self, element_id: ElementId) -> bool {
        self.elements.contains(&element_id)
    }

    /// Returns the number of inactive elements
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Checks if there are no inactive elements
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Drains all inactive elements (for end-of-frame cleanup)
    ///
    /// Returns an iterator over all inactive element IDs, clearing the set.
    #[inline]
    pub fn drain(&mut self) -> impl Iterator<Item = ElementId> + '_ {
        self.elements.drain()
    }

    /// Clears all inactive elements without returning them
    #[inline]
    pub fn clear(&mut self) {
        self.elements.clear();
    }

    /// Reserves capacity for at least `additional` more elements
    pub fn reserve(&mut self, additional: usize) {
        self.elements.reserve(additional);
    }

    /// Shrinks the capacity to fit the current number of elements
    pub fn shrink_to_fit(&mut self) {
        self.elements.shrink_to_fit();
    }
}

// ========== Trait Implementations ==========

impl Clone for InactiveElements {
    /// Clones the inactive elements set
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let inactive = InactiveElements::new();
    /// inactive.add(element_id);
    ///
    /// let snapshot = inactive.clone();
    /// assert_eq!(inactive, snapshot);
    /// ```
    fn clone(&self) -> Self {
        Self {
            elements: self.elements.clone(),
        }
    }
}

impl PartialEq for InactiveElements {
    /// Compares two inactive element sets
    ///
    /// Two sets are equal if they contain the same element IDs.
    fn eq(&self, other: &Self) -> bool {
        self.elements == other.elements
    }
}

impl Eq for InactiveElements {}

impl std::hash::Hash for InactiveElements {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the number of elements
        self.elements.len().hash(state);

        // Hash elements in sorted order for consistency
        let mut ids: Vec<_> = self.elements.iter().collect();
        ids.sort_unstable();
        for id in ids {
            id.hash(state);
        }
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
    fn test_element_lifecycle_default() {
        assert_eq!(ElementLifecycle::default(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_inactive_elements_new() {
        let inactive = InactiveElements::new();
        assert!(inactive.is_empty());
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_inactive_elements_with_capacity() {
        let inactive = InactiveElements::with_capacity(10);
        assert!(inactive.is_empty());
    }

    #[test]
    fn test_inactive_elements_add() {
        let mut inactive = InactiveElements::new();
        let id = ElementId::new();

        assert!(inactive.add(id));
        assert!(!inactive.add(id)); // Already present
        assert_eq!(inactive.len(), 1);
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
    fn test_inactive_elements_remove_nonexistent() {
        let mut inactive = InactiveElements::new();
        let id = ElementId::new();

        let removed = inactive.remove(id);
        assert_eq!(removed, None);
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

    #[test]
    fn test_inactive_elements_clear() {
        let mut inactive = InactiveElements::new();
        inactive.add(ElementId::new());
        inactive.add(ElementId::new());
        assert_eq!(inactive.len(), 2);

        inactive.clear();
        assert!(inactive.is_empty());
    }

    #[test]
    fn test_inactive_elements_clone() {
        let mut inactive = InactiveElements::new();
        let id = ElementId::new();
        inactive.add(id);

        let cloned = inactive.clone();
        assert_eq!(cloned.len(), 1);
        assert!(cloned.contains(id));
        assert_eq!(inactive, cloned);
    }

    #[test]
    fn test_inactive_elements_equality() {
        let mut inactive1 = InactiveElements::new();
        let mut inactive2 = InactiveElements::new();
        let id = ElementId::new();

        inactive1.add(id);
        inactive2.add(id);

        assert_eq!(inactive1, inactive2);
    }

    #[test]
    fn test_inactive_elements_inequality() {
        let mut inactive1 = InactiveElements::new();
        let mut inactive2 = InactiveElements::new();

        inactive1.add(ElementId::new());
        inactive2.add(ElementId::new());

        assert_ne!(inactive1, inactive2);
    }

    #[test]
    fn test_inactive_elements_hash() {
        use std::collections::HashMap;

        let mut inactive = InactiveElements::new();
        inactive.add(ElementId::new());

        let mut map = HashMap::new();
        map.insert(inactive.clone(), "data");

        assert_eq!(map.get(&inactive), Some(&"data"));
    }

    #[test]
    fn test_inactive_elements_reserve() {
        let mut inactive = InactiveElements::new();
        inactive.reserve(100);
        // Should not panic
        assert_eq!(inactive.len(), 0);
    }

    #[test]
    fn test_inactive_elements_shrink_to_fit() {
        let mut inactive = InactiveElements::with_capacity(100);
        let id = ElementId::new();
        inactive.add(id);

        inactive.shrink_to_fit();
        assert_eq!(inactive.len(), 1);
    }
}