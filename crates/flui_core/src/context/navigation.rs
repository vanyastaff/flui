//! Navigation methods for finding ancestors and traversing the tree

use crate::{Element, ElementId, Widget};
use super::core::Context;
use super::iterators::Ancestors;

impl Context {
    /// Iterate over ancestor elements (Rust idiomatic!)
    ///
    /// Returns an iterator that yields ElementIds from parent to root.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let depth = context.ancestors().count();
    /// let container = context.ancestors().find(|&id| /* check */);
    /// ```
    pub fn ancestors(&self) -> Ancestors<'_> {
        let tree = self.tree.read();
        let current = self.parent();
        Ancestors { tree, current }
    }

    /// Visit ancestor elements using callback
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
    pub fn walk_ancestors<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element) -> bool,
    {
        self.visit_ancestor_elements(visitor)
    }

    /// Find nearest ancestor widget of specific type
    pub fn find_ancestor_widget_of_type<W: Widget + 'static>(&self) -> Option<W> {
        let tree = self.tree();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                // TODO: Get widget from element
                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }

    /// Find nearest ancestor widget - short form
    pub fn find_ancestor<W: Widget + 'static>(&self) -> Option<W> {
        self.find_ancestor_widget_of_type()
    }

    /// Find nearest ancestor element of specific type
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

    /// Find nearest ancestor element - short form
    pub fn find_ancestor_element<E: Element + 'static>(&self) -> Option<ElementId> {
        self.find_ancestor_element_of_type::<E>()
    }
}
