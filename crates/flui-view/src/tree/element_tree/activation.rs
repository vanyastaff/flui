//! Deactivation and reactivation of subtrees.

use flui_foundation::ElementId;
use flui_foundation::panic::FirstPanic;

use super::ElementTree;

impl ElementTree {
    /// Reactivate a subtree and schedule dependency lifecycle work for every
    /// node with an inherited read, including a lookup that found no provider.
    pub(super) fn activate_subtree(
        &mut self,
        root: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.get(id) else {
                continue;
            };
            let depth = node.depth();
            let children = self.owned_children(id, owner.child_manager_registry);

            if let Some(node) = self.get_mut(id) {
                node.element_mut().activate(owner);
            }
            if owner.activate_inherited_dependent(id) {
                owner.note_dependency_change(id);
                owner.schedule_build_for(id, depth, crate::RebuildReason::DependencyChange);
            }

            stack.extend(children.into_iter().rev());
        }
    }

    /// Deactivate a subtree and synchronously release every inherited edge.
    ///
    /// Provider maps are clean before this returns, so a notification later in
    /// the same frame cannot enqueue an inactive historical dependent.
    pub(super) fn deactivate_subtree(
        &mut self,
        root: ElementId,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let children = self.owned_children(id, owner.child_manager_registry);
            self.deactivate_inherited_dependencies(id, owner);
            if let Some(node) = self.get_mut(id) {
                node.element_mut().deactivate(owner);
            }
            stack.extend(children.into_iter().rev());
        }
    }

    /// Deactivate an element (temporary removal).
    pub fn deactivate(&mut self, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        self.deactivate_subtree(id, owner);
    }

    /// Activate an element (re-insertion after deactivation).
    pub fn activate(&mut self, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
        let mut caught = FirstPanic::new();
        caught.contain(|| self.activate_subtree(id, owner));
        if caught.is_held() {
            // Public activation has no substituting caller to consume the
            // handoff. Clear it before resuming so a caught unwind cannot
            // suppress an unrelated later recovery on this owner.
            owner.take_staged_lifecycle_panic();
            caught.resume();
        }
    }
}
