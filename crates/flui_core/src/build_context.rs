//! Build context for accessing the element tree
//!
//! This module provides BuildContext, which is passed to widget build methods
//! to provide access to the element tree and framework services.
//!
//! # Overview
//!
//! BuildContext is the primary way widgets interact with the framework. It provides:
//!
//! - **Element Tree Access**: Navigate the element hierarchy
//! - **Ancestor Lookup**: Find ancestor widgets and elements
//! - **InheritedWidget Access**: Efficiently access data from ancestor InheritedWidgets
//! - **Rebuild Scheduling**: Mark elements as needing rebuild
//!
//! # Example
//!
//! ```rust,ignore
//! impl StatelessWidget for MyWidget {
//!     fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
//!         // Access inherited widget data
//!         let theme = context.depend_on_inherited_widget::<Theme>();
//!
//!         // Get size after layout
//!         if let Some(size) = context.size() {
//!             println!("My size: {:?}", size);
//!         }
//!
//!         // Find ancestor widget
//!         let parent = context.find_ancestor_widget_of_type::<Container>();
//!
//!         // Build child widgets
//!         Box::new(Text::new("Hello"))
//!     }
//! }
//! ```

use std::any::TypeId;
use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::{Element, ElementId, ElementTree, InheritedWidget, Size, Widget};

/// Build context provides access to the element tree and services
///
/// Similar to Flutter's BuildContext. Passed to build() methods to provide
/// access to the framework.
///
/// BuildContext is cheap to clone - it contains only references to shared data.
///
/// # Lifetime
///
/// A BuildContext is only valid during the build phase. Do not store it for
/// later use, as the element tree may have changed.
///
/// # Thread Safety
///
/// BuildContext is Send + Sync, but the underlying ElementTree uses RwLock
/// for interior mutability. This means:
/// - Multiple readers can access the tree concurrently
/// - Writers block all readers and other writers
#[derive(Clone)]
pub struct BuildContext {
    /// Reference to the element tree
    tree: Arc<RwLock<ElementTree>>,

    /// ID of the current element
    element_id: ElementId,
}

impl BuildContext {
    /// Create a new build context
    ///
    /// This is an internal API used by the framework.
    ///
    /// # Parameters
    ///
    /// - `tree`: Shared reference to the element tree
    /// - `element_id`: ID of the element this context belongs to
    pub fn new(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    /// Create an empty build context
    ///
    /// This creates a context with an empty tree and dummy element ID.
    /// Use this only when you don't have access to a real ElementTree
    /// (e.g., in incomplete implementations or tests).
    ///
    /// # Warning
    ///
    /// This context will not be able to access any elements or perform
    /// meaningful operations. It's primarily for satisfying type requirements
    /// during development.
    pub fn empty() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();
        Self { tree, element_id }
    }

    /// Create a minimal build context for testing
    ///
    /// This creates a context with an empty tree and dummy element ID.
    /// Use this only in tests.
    #[cfg(test)]
    pub fn test() -> Self {
        Self::empty()
    }

