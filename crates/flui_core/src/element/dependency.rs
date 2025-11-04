//! Dependency tracking for InheritedWidget system

use std::any::Any;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::ElementId;

/// Information about a dependency on an InheritedWidget
///
/// Tracks which element depends on an InheritedWidget, with optional
/// aspect support for partial dependencies (InheritedModel).
///
/// # Examples
///
/// ```rust
/// use flui_core::element::dependency::DependencyInfo;
///
/// let info = DependencyInfo::new(0, None);
/// assert!(!info.has_aspect());
///
/// let simple = DependencyInfo::simple(0);
/// assert_eq!(info, simple); // Aspects are ignored in comparison
/// ```
#[derive(Debug)]
pub struct DependencyInfo {
    /// The element that depends on the InheritedWidget
    pub dependent_id: ElementId,

    /// Optional aspect for partial dependencies
    ///
    /// Used by InheritedModel to track which aspect of the data
    /// this element depends on. Cannot be cloned or compared due
    /// to trait object limitations.
    pub aspect: Option<Box<dyn Any + Send + Sync>>,
}

// ========== Construction ==========

impl DependencyInfo {
    /// Creates a new dependency info with optional aspect
    #[must_use]
    #[inline]
    pub const fn new(dependent_id: ElementId, aspect: Option<Box<dyn Any + Send + Sync>>) -> Self {
        Self {
            dependent_id,
            aspect,
        }
    }

    /// Creates a dependency without aspect (most common case)
    #[must_use]
    #[inline]
    pub const fn simple(dependent_id: ElementId) -> Self {
        Self {
            dependent_id,
            aspect: None,
        }
    }
}

// ========== Queries ==========

impl DependencyInfo {
    /// Checks if this dependency has an aspect
    #[must_use]
    #[inline]
    pub const fn has_aspect(&self) -> bool {
        self.aspect.is_some()
    }
}

// ========== Trait Implementations ==========

impl Default for DependencyInfo {
    fn default() -> Self {
        Self::simple(ElementId::new(1)) // Use dummy ID 1 (0 is invalid)
    }
}

impl Clone for DependencyInfo {
    /// Clones the dependency info
    ///
    /// Note: The aspect cannot be cloned (trait object limitation),
    /// so it's always set to None in the clone.
    fn clone(&self) -> Self {
        Self {
            dependent_id: self.dependent_id,
            aspect: None, // Cannot clone trait object
        }
    }
}

impl PartialEq for DependencyInfo {
    /// Compares dependency info by dependent_id only
    ///
    /// Aspects are ignored because trait objects cannot be compared.
    fn eq(&self, other: &Self) -> bool {
        self.dependent_id == other.dependent_id
    }
}

impl Eq for DependencyInfo {}

impl Hash for DependencyInfo {
    /// Hashes the dependency info by dependent_id only
    ///
    /// Aspects are ignored because trait objects cannot be hashed.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.dependent_id.hash(state);
    }
}

// =============================================================================
// DependencyTracker
// =============================================================================

/// Tracks dependencies for an InheritedElement
///
/// Maintains a registry of which elements depend on a specific InheritedWidget,
/// enabling selective notification when the widget changes.
///
/// # Performance
///
/// - O(1) insertion, removal, and lookup
/// - O(n) iteration over all dependents
/// - Uses HashMap for efficient storage
///
/// # Examples
///
/// ```rust
/// use flui_core::element::dependency::DependencyTracker;
///
/// let mut tracker = DependencyTracker::new();
/// let id = 42;
///
/// tracker.add_dependent(id, None);
/// assert_eq!(tracker.len(), 1);
/// assert!(tracker.has_dependent(id));
///
/// // Iterate over dependents
/// for info in tracker.dependents() {
///     println!("Dependent: {:?}", info.dependent_id);
/// }
///
/// // Cleanup
/// tracker.remove_dependent(id);
/// assert!(tracker.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct DependencyTracker {
    /// Map from dependent element ID to dependency info
    dependents: HashMap<ElementId, DependencyInfo>,
}

// ========== Construction ==========

impl DependencyTracker {
    /// Creates a new empty dependency tracker
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            dependents: HashMap::new(),
        }
    }

    /// Creates a tracker with pre-allocated capacity
    ///
    /// Useful when you know approximately how many dependents there will be.
    #[must_use]
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            dependents: HashMap::with_capacity(capacity),
        }
    }
}

// ========== Registration ==========

