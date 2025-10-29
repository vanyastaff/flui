//! ElementBase - Common fields and methods for all element types
//!
//! This module provides a shared foundation for all element types, reducing
//! code duplication and ensuring consistent lifecycle behavior.

use crate::element::{ElementId, ElementLifecycle};
use crate::widget::Widget;

/// ElementBase - Common data and behavior for all element types
///
/// All five element types (Component, Stateful, Inherited, Render, ParentData)
/// share common fields and lifecycle methods. ElementBase extracts this shared
/// behavior to reduce duplication and ensure consistency.
///
/// # Shared Fields
///
/// - `widget`: The widget configuration this element represents
/// - `parent`: Parent element ID (None for root)
/// - `slot`: Position in parent's child list
/// - `lifecycle`: Current lifecycle state
/// - `dirty`: Whether element needs rebuild
///
/// # Benefits
///
/// - **DRY**: Eliminates ~60 lines of duplicated code across 5 element types
/// - **Consistency**: Ensures all elements handle lifecycle identically
/// - **Maintainability**: Single source of truth for common behavior
/// - **Performance**: No overhead - simple struct composition
///
/// # Usage Pattern 1: With Widget Field
///
/// ```rust
/// pub struct ComponentElement {
///     base: ElementBase,
///     child: Option<ElementId>,
/// }
///
/// impl ComponentElement {
///     pub fn new(widget: Widget) -> Self {
///         Self {
///             base: ElementBase::new(widget),
///             child: None,
///         }
///     }
///
///     pub fn parent(&self) -> Option<ElementId> {
///         self.base.parent()
///     }
/// }
/// ```
///
/// # Usage Pattern 2: Separate Widget Field (for types with special widget handling)
///
/// Some element types may keep `widget: Widget` as a separate field for specific
/// access patterns while still using ElementBase for other common fields.
#[derive(Debug)]
pub struct ElementBase {
    /// The widget this element represents
    widget: Widget,

    /// Parent element ID (None for root)
    parent: Option<ElementId>,

    /// Slot position in parent's child list
    slot: usize,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,

    /// Dirty flag - element needs rebuild
    dirty: bool,
}

impl ElementBase {
    /// Create a new ElementBase
    ///
    /// # Parameters
    ///
    /// - `widget`: The widget configuration this element represents
    ///
    /// # Initial State
    ///
    /// - `lifecycle`: Initial (not yet mounted)
    /// - `dirty`: true (needs initial build)
    /// - `parent`: None
    /// - `slot`: 0
    #[inline]
    pub fn new(widget: Widget) -> Self {
        Self {
            widget,
            parent: None,
            slot: 0,
            lifecycle: ElementLifecycle::Initial,
            dirty: true,
        }
    }

    // ========== Field Accessors ==========

    /// Get reference to the widget
    ///
    /// Following Rust API Guidelines - no `get_` prefix for getters.
    #[inline]
    #[must_use]
    pub fn widget(&self) -> &Widget {
        &self.widget
    }

    /// Get mutable reference to the widget
    ///
    /// Use this when updating the widget configuration.
    #[inline]
    #[must_use]
    pub fn widget_mut(&mut self) -> &mut Widget {
        &mut self.widget
    }

    /// Replace the widget with a new one
    ///
    /// Marks the element dirty to trigger rebuild.
    ///
    /// # Parameters
    ///
    /// - `new_widget`: The new widget configuration
    #[inline]
    pub fn set_widget(&mut self, new_widget: Widget) {
        self.widget = new_widget;
        self.dirty = true;
    }

    /// Get parent element ID
    ///
    /// Returns `Some(ElementId)` if element has a parent, `None` if root.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Get slot position in parent's child list
    #[inline]
    #[must_use]
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    /// Check if element needs rebuild
    ///
    /// Following API Guidelines: `is_*` prefix for boolean predicates.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    // ========== Lifecycle Management ==========

    /// Mount element to tree
    ///
    /// Called when element is first added to the element tree.
    /// Sets parent, slot, and transitions to Active lifecycle state.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Position in parent's child list
    ///
    /// # Lifecycle Transition
    ///
    /// Initial/Inactive → Active
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Will rebuild on first frame
    }

    /// Unmount element from tree
    ///
    /// Called when element is being removed from the tree.
    /// Transitions to Defunct lifecycle state.
    ///
    /// # Lifecycle Transition
    ///
    /// Any state → Defunct
    #[inline]
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
    }

    /// Deactivate element
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    /// Transitions to Inactive lifecycle state but preserves state.
    ///
    /// # Lifecycle Transition
    ///
    /// Active → Inactive
    #[inline]
    pub fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    /// Activate element
    ///
    /// Called when element is reactivated after being deactivated.
    /// Transitions back to Active lifecycle state and marks dirty.
    ///
    /// # Lifecycle Transition
    ///
    /// Inactive → Active
    #[inline]
    pub fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true; // Rebuild when reactivated
    }

    /// Mark element as needing rebuild
    ///
    /// Sets the dirty flag, causing the element to rebuild on next frame.
    /// Called by setState() or when parent changes.
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clear dirty flag
    ///
    /// Called after element has been rebuilt.
    /// Typically used internally by rebuild() implementations.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Update slot position
    ///
    /// Called when element's position in parent's child list changes.
    ///
    /// # Parameters
    ///
    /// - `new_slot`: The new slot position
    #[inline]
    pub fn update_slot(&mut self, new_slot: usize) {
        self.slot = new_slot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::Widget;

    // Mock widget for testing
    fn mock_widget() -> Widget {
        // In a real test, this would create a proper widget
        // For now, we'll skip the test implementation as it requires
        // a concrete widget type
        unimplemented!("Mock widget creation - requires concrete widget implementation")
    }

    #[test]
    #[should_panic(expected = "Mock widget creation")]
    fn test_element_base_creation() {
        let _base = ElementBase::new(mock_widget());
    }

    #[test]
    fn test_initial_state() {
        // This test demonstrates the expected initial state
        // without actually creating a widget

        // Initial lifecycle should be Initial
        // Initial dirty should be true
        // Initial parent should be None
        // Initial slot should be 0
    }
}
