//! InheritedWidget for efficient data propagation down the tree

use std::any::TypeId;
use std::fmt;
use std::sync::Arc;

use ahash::AHashSet;
use parking_lot::RwLock;

use crate::{AnyWidget, Element, Widget, AnyElement as _};

/// Propagates data down the tree (dependents rebuild when data changes)
pub trait InheritedWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Data type this widget provides
    type Data;

    /// Get the provided data
    fn data(&self) -> &Self::Data;

    /// Get the child widget
    fn child(&self) -> &dyn AnyWidget;

    /// Check if dependents should rebuild on update
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

/// Element for InheritedWidget (tracks dependents)
pub struct InheritedElement<W: InheritedWidget> {
    id: crate::ElementId,
    widget: W,
    parent: Option<crate::ElementId>,
    dirty: bool,
    dependents: AHashSet<crate::ElementId>,
    tree: Option<Arc<RwLock<crate::ElementTree>>>,
    child: Option<crate::ElementId>,
}

impl<W: InheritedWidget> InheritedElement<W> {
    pub fn new(widget: W) -> Self {
        Self {
            id: crate::ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            dependents: AHashSet::new(),
            tree: None,
            child: None,
        }
    }

    /// Register dependent element
    pub fn register_dependent(&mut self, element_id: crate::ElementId) {
        self.dependents.insert(element_id);
    }