impl DependencyTracker {
    /// Registers a dependency
    ///
    /// If the element is already registered, its dependency info is updated.
    ///
    /// # Parameters
    ///
    /// - `dependent_id`: ID of the element that depends on the InheritedWidget
    /// - `aspect`: Optional aspect for partial dependencies (InheritedModel)
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// let id = 42;
    ///
    /// tracker.add_dependent(id, None);
    /// assert!(tracker.has_dependent(id));
    /// ```
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

    /// Removes a dependency (called when element is unmounted)
    ///
    /// # Returns
    ///
    /// `true` if the dependent was removed, `false` if it wasn't registered
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// let id = 42;
    ///
    /// tracker.add_dependent(id, None);
    /// assert!(tracker.remove_dependent(id));
    /// assert!(!tracker.remove_dependent(id)); // Already removed
    /// ```
    pub fn remove_dependent(&mut self, dependent_id: ElementId) -> bool {
        self.dependents.remove(&dependent_id).is_some()
    }

    /// Clears all dependencies
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// tracker.add_dependent(1, None);
    /// tracker.add_dependent(2, None);
    ///
    /// tracker.clear();
    /// assert!(tracker.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.dependents.clear();
    }
}

// ========== Queries ==========

impl DependencyTracker {
    /// Checks if an element depends on this InheritedWidget
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// let id = 42;
    ///
    /// assert!(!tracker.has_dependent(id));
    /// tracker.add_dependent(id, None);
    /// assert!(tracker.has_dependent(id));
    /// ```
    #[must_use]
    #[inline]
    pub fn has_dependent(&self, dependent_id: ElementId) -> bool {
        self.dependents.contains_key(&dependent_id)
    }

    /// Returns the number of dependents
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.dependents.len()
    }

    /// Checks if there are no dependents
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dependents.is_empty()
    }
}

// ========== Iteration ==========

impl DependencyTracker {
    /// Returns an iterator over all dependency info
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// tracker.add_dependent(1, None);
    /// tracker.add_dependent(2, None);
    ///
    /// let count = tracker.dependents().count();
    /// assert_eq!(count, 2);
    /// ```
    #[inline]
    pub fn dependents(&self) -> impl Iterator<Item = &DependencyInfo> + '_ {
        self.dependents.values()
    }

    /// Returns an iterator over dependent IDs
    ///
    /// More efficient than mapping over `dependents()`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// let id1 = 1;
    /// let id2 = 2;
    ///
    /// tracker.add_dependent(id1, None);
    /// tracker.add_dependent(id2, None);
    ///
    /// let ids: Vec<_> = tracker.dependent_ids().collect();
    /// assert_eq!(ids.len(), 2);
    /// ```
    pub fn dependent_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.dependents.keys().copied()
    }
}

// ========== Capacity Management ==========

impl DependencyTracker {
    /// Reserves capacity for additional dependents
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::new();
    /// tracker.reserve(100);
    /// // Capacity increased, no reallocation needed for next 100 insertions
    /// ```
    pub fn reserve(&mut self, additional: usize) {
        self.dependents.reserve(additional);
    }

    /// Shrinks capacity to fit current dependents
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use flui_core::element::dependency::DependencyTracker;
    /// let mut tracker = DependencyTracker::with_capacity(100);
    /// tracker.add_dependent(1, None);
    ///
    /// tracker.shrink_to_fit();
    /// // Capacity reduced to fit 1 element
    /// ```
    pub fn shrink_to_fit(&mut self) {
        self.dependents.shrink_to_fit();
    }
}

// ========== Trait Implementations ==========

impl Clone for DependencyTracker {
    /// Clones the dependency tracker
    ///
    /// Note: Aspects in DependencyInfo cannot be cloned, so they're
    /// set to None in the cloned tracker.
    fn clone(&self) -> Self {
        Self {
            dependents: self.dependents.clone(), // Uses DependencyInfo's Clone
        }
    }
}

impl PartialEq for DependencyTracker {
    /// Compares trackers by their dependent IDs
    ///
    /// Two trackers are equal if they have the same set of dependent IDs,
    /// regardless of aspects.
    fn eq(&self, other: &Self) -> bool {
        if self.dependents.len() != other.dependents.len() {
            return false;
        }

        self.dependents
            .keys()
            .all(|key| other.dependents.contains_key(key))
    }
}

impl Eq for DependencyTracker {}

