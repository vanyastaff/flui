//! Slot storage for the render tree, with a lease for the layout walk.
//!
//! A [`NodeSlab`] is a `Vec` of slots with a free list, and the only place that mints a
//! [`RenderId`] or empties a slot. Each slot has a header (its generation, an in-flight bit and
//! the parent link, all outside the `UnsafeCell`) and a state: vacant, occupied by a
//! [`RenderNode`], or retired. The layout walk takes one [`TreeLease`] and acquires slots one at a
//! time as it reaches them, instead of borrowing every node of the dirty subtree up front.
//!
//! # Generations
//!
//! A slot's generation is its occupant's, or, while the slot is vacant, the one its next id will
//! carry. Emptying a slot advances it, so every id of the old occupant goes stale. Ids carry
//! generations up to [`Generation::LAST`], below [`RETIRED`]; a slot whose generation cannot
//! advance past its last takes [`RETIRED`] instead and leaves circulation for good. An id
//! therefore matches its slot's
//! generation exactly while its node occupies the slot, and that comparison is the whole check
//! a lease makes.
//!
//! # Lease invariants
//!
//! 1. A lease holds the slab's exclusive borrow for its whole life, so nothing inserts, removes or
//!    reallocates while it exists, every slot pointer stays valid and no generation changes.
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
use std::num::NonZeroU32;

use flui_foundation::RenderId;
use flui_foundation::generation::Generation;

use super::node::RenderNode;

/// The header of a slot that has used every generation. It is above [`Generation::LAST`], so no
/// id carries it and no id matches a retired slot.
const RETIRED: NonZeroU32 = NonZeroU32::MAX;

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
    /// Out of generations: no node, and never on the free list again.
    Retired,
}

struct Slot {
    /// The occupant's generation, the next id's while vacant, [`RETIRED`] once retired. Written
    /// through `&mut NodeSlab` alone.
    generation: NonZeroU32,
    /// Set while a layout walk holds the slot's `&mut RenderNode`.
    in_flight: Cell<bool>,
    /// The occupant's parent, written by [`NodeSlab::set_parent`] alone. It lives here and not in
    /// the node, so the walk can check a parent-to-child edge without reading a node that is
    /// borrowed.
    parent: Cell<Option<RenderId>>,
    state: UnsafeCell<SlotState>,
}

/// The id of the node in slot `index` at `generation`.
fn id_at(index: usize, generation: NonZeroU32) -> RenderId {
    let index = u32::try_from(index).expect("BUG: a slot index is checked against u32 at insert");
    let generation = Generation::try_from(generation)
        .expect("BUG: an occupied slot's generation is at most Generation::LAST");
    RenderId::new_gen(index, generation)
}

