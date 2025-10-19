//! InheritedWidget - efficient data propagation down the tree
//!
//! InheritedWidgets provide a way to propagate data down the widget tree efficiently.
//! They are similar to React's Context or Flutter's InheritedWidget.

use std::any::{Any, TypeId};
use std::fmt;

use crate::{Element, Widget};

/// InheritedWidget - propagates data down the widget tree
///
/// Similar to Flutter's InheritedWidget. Widgets below this widget in the tree
/// can access its data efficiently using `BuildContext::depend_on_inherited_widget()`.
///
/// When the data changes, only widgets that actually depend on it will rebuild.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Theme {
///     primary_color: Color,
///     child: Box<dyn Widget>,
/// }
///
/// impl InheritedWidget for Theme {
///     type Data = Color;
///
///     fn data(&self) -> &Self::Data {
///         &self.primary_color
///     }
///
///     fn child(&self) -> &dyn Widget {
///         &*self.child
///     }
///
///     fn update_should_notify(&self, old: &Self) -> bool {
///         self.primary_color != old.primary_color
///     }
/// }
///
/// // Access the theme from descendant widgets:
/// impl StatelessWidget for MyButton {
///     fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
///         let theme = context.depend_on_inherited_widget::<Theme>().unwrap();
///         // Use theme.data()...
///     }
/// }
/// ```
pub trait InheritedWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Associated data type that this widget provides
    type Data;

    /// Get the data this widget provides
    fn data(&self) -> &Self::Data;

    /// Get the child widget
    ///
    /// The child and all its descendants can access this inherited widget's data.
    fn child(&self) -> &dyn Widget;

    /// Check if dependents should be notified of changes
    ///
    /// Called when the widget is updated. Return true if widgets that depend on
    /// this inherited widget should rebuild.
    ///
    /// # Parameters
    /// - `old`: The previous version of this widget (same type as Self)
    ///
    /// # Returns
    /// - `true` if dependents should rebuild
    /// - `false` if the data hasn't changed meaningfully
    fn update_should_notify(&self, old: &Self) -> bool;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn flui_foundation::Key> {
        None
    }

    /// Get TypeId for this specific inherited widget type
    ///
    /// Used to look up the correct inherited widget in the tree.
    fn inherited_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// Element for InheritedWidget
///
/// Manages lifecycle of inherited widgets and tracks dependents.
pub struct InheritedElement<W: InheritedWidget> {
    id: crate::ElementId,
    widget: W,
    parent: Option<crate::ElementId>,
    dirty: bool,
    /// Elements that depend on this InheritedWidget
    /// When data changes, these elements will be marked dirty
    dependents: std::collections::HashSet<crate::ElementId>,
}

impl<W: InheritedWidget> InheritedElement<W> {
    /// Create new inherited element
    pub fn new(widget: W) -> Self {
        Self {
            id: crate::ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            dependents: std::collections::HashSet::new(),
        }
    }

    /// Register an element as dependent on this InheritedWidget
    ///
    /// When the data changes, registered dependents will be marked dirty.
    pub fn register_dependent(&mut self, element_id: crate::ElementId) {
        self.dependents.insert(element_id);
    }

    /// Notify all dependents that data has changed
    ///
    /// Marks all dependent elements as dirty so they will rebuild.
    fn notify_dependents(&mut self, tree: &std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>) {
        for &dependent_id in &self.dependents {
            let mut tree_guard = tree.write();
            tree_guard.mark_element_dirty(dependent_id);
        }
    }

    /// Get the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }
}

