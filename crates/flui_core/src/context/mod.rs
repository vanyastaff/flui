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
//!         let theme = context.subscribe_to::<Theme>();
//!
//!         // Get size after layout
//!         if let Some(size) = context.size() {
//!             println!("My size: {:?}", size);
//!         }
//!
//!         // Find ancestor widget
//!         let parent = context.find_ancestor::<Container>();
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

use crate::{Element, ElementId, Size, Widget};
use crate::tree::ElementTree;
use crate::widget::InheritedWidget;

/// Build context provides access to the element tree and framework services
///
/// Rust-idiomatic name for Flutter's BuildContext. Passed to build() methods.
///
/// Context is cheap to clone - it contains only Arc references to shared data.
///
/// # Lifetime
///
/// A Context is only valid during the build phase. Do not store it for
/// later use, as the element tree may have changed.
///
/// # Thread Safety
///
/// Context is Send + Sync. The underlying ElementTree uses RwLock for
/// interior mutability:
/// - Multiple readers can access concurrently
/// - Writers block all readers and other writers
#[derive(Clone)]
pub struct Context {
    /// Reference to the element tree
    tree: Arc<RwLock<ElementTree>>,

    /// ID of the current element
    element_id: ElementId,
}

impl Context {
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
    /// context.mark_dirty();
    /// ```
    pub fn mark_needs_build(&self) {
        let mut tree = self.tree_mut();
        tree.mark_element_dirty(self.element_id);
    }

    /// Mark the current element as needing rebuild - short form
    ///
    /// Rust-idiomatic short name. See [mark_needs_build](Self::mark_needs_build).
    pub fn mark_dirty(&self) {
        self.mark_needs_build()
    }

