//! A teardown: the render nodes of a subtree of elements that unmount
//! together, each of which leaves the tree with its nearest removed ancestor.
//!
//! Elements unmount deepest first, and each removes its own render object, so
//! without a teardown a row of `k` render nodes is taken out of the tree `k`
//! times: every removal collects a subtree, evicts it from the dirty sets,
//! edits its parent's child list and marks that parent, which is the next to
//! go. Inside a teardown a member whose parent is also a member stays in
//! place, and the parent's removal takes it with the rest of the subtree, as
//! Flutter detaches only the topmost render object of a removed element
//! subtree and disposes the rest in place.

use flui_foundation::RenderId;

use super::PipelineOwner;

/// The teardown open on an owner. One owner for this state: it changes only
/// in `PipelineOwner`'s teardown methods.
#[derive(Debug, Default)]
pub(super) struct Teardown {
    open: bool,
    /// The render nodes of the elements the teardown unmounts, sorted. The
    /// buffer is the caller's, lent for the teardown and handed back at its
    /// end, so a teardown allocates nothing of its own.
    members: Vec<RenderId>,
}

impl Teardown {
    fn has(&self, id: RenderId) -> bool {
        self.members.binary_search(&id).is_ok()
    }
}

impl PipelineOwner {
    /// Opens a teardown of `members`, which it keeps until
    /// [`end_teardown`](Self::end_teardown) hands them back. Returns `members`
    /// unopened when a teardown is already open: that one stays the open one,
    /// and this scope's removals take their nodes out at once.
    pub(super) fn begin_teardown(
        &mut self,
        mut members: Vec<RenderId>,
    ) -> Result<(), Vec<RenderId>> {
        if self.teardown.open {
            return Err(members);
        }
        members.sort_unstable();
        self.teardown = Teardown {
            open: true,
            members,
        };
        Ok(())
    }

    /// Whether the open teardown takes `id` out with its parent, so that
    /// removing it now would only be repeated.
    pub(super) fn teardown_takes(&self, id: RenderId) -> bool {
        self.teardown.open
            && self.teardown.has(id)
            && self
                .render_tree
                .parent(id)
                .is_some_and(|parent| self.teardown.has(parent))
    }

    /// Closes the open teardown, removes each member still in the tree (one
    /// whose element left its render object for an ancestor that then kept
    /// its own), and hands the members' buffer back, empty.
    pub(super) fn end_teardown(&mut self) -> Vec<RenderId> {
        let mut members = std::mem::take(&mut self.teardown).members;
        for &id in &members {
            if self.render_tree.contains(id) {
                self.remove_render_object(id);
            }
        }
        members.clear();
        members
    }

    /// Closes the open teardown without removing anything: its scope unwound,
    /// so the members that are still in the tree belong to elements that
    /// never finished unmounting, and they stay with them.
    pub(super) fn abandon_teardown(&mut self) {
        self.teardown = Teardown::default();
    }
}