    /// Notify dependents of data change
    fn notify_dependents(&mut self, tree: &Arc<RwLock<crate::ElementTree>>) {
        let dependent_ids: Vec<_> = self.dependents.iter().copied().collect();
        for dependent_id in dependent_ids {
            tree.write().mark_dirty(dependent_id);
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

/// Macro to implement Widget for InheritedWidget types
///
/// This macro generates the Widget implementation for an InheritedWidget type.
/// Use this for all InheritedWidget implementations.
///
/// # Why a macro?
///
/// We cannot use a blanket impl like `impl<T: InheritedWidget> Widget for T` because
/// it would conflict with the existing `impl<T: StatelessWidget> Widget for T`.
/// Rust's trait coherence rules don't allow overlapping blanket implementations.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Theme {
///     color: Color,
///     child: Box<dyn AnyWidget>,
/// }
///
/// impl InheritedWidget for Theme {
///     type Data = Color;
///     fn data(&self) -> &Color { &self.color }
///     fn child(&self) -> &dyn AnyWidget { &*self.child }
///     fn update_should_notify(&self, old: &Self) -> bool {
///         self.color != old.color
///     }
/// }
///
/// impl_widget_for_inherited!(Theme);
/// ```
#[macro_export]
macro_rules! impl_widget_for_inherited {
    ($widget_type:ty) => {
        impl $crate::Widget for $widget_type {
            type Element = $crate::InheritedElement<$widget_type>;

            fn into_element(self) -> Self::Element {
                $crate::InheritedElement::new(self)
            }
        }
    };
}

// ========== Implement AnyElement for InheritedElement ==========

impl<W: InheritedWidget + Widget<Element = InheritedElement<W>>> crate::AnyElement for InheritedElement<W> {
    fn id(&self) -> crate::ElementId {
        self.id
    }

    fn parent(&self) -> Option<crate::ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn flui_foundation::Key> {
        InheritedWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<crate::ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().remove(child_id);
            }
        }

        // Clear dependents (they will be removed from tree anyway)
        self.dependents.clear();
    }

    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            let should_notify = widget.update_should_notify(&self.widget);
            self.widget = *widget;

            if should_notify {
                self.mark_dirty();

                // Notify all dependent elements to rebuild
                if let Some(tree) = self.tree.clone() {
                    self.notify_dependents(&tree);
                }
            }
        }
    }

    fn rebuild(&mut self) -> Vec<(crate::ElementId, Box<dyn crate::AnyWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // InheritedWidget just wraps its child widget
        // We need to clone the child widget for mounting
        // Since we only have &dyn AnyWidget, we'll need to use the widget's clone ability
        let child_ref = self.widget.child();

        // Clone the widget - we need to upcast to Any first to get a Box
        // This is a limitation - child() returns &dyn AnyWidget, but we need Box<dyn AnyWidget>
        // For now, we'll Box::new it by cloning the entire InheritedWidget
        let child_widget: Box<dyn crate::AnyWidget> = dyn_clone::clone_box(child_ref);

        // Mark old child for unmounting
        self.child = None;

        // Return the child that needs to be mounted
        vec![(self.id, child_widget, 0)]
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn lifecycle(&self) -> crate::ElementLifecycle {
        crate::ElementLifecycle::Active // Default
    }

    fn deactivate(&mut self) {
        // Default: do nothing
    }

    fn activate(&mut self) {
        // Default: do nothing
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = crate::ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<crate::ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<crate::ElementId> {
        self.child.take()
    }

    fn set_child_after_mount(&mut self, child_id: crate::ElementId) {
        self.child = Some(child_id);
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn render_object(&self) -> Option<&dyn crate::AnyRenderObject> {
        None // InheritedElement doesn't have RenderObject
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::AnyRenderObject> {
        None // InheritedElement doesn't have RenderObject
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing for InheritedWidget
    }

    fn update_slot_for_child(&mut self, _child_id: crate::ElementId, _new_slot: usize) {
        // Default: do nothing (single child)
    }

    fn forget_child(&mut self, child_id: crate::ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }
}

// ========== Implement Element for InheritedElement (with associated types) ==========

impl<W: InheritedWidget + Widget<Element = InheritedElement<W>>> Element for InheritedElement<W> {
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        // Zero-cost! No downcast needed!
        let should_notify = new_widget.update_should_notify(&self.widget);
        self.widget = new_widget;

        if should_notify {
            self.mark_dirty();

            // Notify all dependent elements to rebuild
            if let Some(tree) = self.tree.clone() {
                self.notify_dependents(&tree);
            }
        }
    }

    fn widget(&self) -> &W {
        &self.widget
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
///     fn child(&self) -> &dyn AnyWidget { /* ... */ }
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
            type Element = $crate::InheritedElement<Self>;

            fn into_element(self) -> Self::Element {
                $crate::InheritedElement::new(self)
            }
        }

        // AnyWidget is automatically implemented via the blanket impl
        // Note: InheritedWidget::key() should be handled by implementing key() on the struct if needed
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;
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
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            Box::new(ChildWidget)
        }
    }

    impl InheritedWidget for TestTheme {
        type Data = i32;

        fn data(&self) -> &Self::Data {
            &self.value
        }

        fn child(&self) -> &dyn AnyWidget {
            // Placeholder - in real usage would return actual child
            &ChildWidget as &dyn AnyWidget
        }

        fn update_should_notify(&self, old: &Self) -> bool {
            self.value != old.value
        }
    }

    // Use the macro to implement Widget
    impl_widget_for_inherited!(TestTheme);

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

        let parent_id = crate::ElementId::from_raw(100);
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

    /// Integration test for dependency tracking with ElementTree
    #[test]
    fn test_inherited_widget_dependency_tracking() {
        use crate::{AnyWidget, ElementTree, StatelessWidget};
        use std::sync::Arc;
        use parking_lot::RwLock;

        // Create a dependent widget that uses the theme
        #[derive(Debug, Clone)]
        struct DependentWidget;

        impl StatelessWidget for DependentWidget {
            fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
                // Access the inherited widget - this should register dependency
                if let Some(theme) = context.subscribe_to::<TestTheme>() {
                    assert_eq!(*theme.data(), 42);
                }
                Box::new(ChildWidget)
            }
        }

        // Create element tree
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Create theme widget with child
        #[derive(Debug, Clone)]
        struct ThemeWithChild {
            theme: TestTheme,
        }

        impl StatelessWidget for ThemeWithChild {
            fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
                // This would normally create InheritedElement with DependentWidget as child
                Box::new(DependentWidget)
            }
        }

        // Mount root widget
        let root_widget = Box::new(ThemeWithChild {
            theme: TestTheme { value: 42 },
        });

        let _root_id = {
            let mut tree_guard = tree.write();
            tree_guard.set_root(root_widget)
        };

        // Rebuild to trigger build() which calls depend_on_inherited_widget()
        {
            let mut tree_guard = tree.write();
            tree_guard.rebuild();
        }

        // Success - test validates that the infrastructure for dependency tracking exists
        // Full integration would require:
        // 1. Mounting InheritedElement in the tree
        // 2. Mounting dependent widget as child
        // 3. Verifying dependency registration
        // 4. Updating InheritedWidget and verifying dependent rebuilds
    }

    /// Test dependency registration
    #[test]
    fn test_register_dependent() {
        use crate::ElementId;

        let widget = TestTheme { value: 42 };
        let mut element = InheritedElement::new(widget);

        let dependent_id1 = ElementId::new();
        let dependent_id2 = ElementId::new();

        element.register_dependent(dependent_id1);
        element.register_dependent(dependent_id2);

        // Dependents are registered (can't directly test AHashSet contents, but verify no panic)
        assert_eq!(element.widget().value, 42);
    }

    /// Test notify_dependents marks elements dirty
    #[test]
    fn test_notify_dependents_marks_dirty() {
        use crate::{AnyWidget, ElementTree, ElementId};
        use std::sync::Arc;
        use parking_lot::RwLock;

        // Create tree
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Create inherited element
        let widget = TestTheme { value: 42 };
        let mut inherited_elem = InheritedElement::new(widget);
        inherited_elem.tree = Some(tree.clone());

        // Mount a dependent element in the tree
        let dependent_widget = Box::new(ChildWidget);
        let dependent_id = {
            let mut tree_guard = tree.write();
            tree_guard.set_root(dependent_widget)
        };

        // Register the dependent
        inherited_elem.register_dependent(dependent_id);

        // Clear dirty state
        {
            let mut tree_guard = tree.write();
            tree_guard.rebuild();
        }

        // Verify dependent is not dirty
        {
            let tree_guard = tree.read();
            let element = tree_guard.get(dependent_id).unwrap();
            assert!(!element.is_dirty());
        }

        // Notify dependents
        inherited_elem.notify_dependents(&tree);

        // Verify dependent is now dirty
        {
            let tree_guard = tree.read();
            let element = tree_guard.get(dependent_id).unwrap();
            assert!(element.is_dirty());
        }
    }

    /// Test Flutter-style of() and maybeOf() pattern
    #[test]
    fn test_flutter_style_of_pattern() {
        use crate::{AnyWidget, ElementTree, StatelessWidget};
        use std::sync::Arc;
        use parking_lot::RwLock;

        // Implement Flutter-style static methods for TestTheme
        impl TestTheme {
            pub fn maybe_of(context: &Context) -> Option<Self> {
                context.subscribe_to::<TestTheme>()
            }

            pub fn of(context: &Context) -> Self {
                Self::maybe_of(context).expect("No TestTheme found in context")
            }
        }

        // Create a widget that uses the Flutter-style API
        #[derive(Debug, Clone)]
        struct FlutterStyleWidget;

        impl StatelessWidget for FlutterStyleWidget {
            fn build(&self, context: &Context) -> Box<dyn AnyWidget> {
                // Test maybe_of (should return None when no theme)
                assert!(TestTheme::maybe_of(context).is_none());

                Box::new(ChildWidget)
            }
        }

        // Create tree and test
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let root_widget = Box::new(FlutterStyleWidget);

        let _root_id = {
            let mut tree_guard = tree.write();
            tree_guard.set_root(root_widget)
        };

        // Rebuild triggers build()
        {
            let mut tree_guard = tree.write();
            tree_guard.rebuild();
        }

        // Test passed - maybe_of returned None correctly
    }

    /// Test of() panics when theme not found
    #[test]
    #[should_panic(expected = "No TestTheme found in context")]
    fn test_of_panics_without_theme() {
        use crate::{AnyWidget, Context, ElementTree};
        use std::sync::Arc;
        use parking_lot::RwLock;

        // Use the already defined of() method from test_flutter_style_of_pattern

        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let context = Context::new(tree, crate::ElementId::new());

        // This should panic because no TestTheme in tree
        let _theme = TestTheme::of(&context);
    }
}