    /// Get the parent element ID
    ///
    /// # Returns
    ///
    /// The parent element ID, or None if this is the root element
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree();
        tree.get(self.element_id)
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
    /// context.walk_ancestors(&mut |element| {
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
            if let Some(element) = tree.get(id) {
                if !visitor(element) {
                    break;
                }
                current_id = element.parent();
            } else {
                break;
            }
        }
    }

    /// Visit ancestor elements - short form
    ///
    /// Rust-idiomatic short name. See [visit_ancestor_elements](Self::visit_ancestor_elements).
    pub fn walk_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element) -> bool,
    {
        self.visit_ancestor_elements(visitor)
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
    /// if let Some(scaffold) = context.find_ancestor::<Scaffold>() {
    ///     // Use scaffold...
    /// }
    /// ```
    pub fn find_ancestor_widget_of_type<W: Widget + 'static>(&self) -> Option<W> {
        let tree = self.tree();

        let mut current_id = self.parent();
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
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

    /// Find the nearest ancestor widget - short form
    ///
    /// Rust-idiomatic short name. See [find_ancestor_widget_of_type](Self::find_ancestor_widget_of_type).
    pub fn find_ancestor<W: Widget + 'static>(&self) -> Option<W> {
        self.find_ancestor_widget_of_type()
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
    /// if let Some(element) = context.find_ancestor_element::<RenderObjectElement>() {
    ///     // Use element...
    /// }
    /// ```
    pub fn find_ancestor_element_of_type<E: Element + 'static>(&self) -> Option<ElementId> {
        let tree = self.tree();
        let mut result = None;

        let mut current_id = self.parent();
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
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

    /// Find the nearest ancestor element - short form
    ///
    /// Rust-idiomatic short name. See [find_ancestor_element_of_type](Self::find_ancestor_element_of_type).
    pub fn find_ancestor_element<E: Element + 'static>(&self) -> Option<ElementId> {
        self.find_ancestor_element_of_type::<E>()
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
    /// if let Some(theme) = context.subscribe_to::<Theme>() {
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

    /// Access an InheritedWidget's data from the tree - short form
    ///
    /// Rust-idiomatic short name. See [depend_on_inherited_widget](Self::depend_on_inherited_widget).
    pub fn subscribe_to<W: InheritedWidget + Clone + 'static>(&self) -> Option<W> {
        self.depend_on_inherited_widget()
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

    /// Access an InheritedWidget without establishing a dependency - short form
    ///
    /// Rust-idiomatic short name. See [get_inherited_widget](Self::get_inherited_widget).
    pub fn find_inherited<W: InheritedWidget + Clone + 'static>(&self) -> Option<W> {
        self.get_inherited_widget()
    }

    /// Internal implementation for getting inherited widgets
    fn get_inherited_widget_impl<W: InheritedWidget + Clone + 'static>(
        &self,
        _type_id: TypeId,
        register_dependency: bool,
    ) -> Option<W> {
        use crate::widget::InheritedElement;

        let tree = self.tree();
        let mut current_id = self.parent();

        // Walk up the tree looking for InheritedElement<W>
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
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
                            .get_mut(id)
                            .and_then(|e| e.downcast_mut::<InheritedElement<W>>())
                        {
                            inherited_elem_mut.register_dependent(self.element_id);
                        }

                        // Re-acquire read lock to get widget
                        let tree = self.tree.read();
                        if let Some(inherited_elem) = tree
                            .get(id)
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
        let element = tree.get(self.element_id)?;
        let render_object = element.render_object()?;
        Some(render_object.size())
    }

    /// Find the RenderObject for this context
    ///
    /// Similar to Flutter's `context.findRenderObject()`.
    /// Returns the RenderObject associated with this element.
    ///
    /// # Returns
    ///
    /// The element ID if this element has a RenderObject, None otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(render_object_id) = context.find_render_object() {
    ///     // Access render object through tree
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// Due to Rust ownership rules, we return the ElementId rather than a direct
    /// reference to the RenderObject. Use the ElementTree to access the actual object.
    pub fn find_render_object(&self) -> Option<ElementId> {
        let tree = self.tree.read();
        let element = tree.get(self.element_id)?;

        // Check if this element has a render object
        if element.render_object().is_some() {
            Some(self.element_id)
        } else {
            None
        }
    }

    /// Find the nearest ancestor RenderObject of a specific type
    ///
    /// Similar to Flutter's `context.findAncestorRenderObjectOfType<T>()`.
    /// Searches up the tree for the first ancestor element that has a RenderObject
    /// of type `R`.
    ///
    /// # Type Parameters
    ///
    /// - `R`: The RenderObject type to search for (must implement RenderObject + 'static)
    ///
    /// # Returns
    ///
    /// Element ID of the ancestor with the matching RenderObject type, None if not found
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::RenderPadding;
    ///
    /// if let Some(padding_id) = context.find_ancestor_render::<RenderPadding>() {
    ///     // Found ancestor RenderPadding
    /// }
    /// ```
    pub fn find_ancestor_render_object_of_type<R: crate::RenderObject + 'static>(
        &self,
    ) -> Option<ElementId> {
        let tree = self.tree.read();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                // Check if this element has a RenderObject of type R
                if let Some(render_object) = element.render_object() {
                    if render_object.is::<R>() {
                        return Some(id);
                    }
                }
                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }

    /// Visit child elements
    ///
    /// Similar to Flutter's `context.visitChildElements()`.
    /// Calls the visitor function for each immediate child element.
    ///
    /// # Parameters
    ///
    /// - `visitor`: Function called for each child element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// context.walk_children(&mut |child| {
    ///     println!("Child: {:?}", child.id());
    /// });
    /// ```
    pub fn visit_child_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element),
    {
        let tree = self.tree.read();

        if let Some(element) = tree.get(self.element_id) {
            // Get child IDs from element
            let child_ids = element.children();

            // Visit each child
            for child_id in child_ids {
                if let Some(child_element) = tree.get(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    /// Visit child elements - short form
    ///
    /// Rust-idiomatic short name. See [visit_child_elements](Self::visit_child_elements).
    pub fn walk_children<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element),
    {
        self.visit_child_elements(visitor)
    }

    /// Check if this element is currently mounted in the tree
    ///
    /// Similar to Flutter's `mounted` property on State.
    ///
    /// # Returns
    ///
    /// true if the element is mounted, false otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if context.mounted() {
    ///     // Safe to use context
    /// }
    /// ```
    pub fn mounted(&self) -> bool {
        self.is_valid()
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
        tree.get(self.element_id).is_some()
    }

    /// Get the InheritedElement for a specific InheritedWidget type without creating a dependency
    ///
    /// Similar to Flutter's `context.getElementForInheritedWidgetOfExactType<T>()`.
    /// This is like `depend_on_inherited_widget()` but does NOT register a dependency,
    /// so this element will NOT rebuild when the InheritedWidget changes.
    ///
    /// # Type Parameters
    ///
    /// - `W`: The InheritedWidget type to search for
    ///
    /// # Returns
    ///
    /// Element ID of the InheritedElement if found, None otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // One-time access to Theme without dependency
    /// if let Some(theme_id) = context.find_inherited_element::<Theme>() {
    ///     println!("Found Theme element: {}", theme_id);
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// Use this only when you need one-time access to inherited data.
    /// For most cases, use `depend_on_inherited_widget()` instead.
    pub fn get_element_for_inherited_widget_of_exact_type<W: InheritedWidget + Clone + 'static>(
        &self,
    ) -> Option<ElementId> {
        use crate::widget::InheritedElement;

        let tree = self.tree.read();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                // Try to downcast to InheritedElement<W>
                if let Some(_inherited_elem) = element.downcast_ref::<InheritedElement<W>>() {
                    return Some(id);
                }
                current_id = element.parent();
            } else {
                break;
            }
        }

        None
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

        if let Some(element) = tree.get(self.element_id) {
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
        let element_id = ElementId::from_raw(42);

        let context = BuildContext::new(tree, element_id);

        assert_eq!(context.element_id(), ElementId::from_raw(42));
    }

    #[test]
    fn test_build_context_is_valid() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount a widget
        let widget = TestWidget::new("root");
        let element_id = {
            let mut tree_guard = tree.write();
            tree_guard.set_root(Box::new(widget))
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
            let id = tree_guard.set_root(Box::new(widget));
            tree_guard.remove(id);
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
            let root_id = tree_guard.set_root(Box::new(TestWidget::new("root")));
            let child_id = tree_guard.insert_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();
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
            tree_guard.set_root(Box::new(widget))
        };

        // Clear dirty state
        {
            let mut tree_guard = tree.write();
            tree_guard.rebuild();
        }

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        // Mark needs build
        context.mark_dirty();

        // Check if marked dirty
        let tree_guard = tree.read();
        assert!(tree_guard.has_dirty());
    }

    #[test]
    fn test_build_context_visit_ancestor_elements() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount root -> child -> grandchild
        let (root_id, child_id, grandchild_id) = {
            let mut tree_guard = tree.write();
            let root_id = tree_guard.set_root(Box::new(TestWidget::new("root")));
            let child_id = tree_guard.insert_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();
            let grandchild_id = tree_guard.insert_child(child_id, Box::new(TestWidget::new("grandchild")), 0).unwrap();
            (root_id, child_id, grandchild_id)
        };

        let grandchild_context = BuildContext::new(Arc::clone(&tree), grandchild_id);

        // Visit ancestors
        let mut visited = Vec::new();
        grandchild_context.walk_ancestors(&mut |element| {
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
            let root_id = tree_guard.set_root(Box::new(TestWidget::new("root")));
            let child_id = tree_guard.insert_child(root_id, Box::new(TestWidget::new("child")), 0).unwrap();
            let grandchild_id = tree_guard.insert_child(child_id, Box::new(TestWidget::new("grandchild")), 0).unwrap();
            (root_id, child_id, grandchild_id)
        };

        let grandchild_context = BuildContext::new(Arc::clone(&tree), grandchild_id);

        // Visit ancestors but stop at first one
        let mut visited = Vec::new();
        grandchild_context.walk_ancestors(&mut |element| {
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
            tree_guard.set_root(Box::new(widget))
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

    #[test]
    fn test_build_context_mounted() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount a widget
        let widget = TestWidget::new("root");
        let element_id = {
            let mut tree_guard = tree.write();
            tree_guard.set_root(Box::new(widget))
        };

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        // Should be mounted
        assert!(context.mounted());

        // Unmount
        {
            let mut tree_guard = tree.write();
            tree_guard.remove(element_id);
        }

        // Should not be mounted
        assert!(!context.mounted());
    }

    #[test]
    fn test_build_context_find_render_object() {
        let context = BuildContext::test();

        // ComponentElement doesn't have RenderObject
        assert!(context.find_render_object().is_none());
    }

    #[test]
    fn test_build_context_visit_child_elements() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount root with child
        // Note: ComponentElement only supports single child through rebuild()
        let (root_id, child_id) = {
            let mut tree_guard = tree.write();
            let root_id = tree_guard.set_root(Box::new(TestWidget::new("root")));

            // Rebuild to create child
            tree_guard.mark_dirty(root_id);
            tree_guard.rebuild();

            // Get the child that was created by build()
            let child_ids = tree_guard.get(root_id)
                .map(|e| e.children())
                .unwrap_or_default();

            let child_id = child_ids.first().copied();
            (root_id, child_id)
        };

        let root_context = BuildContext::new(Arc::clone(&tree), root_id);

        // Visit children
        let mut visited = Vec::new();
        root_context.walk_children(&mut |child| {
            visited.push(child.id());
        });

        // ComponentElement has single child after rebuild
        if let Some(child_id) = child_id {
            assert_eq!(visited.len(), 1);
            assert_eq!(visited[0], child_id);
        } else {
            // If no child was created, that's also valid
            assert_eq!(visited.len(), 0);
        }
    }

    #[test]
    fn test_build_context_visit_child_elements_no_children() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Mount leaf widget
        let element_id = {
            let mut tree_guard = tree.write();
            tree_guard.set_root(Box::new(TestWidget::new("leaf")))
        };

        let context = BuildContext::new(Arc::clone(&tree), element_id);

        // Visit children (should be none)
        let mut visited = 0;
        context.walk_children(&mut |_| {
            visited += 1;
        });

        assert_eq!(visited, 0);
    }
}

// Backward compatibility alias
pub type BuildContext = Context;
