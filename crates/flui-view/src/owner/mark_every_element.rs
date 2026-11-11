//! Marking every live element for a build: hot reload, and a presentation
//! that starts or stops collecting its semantics tree.

use flui_foundation::ElementId;

use super::{BuildOwner, RebuildReason};
use crate::tree::ElementTree;

impl BuildOwner {
    /// Mark every live element dirty so the next [`build_scope`](Self::build_scope)
    /// re-runs all `build()` methods.
    ///
    /// Used by hot reload. **Does not** unmount elements or dispose `State` — stateful
    /// elements keep their in-tree `ViewState` across the call.
    ///
    /// Both halves are required, and for a while only one was here: the drain
    /// skips any element whose own `is_dirty()` is false (its guard exists so a
    /// clean entry that reached the heap through an inherited-dependency change
    /// cannot have its children reconciled away), so marking the *heap* without
    /// marking the *element* queued work the drain then dropped — a hot reload
    /// that rebuilt nothing. `tree.mark_needs_build` sets the flag;
    /// `schedule_build_for` orders the entry. The external-inbox drain works the
    /// same two-step way.
    pub fn reassemble(&mut self, tree: &mut ElementTree) {
        self.mark_every_element(tree, RebuildReason::HotReload);
        tracing::info!(
            count = self.dirty_count(),
            "BuildOwner::reassemble — all elements marked dirty for hot reload"
        );
    }

    /// Whether the presentation collects its semantics tree; see
    /// [`RenderObjectContext::collects_semantics`](crate::RenderObjectContext::collects_semantics).
    #[must_use]
    pub const fn collects_semantics(&self) -> bool {
        self.collects_semantics
    }

    /// Record whether the presentation collects its semantics tree. A change
    /// marks every live element dirty, so the next
    /// [`build_scope`](Self::build_scope) pushes each render view's
    /// configuration again and what exists only for a collected tree is
    /// registered or released in that pass. Returns whether anything was
    /// marked. The change is rare (assistive technology attaches, an agent
    /// takes the tree), so it costs one full build instead of a dependency
    /// every element with a semantics action would carry.
    pub fn set_collects_semantics(&mut self, tree: &mut ElementTree, collects: bool) -> bool {
        if self.collects_semantics == collects {
            return false;
        }
        self.collects_semantics = collects;
        self.mark_every_element(tree, RebuildReason::DependencyChange) > 0
    }

    /// Mark every live element dirty for `reason`; returns how many.
    fn mark_every_element(&mut self, tree: &mut ElementTree, reason: RebuildReason) -> usize {
        let ids: Vec<(ElementId, usize)> = tree
            .iter_nodes()
            .map(|(id, node)| (id, node.depth()))
            .collect();
        let marked = ids.len();
        for (id, depth) in ids {
            tree.mark_needs_build(id);
            self.schedule_build_for(id, depth, reason);
        }
        marked
    }
}
