//! Iterator patterns for traversing the element tree
//!
//! Rust-idiomatic iterators for walking ancestors, descendants, etc.


use crate::ElementId;
use crate::tree::ElementTree;

/// Iterator over ancestor elements
///
/// Iterates from the immediate parent up to the root.
///
/// # Example
///
/// ```rust,ignore
/// // Count ancestors
/// let depth = context.ancestors().count();
///
/// // Find first matching ancestor
/// let container = context.ancestors()
///     .find(|&id| /* check condition */);
/// ```
pub struct Ancestors<'a> {
    pub(super) tree: parking_lot::RwLockReadGuard<'a, ElementTree>,
    pub(super) current: Option<ElementId>,
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = ElementId;

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
