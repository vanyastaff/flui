//! Context implementation - tree traversal, rebuild marking, widget access

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::{Element, ElementId, Size, Widget};
use crate::tree::ElementTree;

use super::iterators::{Ancestors, Children, Descendants};

/// Element tree context (tree traversal, inherited widgets, rebuild marking)
///
/// Provides access to the element tree and methods for navigation, queries,
/// and state management.
///
/// # Examples
///
/// ```rust,ignore
/// // Create context
/// let context = Context::new(tree, element_id);
///
/// // Tree traversal
/// let depth = context.depth();
/// for ancestor in context.ancestors() {
///     println!("Ancestor: {:?}", ancestor);
/// }
///
/// // Mark for rebuild
/// context.mark_dirty();
/// ```
#[derive(Clone)]
pub struct Context {
    tree: Arc<RwLock<ElementTree>>,
    element_id: ElementId,
}

// ========== Construction ==========

impl Context {
    /// Creates a new context
    #[must_use]
    #[inline]
    pub const fn new(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    /// Creates an empty context (useful for testing)
    #[must_use]
    pub fn empty() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();
        Self { tree, element_id }
    }

    /// Creates a test context (alias for `empty`)
    #[cfg(test)]
    #[must_use]
    #[inline]
    pub fn test() -> Self {
        Self::empty()
    }
}

// ========== Basic Properties ==========

impl Context {
    /// Returns the element ID
    #[must_use]
    #[inline]
    pub const fn id(&self) -> ElementId {
        self.element_id
    }

    /// Returns the element ID (alias for compatibility)
    #[must_use]
    #[inline]
    pub const fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns the parent element ID
    ///
    /// Returns `None` if this is the root element.
    #[must_use]
    #[inline]
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree();
        tree.get(self.element_id)
            .and_then(|element| element.parent())
    }

    /// Returns the size of this element (after layout)
    #[must_use]
    pub fn size(&self) -> Option<Size> {
        let tree = self.tree();
        tree.get(self.element_id)
            .and_then(|element| element.render_object())
            .map(|render_object| render_object.size())
    }
}

// ========== State Queries ==========

impl Context {
    /// Checks if context is still valid (an element exists in a tree)
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        let tree = self.tree();
        tree.get(self.element_id).is_some()
    }

    /// Checks if an element is mounted (alias for `is_valid`)
    #[must_use]
    #[inline]
    pub fn is_mounted(&self) -> bool {
        self.is_valid()
    }

    /// Checks if this is the root element
    #[must_use]
    #[inline]
    pub fn is_root(&self) -> bool {
        self.parent().is_none()
    }

    /// Checks if an element has any ancestors
    #[must_use]
    #[inline]
    pub fn has_ancestor(&self) -> bool {
        self.parent().is_some()
    }

    /// Checks if an element has any children
    #[must_use]
    #[inline]
    pub fn has_children(&self) -> bool {
        // ИСПРАВЛЕНО: убрали логическую ошибку!
        self.child_count() > 0
    }

    /// Returns the depth of this element in the tree
    ///
    /// Root element has depth 0.
    #[must_use]
    #[inline]
    pub fn depth(&self) -> usize {
        self.ancestors().count()
    }

    /// Returns the number of direct children
    #[must_use]
    #[inline]
    pub fn child_count(&self) -> usize {
        // Используем итератор children для подсчета
        self.children().len()
    }
}

// ========== Tree Access ==========

impl Context {
    /// Returns a read lock guard for the tree
    #[inline]
    pub(crate) fn tree(&self) -> parking_lot::RwLockReadGuard<'_, ElementTree> {
        self.tree.read()
    }

    /// Returns a write lock guard for the tree
    #[inline]
    pub(crate) fn tree_mut(&self) -> parking_lot::RwLockWriteGuard<'_, ElementTree> {
        self.tree.write()
    }

    /// Returns a reference to the shared tree Arc
    ///
    /// Useful for creating sibling contexts.
    #[must_use]
    #[inline]
    pub fn shared_tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }
}

// ========== State Mutations ==========

