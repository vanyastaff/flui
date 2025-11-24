//! Element type for component views.

use std::any::Any;

use crate::element::{Element, ElementBase, ElementLifecycle};
use crate::foundation::Slot;
use crate::ElementId;

/// Build function that produces a child element.
///
/// Captures the view and calls `view.build()` with thread-local [`BuildContext`].
///
/// [`BuildContext`]: crate::view::BuildContext
pub type BuildFn = Box<dyn Fn() -> Element + Send + Sync>;

/// Element that manages a [`View`]'s lifecycle.
///
/// Stores a build function and hook state that persists across rebuilds.
///
/// # Memory layout
///
/// ```text
/// ViewElement {
///     base: ElementBase,         // 16 bytes
///     builder: BuildFn,          // 16 bytes
///     state: Box<dyn Any + Send>, // 16 bytes (HookContext)
///     child: Option<ElementId>,  // 8 bytes (niche-optimized)
/// }
/// // Total: 56 bytes
/// ```
///
/// [`View`]: crate::view::View
pub struct ViewElement {
    /// Base element data (lifecycle, parent, slot)
    pub base: ElementBase,
    builder: BuildFn,
    state: Box<dyn Any + Send>,
    child: Option<ElementId>,
}

impl std::fmt::Debug for ViewElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewElement")
            .field("base", &self.base)
            .field("builder", &"<BuildFn>")
            .field("state", &"<dyn Any>")
            .field("child", &self.child)
            .finish()
    }
}

impl ViewElement {
    /// Creates a new `ViewElement` with the given build function.
    pub fn new(builder: BuildFn) -> Self {
        Self {
            base: ElementBase::new(),
            builder,
            state: Box::new(()),
            child: None,
        }
    }

    /// Calls the build function to produce a child element.
    #[inline]
    pub fn build(&self) -> Element {
        (self.builder)()
    }

    /// Returns a mutable reference to the state.
    ///
    /// State is typically a `HookContext` that can be downcast.
    #[inline]
    #[must_use]
    pub fn state_mut(&mut self) -> &mut dyn Any {
        &mut *self.state
    }

    /// Replaces the state.
    #[inline]
    pub fn set_state(&mut self, state: Box<dyn Any + Send>) {
        self.state = state;
    }

    /// Returns the child element ID, if any.
    #[inline]
    #[must_use]
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Sets the child element ID.
    #[inline]
    pub fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Clears the child.
    #[inline]
    pub fn clear_child(&mut self) {
        self.child = None;
    }

    /// Returns the parent element ID, if any.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Returns the current lifecycle state.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Returns `true` if this element needs rebuild.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Marks this element as needing rebuild.
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Clears the dirty flag after successful rebuild.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.base.clear_dirty();
    }

    /// Mounts this element to the tree.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmounts this element from the tree.
    #[inline]
    pub fn unmount(&mut self) {
        self.base.unmount();
    }

    /// Deactivates this element (moves to cache).
    #[inline]
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activates this element (restores from cache).
    #[inline]
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Returns an iterator over child element IDs.
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        match self.child {
            Some(child) => Box::new(std::iter::once(child)),
            None => Box::new(std::iter::empty()),
        }
    }

    /// Removes a child from internal storage without unmounting it.
    pub fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.clear_child();
        }
    }

    /// Updates slot for a child. No-op for `ViewElement`.
    pub fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {}

    /// Handles an event. Returns `false` (delegates to child).
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false
    }

    /// Returns the child to hit test, if any.
    #[inline]
    pub fn hit_test_child(&self) -> Option<ElementId> {
        self.child
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::BuildContext;
    use std::any::Any;

    // Mock ViewObject for testing
    struct MockViewObject;

    impl crate::view::ViewObject for MockViewObject {
        fn mode(&self) -> crate::view::ViewMode {
            crate::view::ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &BuildContext) -> crate::element::Element {
            // Return a simple element
            crate::element::Element::new(Box::new(MockViewObject))
        }

        fn init(&mut self, _ctx: &BuildContext) {}
        fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}
        fn did_update(&mut self, _new_view: &dyn Any, _ctx: &BuildContext) {}
        fn deactivate(&mut self, _ctx: &BuildContext) {}
        fn dispose(&mut self, _ctx: &BuildContext) {}

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_view_element_creation() {
        let builder: BuildFn = Box::new(|| crate::element::Element::new(Box::new(MockViewObject)));

        let component = ViewElement::new(builder);
        assert_eq!(component.child(), None);
        assert!(component.is_dirty());
    }
}
