//! InheritedElement support traits.
//!
//! This module provides abstractions for Flutter's `InheritedWidget`/`InheritedElement`
//! pattern, which enables efficient propagation of data down the element tree.
//!
//! # Flutter's InheritedWidget Pattern
//!
//! InheritedWidgets provide a way to efficiently pass data down the widget tree:
//! - Data is stored in an `InheritedElement`
//! - Descendant elements can "depend on" inherited elements
//! - When inherited data changes, all dependents are automatically rebuilt
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{InheritedData, DependencyTracker};
//! use flui_foundation::ElementId;
//!
//! // Theme data that can be inherited
//! struct ThemeData {
//!     primary_color: u32,
//!     font_size: f32,
//! }
//!
//! impl InheritedData for ThemeData {
//!     fn update_should_notify(&self, old: &Self) -> bool {
//!         self.primary_color != old.primary_color
//!             || self.font_size != old.font_size
//!     }
//! }
//! ```

use flui_foundation::ElementId;
use std::any::TypeId;
use std::collections::HashSet;

// ============================================================================
// INHERITED DATA TRAIT
// ============================================================================

/// Trait for data that can be inherited through the element tree.
///
/// Implement this trait for any data type that should be propagatable
/// via `InheritedElement`.
pub trait InheritedData: Send + Sync + 'static {
    /// Determine if dependents should be notified when data changes.
    ///
    /// This is called when the inherited widget is updated with new data.
    /// Return `true` if dependents should rebuild, `false` otherwise.
    ///
    /// # Arguments
    /// * `old` - The previous data value
    ///
    /// # Returns
    /// `true` if the change is significant enough to notify dependents
    fn update_should_notify(&self, old: &Self) -> bool;

    /// Get a unique type identifier for this inherited data.
    ///
    /// Used to look up the correct inherited element by type.
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

// ============================================================================
// DEPENDENCY TRACKING
// ============================================================================

/// Trait for tracking dependencies on inherited elements.
///
/// Elements that depend on inherited data implement this trait
/// to register and manage their dependencies.
pub trait DependencyTracker: Send + Sync {
    /// Register a dependency on an inherited element.
    ///
    /// Called when an element reads from an inherited element
    /// during its build phase.
    fn depend_on(&mut self, inherited: ElementId);

    /// Remove dependency on an inherited element.
    ///
    /// Called when element no longer needs the inherited data.
    fn remove_dependency(&mut self, inherited: ElementId);

    /// Get all inherited elements this element depends on.
    fn dependencies(&self) -> &HashSet<ElementId>;

    /// Clear all dependencies.
    ///
    /// Called before rebuild to re-establish dependencies.
    fn clear_dependencies(&mut self);

    /// Check if this element depends on a specific inherited element.
    fn depends_on(&self, inherited: ElementId) -> bool {
        self.dependencies().contains(&inherited)
    }
}

/// Default implementation of dependency tracking.
#[derive(Debug, Clone, Default)]
pub struct Dependencies {
    /// Set of inherited element IDs this element depends on.
    inherited: HashSet<ElementId>,
}

impl Dependencies {
    /// Create new empty dependencies.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inherited: HashSet::with_capacity(capacity),
        }
    }
}

impl DependencyTracker for Dependencies {
    fn depend_on(&mut self, inherited: ElementId) {
        self.inherited.insert(inherited);
    }

    fn remove_dependency(&mut self, inherited: ElementId) {
        self.inherited.remove(&inherited);
    }

    fn dependencies(&self) -> &HashSet<ElementId> {
        &self.inherited
    }

    fn clear_dependencies(&mut self) {
        self.inherited.clear();
    }
}

// ============================================================================
// INHERITED ELEMENT TRAIT
// ============================================================================

/// Trait for elements that provide inherited data to descendants.
///
/// This is the element-side interface for Flutter's `InheritedElement`.
pub trait InheritedElement: Send + Sync {
    /// The type of data this inherited element provides.
    type Data: InheritedData;