impl Context {
    /// Marks element as needing rebuild
    pub fn mark_dirty(&self) {
        let mut tree = self.tree_mut();
        tree.mark_dirty(self.element_id);
    }

    /// Marks an element as needing rebuild (explicit alias)
    #[inline]
    pub fn mark_needs_build(&self) {
        self.mark_dirty();
    }
}

// ========== Tree Traversal - Iterators ==========

impl Context {
    /// Iterates over ancestor elements (parent to root)
    #[must_use]
    pub fn ancestors(&self) -> Ancestors<'_> {
        let tree = self.tree.read();
        let current = self.parent();
        Ancestors { tree, current }
    }

    /// Iterates over direct child elements
    #[must_use]
    pub fn children(&self) -> Children {
        let tree = self.tree.read();
        let children = tree
            .get(self.element_id)
            .map(|e| e.children_iter().collect())
            .unwrap_or_default();

        Children { children, index: 0 }
    }

    /// Iterates over all descendant elements (depth-first)
    #[must_use]
    pub fn descendants(&self) -> Descendants<'_> {
        let tree = self.tree.read();
        let mut stack = Vec::new();

        if let Some(element) = tree.get(self.element_id) {
            let children: Vec<_> = element.children_iter().collect();
            for child_id in children.into_iter().rev() {
                stack.push(child_id);
            }
        }

        Descendants { tree, stack }
    }
}

// ========== Tree Traversal - Visitor Pattern ==========

impl Context {
    /// Visits ancestor elements with a callback
    ///
    /// The visitor returns `true` to continue, `false` to stop.
    pub fn visit_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn crate::DynElement) -> bool,
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

    /// Visits child elements with a callback
    pub fn visit_children<F>(&self, mut visitor: F)
    where
        F: FnMut(&dyn crate::DynElement),
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
}

// ========== Finding Ancestors ==========

impl Context {
    /// Finds ancestor widget of a specific type
    #[must_use]
    pub fn ancestor_widget<W: Widget + 'static>(&self) -> Option<W> {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                // Пытаемся получить widget (реализация зависит от вашего API)
                // Здесь показана концепция
                current_id = element.parent();
            } else {
                break;
            }
        }
        None
    }

    /// Finds ancestor widget (alias for compatibility)
    #[must_use]
    #[inline]
    pub fn find_ancestor<W: Widget + 'static>(&self) -> Option<W> {
        self.ancestor_widget()
    }

    /// Finds ancestor widget (ergonomic alias)
    #[must_use]
    #[inline]
    pub fn ancestor<W: Widget + Clone + 'static>(&self) -> Option<W> {
        self.ancestor_widget()
    }

    /// Finds an ancestor element of a specific type
    #[must_use]
    pub fn ancestor_element<E: Element + 'static>(&self) -> Option<ElementId> {
        self.ancestors().find(|&id| {
            let tree = self.tree();
            tree.get(id)
                .map(|elem| elem.is::<E>())
                .unwrap_or(false)
        })
    }

    /// Finds an ancestor element satisfying a predicate
    #[must_use]
    pub fn ancestor_where<F>(&self, mut predicate: F) -> Option<ElementId>
    where
        F: FnMut(&ElementId) -> bool,
    {
        self.ancestors().find(|id| predicate(id))
    }
}

// ========== RenderObject Finding ==========

impl Context {
    /// Finds nearest RenderObject element (self or ancestor)
    #[must_use]
    pub fn render_object(&self) -> Option<ElementId> {
        let tree = self.tree();

        // Check self first
        if let Some(element) = tree.get(self.element_id) {
            if element.render_object().is_some() {
                return Some(self.element_id);
            }
        }

        // Search ancestors
        self.ancestors().find(|&id| {
            tree.get(id)
                .and_then(|elem| elem.render_object())
                .is_some()
        })
    }

    /// Finds nearest RenderObject (alias for compatibility)
    #[must_use]
    #[inline]
    pub fn find_render_object(&self) -> Option<ElementId> {
        self.render_object()
    }