    /// Get the element ID this context belongs to
    ///
    /// # Returns
    ///
    /// The ElementId of the element this context was created for
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get a reference to the element tree
    ///
    /// This is an internal API. Most widgets should use higher-level methods
    /// like `find_ancestor_widget_of_type` instead.
    ///
    /// # Returns
    ///
    /// Read lock on the element tree
    ///
    /// # Note
    ///
    /// Uses parking_lot::RwLock which provides deadlock-free locking and
    /// better performance than std::sync::RwLock.
    pub fn tree(&self) -> parking_lot::RwLockReadGuard<'_, ElementTree> {
        self.tree.read()
    }

    /// Get a mutable reference to the element tree
    ///
    /// This is an internal API used by the framework.
    ///
    /// # Returns
    ///
    /// Write lock on the element tree
    ///
    /// # Note
    ///
    /// Uses parking_lot::RwLock which provides deadlock-free locking and
    /// better performance than std::sync::RwLock.
    pub(crate) fn tree_mut(&self) -> parking_lot::RwLockWriteGuard<'_, ElementTree> {
        self.tree.write()
    }

    /// Mark the current element as needing rebuild
    ///
    /// Similar to Flutter's `setState()` for StatefulWidget.
    /// Schedules this element to be rebuilt on the next frame.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// context.mark_needs_build();
    /// ```
    pub fn mark_needs_build(&self) {
        let mut tree = self.tree_mut();
        tree.mark_element_dirty(self.element_id);
    }

    /// Get the parent element ID
    ///
    /// # Returns
    ///
    /// The parent element ID, or None if this is the root element
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree();
        tree.get_element(self.element_id)
            .and_then(|element| element.parent())
    }

    /// Visit ancestor elements (going up the tree)
    ///
    /// Calls the visitor function for each ancestor element, starting with the
    /// immediate parent and moving up to the root.
    ///
    /// The visitor returns `true` to continue visiting, or `false` to stop.
    ///
    /// # Parameters
    ///
    /// - `visitor`: Function called for each ancestor
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// context.visit_ancestor_elements(&mut |element| {
    ///     println!("Ancestor: {:?}", element.id());
    ///     true // Continue visiting
    /// });
    /// ```
    pub fn visit_ancestor_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element) -> bool,
    {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get_element(id) {
                if !visitor(element) {
                    break;
                }
                current_id = element.parent();
            } else {
                break;
            }
        }
    }

    /// Find the nearest ancestor widget of a specific type
    ///
    /// Searches up the tree for the first ancestor widget that matches the type `W`.
    /// This is useful for accessing configuration from parent widgets.
    ///
    /// # Type Parameters
    ///
    /// - `W`: The widget type to search for
    ///
    /// # Returns
    ///
    /// Reference to the widget if found, None otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(scaffold) = context.find_ancestor_widget_of_type::<Scaffold>() {
    ///     // Use scaffold...
    /// }
    /// ```
    pub fn find_ancestor_widget_of_type<W: Widget + 'static>(&self) -> Option<W> {
        let tree = self.tree();

        let mut current_id = self.parent();
        while let Some(id) = current_id {
            if let Some(element) = tree.get_element(id) {
                // Try to get the widget from the element
                // Note: This is a simplified version. In a full implementation,
                // we'd need a way to get the widget from any element type.
                // For now, we'll return None as we don't have a generic way
                // to access the widget from an element.

                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }

    /// Find the nearest ancestor element of a specific type
    ///
    /// Searches up the tree for the first ancestor element that matches the type `E`.
    ///
    /// # Type Parameters
    ///
    /// - `E`: The element type to search for
    ///
    /// # Returns
    ///
    /// Reference to the element if found, None otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(element) = context.find_ancestor_element_of_type::<RenderObjectElement>() {
    ///     // Use element...
    /// }
    /// ```
    pub fn find_ancestor_element_of_type<E: Element + 'static>(&self) -> Option<ElementId> {
        let tree = self.tree();
        let mut result = None;

        let mut current_id = self.parent();
        while let Some(id) = current_id {
            if let Some(element) = tree.get_element(id) {
                if element.is::<E>() {
                    result = Some(id);
                    break;
                }
                current_id = element.parent();
            } else {
                break;
            }
        }

        result
    }

    /// Access an InheritedWidget's data from the tree
    ///
    /// Searches up the tree for an InheritedWidget of type `W` and returns its data.
    /// This establishes a dependency - when the InheritedWidget's data changes,
    /// this element will be rebuilt.
    ///
    /// Similar to Flutter's `context.dependOnInheritedWidgetOfExactType<T>()`.
    ///
    /// # Type Parameters
    ///
    /// - `W`: The InheritedWidget type to search for
    ///
    /// # Returns
    ///
    /// Reference to the widget if found, None otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(theme) = context.depend_on_inherited_widget::<Theme>() {
    ///     let color = theme.data().primary_color;
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// Currently, this does not track dependencies. In a full implementation,
    /// we would register this element as a dependent so it rebuilds when the
    /// InheritedWidget changes.
    pub fn depend_on_inherited_widget<W: InheritedWidget + Clone + 'static>(&self) -> Option<W> {
        self.get_inherited_widget_impl::<W>(TypeId::of::<W>(), true)
    }

    /// Access an InheritedWidget without establishing a dependency
    ///
    /// Similar to `depend_on_inherited_widget`, but does NOT establish a dependency.
    /// This element will not rebuild when the InheritedWidget changes.
    ///
    /// Use this when you only need to read the data once and don't need updates.
    ///
    /// # Type Parameters
    ///
    /// - `W`: The InheritedWidget type to search for
    ///
    /// # Returns
    ///
    /// Reference to the widget if found, None otherwise
    pub fn get_inherited_widget<W: InheritedWidget + Clone + 'static>(&self) -> Option<W> {
        self.get_inherited_widget_impl::<W>(TypeId::of::<W>(), false)
    }

    /// Internal implementation for getting inherited widgets
    fn get_inherited_widget_impl<W: InheritedWidget + Clone + 'static>(
        &self,
        _type_id: TypeId,
        register_dependency: bool,
    ) -> Option<W> {
        use crate::InheritedElement;

        let tree = self.tree();
        let mut current_id = self.parent();

        // Walk up the tree looking for InheritedElement<W>
        while let Some(id) = current_id {
            if let Some(element) = tree.get_element(id) {
                // Try to downcast to InheritedElement<W>
                if let Some(inherited_elem) = element.downcast_ref::<InheritedElement<W>>() {
                    // Found matching InheritedWidget!

                    // Register dependency if requested
                    if register_dependency {
                        // Drop read lock before acquiring write lock to avoid deadlock
                        drop(tree);

                        // Register this element as dependent
                        let mut tree_mut = self.tree.write();
                        if let Some(inherited_elem_mut) = tree_mut
                            .get_element_mut(id)
                            .and_then(|e| e.downcast_mut::<InheritedElement<W>>())
                        {
                            inherited_elem_mut.register_dependent(self.element_id);
                        }

                        // Re-acquire read lock to get widget
                        let tree = self.tree.read();
                        if let Some(inherited_elem) = tree
                            .get_element(id)
                            .and_then(|e| e.downcast_ref::<InheritedElement<W>>())
                        {
                            return Some(inherited_elem.widget().clone());
                        }
                        return None;
                    } else {
                        // No dependency registration needed, just return widget
                        return Some(inherited_elem.widget().clone());
                    }
                }

                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }

    /// Get the size of this element (after layout)
    ///
    /// Returns None if layout hasn't run yet or if this element doesn't have
    /// a RenderObject.
    ///
    /// # Returns
    ///
    /// The size of this element's RenderObject, or None
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(size) = context.size() {
    ///     println!("Width: {}, Height: {}", size.width, size.height);
    /// }
    /// ```
    pub fn size(&self) -> Option<Size> {
        let tree = self.tree.read();
        let element = tree.get_element(self.element_id)?;
        let render_object = element.render_object()?;
        Some(render_object.size())
    }

    /// Check if this context is still valid
    ///
    /// A context becomes invalid if its element has been unmounted from the tree.
    ///
    /// # Returns
    ///
    /// true if the element still exists in the tree, false otherwise
    pub fn is_valid(&self) -> bool {
        let tree = self.tree();
        tree.get_element(self.element_id).is_some()
    }

    /// Get debug information about this context
    ///
    /// Returns a string with information about the element and its position in the tree.
    ///
    /// # Returns
    ///
    /// Debug string describing this context
    pub fn debug_info(&self) -> String {
        let tree = self.tree();

        if let Some(element) = tree.get_element(self.element_id) {
            let parent_str = match element.parent() {
                Some(parent_id) => format!("Some({})", parent_id),
                None => "None (root)".to_string(),
            };

            format!(
                "BuildContext {{ element_id: {}, parent: {}, dirty: {} }}",
                self.element_id,
                parent_str,
                element.is_dirty()
            )
        } else {
            format!("BuildContext {{ element_id: {} (invalid) }}", self.element_id)
        }
    }
}

impl fmt::Debug for BuildContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildContext")
            .field("element_id", &self.element_id)
            .field("valid", &self.is_valid())
            .finish()
    }
}

