//! ComponentElement - Manages View lifecycle
//!
//! ComponentElement is created by Views and manages their lifecycle.
//! Per FINAL_ARCHITECTURE_V2.md, it stores:
//! - `view: Box<dyn AnyView>` - The view that created this element
//! - `state: Box<dyn Any>` - State that persists across rebuilds
//! - `child: Option<ElementId>` - Single child (niche-optimized, same size as ElementId)
//!
//! # Architecture
//!
//! ComponentElement handles all component views.
//! The View system handles both stateless and stateful cases:
//! - View with State=() → stateless behavior
//! - View with State=T → stateful behavior

use std::any::Any;

use super::{ElementBase, ElementLifecycle};
use crate::foundation::Slot;
use crate::view::AnyView;
use crate::ElementId;

// ============================================================================
// ComponentElement
// ============================================================================

/// ComponentElement - manages View lifecycle
///
/// Per FINAL_ARCHITECTURE_V2.md:
///
/// ```rust
/// pub struct ComponentElement {
///     base: ElementBase,             // 16 bytes (parent, slot, lifecycle, dirty)
///     view: Box<dyn AnyView>,         // 16 bytes
///     state: Box<dyn Any>,            // 16 bytes
///     child: Option<ElementId>,       // 8 bytes (niche-optimized!)
/// }
/// // Total: 56 bytes (Option<ElementId> is same size as ElementId)
/// ```
///
/// # Responsibilities
///
/// - Manages View lifecycle (build, rebuild, teardown)
/// - Stores view-specific state
/// - Delegates to child for rendering
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::{ComponentElement, View, AnyView};
///
/// // Create from a View
/// let view = MyView { count: 0 };
/// let (element, state) = view.build(&mut ctx);
/// let component = ComponentElement::new(
///     Box::new(view),
///     Box::new(state),
/// );
/// ```
pub struct ComponentElement {
    /// Common element data (parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// View that created this element
    view: Box<dyn AnyView>,

    /// State for rebuilding (can be (), HookState, or CustomState)
    state: Box<dyn Any>,

    /// Child element (`Option<ElementId>` is niche-optimized to same size as ElementId)
    child: Option<ElementId>,
}

impl std::fmt::Debug for ComponentElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentElement")
            .field("base", &self.base)
            .field("view", &"<dyn AnyView>")
            .field("state", &"<dyn Any>")
            .field("child", &self.child)
            .finish()
    }
}

impl ComponentElement {
    /// Create a new ComponentElement
    ///
    /// # Parameters
    ///
    /// - `view`: The view that created this element
    /// - `state`: State for rebuilding (typically from View::build())
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let component = ComponentElement::new(
    ///     Box::new(my_view),
    ///     Box::new(my_state),
    /// );
    /// ```
    pub fn new(view: Box<dyn AnyView>, state: Box<dyn Any>) -> Self {
        Self {
            base: ElementBase::new(),
            view,
            state,
            child: None,
        }
    }

    // ========== Field Accessors ==========

    /// Get reference to the view
    #[inline]
    #[must_use]
    pub fn view(&self) -> &dyn AnyView {
        &*self.view
    }

    /// Get mutable reference to state
    ///
    /// State can be downcast to concrete type using `downcast_ref`/`downcast_mut`.
    #[inline]
    #[must_use]
    pub fn state_mut(&mut self) -> &mut dyn Any {
        &mut *self.state
    }

    /// Replace the state with a new value
    ///
    /// Used by the build pipeline to manage HookContext state.
    /// Public to allow app initialization code to set initial HookContext.
    #[inline]
    pub fn set_state(&mut self, state: Box<dyn Any>) {
        self.state = state;
    }

    /// Get child element ID
    ///
    /// Returns `Some(ElementId)` if child exists, `None` otherwise.
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set the child element ID
    ///
    /// Called by ElementTree after mounting the child.
    #[inline]
    pub fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Clear the child
    ///
    /// Sets child to None.
    #[inline]
    pub fn clear_child(&mut self) {
        self.child = None;
    }

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Get lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Check if element is dirty
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as dirty (needs rebuild)
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    // ========== Lifecycle Management ==========

    /// Mount element to tree
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Position in parent's child list (None for root)
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree
    #[inline]
    pub fn unmount(&mut self) {
        self.base.unmount();
    }

    /// Deactivate element (move to cache)
    #[inline]
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate element (restore from cache)
    #[inline]
    pub fn activate(&mut self) {
        self.base.activate();
    }

    // ========== Children Management ==========

    /// Get iterator over children
    ///
    /// ComponentElement has at most one child.
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        if let Some(child) = self.child() {
            Box::new(std::iter::once(child))
        } else {
            Box::new(std::iter::empty())
        }
    }

    /// Forget a child (called when child is unmounted)
    ///
    /// Removes child from internal storage without unmounting it.
    pub fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.clear_child();
        }
    }

    /// Update slot for a child
    ///
    /// No-op for ComponentElement since it has at most one child with no slot.
    pub fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // ComponentElement's single child doesn't use slot
    }

    // ========== View Rebuild ==========

    /// Handle an event
    ///
    /// ComponentElements typically don't need to handle events directly,
    /// as their child elements will handle them. However, this can be overridden
    /// if needed for special component behavior.
    ///
    /// Default implementation: does not handle events (returns false)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// match event {
    ///     Event::Window(WindowEvent::FocusChanged { focused }) => {
    ///         // Custom focus handling for this component
    ///         true
    ///     }
    ///     _ => false
    /// }
    /// ```
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false // ComponentElements don't handle events by default
    }

    /// Hit test - delegate to child
    ///
    /// ComponentElements don't have bounds themselves, they just wrap their child.
    /// Hit testing is delegated to the child element.
    ///
    /// Returns the child ElementId that should be tested (if any).
    #[inline]
    pub fn hit_test_child(&self) -> Option<ElementId> {
        self.child()
    }
}

// ========== ViewElement Implementation ==========

use crate::view::view::ViewElement;

impl ViewElement for ComponentElement {
    fn into_element(self: Box<Self>) -> crate::element::Element {
        crate::element::Element::Component(*self)
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

// ========== Debug Implementation ==========

// Debug is derived, but we could customize it if needed

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Component element creation is tested indirectly through integration tests
    // Direct unit tests require concrete View implementations which are in flui_widgets

    #[test]
    fn test_child_sentinel() {
        use crate::testing::TestWidget;
        let view: Box<dyn AnyView> = Box::new(TestWidget);
        let state: Box<dyn Any> = Box::new(());

        let component = ComponentElement::new(view, state);

        // Initially no child
        assert_eq!(component.child(), None);
        assert_eq!(component.child, None);
    }
}
