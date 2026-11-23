//! Partitioning of the dirty queue by layout-builder scope, so a layout builder drains only its own subtree.

use super::super::dirty_queue::DirtyElement;
use super::BuildOwner;
use crate::tree::ElementTree;
use flui_foundation::{ElementId, SlotSet};
use rustc_hash::FxHashMap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[cfg(test)]
thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static LAYOUT_SCOPE_CLASSIFICATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static LAYOUT_SCOPE_HOPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildScopeTarget {
    /// Drain every dirty element. Used when no layout-builder scopes exist.
    All,
    /// Drain only elements outside every live layout-builder scope.
    Global,
    /// Drain the builder itself and descendants whose nearest live scope is it.
    LayoutBuilder(ElementId),
}

#[derive(Debug, Default)]
pub(crate) struct BuildScopeQueues {
    /// Dirty work currently outside every live layout-builder scope.
    pub(super) root: BinaryHeap<Reverse<DirtyElement>>,
    /// Dirty work keyed by its current nearest live layout-builder scope.
    pub(super) isolated: FxHashMap<ElementId, BinaryHeap<Reverse<DirtyElement>>>,
    /// Reused while rerouting after scope removal or tree reparenting.
    scratch: Vec<DirtyElement>,
    /// Nearest scope per element, reused by each repartition.
    scope_memo: super::super::dirty_queue::ScopeMemo,
}

impl BuildOwner {
    /// Re-key every queued dirty element to its authoritative TREE depth.
    ///
    /// The dirty heap orders by depth, but `schedule_build_for` is handed a
    /// depth by its caller and trusts it. A caller that passes a stale or
    /// wrong depth would let a deeply-nested rebuild sort as if it were
    /// shallow, so a child could rebuild before its parent — violating
    /// the shallowest-first contract. Rebuilding the heap keyed on each
    /// node's current depth (`ElementNode::depth`, the same authority the
    /// external-inbox drain uses) keeps the contract regardless of what
    /// `schedule_build_for` was told. Each entry keeps its queue order.
    pub(super) fn rekey_dirty_depths(&mut self, tree: &ElementTree) {
        self.dirty_elements
            .rekey(|id| tree.get(id).map_or(0, crate::tree::ElementNode::depth));
    }

    /// Return the nearest mounted layout-builder scope containing `element`.
    /// The search includes `element` itself: a scope root belongs to its own
    /// scope.
    pub(super) fn live_layout_scope_ids(&self, tree: &ElementTree) -> SlotSet<ElementId> {
        self.layout_builder_registry.with_entries(|entries| {
            let mut live = SlotSet::new();
            for entry in entries
                .values()
                .filter(|entry| tree.contains(entry.element))
            {
                live.insert(entry.element);
            }
            live
        })
    }

    pub(super) fn nearest_layout_builder_scope(
        tree: &ElementTree,
        element: ElementId,
        live_scopes: &SlotSet<ElementId>,
    ) -> Option<ElementId> {
        #[cfg(test)]
        LAYOUT_SCOPE_CLASSIFICATIONS.with(|count| count.set(count.get() + 1));
        let mut cursor = Some(element);
        while let Some(candidate) = cursor {
            #[cfg(test)]
            LAYOUT_SCOPE_HOPS.with(|count| count.set(count.get() + 1));
            if live_scopes.contains(candidate) && tree.contains(candidate) {
                return Some(candidate);
            }
            cursor = tree
                .get(candidate)
                .and_then(crate::tree::ElementNode::parent);
        }
        None
    }

    pub(super) fn defer_dirty_element(&mut self, scope: Option<ElementId>, dirty: DirtyElement) {
        let queues = self
            .build_scope_queues
            .get_or_insert_with(|| Box::new(BuildScopeQueues::default()));
        if let Some(scope) = scope {
            queues
                .isolated
                .entry(scope)
                .or_default()
                .push(Reverse(dirty));
        } else {
            queues.root.push(Reverse(dirty));
        }
    }

