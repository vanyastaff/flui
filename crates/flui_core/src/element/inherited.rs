//! InheritedElement - element for InheritedWidget
//!
//! Manages data propagation down the tree with efficient dependency tracking.

use std::collections::HashSet;
use std::fmt;

use crate::ElementId;
use crate::widget::{InheritedWidget, BoxedWidget};
use crate::element::{DynElement, ElementLifecycle as PublicLifecycle};
use crate::render::DynRenderObject;

/// Element for InheritedWidget
///
/// InheritedElement stores the widget data and tracks which descendant elements
/// depend on it. When the widget updates, only dependent elements are rebuilt.
///
/// # Architecture
///
/// ```text
/// InheritedElement<Theme>
///   ├─ widget: Theme (the data)
///   ├─ dependents: HashSet<ElementId> (who depends on this)
///   ├─ child_id: ElementId (single child)
///   └─ parent: Option<ElementId>
/// ```
///
/// # Dependency Tracking
///
/// - Descendants call `context.depend_on::<Theme>()` to register dependency
/// - When widget updates, `update_should_notify()` decides if dependents rebuild
/// - Only registered dependents are notified (efficient selective updates)
///
/// # Lifecycle
///
/// 1. **mount()** - Insert into tree
/// 2. **update(new_widget)** - Check if dependents should be notified
/// 3. **unmount()** - Remove from tree, clear dependencies
#[derive(Debug)]
pub struct InheritedElement<W: InheritedWidget> {
    /// The inherited widget containing data
    widget: W,

    /// Set of elements that depend on this InheritedWidget
    ///
    /// When the widget changes, these elements will be marked dirty for rebuild
    /// if `update_should_notify()` returns true.
    dependents: HashSet<ElementId>,

    /// The single child element
    child_id: Option<ElementId>,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,
}

/// Lifecycle states for an element
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementLifecycle {
    /// Element is created but not yet in tree
    Initial,
    /// Element is active in the tree
    Active,
    /// Element has been removed from tree
    Defunct,
}

impl<W: InheritedWidget> InheritedElement<W> {
    /// Create a new InheritedElement
    ///
    /// # Arguments
    ///
    /// - `widget`: The InheritedWidget containing data to propagate
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let element = InheritedElement::new(Theme {
    ///     primary_color: Color::blue(),
    ///     text_size: 16.0,
    /// });
    /// ```
    pub fn new(widget: W) -> Self {
        Self {
            widget,
            dependents: HashSet::new(),
            child_id: None,
            parent: None,
            lifecycle: ElementLifecycle::Initial,
        }
    }

    /// Get reference to the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }

    /// Update the widget and notify dependents if needed
    ///
    /// # Arguments
    ///
    /// - `new_widget`: The new widget to replace the current one
    ///
    /// # Returns
    ///
    /// `true` if dependents should be rebuilt, `false` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let should_notify = element.update(new_theme);
    /// if should_notify {
    ///     // Mark all dependents as dirty
    ///     for &dependent_id in element.dependents() {
    ///         mark_needs_rebuild(dependent_id);
    ///     }
    /// }
    /// ```
    pub fn update(&mut self, new_widget: W) -> bool {
        let should_notify = new_widget.update_should_notify(&self.widget);
        self.widget = new_widget;
        should_notify
    }

    /// Register a dependent element
    ///
    /// Called when a descendant element calls `context.depend_on::<W>()`.
    ///
    /// # Arguments
    ///
    /// - `dependent_id`: The element that depends on this InheritedWidget
    pub fn add_dependent(&mut self, dependent_id: ElementId) {
        self.dependents.insert(dependent_id);
    }

    /// Remove a dependent element
    ///
    /// Called when a dependent element is unmounted or no longer depends on this widget.
    ///
    /// # Arguments
    ///
    /// - `dependent_id`: The element to remove from dependents
    pub fn remove_dependent(&mut self, dependent_id: ElementId) {
        self.dependents.remove(&dependent_id);
    }

    /// Get the set of dependent elements
    pub fn dependents(&self) -> &HashSet<ElementId> {
        &self.dependents
    }

    /// Get the child element ID
    pub fn child(&self) -> Option<ElementId> {
        self.child_id
    }

    /// Set the child element ID
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child_id = Some(child_id);
    }

    /// Remove the child element
    pub(crate) fn forget_child(&mut self, _child_id: ElementId) {
        self.child_id = None;
    }

    /// Get the parent element ID
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set the parent element ID
    pub(crate) fn set_parent(&mut self, parent_id: Option<ElementId>) {
        self.parent = parent_id;
    }

    /// Mount the element into the tree
    pub(crate) fn mount(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
    }

    /// Unmount the element from the tree
    pub(crate) fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        // Clear all dependencies when unmounting
        self.dependents.clear();
    }

    /// Check if element is active
    pub fn is_active(&self) -> bool {
        self.lifecycle == ElementLifecycle::Active
    }

    /// Build the child widget
    ///
    /// Gets the child widget from the InheritedWidget for building the child element.
    pub fn build(&self) -> BoxedWidget {
        self.widget.child()
    }
}

