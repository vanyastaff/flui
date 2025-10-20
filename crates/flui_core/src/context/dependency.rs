//! Dependency tracking for InheritedWidget system

use std::any::Any;
use std::collections::HashMap;

use crate::ElementId;

/// Information about a dependency on an InheritedWidget
#[derive(Debug)]
pub struct DependencyInfo {
    /// The element that depends on the InheritedWidget
    pub dependent_id: ElementId,

    /// Optional aspect for partial dependencies (future: InheritedModel support)
    /// Note: Not Clone because aspect may contain non-cloneable data
    pub aspect: Option<Box<dyn Any + Send + Sync>>,
}

impl Clone for DependencyInfo {
    fn clone(&self) -> Self {
        Self {
            dependent_id: self.dependent_id,
            // Aspect is not cloned - set to None
            // This is acceptable because aspects are rarely used (InheritedModel feature)
            aspect: None,
        }
    }
}

/// Tracks dependencies for an InheritedElement
///
/// Maintains a registry of which elements depend on a specific InheritedWidget,
/// enabling selective notification when the widget changes.
#[derive(Debug)]
pub struct DependencyTracker {
    /// Map from dependent element ID to dependency info
    dependents: HashMap<ElementId, DependencyInfo>,
}

impl DependencyTracker {
    /// Create a new empty dependency tracker
    pub fn new() -> Self {
        Self {
            dependents: HashMap::new(),
        }
    }

    /// Register a dependency
    ///
    /// # Parameters
    ///
    /// - `dependent_id`: ID of the element that depends on the InheritedWidget
    /// - `aspect`: Optional aspect for partial dependencies (future: InheritedModel)
    pub fn add_dependent(
        &mut self,
        dependent_id: ElementId,
        aspect: Option<Box<dyn Any + Send + Sync>>,
    ) {
        self.dependents.insert(
            dependent_id,
            DependencyInfo {
                dependent_id,
                aspect,
            },
        );
    }

    /// Remove a dependency (when element is unmounted)
    ///
    /// # Parameters
    ///
    /// - `dependent_id`: ID of the element to remove
    ///
    /// # Returns
    ///
    /// true if the dependent was removed, false if it wasn't registered
    pub fn remove_dependent(&mut self, dependent_id: ElementId) -> bool {
        self.dependents.remove(&dependent_id).is_some()
    }

    /// Get all dependents
    pub fn dependents(&self) -> impl Iterator<Item = &DependencyInfo> + '_ {
        self.dependents.values()
    }

    /// Check if an element depends on this
    pub fn has_dependent(&self, dependent_id: ElementId) -> bool {
        self.dependents.contains_key(&dependent_id)
    }

    /// Get count of dependents
    pub fn dependent_count(&self) -> usize {
        self.dependents.len()
    }

    /// Clear all dependencies
    pub fn clear(&mut self) {
        self.dependents.clear();
    }

    /// Check if there are any dependents
    pub fn is_empty(&self) -> bool {
        self.dependents.is_empty()
    }
}

impl Default for DependencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_tracker_creation() {
        let tracker = DependencyTracker::new();
        assert_eq!(tracker.dependent_count(), 0);
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_dependency_tracker_default() {
        let tracker = DependencyTracker::default();
        assert_eq!(tracker.dependent_count(), 0);
    }

    #[test]
    fn test_add_dependent() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();

        tracker.add_dependent(id1, None);

        assert_eq!(tracker.dependent_count(), 1);
        assert!(tracker.has_dependent(id1));
        assert!(!tracker.is_empty());
    }

    #[test]
    fn test_remove_dependent() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();

        tracker.add_dependent(id1, None);
        assert_eq!(tracker.dependent_count(), 1);

        let removed = tracker.remove_dependent(id1);
        assert!(removed);
        assert_eq!(tracker.dependent_count(), 0);
        assert!(!tracker.has_dependent(id1));
    }

    #[test]
    fn test_remove_nonexistent_dependent() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();

        let removed = tracker.remove_dependent(id1);
        assert!(!removed);
    }

    #[test]
    fn test_multiple_dependents() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        tracker.add_dependent(id1, None);
        tracker.add_dependent(id2, None);
        tracker.add_dependent(id3, None);

        assert_eq!(tracker.dependent_count(), 3);
        assert!(tracker.has_dependent(id1));
        assert!(tracker.has_dependent(id2));
        assert!(tracker.has_dependent(id3));
    }

    #[test]
    fn test_add_duplicate_dependent() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();

        tracker.add_dependent(id1, None);
        tracker.add_dependent(id1, None); // Add again

        // Should still have only 1 (HashMap deduplicates)
        assert_eq!(tracker.dependent_count(), 1);
    }

    #[test]
    fn test_clear() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        tracker.add_dependent(id1, None);
        tracker.add_dependent(id2, None);
        assert_eq!(tracker.dependent_count(), 2);

        tracker.clear();
        assert_eq!(tracker.dependent_count(), 0);
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_dependents_iterator() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        tracker.add_dependent(id1, None);
        tracker.add_dependent(id2, None);

        let count = tracker.dependents().count();
        assert_eq!(count, 2);

        // Check all IDs present
        let ids: Vec<ElementId> = tracker.dependents().map(|info| info.dependent_id).collect();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_dependency_info_aspect() {
        let mut tracker = DependencyTracker::new();
        let id1 = ElementId::new();

        // Add without aspect
        tracker.add_dependent(id1, None);

        let info = tracker.dependents().next().unwrap();
        assert!(info.aspect.is_none());
    }
}