/// Render-node storage. Ids are slot indices with the slot's generation.
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

    /// The slot `id`'s node occupies, or `None` for a stale id.
    fn resolve(&self, id: RenderId) -> Option<&Slot> {
        self.slots
            .get(id.index() as usize)
            .filter(|slot| slot.generation == id.generation().as_nonzero())
    }

    pub(super) fn contains(&self, id: RenderId) -> bool {
        self.get(id).is_some()
    }

    pub(super) fn get(&self, id: RenderId) -> Option<&RenderNode> {
        let slot = self.resolve(id)?;
        // SAFETY: `&self` excludes a lease (a lease holds `&mut NodeSlab`) and every `&mut` into
        // a slot, so a shared read of the state is the only access.
        match unsafe { &*slot.state.get() } {
            SlotState::Occupied(node) => Some(node),
            SlotState::Vacant { .. } | SlotState::Retired => None,
        }
    }

    pub(super) fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode> {
        let slot = self
            .slots
            .get_mut(id.index() as usize)
            .filter(|slot| slot.generation == id.generation().as_nonzero())?;
        match slot.state.get_mut() {
            SlotState::Occupied(node) => Some(node),
            SlotState::Vacant { .. } | SlotState::Retired => None,
        }
    }

    /// Stores `node` in the first free slot, or a new one, and mints its id.
    pub(super) fn insert(&mut self, node: RenderNode) -> RenderId {
        let index = if let Some(head) = self.free_head {
            let slot = &mut self.slots[head as usize];
            let SlotState::Vacant { next_free } = *slot.state.get_mut() else {
                unreachable!("BUG: the free list names a slot that is not vacant");
            };
            *slot.state.get_mut() = SlotState::Occupied(node);
            self.free_head = next_free;
            head as usize
        } else {
            let index = self.slots.len();
            u32::try_from(index)
                .expect("BUG: render tree exceeds u32::MAX slots; RenderId index field is 32 bits");
            self.slots.push(Slot {
                generation: NonZeroU32::MIN,
                in_flight: Cell::new(false),
                parent: Cell::new(None),
                state: UnsafeCell::new(SlotState::Occupied(node)),
            });
            index
        };
        self.len += 1;
        id_at(index, self.slots[index].generation)
    }

    /// Empties the slot `id`'s node occupies and returns the node, which the caller drops after
    /// its own bookkeeping. `None` for a stale id.
    pub(super) fn remove(&mut self, id: RenderId) -> Option<RenderNode> {
        self.resolve(id)?;
        self.vacate(id.index() as usize)
    }

    /// Takes every node out, leaving each slot vacant under its next generation or retired, and
    /// returns the nodes for the caller to drop after its own bookkeeping.
    pub(super) fn take_all(&mut self) -> Vec<RenderNode> {
        let mut nodes = Vec::with_capacity(self.len);
        for index in 0..self.slots.len() {
            nodes.extend(self.vacate(index));
        }
        nodes
    }

    /// Empties an occupied slot and advances its generation, or retires the slot when no
    /// generation below [`RETIRED`] is left. Nothing after the first write can fail, so the slot,
    /// its generation and the free list are consistent on every exit.
    fn vacate(&mut self, index: usize) -> Option<RenderNode> {
        let slot = self.slots.get_mut(index)?;
        if !matches!(slot.state.get_mut(), SlotState::Occupied(_)) {
            return None;
        }
        let link =
            u32::try_from(index).expect("BUG: a slot index is checked against u32 at insert");
        let next = Generation::try_from(slot.generation)
            .ok()
            .and_then(Generation::next)
            .map(Generation::as_nonzero);
        let emptied = match next {
            Some(_) => SlotState::Vacant {
                next_free: self.free_head,
            },
            None => SlotState::Retired,
        };
        let SlotState::Occupied(node) = std::mem::replace(slot.state.get_mut(), emptied) else {
            unreachable!("BUG: checked occupied just above");
        };
        slot.parent.set(None);
        slot.in_flight.set(false);
        slot.generation = next.unwrap_or(RETIRED);
        if next.is_some() {
            self.free_head = Some(link);
        }
        self.len -= 1;
        Some(node)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (RenderId, &RenderNode)> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            // SAFETY: as in `get`.
            match unsafe { &*slot.state.get() } {
                SlotState::Occupied(node) => Some((id_at(index, slot.generation), node)),
                SlotState::Vacant { .. } | SlotState::Retired => None,
            }
        })
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = (RenderId, &mut RenderNode)> + '_ {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| {
                let generation = slot.generation;
                match slot.state.get_mut() {
                    SlotState::Occupied(node) => Some((id_at(index, generation), node)),
                    SlotState::Vacant { .. } | SlotState::Retired => None,
                }
            })
    }

    /// The occupant's parent as the slot header holds it, or `None` for a stale id.
    pub(super) fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.resolve(id)?.parent.get()
    }

    /// Records the occupant's parent in the slot header, where the link lives. A stale id writes
    /// nothing.
    pub(super) fn set_parent(&mut self, id: RenderId, parent: Option<RenderId>) {
        if let Some(slot) = self.resolve(id) {
            slot.parent.set(parent);
        }
    }

    /// Ages the slot the next insert takes to the last generation an id can carry, so a test
    /// reaches exhaustion without `Generation::LAST` removals. `false` when no slot is free.
    #[cfg(any(test, feature = "testing"))]
    pub(super) fn age_free_head(&mut self) -> bool {
        let Some(head) = self.free_head else {
            return false;
        };
        self.slots[head as usize].generation = Generation::LAST.as_nonzero();
        true
    }

    /// Takes the exclusive borrow for a layout walk.
    pub(super) fn lease(&mut self) -> TreeLease<'_> {
        TreeLease {
            base: self.slots.as_mut_ptr(),
            len: self.slots.len(),
            _slab: PhantomData,
        }
    }
}

/// A layout walk's access to the render tree: resolves ids and hands out nodes one at a time,
/// with no per-walk collection of the subtree. See the module docs for the invariants.
pub(crate) struct TreeLease<'a> {
    base: *mut Slot,
    len: usize,
    _slab: PhantomData<&'a mut NodeSlab>,
}