    /// Get the current inherited data.
    fn data(&self) -> &Self::Data;

    /// Update the inherited data.
    ///
    /// Returns `true` if dependents should be notified (rebuild).
    fn update_data(&mut self, new_data: Self::Data) -> bool;

    /// Get all elements that depend on this inherited element.
    fn dependents(&self) -> &HashSet<ElementId>;

    /// Add a dependent element.
    fn add_dependent(&mut self, dependent: ElementId);

    /// Remove a dependent element.
    fn remove_dependent(&mut self, dependent: ElementId);

    /// Notify all dependents that data has changed.
    ///
    /// This should schedule rebuilds for all dependent elements.
    fn notify_dependents(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.dependents().iter().copied()
    }
}

/// Default implementation of inherited element state.
#[derive(Debug, Clone)]
pub struct InheritedState<T: InheritedData> {
    /// The inherited data.
    data: T,
    /// Elements that depend on this inherited element.
    dependents: HashSet<ElementId>,
}

impl<T: InheritedData> InheritedState<T> {
    /// Create new inherited state with initial data.
    pub fn new(data: T) -> Self {
        Self {
            data,
            dependents: HashSet::new(),
        }
    }

    /// Create with pre-allocated dependent capacity.
    pub fn with_capacity(data: T, capacity: usize) -> Self {
        Self {
            data,
            dependents: HashSet::with_capacity(capacity),
        }
    }
}

impl<T: InheritedData> InheritedElement for InheritedState<T> {
    type Data = T;

    fn data(&self) -> &T {
        &self.data
    }

    fn update_data(&mut self, new_data: T) -> bool {
        let should_notify = new_data.update_should_notify(&self.data);
        self.data = new_data;
        should_notify
    }

    fn dependents(&self) -> &HashSet<ElementId> {
        &self.dependents
    }

    fn add_dependent(&mut self, dependent: ElementId) {
        self.dependents.insert(dependent);
    }

    fn remove_dependent(&mut self, dependent: ElementId) {
        self.dependents.remove(&dependent);
    }
}

// ============================================================================
// INHERITED LOOKUP
// ============================================================================

/// Trait for looking up inherited elements in the tree.
///
/// Provides the `of` pattern from Flutter: `Theme.of(context)`.
pub trait InheritedLookup: Send + Sync {
    /// Find the nearest inherited element of a specific type.
    ///
    /// This walks up the tree from the current element looking for
    /// an inherited element that provides the requested data type.
    ///
    /// # Type Parameters
    /// * `T` - The inherited data type to look for
    ///
    /// # Returns
    /// The element ID of the nearest matching inherited element, or None.
    fn find_inherited<T: InheritedData>(&self) -> Option<ElementId>;

    /// Find inherited element and register dependency.
    ///
    /// Like `find_inherited`, but also registers the current element
    /// as a dependent of the found inherited element.
    ///
    /// This is the typical usage pattern during build.
    fn depend_on_inherited<T: InheritedData>(&mut self) -> Option<ElementId>;

    /// Get inherited data of a specific type.
    ///
    /// Convenience method that finds the inherited element and
    /// returns a reference to its data.
    fn get_inherited<T: InheritedData>(&self) -> Option<&T>;

    /// Get inherited data, registering dependency.
    ///
    /// Like `get_inherited`, but also registers as dependent.
    fn watch_inherited<T: InheritedData>(&mut self) -> Option<&T>;
}

// ============================================================================
// SCOPE TRAITS
// ============================================================================

/// Trait for scoped inherited data providers.
///
/// Scopes allow providing different inherited values to different
/// subtrees, similar to Flutter's `InheritedWidget.wrap`.
pub trait InheritedScope: Send + Sync {
    /// The type of data this scope provides.
    type Data: InheritedData;

    /// Get the scope's data.
    fn scope_data(&self) -> &Self::Data;

    /// Get the parent scope, if any.
    fn parent_scope(&self) -> Option<ElementId>;