    fn collect_partitioned_dirty(&mut self) {
        let mut scratch = self
            .build_scope_queues
            .as_mut()
            .map_or_else(Vec::new, |queues| std::mem::take(&mut queues.scratch));
        if let Some(queues) = self.build_scope_queues.as_mut() {
            scratch.extend(
                std::mem::take(&mut queues.root)
                    .into_iter()
                    .map(|Reverse(dirty)| dirty),
            );
            for bucket in std::mem::take(&mut queues.isolated).into_values() {
                scratch.extend(bucket.into_iter().map(|Reverse(dirty)| dirty));
            }
        }
        scratch.extend(self.dirty_elements.take_entries());
        self.build_scope_queues
            .get_or_insert_with(|| Box::new(BuildScopeQueues::default()))
            .scratch = scratch;
    }

    pub(crate) fn repartition_dirty(
        &mut self,
        tree: &ElementTree,
        live_scopes: &SlotSet<ElementId>,
    ) {
        self.collect_partitioned_dirty();
        let mut memo = std::mem::take(
            &mut self
                .build_scope_queues
                .as_mut()
                .expect("BUG: repartition creates scoped queues")
                .scope_memo,
        );
        memo.clear();
        while let Some(mut dirty) = self
            .build_scope_queues
            .as_mut()
            .expect("BUG: repartition creates scoped queues")
            .scratch
            .pop()
        {
            let depth = tree
                .get(dirty.id())
                .map_or(0, crate::tree::ElementNode::depth);
            dirty = dirty.at_depth(depth);
            #[cfg(test)]
            LAYOUT_SCOPE_CLASSIFICATIONS.with(|count| count.set(count.get() + 1));
            let scope = memo.nearest(
                dirty.id(),
                |id| live_scopes.contains(id) && tree.contains(id),
                |id| {
                    #[cfg(test)]
                    LAYOUT_SCOPE_HOPS.with(|count| count.set(count.get() + 1));
                    tree.get(id).and_then(crate::tree::ElementNode::parent)
                },
            );
            self.defer_dirty_element(scope, dirty);
        }
        if let Some(queues) = self.build_scope_queues.as_mut() {
            queues.scope_memo = memo;
        }
    }

    pub(crate) fn activate_build_target(&mut self, target: BuildScopeTarget) {
        let Some(queues) = self.build_scope_queues.as_mut() else {
            return;
        };
        let mut active = match target {
            BuildScopeTarget::All => return,
            BuildScopeTarget::Global => std::mem::take(&mut queues.root),
            BuildScopeTarget::LayoutBuilder(scope) => {
                queues.isolated.remove(&scope).unwrap_or_default()
            }
        };
        self.dirty_elements.append(&mut active);
    }

    pub(crate) fn ready_layout_scopes(&self) -> Vec<ElementId> {
        self.build_scope_queues
            .as_ref()
            .map_or_else(Vec::new, |queues| queues.isolated.keys().copied().collect())
    }

    pub(crate) fn has_pending_layout_scope(&self, scope: ElementId) -> bool {
        self.build_scope_queues
            .as_ref()
            .is_some_and(|queues| queues.isolated.contains_key(&scope))
    }

    pub(crate) fn has_pending_global_builds(&self) -> bool {
        self.build_scope_queues
            .as_ref()
            .is_some_and(|queues| !queues.root.is_empty())
    }

    pub(crate) fn has_partitioned_builds(&self) -> bool {
        self.build_scope_queues.as_ref().is_some_and(|queues| {
            !queues.root.is_empty() || queues.isolated.values().any(|bucket| !bucket.is_empty())
        })
    }

    pub(crate) fn collapse_empty_build_scope_queues(&mut self) {
        let is_empty = self.build_scope_queues.as_ref().is_some_and(|queues| {
            queues.root.is_empty() && queues.isolated.is_empty() && queues.scratch.is_empty()
        });
        if is_empty {
            self.build_scope_queues = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn reset_layout_scope_classifications() {
        LAYOUT_SCOPE_CLASSIFICATIONS.with(|count| count.set(0));
    }

    #[cfg(test)]
    pub(crate) fn layout_scope_classifications() -> usize {
        LAYOUT_SCOPE_CLASSIFICATIONS.with(std::cell::Cell::get)
    }

    /// Resets and returns the parent-link steps scope classification took.
    #[cfg(test)]
    pub(crate) fn take_layout_scope_hops() -> usize {
        LAYOUT_SCOPE_HOPS.with(|count| count.replace(0))
    }
}