impl<W: InheritedWidget> fmt::Debug for InheritedElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InheritedElement")
            .field("id", &self.id)
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl<W: InheritedWidget> Element for InheritedElement<W> {
    fn mount(&mut self, parent: Option<crate::ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
        // TODO: Mount child element
    }

    fn unmount(&mut self) {
        // TODO: Unmount child and notify dependents
    }

    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>) {
        if let Ok(new_w) = new_widget.downcast::<W>() {
            let should_notify = new_w.update_should_notify(&self.widget);
            self.widget = *new_w;

            if should_notify {
                // TODO: Notify dependent elements to rebuild
                self.mark_dirty();
            }
        }
    }

    fn rebuild(&mut self) -> Vec<(crate::ElementId, Box<dyn crate::Widget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;
        // TODO: Rebuild child
        Vec::new()
    }

    fn id(&self) -> crate::ElementId {
        self.id
    }

    fn parent(&self) -> Option<crate::ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn flui_foundation::Key> {
        self.widget.key()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

// Note: Widget is NOT automatically implemented for InheritedWidget
// Users must implement Widget manually for their InheritedWidget types
// This is intentional to avoid conflicting blanket implementations

/// Helper macro to implement Widget for InheritedWidget types
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyTheme {
///     primary_color: Color,
/// }
///
/// impl InheritedWidget for MyTheme {
///     type Data = Color;
///     fn data(&self) -> &Self::Data { &self.primary_color }
///     fn child(&self) -> &dyn Widget { /* ... */ }
///     fn update_should_notify(&self, old: &Self) -> bool { /* ... */ }
/// }
///
/// // Automatically implement Widget trait:
/// impl_inherited_widget!(MyTheme);
/// ```
#[macro_export]
macro_rules! impl_inherited_widget {
    ($ty:ty) => {
        impl $crate::Widget for $ty {
            fn create_element(&self) -> Box<dyn $crate::Element> {
                Box::new($crate::InheritedElement::new(self.clone()))
            }

            fn key(&self) -> Option<&dyn flui_foundation::Key> {
                <Self as $crate::InheritedWidget>::key(self)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildContext;
    use crate::StatelessWidget;

    // Test inherited widget
    #[derive(Debug, Clone, PartialEq)]
    struct TestTheme {
        value: i32,
    }

    // Dummy child widget
    #[derive(Debug, Clone)]
    struct ChildWidget;

    impl StatelessWidget for ChildWidget {
        fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
            Box::new(ChildWidget)
        }
    }

    impl InheritedWidget for TestTheme {
        type Data = i32;

        fn data(&self) -> &Self::Data {
            &self.value
        }

        fn child(&self) -> &dyn Widget {
            // Placeholder - in real usage would return actual child
            &ChildWidget as &dyn Widget
        }

        fn update_should_notify(&self, old: &Self) -> bool {
            self.value != old.value
        }
    }

    // Use the macro to implement Widget
    impl_inherited_widget!(TestTheme);

    #[test]
    fn test_inherited_widget_create_element() {
        let widget = TestTheme { value: 42 };
        let element = widget.create_element();

        assert!(element.is_dirty());
    }

    #[test]
    fn test_inherited_widget_update_should_notify() {
        let widget1 = TestTheme { value: 1 };
        let widget2 = TestTheme { value: 2 };
        let widget3 = TestTheme { value: 2 };

        assert!(widget2.update_should_notify(&widget1));
        assert!(!widget3.update_should_notify(&widget2));
    }

    #[test]
    fn test_inherited_widget_data() {
        let widget = TestTheme { value: 42 };
        assert_eq!(*widget.data(), 42);
    }

    #[test]
    fn test_inherited_element_mount() {
        let widget = TestTheme { value: 42 };
        let mut element = InheritedElement::new(widget);

        let parent_id = crate::ElementId(100);
        element.mount(Some(parent_id), 0);

        assert_eq!(element.parent(), Some(parent_id));
        assert!(element.is_dirty());
    }

    #[test]
    fn test_inherited_element_update() {
        let widget1 = TestTheme { value: 1 };
        let mut element = InheritedElement::new(widget1);
        element.dirty = false;

        let widget2 = TestTheme { value: 2 };
        element.update(Box::new(widget2.clone()));

        assert_eq!(element.widget().value, 2);
        assert!(element.is_dirty()); // Should be dirty because value changed
    }

    #[test]
    fn test_inherited_element_update_no_notify() {
        let widget1 = TestTheme { value: 1 };
        let mut element = InheritedElement::new(widget1);
        element.dirty = false;

        let widget2 = TestTheme { value: 1 }; // Same value
        element.update(Box::new(widget2));

        assert_eq!(element.widget().value, 1);
        assert!(!element.is_dirty()); // Should not be dirty because value didn't change
    }
}