    /// Check if this scope shadows a parent scope.
    fn is_shadowing(&self) -> bool {
        self.parent_scope().is_some()
    }
}

/// Notification policy for inherited data changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationPolicy {
    /// Always notify dependents on any change.
    #[default]
    Always,

    /// Never notify dependents (manual notification only).
    Never,

    /// Notify only if `update_should_notify` returns true.
    Conditional,

    /// Batch notifications until end of frame.
    Batched,
}

// ============================================================================
// INHERITED REGISTRY
// ============================================================================

/// Trait for registering and managing inherited elements.
///
/// The registry tracks all inherited elements in the tree and
/// provides fast lookup by type.
pub trait InheritedRegistry: Send + Sync {
    /// Register an inherited element.
    fn register_inherited(&mut self, element: ElementId, type_id: TypeId);

    /// Unregister an inherited element.
    fn unregister_inherited(&mut self, element: ElementId, type_id: TypeId);

    /// Find nearest inherited element of a type from a starting element.
    fn find_nearest(&self, from: ElementId, type_id: TypeId) -> Option<ElementId>;

    /// Get all inherited elements of a specific type.
    fn get_all_of_type(&self, type_id: TypeId) -> Vec<ElementId>;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test inherited data
    #[derive(Debug, Clone, PartialEq)]
    struct TestTheme {
        color: u32,
    }

    impl InheritedData for TestTheme {
        fn update_should_notify(&self, old: &Self) -> bool {
            self.color != old.color
        }
    }

    #[test]
    fn test_inherited_data_notify() {
        let theme1 = TestTheme { color: 0xFF0000 };
        let theme2 = TestTheme { color: 0xFF0000 };
        let theme3 = TestTheme { color: 0x00FF00 };

        assert!(!theme1.update_should_notify(&theme2)); // Same
        assert!(theme1.update_should_notify(&theme3)); // Different
    }

    #[test]
    fn test_dependencies() {
        let mut deps = Dependencies::new();
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);

        deps.depend_on(e1);
        deps.depend_on(e2);

        assert!(deps.depends_on(e1));
        assert!(deps.depends_on(e2));
        assert_eq!(deps.dependencies().len(), 2);

        deps.remove_dependency(e1);
        assert!(!deps.depends_on(e1));
        assert!(deps.depends_on(e2));

        deps.clear_dependencies();
        assert!(deps.dependencies().is_empty());
    }

    #[test]
    fn test_inherited_state() {
        let mut state = InheritedState::new(TestTheme { color: 0xFF0000 });
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);

        // Add dependents
        state.add_dependent(e1);
        state.add_dependent(e2);
        assert_eq!(state.dependents().len(), 2);

        // Update with same value - no notify
        let should_notify = state.update_data(TestTheme { color: 0xFF0000 });
        assert!(!should_notify);

        // Update with different value - should notify
        let should_notify = state.update_data(TestTheme { color: 0x00FF00 });
        assert!(should_notify);

        // Check data updated
        assert_eq!(state.data().color, 0x00FF00);

        // Remove dependent
        state.remove_dependent(e1);
        assert_eq!(state.dependents().len(), 1);
        assert!(!state.dependents().contains(&e1));
        assert!(state.dependents().contains(&e2));
    }

    #[test]
    fn test_inherited_state_notify_iterator() {
        let mut state = InheritedState::new(TestTheme { color: 0xFF0000 });
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);
        let e3 = ElementId::new(3);

        state.add_dependent(e1);
        state.add_dependent(e2);
        state.add_dependent(e3);

        let notified: HashSet<_> = state.notify_dependents().collect();
        assert_eq!(notified.len(), 3);
        assert!(notified.contains(&e1));
        assert!(notified.contains(&e2));
        assert!(notified.contains(&e3));
    }

    #[test]
    fn test_notification_policy() {
        assert_eq!(NotificationPolicy::default(), NotificationPolicy::Always);
    }
}