impl<W: InheritedWidget> fmt::Display for InheritedElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "InheritedElement<{}> (dependents: {})",
            std::any::type_name::<W>(),
            self.dependents.len()
        )
    }
}

// ========== DynElement Implementation ==========

impl<W> DynElement for InheritedElement<W>
where
    W: InheritedWidget + crate::Widget + crate::DynWidget,
    W::Element: DynElement,
{
    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child_id.into_iter())
    }

    fn lifecycle(&self) -> PublicLifecycle {
        match self.lifecycle {
            ElementLifecycle::Initial => PublicLifecycle::Initial,
            ElementLifecycle::Active => PublicLifecycle::Active,
            ElementLifecycle::Defunct => PublicLifecycle::Defunct,
        }
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        self.dependents.clear();
    }

    fn deactivate(&mut self) {
        // InheritedElements don't support deactivation - unmount instead
        self.unmount();
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
    }

    fn update_any(&mut self, new_widget: Box<dyn crate::DynWidget>) {
        use crate::DynWidget;
        // Try to downcast to our widget type
        if let Some(new_widget) = (&*new_widget as &dyn std::any::Any).downcast_ref::<W>() {
            self.update(Clone::clone(new_widget));
        }
    }

    fn is_dirty(&self) -> bool {
        // InheritedElements don't have their own dirty state
        // They trigger rebuilds in dependents instead
        false
    }

    fn mark_dirty(&mut self) {
        // No-op for InheritedElements
    }

    fn rebuild(&mut self, _element_id: ElementId) -> Vec<(ElementId, Box<dyn crate::DynWidget>, usize)> {
        // InheritedElements don't rebuild themselves
        // They just hold data and trigger dependent rebuilds
        Vec::new()
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child_id == Some(child_id) {
            self.child_id = None;
        }
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // InheritedElement only has one child, slot is always 0
    }

    // InheritedElement doesn't have RenderObject - use defaults
    // InheritedElement doesn't have RenderState - use defaults
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Widget, DynWidget, impl_widget_for_inherited};

    #[derive(Debug, Clone, PartialEq)]
    struct TestTheme {
        color: u32,
        size: f32,
    }

    impl InheritedWidget for TestTheme {
        fn update_should_notify(&self, old: &Self) -> bool {
            self.color != old.color || self.size != old.size
        }

        fn child(&self) -> BoxedWidget {
            // Return a dummy widget for testing
            Box::new(DummyWidget)
        }
    }

    impl_widget_for_inherited!(TestTheme);

    #[derive(Debug, Clone)]
    struct DummyWidget;

    impl Widget for DummyWidget {
        type Kind = RenderObjectKind;
    }
    impl DynWidget for DummyWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    #[test]
    fn test_inherited_element_creation() {
        let theme = TestTheme { color: 0xFF0000, size: 16.0 };
        let element = InheritedElement::new(theme.clone());

        assert_eq!(element.widget(), &theme);
        assert_eq!(element.dependents().len(), 0);
        assert_eq!(element.child(), None);
        assert!(!element.is_active());
    }

    #[test]
    fn test_inherited_element_mount() {
        let theme = TestTheme { color: 0xFF0000, size: 16.0 };
        let mut element = InheritedElement::new(theme);

        element.mount();
        assert!(element.is_active());
    }

    #[test]
    fn test_inherited_element_dependents() {
        let theme = TestTheme { color: 0xFF0000, size: 16.0 };
        let mut element = InheritedElement::new(theme);

        element.add_dependent(1);
        element.add_dependent(2);
        element.add_dependent(3);

        assert_eq!(element.dependents().len(), 3);
        assert!(element.dependents().contains(&1));
        assert!(element.dependents().contains(&2));
        assert!(element.dependents().contains(&3));

        element.remove_dependent(2);
        assert_eq!(element.dependents().len(), 2);
        assert!(!element.dependents().contains(&2));
    }

    #[test]
    fn test_inherited_element_update_notify() {
        let theme = TestTheme { color: 0xFF0000, size: 16.0 };
        let mut element = InheritedElement::new(theme.clone());

        // Update with same data - should not notify
        let new_theme = TestTheme { color: 0xFF0000, size: 16.0 };
        assert!(!element.update(new_theme));

        // Update with different color - should notify
        let new_theme = TestTheme { color: 0x00FF00, size: 16.0 };
        assert!(element.update(new_theme));

        // Update with different size - should notify
        let new_theme = TestTheme { color: 0x00FF00, size: 18.0 };
        assert!(element.update(new_theme));
    }

    #[test]
    fn test_inherited_element_unmount() {
        let theme = TestTheme { color: 0xFF0000, size: 16.0 };
        let mut element = InheritedElement::new(theme);

        element.add_dependent(1);
        element.add_dependent(2);
        element.mount();

        assert_eq!(element.dependents().len(), 2);
        assert!(element.is_active());

        element.unmount();
        assert!(!element.is_active());
        assert_eq!(element.dependents().len(), 0); // Cleared on unmount
    }
}
