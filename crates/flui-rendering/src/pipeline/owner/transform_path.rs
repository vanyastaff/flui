//! Owner-local reusable ancestry storage, released before authored transforms.

use std::cell::RefCell;

use flui_foundation::RenderId;

/// A checkout owns its vector outright. Reentrant read-only queries acquire a
/// different buffer, so neither an arena borrow nor a cache borrow spans user
/// code. Every exit, including unwinding, returns the allocation to this owner.
pub(super) struct TransformPath<'a> {
    cache: &'a RefCell<Vec<Vec<RenderId>>>,
    pub(super) nodes: Vec<RenderId>,
}

impl<'a> TransformPath<'a> {
    pub(super) fn new(cache: &'a RefCell<Vec<Vec<RenderId>>>) -> Self {
        let nodes = cache.borrow_mut().pop().unwrap_or_default();
        Self { cache, nodes }
    }
}

impl Drop for TransformPath<'_> {
    fn drop(&mut self) {
        self.nodes.clear();
        self.cache
            .borrow_mut()
            .push(std::mem::take(&mut self.nodes));
    }
}
