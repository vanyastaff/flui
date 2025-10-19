//! Methods for working with child elements

use crate::Element;
use super::core::Context;

impl Context {
    /// Visit child elements using callback
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
    pub fn walk_children<F>(&self, visitor: &mut F)
    where
        F: FnMut(&dyn Element),
    {
        self.visit_child_elements(visitor)
    }
}
