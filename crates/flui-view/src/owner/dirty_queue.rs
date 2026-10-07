//! The build drain's dirty-element queue.

use std::{cmp::Reverse, collections::BinaryHeap};

use flui_foundation::ElementId;

/// Entry in the dirty elements heap.
///
/// Ordered shallowest first, then by the order the element was first queued
/// ([`DirtyQueue`]), then by id. A heap gives equal keys no order of its own,
/// so a depth-only key let the build order of siblings follow whatever else
/// happened to be queued — inherited dependents iterated from a hash map —
/// and a fresh subtree's same-depth children (each `Focus` attaching its node
/// in its first build) mounted in an order that changed between runs. The
/// queue order makes a subtree build level by level in child order.
///
/// The field order is the comparison order the derived `Ord` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DirtyElement {
    depth: usize,
    order: u64,
    id: ElementId,
}

impl DirtyElement {
    /// The element id queued for rebuild.
    pub(crate) fn id(&self) -> ElementId {
        self.id
    }

    /// This entry at `depth`, keeping its queue order.
    pub(crate) fn at_depth(self, depth: usize) -> Self {
        Self { depth, ..self }
    }
}

/// The owner's dirty-element heap and the counter that stamps each newly
/// queued entry with its queue order.
///
/// An entry keeps its stamp while it moves between the active heap, a
/// layout-builder scope bucket and back, or is re-keyed to a new depth, so
/// moving it never reorders it among its peers.
#[derive(Debug, Default)]
pub(crate) struct DirtyQueue {
    heap: BinaryHeap<Reverse<DirtyElement>>,
    next_order: u64,
}

impl DirtyQueue {
    /// Queue `id` behind every entry already queued at `depth`.
    pub(crate) fn push(&mut self, id: ElementId, depth: usize) {
        let order = self.next_order;
        // Saturating: past 2^64 stamps the id still orders the tie.
        self.next_order = order.saturating_add(1);
        self.heap.push(Reverse(DirtyElement { depth, order, id }));
    }

    /// Return a popped or deferred entry with its original queue order.
    pub(crate) fn requeue(&mut self, dirty: DirtyElement) {
        self.heap.push(Reverse(dirty));
    }

    /// The shallowest, earliest-queued entry.
    pub(crate) fn pop(&mut self) -> Option<DirtyElement> {
        self.heap.pop().map(|Reverse(dirty)| dirty)
    }

    /// Remove every entry, keeping the order counter.
    pub(crate) fn take_entries(&mut self) -> impl Iterator<Item = DirtyElement> + use<> {
        std::mem::take(&mut self.heap)
            .into_iter()
            .map(|Reverse(dirty)| dirty)
    }

    /// Move every entry of `other` into this queue.
    pub(crate) fn append(&mut self, other: &mut BinaryHeap<Reverse<DirtyElement>>) {
        self.heap.append(other);
    }

    pub(crate) fn len(&self) -> usize {
        self.heap.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}