// Note: We don't implement Default because BuildContext requires an ElementTree
// and ElementId. Use BuildContext::test() for testing instead.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StatelessWidget, Widget};

    // Test widget
    #[derive(Debug, Clone)]
    struct TestWidget {
        name: String,
    }

    impl TestWidget {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
            Box::new(TestWidget::new(format!("{}_child", self.name)))
        }
    }

    #[test]
    fn test_build_context_creation() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();

        let context = BuildContext::new(tree, element_id);

        assert_eq!(context.element_id(), element_id);
        assert!(!context.is_valid()); // No element mounted yet
    }

    #[test]
    fn test_build_context_test_helper() {
        let context = BuildContext::test();
        assert!(!context.is_valid());
    }

    #[test]
    fn test_build_context_element_id() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId(42);

        let context = BuildContext::new(tree, element_id);

        assert_eq!(context.element_id(), ElementId(42));
    }

    #[test]
    fn test_build_context_is_valid() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount a widget
        let widget = TestWidget::new("root");
        let element_id = {
            let mut tree_guard = tree.write();
            tree_guard.mount_root(Box::new(widget))
        };

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        assert!(context.is_valid());
    }

    #[test]
    fn test_build_context_is_valid_after_unmount() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount and unmount a widget
        let widget = TestWidget::new("root");
        let element_id = {
            let mut tree_guard = tree.write();
            let id = tree_guard.mount_root(Box::new(widget));
            tree_guard.unmount_element(id);
            id
        };

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        assert!(!context.is_valid());
    }

    #[test]
    fn test_build_context_parent() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount root and child
        let (root_id, child_id) = {
            let mut tree_guard = tree.write();
            let root_id = tree_guard.mount_root(Box::new(TestWidget::new("root")));
            let child_id = tree_guard.mount_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();
            (root_id, child_id)
        };

        // Root should have no parent
        let root_context = BuildContext::new(Arc::clone(&tree), root_id);
        assert_eq!(root_context.parent(), None);

        // Child should have root as parent
        let child_context = BuildContext::new(Arc::clone(&tree), child_id);
        assert_eq!(child_context.parent(), Some(root_id));
    }

    #[test]
    fn test_build_context_mark_needs_build() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount a widget
        let widget = TestWidget::new("root");
        let element_id = {
            let mut tree_guard = tree.write();
            tree_guard.mount_root(Box::new(widget))
        };

        // Clear dirty state
        {
            let mut tree_guard = tree.write();
            tree_guard.rebuild_dirty_elements();
        }

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        // Mark needs build
        context.mark_needs_build();

        // Check if marked dirty
        let tree_guard = tree.read();
        assert!(tree_guard.has_dirty_elements());
    }

    #[test]
    fn test_build_context_visit_ancestor_elements() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount root -> child -> grandchild
        let (root_id, child_id, grandchild_id) = {
            let mut tree_guard = tree.write();
            let root_id = tree_guard.mount_root(Box::new(TestWidget::new("root")));
            let child_id = tree_guard.mount_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();
            let grandchild_id = tree_guard.mount_child(child_id, Box::new(TestWidget::new("grandchild")), 0).unwrap();
            (root_id, child_id, grandchild_id)
        };

        let grandchild_context = BuildContext::new(Arc::clone(&tree), grandchild_id);

        // Visit ancestors
        let mut visited = Vec::new();
        grandchild_context.visit_ancestor_elements(&mut |element| {
            visited.push(element.id());
            true
        });

        assert_eq!(visited, vec![child_id, root_id]);
    }

    #[test]
    fn test_build_context_visit_ancestor_elements_early_stop() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount root -> child -> grandchild
        let (_root_id, child_id, grandchild_id) = {
            let mut tree_guard = tree.write();
            let root_id = tree_guard.mount_root(Box::new(TestWidget::new("root")));
            let child_id = tree_guard.mount_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();
            let grandchild_id = tree_guard.mount_child(child_id, Box::new(TestWidget::new("grandchild")), 0).unwrap();
            (root_id, child_id, grandchild_id)
        };

        let grandchild_context = BuildContext::new(Arc::clone(&tree), grandchild_id);

        // Visit ancestors but stop at first one
        let mut visited = Vec::new();
        grandchild_context.visit_ancestor_elements(&mut |element| {
            visited.push(element.id());
            false // Stop immediately
        });

        assert_eq!(visited, vec![child_id]); // Should only visit immediate parent
    }

    #[test]
    fn test_build_context_debug_info() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        let widget = TestWidget::new("root");
        let element_id = {
            let mut tree_guard = tree.write();
            tree_guard.mount_root(Box::new(widget))
        };

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        let debug_str = context.debug_info();
        assert!(debug_str.contains("BuildContext"));
        assert!(debug_str.contains(&element_id.to_string()));
    }

    #[test]
    fn test_build_context_debug() {
        let context = BuildContext::test();
        let debug_str = format!("{:?}", context);
        assert!(debug_str.contains("BuildContext"));
        assert!(debug_str.contains("element_id"));
    }

    #[test]
    fn test_build_context_size() {
        let context = BuildContext::test();
        // Returns None for ComponentElement (no RenderObject)
        assert!(context.size().is_none());
    }

    #[test]
    fn test_build_context_clone() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();

        let context1 = BuildContext::new(tree, element_id);
        let context2 = context1.clone();

        assert_eq!(context1.element_id(), context2.element_id());
    }
}
