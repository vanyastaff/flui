//! What an action changed in the published tree (R16): the nodes that
//! appeared, the ones that are gone, and the ones whose description changed,
//! matched by element id.
//!
//! A [`TreeSnapshot`] is the tree as a read reports it, flattened by id, with
//! the revision it was read on. Taking one is a read; comparing two is pure.
//! The element id is the generational id of the node's render object, so a
//! node whose slot was reused is a different id: it is `gone` and a new one
//! `appeared`, never a change.

use std::collections::HashMap;

use flui_protocol::{
    ChangedField, ElementId, ElementRef, Node, NodeChange, ReadQuery, SnapshotDelta, Tree,
    TreeRevision, WindowId,
};

use super::{Placement, WireReadError};
use crate::owner::SemanticsOwner;

/// The most nodes a snapshot keeps; a window with more is truncated and its
/// delta says so.
const SNAPSHOT_NODES: usize = 4096;

/// The tree of one window at one revision, flattened for comparison.
#[derive(Debug, Clone)]
pub struct TreeSnapshot {
    window: WindowId,
    revision: TreeRevision,
    /// The nodes without children, in tree order.
    nodes: Vec<Node>,
    by_id: HashMap<ElementId, usize>,
    /// The read stopped at the node budget.
    truncated: bool,
}

impl TreeSnapshot {
    fn new(window: WindowId, revision: TreeRevision, tree: &Tree) -> Self {
        fn flatten(nodes: &[Node], out: &mut Vec<Node>) {
            for node in nodes {
                let mut flat = node.clone();
                flat.children = Vec::new();
                flat.omitted_children = None;
                flat.children_unread = false;
                out.push(flat);
                flatten(&node.children, out);
            }
        }
        let mut flat = Vec::new();
        flatten(&tree.roots, &mut flat);
        let by_id = flat
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();
        Self {
            window,
            revision,
            nodes: flat,
            by_id,
            truncated: tree.truncated,
        }
    }

    /// The revision of the window's tree this was read on.
    #[must_use]
    pub fn revision(&self) -> TreeRevision {
        self.revision
    }

    /// What differs between this snapshot and `later`, which must be of the
    /// same window. Bounds-only changes are left out unless `bounds`, so a
    /// scroll does not flood the delta. At most `cap` nodes are reported
    /// across `appeared`, `gone` and `changed`; past it the delta is
    /// `truncated` and the caller reads the tree.
    #[must_use]
    pub fn delta_to(&self, later: &Self, bounds: bool, cap: usize) -> SnapshotDelta {
        let mut delta = SnapshotDelta::new(self.revision, later.revision);
        delta.truncated = self.truncated || later.truncated;
        let mut room = cap;
        let mut take = |delta: &mut SnapshotDelta| {
            if room == 0 {
                delta.truncated = true;
                false
            } else {
                room -= 1;
                true
            }
        };
        for node in &later.nodes {
            match self.by_id.get(&node.id).map(|&index| &self.nodes[index]) {
                Some(before) if before.role == node.role => {
                    let changed = changed_fields(before, node, bounds);
                    if !changed.is_empty() && take(&mut delta) {
                        delta.changed.push(NodeChange::new(node.clone(), changed));
                    }
                }
                // A node whose role changed is a different kind of element
                // under the same id: it appeared.
                _ => {
                    if take(&mut delta) {
                        delta.appeared.push(node.clone());
                    }
                }
            }
        }
        for node in &self.nodes {
            if !later.by_id.contains_key(&node.id) && take(&mut delta) {
                delta.gone.push(ElementRef::new(self.window, node.id));
            }
        }
        delta
    }
}

/// The fields of `after` that differ from `before`, in the vocabulary's order.
fn changed_fields(before: &Node, after: &Node, bounds: bool) -> Vec<ChangedField> {
    let mut changed = Vec::new();
    if before.name != after.name {
        changed.push(ChangedField::Name);
    }
    if before.value != after.value {
        changed.push(ChangedField::Value);
    }
    if before.description != after.description {
        changed.push(ChangedField::Description);
    }
    if (
        before.disabled,
        before.focused,
        before.focusable,
        before.checked,
        before.expanded,
        before.selected,
    ) != (
        after.disabled,
        after.focused,
        after.focusable,
        after.checked,
        after.expanded,
        after.selected,
    ) {
        changed.push(ChangedField::State);
    }
    if before.actions != after.actions {
        changed.push(ChangedField::Actions);
    }
    if bounds && (before.rect != after.rect || before.surface_rect != after.surface_rect) {
        changed.push(ChangedField::Bounds);
    }
    changed
}

impl SemanticsOwner {
    /// The window's tree as a read reports it now, for a later
    /// [`TreeSnapshot::delta_to`].
    ///
    /// # Errors
    ///
    /// [`WireReadError::NoTree`] before the first assembly,
    /// [`WireReadError::Malformed`] when the published tree breaks
    /// AccessKit's contract.
    pub fn snapshot_wire(&self, placement: Placement) -> Result<TreeSnapshot, WireReadError> {
        let revision = self.revision();
        let mut query = ReadQuery::new();
        query.max_nodes = Some(SNAPSHOT_NODES);
        let tree = self.read_wire(&query, placement)?;
        Ok(TreeSnapshot::new(placement.window, revision, &tree))
    }
}