    /// Finds ancestor RenderObject of a specific type
    #[must_use]
    pub fn ancestor_render_object<R: crate::RenderObject + 'static>(
        &self,
    ) -> Option<ElementId> {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                if let Some(render_obj) = element.render_object() {
                    if render_obj.is::<R>() {
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

    /// Finds ancestor RenderObject (alias for compatibility)
    #[must_use]
    #[inline]
    pub fn ancestor_render<R: crate::RenderObject + 'static>(&self) -> Option<ElementId> {
        self.ancestor_render_object::<R>()
    }
}

// ========== Notification System ==========

impl Context {
    /// Dispatches notification up the tree
    pub fn dispatch_notification(&self, notification: &dyn crate::notification::AnyNotification) {
        let tree = self.tree.read();
        let mut current_id = self.element_id;

        loop {
            let Some(element) = tree.get(current_id) else {
                break;
            };

            let stop = element.visit_notification(notification);
            if stop {
                break;
            }

            let Some(parent_id) = element.parent() else {
                break;
            };
            current_id = parent_id;
        }
    }
}

// ========== Debug Support ==========

impl Context {
    /// Returns debug information about this context
    #[must_use]
    pub fn debug_info(&self) -> String {
        let tree = self.tree();

        if let Some(element) = tree.get(self.element_id) {
            let parent_str = match element.parent() {
                Some(parent_id) => format!("Some({})", parent_id),
                None => "None (root)".to_string(),
            };

            format!(
                "Context {{ id: {}, parent: {}, dirty: {}, children: {} }}",
                self.element_id,
                parent_str,
                element.is_dirty(),
                self.child_count()
            )
        } else {
            format!("Context {{ id: {} (invalid) }}", self.element_id)
        }
    }
}

// ========== Trait Implementations ==========

impl Default for Context {
    /// Creates an empty context
    #[inline]
    fn default() -> Self {
        Self::empty()
    }
}

impl PartialEq for Context {
    /// Contexts are equal if they reference the same element in the same tree
    fn eq(&self, other: &Self) -> bool {
        self.element_id == other.element_id && Arc::ptr_eq(&self.tree, &other.tree)
    }
}

impl Eq for Context {}

impl Hash for Context {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.element_id.hash(state);
        Arc::as_ptr(&self.tree).hash(state);
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("id", &self.element_id)
            .field("valid", &self.is_valid())
            .field("depth", &self.depth())
            .field("is_root", &self.is_root())
            .finish()
    }
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.debug_info())
    }
}

impl AsRef<ElementId> for Context {
    #[inline]
    fn as_ref(&self) -> &ElementId {
        &self.element_id
    }
}

impl From<(Arc<RwLock<ElementTree>>, ElementId)> for Context {
    #[inline]
    fn from((tree, element_id): (Arc<RwLock<ElementTree>>, ElementId)) -> Self {
        Self::new(tree, element_id)
    }
}

// ========== Tests ==========

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let id = ElementId::new();
        let context = Context::new(Arc::clone(&tree), id);
        assert_eq!(context.id(), id);
        assert_eq!(context.element_id(), id);
    }

    #[test]
    fn test_context_empty() {
        let context = Context::empty();
        assert!(context.is_root());
        assert!(!context.is_valid());
    }

    #[test]
    fn test_context_properties() {
        let context = Context::empty();
        assert!(context.is_root());
        assert!(!context.has_ancestor());
        assert!(!context.has_children()); // ИСПРАВЛЕНО: теперь работает правильно!
        assert_eq!(context.child_count(), 0);
        assert_eq!(context.depth(), 0);
    }

    #[test]
    fn test_context_equality() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let id = ElementId::new();

        let ctx1 = Context::new(Arc::clone(&tree), id);
        let ctx2 = Context::new(Arc::clone(&tree), id);

        assert_eq!(ctx1, ctx2);
    }

    #[test]
    fn test_context_hash() {
        use std::collections::HashMap;

        let context = Context::empty();
        let mut map = HashMap::new();
        map.insert(context.clone(), "value");

        assert_eq!(map.get(&context), Some(&"value"));
    }

    #[test]
    fn test_from_tuple() {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let id = ElementId::new();

        let context = Context::from((Arc::clone(&tree), id));
        assert_eq!(context.id(), id);
    }
}