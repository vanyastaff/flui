//! ProviderElement (InheritedElement) - Provides context/inherited data
//!
//! Manages data propagation down the tree with efficient dependency tracking.
//! Per FINAL_ARCHITECTURE_V2.md, renamed from InheritedElement to ProviderElement.

use std::collections::HashSet;

use crate::ElementId;
use crate::element::{ElementBase, ElementLifecycle};
use crate::view::AnyView;
use crate::foundation::Slot;

/// ProviderElement - provides context/inherited data
///
/// Stores view and tracks which descendant elements depend on it.
/// When the view updates, only dependent elements are rebuilt.
///
/// # Architecture (per FINAL_ARCHITECTURE_V2.md)
///
/// ```rust
/// pub struct InheritedElement {
///     base: ElementBase,           // Common fields
///     view: Box<dyn AnyView>,       // View that created this element
///     dependencies: HashSet<ElementId>,
///     child: ElementId,            // NOT Option!
/// }
/// ```
///
/// # Dependency Tracking
///
/// - Descendants call `context.depend_on::<Theme>()` to register dependency
/// - When view updates, `View::rebuild()` decides if dependents rebuild
/// - Only registered dependents are notified (efficient selective updates)
///
/// # Lifecycle
///
/// 1. **mount()** - Insert into tree
/// 2. **rebuild()** - Check if dependents should be notified
/// 3. **unmount()** - Remove from tree, clear dependencies
pub struct InheritedElement {
    /// Common element data (parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// View that created this element
    view: Box<dyn AnyView>,

    /// Set of elements that depend on this provider
    ///
    /// When the view changes, these elements will be marked dirty for rebuild
    /// if `View::rebuild()` indicates changes.
    dependents: HashSet<ElementId>,

    /// The single child element (Option<ElementId> is niche-optimized)
    child: Option<ElementId>,
}

impl std::fmt::Debug for InheritedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InheritedElement")
            .field("base", &self.base)
            .field("view", &"<dyn AnyView>")
            .field("dependents", &self.dependents)
            .field("child", &self.child)
            .finish()
    }
}

impl InheritedElement {
    /// Create a new ProviderElement (InheritedElement)
    ///
    /// # Parameters
    ///
    /// - `view`: The view that created this element
    pub fn new(view: Box<dyn AnyView>) -> Self {
        Self {
            base: ElementBase::new(),
            view,
            dependents: HashSet::new(),
            child: None,
        }
    }

    /// Get reference to the view
    #[inline]
    #[must_use]
    pub fn view(&self) -> &dyn AnyView {
        &*self.view
    }

    // Note: update() method removed - will be replaced with View::rebuild()
    // TODO(Phase 5): Implement proper View-based rebuild

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

    // ========== DynElement-like Interface ==========

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

    // Note: rebuild() method removed - will be replaced with View::rebuild()
    // TODO(Phase 5): Implement proper View-based rebuild

    /// Forget child element
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    /// Update slot for child
    ///
    /// InheritedElement always has slot 0 for its single child, so this is a no-op.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // InheritedElement always has exactly one child at slot 0
        // Nothing to update
    }
}

// ========== ViewElement Implementation ==========

use crate::view::view::ViewElement;
use std::any::Any;

impl ViewElement for InheritedElement {
    fn into_element(self: Box<Self>) -> crate::element::Element {
        crate::element::Element::Provider(*self)
    }

    fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// TODO(Phase 5): Add tests using View API
#[cfg(test)]
mod tests {
    // Tests removed - need to be rewritten with View API
}