// ========== Tests ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_info_new() {
        let id = 1;
        let info = DependencyInfo::new(id, None);
        assert_eq!(info.dependent_id, id);
        assert!(!info.has_aspect());
    }

    #[test]
    fn test_dependency_info_simple() {
        let id = 1;
        let info = DependencyInfo::simple(id);
        assert_eq!(info.dependent_id, id);
        assert!(!info.has_aspect());
    }

    #[test]
    fn test_dependency_info_with_aspect() {
        let id = 1;
        let aspect: Box<dyn Any + Send + Sync> = Box::new(42);
        let info = DependencyInfo::new(id, Some(aspect));
        assert_eq!(info.dependent_id, id);
        assert!(info.has_aspect());
    }

    #[test]
    fn test_dependency_info_equality() {
        let id = 1;
        let info1 = DependencyInfo::simple(id);
        let info2 = DependencyInfo::simple(id);
        assert_eq!(info1, info2);

        let id2 = 2;
        let info3 = DependencyInfo::simple(id2);
        assert_ne!(info1, info3);
    }

    #[test]
    fn test_dependency_info_hash() {
        use std::collections::HashSet;

        let id = 1;
        let info1 = DependencyInfo::simple(id);
        let info2 = DependencyInfo::simple(id);

        let mut set = HashSet::new();
        set.insert(info1);
        assert!(set.contains(&info2)); // Same hash
    }

    #[test]
    fn test_dependency_info_clone() {
        let id = 1;
        let info = DependencyInfo::simple(id);
        let cloned = info.clone();
        assert_eq!(info, cloned);
        assert!(!cloned.has_aspect()); // Aspect not cloned
    }

    #[test]
    fn test_tracker_creation() {
        let tracker = DependencyTracker::new();
        assert_eq!(tracker.len(), 0);
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_with_capacity() {
        let tracker = DependencyTracker::with_capacity(10);
        assert_eq!(tracker.len(), 0);
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_add_remove() {
        let mut tracker = DependencyTracker::new();
        let id = 1;

        tracker.add_dependent(id, None);
        assert_eq!(tracker.len(), 1);
        assert!(tracker.has_dependent(id));

        assert!(tracker.remove_dependent(id));
        assert!(!tracker.has_dependent(id));
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_tracker_duplicate_add() {
        let mut tracker = DependencyTracker::new();
        let id = 1;

        tracker.add_dependent(id, None);
        tracker.add_dependent(id, None); // Should replace

        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn test_tracker_multiple_dependents() {
        let mut tracker = DependencyTracker::new();

        for id in 1..=10 {
            tracker.add_dependent(id, None);
        }

        assert_eq!(tracker.len(), 10);

        for id in 1..=10 {
            assert!(tracker.has_dependent(id));
        }
    }

    #[test]
    fn test_tracker_iteration() {
        let mut tracker = DependencyTracker::new();
        let id1 = 1;
        let id2 = 2;

        tracker.add_dependent(id1, None);
        tracker.add_dependent(id2, None);

        let ids: Vec<_> = tracker.dependent_ids().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_tracker_dependents_iter() {
        let mut tracker = DependencyTracker::new();
        tracker.add_dependent(1, None);
        tracker.add_dependent(2, None);
        tracker.add_dependent(3, None);

        let count = tracker.dependents().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_tracker_clear() {
        let mut tracker = DependencyTracker::new();
        tracker.add_dependent(1, None);
        tracker.add_dependent(2, None);

        assert_eq!(tracker.len(), 2);

        tracker.clear();
        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
    }

    #[test]
    fn test_tracker_remove_nonexistent() {
        let mut tracker = DependencyTracker::new();
        assert!(!tracker.remove_dependent(999));
    }

    #[test]
    fn test_tracker_equality() {
        let mut tracker1 = DependencyTracker::new();
        let mut tracker2 = DependencyTracker::new();
        let id = 1;

        tracker1.add_dependent(id, None);
        tracker2.add_dependent(id, None);

        assert_eq!(tracker1, tracker2);

        tracker1.add_dependent(2, None);
        assert_ne!(tracker1, tracker2);
    }

    #[test]
    fn test_tracker_clone() {
        let mut tracker = DependencyTracker::new();
        tracker.add_dependent(1, None);
        tracker.add_dependent(2, None);

        let cloned = tracker.clone();
        assert_eq!(tracker, cloned);
        assert_eq!(cloned.len(), 2);
    }

    #[test]
    fn test_tracker_reserve() {
        let mut tracker = DependencyTracker::new();
        tracker.reserve(100);
        // No panic, just ensuring it works
    }

    #[test]
    fn test_tracker_shrink_to_fit() {
        let mut tracker = DependencyTracker::with_capacity(100);
        tracker.add_dependent(1, None);
        tracker.shrink_to_fit();
        // No panic, just ensuring it works
        assert_eq!(tracker.len(), 1);
    }
}
