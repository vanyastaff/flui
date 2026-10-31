//! Slot storage for the render tree, with a lease for the layout walk.
//!
//! A [`NodeSlab`] is a `Vec` of slots with a free list. Each slot has a header (an in-flight bit and
//! the parent link, both outside the `UnsafeCell`) and a state that is either vacant or owns a
//! [`RenderNode`]. The layout walk takes one [`SlabLease`] and acquires slots one at a time as it
//! reaches them, instead of borrowing every node of the dirty subtree up front.
//!
//! # Lease invariants
//!
//! 1. A lease holds the slab's exclusive borrow for its whole life, so nothing inserts, removes or
//!    reallocates while it exists and every slot pointer stays valid.
//! 2. Every slot pointer derives from the one base pointer taken from `Vec::as_mut_ptr`, so each
//!    carries whole-allocation provenance.
//! 3. A slot's `&mut RenderNode` is created only by the walk and only for a slot whose in-flight
//!    bit it just set, so at most one is live per slot.
//! 4. The headers are read and written through field projections of the raw slot pointer, never
//!    through a reference to the whole slot, so reading the header of a slot whose node is borrowed
//!    up the call stack touches no memory that borrow covers.

#![expect(unsafe_code)]

use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;

use flui_foundation::RenderId;

use super::node::RenderNode;

/// What a slot holds.
#[expect(
    clippy::large_enum_variant,
    reason = "a vacant slot is the rare, short-lived state; boxing the node would put an allocation on every insert"
)]
enum SlotState {
    /// No node. `next_free` is the next vacant slot in the free list.
    Vacant {
        next_free: Option<u32>,
    },
    Occupied(RenderNode),
}

struct Slot {
    /// Set while a layout walk holds the slot's `&mut RenderNode`.
    in_flight: Cell<bool>,
    /// The occupant's parent, written by [`NodeSlab::set_parent`] alone. It lives here and not in
    /// the node, so the walk can check a parent-to-child edge without reading a node that is
    /// borrowed.
    parent: Cell<Option<RenderId>>,
    state: UnsafeCell<SlotState>,
}

/// Render-node storage: indices are slot numbers, validity is the caller's generation table.
pub(super) struct NodeSlab {
    slots: Vec<Slot>,
    free_head: Option<u32>,
    len: usize,
}

impl std::fmt::Debug for NodeSlab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeSlab")
            .field("len", &self.len)
            .field("slots", &self.slots.len())
            .finish_non_exhaustive()
    }
}

impl NodeSlab {
    pub(super) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
        }
    }

    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            free_head: None,
            len: 0,
        }
    }

    pub(super) const fn len(&self) -> usize {
        self.len
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn reserve(&mut self, additional: usize) {
        self.slots.reserve(additional);
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        self.get(index).is_some()
    }

    pub(super) fn get(&self, index: usize) -> Option<&RenderNode> {
        let slot = self.slots.get(index)?;
        // SAFETY: `&self` excludes a lease (a lease holds `&mut NodeSlab`) and every `&mut` into
        // a slot, so a shared read of the state is the only access.
        match unsafe { &*slot.state.get() } {
            SlotState::Occupied(node) => Some(node),
            SlotState::Vacant { .. } => None,
        }
    }

    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut RenderNode> {
        match self.slots.get_mut(index)?.state.get_mut() {
            SlotState::Occupied(node) => Some(node),
            SlotState::Vacant { .. } => None,
        }
    }

    /// Stores `node` in the first free slot, or a new one, and returns its index.
    pub(super) fn insert(&mut self, node: RenderNode) -> usize {
        self.len += 1;
        if let Some(head) = self.free_head {
            let index = head as usize;
            let slot = &mut self.slots[index];
            let SlotState::Vacant { next_free } = *slot.state.get_mut() else {
                unreachable!("BUG: the free list names an occupied slot");
            };
            self.free_head = next_free;
            *slot.state.get_mut() = SlotState::Occupied(node);
            slot.parent.set(None);
            return index;
        }
        let index = self.slots.len();
        assert!(
            u32::try_from(index).is_ok(),
            "BUG: render tree exceeds u32::MAX slots; RenderId index field is 32 bits"
        );
        self.slots.push(Slot {
            in_flight: Cell::new(false),
            parent: Cell::new(None),
            state: UnsafeCell::new(SlotState::Occupied(node)),
        });
        index
    }

    /// Empties the slot and returns its node; the caller drops it, after its own bookkeeping.
    pub(super) fn try_remove(&mut self, index: usize) -> Option<RenderNode> {
        let slot = self.slots.get_mut(index)?;
        if !matches!(slot.state.get_mut(), SlotState::Occupied(_)) {
            return None;
        }
        let vacant = SlotState::Vacant {
            next_free: self.free_head,
        };
        let SlotState::Occupied(node) = std::mem::replace(slot.state.get_mut(), vacant) else {
            unreachable!("BUG: checked occupied just above");
        };
        slot.parent.set(None);
        slot.in_flight.set(false);
        self.free_head = Some(u32::try_from(index).expect("BUG: checked at insert"));
        self.len -= 1;
        Some(node)
    }

    pub(super) fn clear(&mut self) {
        // Moved out first so a panicking `Drop` leaves an empty slab.
        let slots = std::mem::take(&mut self.slots);
        self.free_head = None;
        self.len = 0;
        drop(slots);
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &RenderNode)> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            // SAFETY: as in `get`.
            match unsafe { &*slot.state.get() } {
                SlotState::Occupied(node) => Some((index, node)),
                SlotState::Vacant { .. } => None,
            }
        })
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut RenderNode)> + '_ {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| match slot.state.get_mut() {
                SlotState::Occupied(node) => Some((index, node)),
                SlotState::Vacant { .. } => None,
            })
    }

    /// The occupant's parent as the slot header holds it.
    pub(super) fn parent(&self, index: usize) -> Option<RenderId> {
        self.slots.get(index)?.parent.get()
    }

    /// Records the occupant's parent in the slot header, where the link lives.
    pub(super) fn set_parent(&mut self, index: usize, parent: Option<RenderId>) {
        if let Some(slot) = self.slots.get(index) {
            slot.parent.set(parent);
        }
    }

    /// Takes the exclusive borrow for a layout walk.
    pub(super) fn lease(&mut self) -> SlabLease<'_> {
        SlabLease {
            base: self.slots.as_mut_ptr(),
            len: self.slots.len(),
            _slab: PhantomData,
        }
    }
}

