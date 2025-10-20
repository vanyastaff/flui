//! Element tree context for widgets

use std::fmt;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::{Element, ElementId, Size, Widget};
use crate::tree::ElementTree;

mod inherited;
mod iterators;

pub use iterators::{Ancestors, Children, Descendants};

// Re-export inherited methods

/// Element tree context (tree traversal, inherited widgets, rebuild marking)
#[derive(Clone)]
pub struct Context {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
}

impl Context {
    /// Create a new context
    pub fn new(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    /// Create an empty context
    pub fn empty() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();
        Self { tree, element_id }
    }

    /// Create a test context
    #[cfg(test)]
    pub fn test() -> Self {
        Self::empty()
    }

    /// Get element ID
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get tree reference
    pub(crate) fn tree(&self) -> parking_lot::RwLockReadGuard<'_, ElementTree> {
        self.tree.read()
    }

    /// Get mutable tree reference
    pub(crate) fn tree_mut(&self) -> parking_lot::RwLockWriteGuard<'_, ElementTree> {
        self.tree.write()
    }

    /// Get parent element ID
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree();
        tree.get(self.element_id)
            .and_then(|element| element.parent())
    }

    /// Check if context is still valid
    pub fn is_valid(&self) -> bool {
        let tree = self.tree();
        tree.get(self.element_id).is_some()
    }

    /// Check if element is mounted
    pub fn mounted(&self) -> bool {
        self.is_valid()
    }

    /// Mark element as needing rebuild
    pub fn mark_needs_build(&self) {
        let mut tree = self.tree_mut();
        tree.mark_dirty(self.element_id);
    }

    /// Mark element as dirty - short form
    pub fn mark_dirty(&self) {
        self.mark_needs_build()
    }

    /// Iterate over ancestor elements (Rust idiomatic!)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let depth = context.ancestors().count();
    /// let dirty_ancestors: Vec<_> = context.ancestors()
    ///     .filter(|&id| is_dirty(id))
    ///     .collect();
    /// ```
    pub fn ancestors(&self) -> Ancestors<'_> {
        let tree = self.tree.read();
        let current = self.parent();
        Ancestors { tree, current }
    }

    /// Iterate over direct child elements (Rust idiomatic!)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let child_count = context.children().count();
    /// for child_id in context.children() {
    ///     println!("Child: {:?}", child_id);
    /// }
    /// ```
    pub fn children(&self) -> Children {
        let tree = self.tree.read();
        let children = if let Some(element) = tree.get(self.element_id) {
            element.children_iter().collect()
        } else {
            Vec::new()
        };
        Children { children, index: 0 }
    }

    /// Iterate over all descendant elements in depth-first order (Rust idiomatic!)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Count all descendants
    /// let total = context.descendants().count();
    ///
    /// // Find first dirty descendant
    /// let dirty = context.descendants()
    ///     .find(|&id| {
    ///         let tree = context.tree();
    ///         tree.get(id).map(|e| e.is_dirty()).unwrap_or(false)
    ///     });
    /// ```
    pub fn descendants(&self) -> Descendants<'_> {
        let tree = self.tree.read();
        let mut stack = Vec::new();

        // Initialize with current element's children
        if let Some(element) = tree.get(self.element_id) {
            let children: Vec<_> = element.children_iter().collect();
            // Push in reverse for correct depth-first order
            for child_id in children.into_iter().rev() {
                stack.push(child_id);
            }
        }

        Descendants { tree, stack }
    }

    /// Visit ancestor elements
    pub fn visit_ancestor_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn crate::AnyElement) -> bool,
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

    /// Visit ancestors - short form
    pub fn walk_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn crate::AnyElement) -> bool,
    {
        self.visit_ancestor_elements(visitor)
    }

    /// Find ancestor widget of specific type
    pub fn find_ancestor_widget_of_type<W: Widget + 'static>(&self) -> Option<W> {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                current_id = element.parent();
            } else {
                break;
            }
        }
        None
    }

    /// Find ancestor widget - short form
    pub fn find_ancestor<W: Widget + 'static>(&self) -> Option<W> {
        self.find_ancestor_widget_of_type()
    }

    /// Find ancestor element of specific type (iterator-based)
    ///
    /// Uses the iterator API for efficient traversal without manual loops.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find first StatefulElement ancestor
    /// if let Some(id) = context.find_ancestor_element::<StatefulElement>() {
    ///     println!("Found stateful ancestor: {:?}", id);
    /// }
    /// ```
    pub fn find_ancestor_element_of_type<E: Element + 'static>(&self) -> Option<ElementId> {
        self.ancestors().find(|&id| {
            let tree = self.tree();
            tree.get(id)
                .map(|elem| elem.is::<E>())
                .unwrap_or(false)
        })
    }

    /// Find ancestor element - short form (Rust-idiomatic)
    ///
    /// Generic version that's easier to use with turbofish syntax.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stateful_id = context.find_ancestor_element::<StatefulElement>();
    /// ```
    pub fn find_ancestor_element<E: Element + 'static>(&self) -> Option<ElementId> {
        self.find_ancestor_element_of_type::<E>()
    }

    /// Visit child elements
    pub fn visit_child_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn crate::AnyElement),
    {
        let tree = self.tree.read();

        if let Some(element) = tree.get(self.element_id) {
            for child_id in element.children_iter() {
                if let Some(child_element) = tree.get(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    /// Visit children - short form
    pub fn walk_children<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn crate::AnyElement),
    {
        self.visit_child_elements(visitor)
    }

    /// Get size of this element (after layout)
    pub fn size(&self) -> Option<Size> {
        let tree = self.tree();
        tree.get(self.element_id)
            .and_then(|element| element.render_object())
            .map(|render_object| render_object.size())
    }

    /// Get depth of this element in the tree (iterator-based)
    ///
    /// Returns the number of ancestors, i.e., distance from root.
    /// Root element has depth 0.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let depth = context.depth();
    /// println!("Element is at depth {}", depth);
    /// ```
    pub fn depth(&self) -> usize {
        self.ancestors().count()
    }

    /// Check if element has any ancestors (is not root)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if context.has_ancestor() {
    ///     println!("Not a root element");
    /// }
    /// ```
    pub fn has_ancestor(&self) -> bool {
        self.parent().is_some()
    }

    /// Find ancestor element satisfying a predicate (iterator-based)
    ///
    /// More flexible than type-based search - allows custom predicates.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Find first dirty ancestor
    /// let dirty_ancestor = context.find_ancestor_where(|id| {
    ///     let tree = context.tree();
    ///     tree.get(*id).map(|e| e.is_dirty()).unwrap_or(false)
    /// });
    /// ```
    pub fn find_ancestor_where<F>(&self, mut predicate: F) -> Option<ElementId>
    where
        F: FnMut(&ElementId) -> bool,
    {
        self.ancestors().find(|id| predicate(id))
    }

    /// Find nearest RenderObject element (iterator-based)
    ///
    /// Searches current element first, then ancestors.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(render_id) = context.find_render_object() {
    ///     println!("Found render object at: {:?}", render_id);
    /// }
    /// ```
    pub fn find_render_object(&self) -> Option<ElementId> {
        let tree = self.tree();

        // Check current element
        if let Some(element) = tree.get(self.element_id) {
            if element.render_object().is_some() {
                return Some(self.element_id);
            }
        }

        // Check ancestors using iterator
        self.ancestors().find(|&id| {
            tree.get(id)
                .and_then(|elem| elem.render_object())
                .is_some()
        })
    }

    /// Find ancestor RenderObject of specific type
    pub fn find_ancestor_render_object_of_type<R: crate::RenderObject + 'static>(
        &self,
    ) -> Option<ElementId> {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
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

    /// Get debug info
    pub fn debug_info(&self) -> String {
        let tree = self.tree();

        if let Some(element) = tree.get(self.element_id) {
            let parent_str = match element.parent() {
                Some(parent_id) => format!("Some({})", parent_id),
                None => "None (root)".to_string(),
            };

            format!(
                "Context {{ element_id: {}, parent: {}, dirty: {} }}",
                self.element_id,
                parent_str,
                element.is_dirty()
            )
        } else {
            format!("Context {{ element_id: {} (invalid) }}", self.element_id)
        }
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("element_id", &self.element_id)
            .field("valid", &self.is_valid())
            .finish()
    }
}
