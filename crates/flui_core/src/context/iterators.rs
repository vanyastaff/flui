//! Iterator patterns for traversing the element tree

use crate::ElementId;
use crate::tree::ElementTree;

/// Iterator over ancestor elements
///
/// Iterates from parent to root.
///
/// # Example
///
/// ```rust,ignore
/// let depth = context.ancestors().count();
/// let container = context.ancestors().find(|&id| /* check */);
/// ```
pub struct Ancestors<'a> {
    pub(super) tree: parking_lot::RwLockReadGuard<'a, ElementTree>,
    pub(super) current: Option<ElementId>,
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = ElementId;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let current_id = self.current?;

        // Get parent before returning current
        if let Some(element) = self.tree.get(current_id) {
            let parent_id = element.parent();
            self.current = parent_id;
            Some(current_id)
        } else {
            self.current = None;
            None
        }
    }
}

/// Iterator over child elements
///
/// Iterates over direct children of an element.
/// Collects children into a Vec to avoid lifetime issues.
///
/// # Example
///
/// ```rust,ignore
/// let child_count = context.children().count();
/// for child_id in context.children() {
///     println!("Child: {:?}", child_id);
/// }
/// ```
pub struct Children {
    pub(super) children: Vec<ElementId>,
    pub(super) index: usize,
}

impl Iterator for Children {
    type Item = ElementId;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.children.len() {
            let id = self.children[self.index];
            self.index += 1;
            Some(id)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.children.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Children {
    #[inline]
    fn len(&self) -> usize {
        self.children.len() - self.index
    }
}

/// Iterator over descendant elements (depth-first)
///
/// Iterates over all descendants in depth-first order.
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
///         context.tree().get(id)
///             .map(|e| e.is_dirty())
///             .unwrap_or(false)
///     });
/// ```
pub struct Descendants<'a> {
    pub(super) tree: parking_lot::RwLockReadGuard<'a, ElementTree>,
    pub(super) stack: Vec<ElementId>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        let current_id = self.stack.pop()?;

        // Add children to stack (in reverse order for correct depth-first)
        if let Some(element) = self.tree.get(current_id) {
            let children: Vec<_> = element.children_iter().collect();
            // Push in reverse order so first child is processed first
            for child_id in children.into_iter().rev() {
                self.stack.push(child_id);
            }
        }

        Some(current_id)
    }
}
