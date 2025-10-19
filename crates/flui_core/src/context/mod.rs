//! Build context for accessing the element tree

use std::fmt;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::{Element, ElementId, Size, Widget};
use crate::tree::ElementTree;
use crate::widget::InheritedWidget;

mod inherited;
mod iterators;

pub use iterators::Ancestors;

// Re-export inherited methods
pub use inherited::*;

/// Build context provides access to the element tree and framework services
///
/// Rust-idiomatic name for Flutter's BuildContext.
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
    pub fn ancestors(&self) -> Ancestors<'_> {
        let tree = self.tree.read();
        let current = self.parent();
        Ancestors { tree, current }
    }

    /// Visit ancestor elements
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

    /// Visit ancestors - short form
    pub fn walk_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element) -> bool,
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

    /// Find ancestor element of specific type
    pub fn find_ancestor_element_of_type<E: Element + 'static>(&self) -> Option<ElementId> {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                if element.is::<E>() {
                    return Some(id);
                }
                current_id = element.parent();
            } else {
                break;
            }
        }
        None
    }

    /// Find ancestor element - short form
    pub fn find_ancestor_element<E: Element + 'static>(&self) -> Option<ElementId> {
        self.find_ancestor_element_of_type::<E>()
    }

    /// Visit child elements
    pub fn visit_child_elements<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element),
    {
        let tree = self.tree.read();

        if let Some(element) = tree.get(self.element_id) {
            let child_ids = element.children();

            for child_id in child_ids {
                if let Some(child_element) = tree.get(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    /// Visit children - short form
    pub fn walk_children<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element),
    {
        self.visit_child_elements(visitor)
    }

    /// Get size of this element (after layout)
    pub fn size(&self) -> Option<Size> {
        let tree = self.tree();
        tree.get(self.element_id)
            .and_then(|element| element.render_object())
            .and_then(|render_object| Some(render_object.size()))
    }

    /// Find nearest RenderObject element
    pub fn find_render_object(&self) -> Option<ElementId> {
        let tree = self.tree();

        // Check current element
        if let Some(element) = tree.get(self.element_id) {
            if element.render_object().is_some() {
                return Some(self.element_id);
            }
        }

        // Check ancestors
        let mut current_id = self.parent();
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                if element.render_object().is_some() {
                    return Some(id);
                }
                current_id = element.parent();
            } else {
                break;
            }
        }
        None
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

// Backward compatibility alias
pub type BuildContext = Context;
