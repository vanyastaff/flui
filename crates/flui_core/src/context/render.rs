//! Methods for accessing RenderObjects

use crate::{ElementId, Size};
use super::core::Context;

impl Context {
    /// Get the size of this element (after layout)
    pub fn size(&self) -> Option<Size> {
        let tree = self.tree();

        tree.get(self.element_id)
            .and_then(|element| element.render_object())
            .and_then(|render_object| Some(render_object.size()))
    }

    /// Find the nearest RenderObject element
    pub fn find_render_object(&self) -> Option<ElementId> {
        let tree = self.tree();

        // Check if current element has RenderObject
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

    /// Find nearest ancestor RenderObject of specific type
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
}