/// The layout walk's view of a [`NodeSlab`]; see the module docs for the invariants.
pub(super) struct SlabLease<'a> {
    base: *mut Slot,
    len: usize,
    _slab: PhantomData<&'a mut NodeSlab>,
}

impl SlabLease<'_> {
    /// The slot pointer, or `None` past the end.
    fn slot(&self, index: usize) -> Option<*mut Slot> {
        // SAFETY: `index < len` keeps the offset inside the allocation `base` came from.
        (index < self.len).then(|| unsafe { self.base.add(index) })
    }

    /// `true` while the slot's node is borrowed up the call stack.
    pub(super) fn is_in_flight(&self, index: usize) -> bool {
        self.slot(index).is_some_and(|slot| {
            // SAFETY: a field projection of a slot inside the leased allocation; the header is
            // outside the `UnsafeCell` (invariant 4).
            unsafe { (*slot).in_flight.get() }
        })
    }

    /// Sets the in-flight bit; `false` if it was already set, or the slot does not exist.
    pub(super) fn try_enter(&self, index: usize) -> bool {
        let Some(slot) = self.slot(index) else {
            return false;
        };
        // SAFETY: as in `is_in_flight`.
        unsafe { !(*slot).in_flight.replace(true) }
    }

    /// Clears the in-flight bit.
    pub(super) fn leave(&self, index: usize) {
        if let Some(slot) = self.slot(index) {
            // SAFETY: as in `is_in_flight`.
            unsafe { (*slot).in_flight.set(false) }
        }
    }

    /// The occupant's parent as the header holds it. Reads no node.
    pub(super) fn parent(&self, index: usize) -> Option<RenderId> {
        let slot = self.slot(index)?;
        // SAFETY: as in `is_in_flight`.
        unsafe { (*slot).parent.get() }
    }

    /// A pointer to the occupant, or `None` for a vacant or missing slot.
    ///
    /// The caller must hold no live `&mut RenderNode` for this slot (it is not in flight, or it is
    /// the caller's own in-flight slot); the returned pointer is then the only access to it.
    pub(super) fn node(&self, index: usize) -> Option<*mut RenderNode> {
        let slot = self.slot(index)?;
        // SAFETY: the state cell is a field projection of a slot inside the leased allocation;
        // the match reads the discriminant through the raw place and takes `&mut` to the payload
        // only, turned into a pointer at once. No other reference to this slot's state is live
        // (the contract above).
        unsafe {
            let state = UnsafeCell::raw_get(&raw const (*slot).state);
            match *state {
                SlotState::Occupied(ref mut node) => Some(std::ptr::from_mut(node)),
                SlotState::Vacant { .. } => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::Leaf;
    use flui_foundation::geometry::Size;

    use super::*;
    use crate::{context::BoxLayoutContext, parent_data::BoxParentData, traits::RenderBox};

    #[derive(Debug, Default)]
    struct LeafStub;
    impl flui_foundation::inspect::Inspect for LeafStub {
        fn kind(&self) -> &'static str {
            "LeafStub"
        }

        fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
    }
    impl RenderBox for LeafStub {
        type Arity = Leaf;
        type ParentData = BoxParentData;
        fn perform_layout(
            &mut self,
            _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
        ) -> crate::RenderResult<Size> {
            Ok(Size::new(10.0, 10.0))
        }
        fn paint(&self, _ctx: &mut crate::context::PaintCx<'_, Leaf>) {}
    }

    fn node() -> RenderNode {
        RenderNode::new_box(Box::new(LeafStub))
    }

    #[test]
    fn lease_refuses_a_double_enter_and_slots_are_reused() {
        let mut slab = NodeSlab::new();
        let a = slab.insert(node());
        let b = slab.insert(node());
        assert_eq!(slab.len(), 2);
        assert!(slab.try_remove(a).is_some());
        assert!(slab.try_remove(a).is_none(), "a vacant slot yields nothing");
        assert_eq!(slab.insert(node()), a, "the free list hands the slot back");

        let lease = slab.lease();
        assert!(lease.try_enter(b));
        assert!(!lease.try_enter(b), "a second enter is refused");
        assert!(lease.is_in_flight(b));
        lease.leave(b);
        assert!(!lease.is_in_flight(b));
        assert!(lease.node(b).is_some());
        assert!(lease.node(99).is_none());
    }

    #[test]
    fn lease_reads_the_header_parent_while_a_node_is_borrowed() {
        let mut slab = NodeSlab::new();
        let a = slab.insert(node());
        let b = slab.insert(node());
        let parent = RenderId::new_gen(0, std::num::NonZeroU32::MIN);
        slab.set_parent(b, Some(parent));

        let lease = slab.lease();
        assert!(lease.try_enter(a));
        let held = lease.node(a).expect("occupied");
        // SAFETY: the slot is in flight and this is the only access to it.
        let _live: &mut RenderNode = unsafe { &mut *held };
        assert_eq!(lease.parent(b), Some(parent));
        assert!(lease.is_in_flight(a));
    }
}
