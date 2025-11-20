//! ProviderElement - Provides context/inherited data
//!
//! Manages data propagation down the tree with efficient dependency tracking.

use std::any::Any;
use std::collections::HashSet;

use crate::element::{Element, ElementBase, ElementLifecycle};
use crate::foundation::Slot;
use crate::view::BuildFn;
use crate::ElementId;

/// ProviderElement - provides context/inherited data
///
/// Stores provided data and tracks which descendant elements depend on it.
/// When the data updates, only dependent elements are rebuilt.
///
/// # Architecture
///
/// ```rust
/// pub struct ProviderElement {
///     base: ElementBase,              // Common fields
///     provided: Box<dyn Any>,         // The data being provided
///     builder: BuildFn,               // Function to rebuild child
///     dependents: HashSet<ElementId>, // Dependent elements
///     child: Option<ElementId>,       // Single child
/// }
/// ```
///
/// # Dependency Tracking
///
/// - Descendants call `context.depend_on::<Theme>()` to register dependency
/// - When provided data changes, dependents are notified
/// - Only registered dependents are rebuilt (efficient selective updates)
pub struct ProviderElement {
    /// Common element data (parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// The data being provided to descendants
    provided: Box<dyn Any + Send + Sync>,

    /// Build function that produces child element
    builder: BuildFn,

    /// Set of elements that depend on this provider
    dependents: HashSet<ElementId>,

    /// The single child element
    child: Option<ElementId>,
}

impl std::fmt::Debug for ProviderElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderElement")
            .field("base", &self.base)
            .field("provided", &"<dyn Any>")
            .field("builder", &"<BuildFn>")
            .field("dependents", &self.dependents)
            .field("child", &self.child)
            .finish()
    }
}

impl ProviderElement {
    /// Create a new ProviderElement
    ///
    /// # Parameters
    ///
    /// - `provided`: The data to provide to descendants
    /// - `builder`: Function that builds child element
    pub fn new(provided: Box<dyn Any + Send + Sync>, builder: BuildFn) -> Self {
        Self {
            base: ElementBase::new(),
            provided,
            builder,
            dependents: HashSet::new(),
            child: None,
        }
    }

    /// Get reference to provided data
    #[inline]
    #[must_use]
    pub fn provided(&self) -> &dyn Any {
        &*self.provided
    }

    /// Get mutable reference to provided data
    #[inline]
    #[must_use]
    pub fn provided_mut(&mut self) -> &mut dyn Any {
        &mut *self.provided
    }

    /// Call the build function to produce a new child element
    #[inline]
    pub fn build(&self) -> Element {
        (self.builder)()
    }

    /// Register a dependent element
    ///
    /// Called by BuildContext when a descendant element accesses inherited data.
    pub fn add_dependent(&mut self, element_id: ElementId) {
        self.dependents.insert(element_id);
    }

    /// Remove a dependent element
    pub fn remove_dependent(&mut self, element_id: ElementId) {
        self.dependents.remove(&element_id);
    }

    /// Get all dependent element IDs
    #[must_use]
    pub fn dependents(&self) -> &HashSet<ElementId> {
        &self.dependents
    }

    /// Get child element ID
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set child element ID
    #[allow(dead_code)]
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Clear the child
    #[inline]
    pub fn clear_child(&mut self) {
        self.child = None;
    }

    // ========== Common Interface ==========

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Get iterator over child element IDs
    #[inline]
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        if let Some(child) = self.child() {
            Box::new(std::iter::once(child))
        } else {
            Box::new(std::iter::empty())
        }
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Mount element to tree
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree
    pub fn unmount(&mut self) {
        self.base.unmount();
        self.child = None;
        self.dependents.clear();
    }

    /// Deactivate element
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate element
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Check if element needs rebuild
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as needing rebuild
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Clear dirty flag (after successful rebuild)
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.base.clear_dirty();
    }

    /// Forget child element
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    /// Update slot for child (no-op for single child)
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // ProviderElement always has exactly one child at slot 0
    }

    /// Handle an event
    ///
    /// Default implementation: does not handle events (returns false)
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false
    }

    /// Hit test - delegate to child
    #[inline]
    pub fn hit_test_child(&self) -> Option<ElementId> {
        self.child()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_element_creation() {
        let provided: Box<dyn Any + Send + Sync> = Box::new(42i32);
        let builder: BuildFn = Box::new(|| {
            Element::Provider(ProviderElement::new(
                Box::new(()),
                Box::new(|| panic!("not called")),
            ))
        });

        let provider = ProviderElement::new(provided, builder);
        assert_eq!(provider.child(), None);
        assert!(provider.dependents().is_empty());
    }
}
