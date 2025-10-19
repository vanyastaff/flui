//! Lifecycle management methods

use super::core::Context;

impl Context {
    /// Mark this element as needing rebuild
    ///
    /// Similar to Flutter's `setState()`.
    pub fn mark_needs_build(&self) {
        let mut tree = self.tree_mut();
        tree.mark_dirty(self.element_id);
    }

    /// Mark element as dirty - short form
    pub fn mark_dirty(&self) {
        self.mark_needs_build()
    }
}