impl TreeLease<'_> {
    /// The slot `id`'s node occupies, or `None` for a stale id. A retired slot carries a
    /// generation no id has, so the generation alone answers.
    fn slot(&self, id: RenderId) -> Option<*mut Slot> {
        let index = id.index() as usize;
        if index >= self.len {
            return None;
        }
        // SAFETY: `index < len` keeps the offset inside the allocation `base` came from.
        let slot = unsafe { self.base.add(index) };
        // SAFETY: a field projection of a slot inside the leased allocation; the generation is a
        // header outside the `UnsafeCell` (invariant 4) and nothing writes it while the lease
        // lives (invariant 1).
        let generation = unsafe { (*slot).generation };
        (generation == id.generation().as_nonzero()).then_some(slot)
    }

    /// `true` when `child` is a current id and its header names `parent`. Reads headers only, so
    /// it is sound while `parent`'s node is borrowed by the walk.
    pub(crate) fn is_child_of(&self, child: RenderId, parent: RenderId) -> bool {
        self.parent(child) == Some(parent)
    }

    /// The parent of a current id, from its header. Reads no node, so it is sound while the node
    /// (or any other) is borrowed by the walk.
    pub(crate) fn parent(&self, id: RenderId) -> Option<RenderId> {
        let slot = self.slot(id)?;
        // SAFETY: a header field projection (invariant 4).
        unsafe { (*slot).parent.get() }
    }

    /// `true` while the node `id` names is borrowed up the call stack.
    pub(crate) fn is_in_flight(&self, id: RenderId) -> bool {
        self.slot(id).is_some_and(|slot| {
            // SAFETY: a header field projection (invariant 4).
            unsafe { (*slot).in_flight.get() }
        })
    }

    /// Sets the in-flight bit; `false` if the id is stale or the bit was already set.
    pub(crate) fn try_enter(&self, id: RenderId) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        // SAFETY: a header field projection (invariant 4).
        unsafe { !(*slot).in_flight.replace(true) }
    }

    /// Clears the in-flight bit of a current id.
    pub(crate) fn leave(&self, id: RenderId) {
        if let Some(slot) = self.slot(id) {
            // SAFETY: a header field projection (invariant 4).
            unsafe { (*slot).in_flight.set(false) }
        }
    }

    /// A pointer to the node `id` names, or `None` for a stale id.
    ///
    /// The caller must hold no live `&mut RenderNode` for this slot (it is not in flight, or it is
    /// the caller's own in-flight slot); the returned pointer is then the only access to it.
    pub(crate) fn node(&self, id: RenderId) -> Option<*mut RenderNode> {
        let slot = self.slot(id)?;
        // SAFETY: the state cell is a field projection of a slot inside the leased allocation;
        // the match reads the discriminant through the raw place and takes `&mut` to the payload
        // only, turned into a pointer at once. No other reference to this slot's state is live
        // (the contract above).
        unsafe {
            let state = UnsafeCell::raw_get(&raw const (*slot).state);
            match *state {
                SlotState::Occupied(ref mut node) => Some(std::ptr::from_mut(node)),
                SlotState::Vacant { .. } | SlotState::Retired => None,
            }
        }
    }

    /// Slots in the slab: an upper bound on how many nodes a walk can reach.
    pub(crate) fn capacity(&self) -> usize {
        self.len
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
        assert!(slab.remove(a).is_some());
        assert!(slab.remove(a).is_none(), "a stale id removes nothing");
        let reused = slab.insert(node());
        assert_eq!(
            reused.index(),
            a.index(),
            "the free list hands the slot back"
        );
        assert_ne!(reused, a, "under the next generation");

        let lease = slab.lease();
        assert!(lease.try_enter(b));
        assert!(!lease.try_enter(b), "a second enter is refused");
        assert!(lease.is_in_flight(b));
        lease.leave(b);
        assert!(!lease.is_in_flight(b));
        assert!(lease.node(b).is_some());
        assert!(lease.node(a).is_none(), "a stale id names no node");
        assert!(
            lease
                .node(RenderId::new_gen(
                    99,
                    flui_foundation::generation::Generation::FIRST
                ))
                .is_none()
        );
    }

    #[test]
    fn lease_reads_the_header_parent_while_a_node_is_borrowed() {
        let mut slab = NodeSlab::new();
        let a = slab.insert(node());
        let b = slab.insert(node());
        slab.set_parent(b, Some(a));

        let lease = slab.lease();
        assert!(lease.try_enter(a));
        let held = lease.node(a).expect("occupied");
        // SAFETY: the slot is in flight and this is the only access to it.
        let _live: &mut RenderNode = unsafe { &mut *held };
        assert_eq!(lease.parent(b), Some(a));
        assert!(lease.is_in_flight(a));
    }
}
