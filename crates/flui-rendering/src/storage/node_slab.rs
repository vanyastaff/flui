//! Slot storage for the render tree, with a lease for the layout walk.
//!
//! A [`NodeSlab`] is a `Vec` of slots with a free list, and the only place that mints a
//! [`RenderId`] or empties a slot. Each slot has a header (its generation, an in-flight bit and
//! the parent link, all outside the `UnsafeCell`) and a state: vacant, occupied by a
//! [`RenderNode`], or retired. The layout walk takes one [`TreeLease`] and enters slots one at a
//! time as it reaches them, instead of borrowing every node of the dirty subtree up front; an
//! [`EnteredNode`] is the walk's only way to a node.
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
//! 3. A reference into a slot's node exists only through an [`EnteredNode`]. [`TreeLease::enter`]
//!    makes one only after setting the slot's in-flight bit and refuses a slot whose bit is set,
//!    and the entry clears the bit when it drops, so at most one entry, and through it at most
//!    one `&mut RenderNode`, is live per slot.
//! 4. The headers are read and written through field projections of the raw slot pointer, never
//!    through a reference to the whole slot, so reading the header of a slot whose node is borrowed
//!    up the call stack touches no memory that borrow covers.

#![expect(
    unsafe_code,
    reason = "slots are `UnsafeCell`s reached through raw pointers under the invariants in the module docs"
)]

use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ptr::NonNull;

use flui_foundation::RenderId;
use flui_foundation::generation::Generation;

use super::node::RenderNode;

/// The header of a slot that has used every generation. It is above [`Generation::LAST`], so no
/// id carries it and no id matches a retired slot.
const RETIRED: NonZeroU32 = NonZeroU32::MAX;

/// What a slot holds. On a 32-bit target the node is under clippy's 200-byte variant gap, so
/// the lint fires only where pointers are 64 bits.
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::large_enum_variant,
        reason = "a vacant slot is the rare, short-lived state; boxing the node would put an allocation on every insert"
    )
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
    /// Set while an [`EnteredNode`] of the slot lives.
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
    #[cfg(any(test, feature = "testing"))]
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

/// Why [`TreeLease::enter`] lent no node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnterRefusal {
    /// The id is stale, its slot vacant or retired, or it never existed.
    Missing,
    /// The node is entered up the call stack: only a cyclic child link reaches it again.
    InFlight,
}

impl<'tree> TreeLease<'tree> {
    /// The slot `id`'s node occupies, or `None` for a stale id. A retired slot carries a
    /// generation no id has, so the generation alone answers.
    #[inline]
    fn slot(&self, id: RenderId) -> Option<*mut Slot> {
        let index = id.index() as usize;
        if index >= self.len {
            return None;
        }
        // SAFETY: `index < len` keeps the offset inside the allocation `base` came from
        // (invariants 1 and 2).
        let slot = unsafe { self.base.add(index) };
        // SAFETY: a field projection of a slot inside the leased allocation; the generation is a
        // header outside the `UnsafeCell` (invariant 4) and nothing writes it while the lease
        // lives (invariant 1).
        let generation = unsafe { (*slot).generation };
        (generation == id.generation().as_nonzero()).then_some(slot)
    }

    /// The in-flight bit of a slot [`Self::slot`] resolved.
    #[inline]
    #[expect(
        clippy::unused_self,
        reason = "the borrow of the lease is what bounds the header borrow (invariant 1)"
    )]
    fn in_flight(&self, slot: *mut Slot) -> &Cell<bool> {
        // SAFETY: a header field projection of a slot inside the leased allocation (invariant 4),
        // which stays in place for as long as this borrow of the lease (invariant 1).
        unsafe { &(*slot).in_flight }
    }

    /// `true` when `child` is a current id and its header names `parent`. Reads headers only, so
    /// it is sound while `parent`'s node is entered.
    pub(crate) fn is_child_of(&self, child: RenderId, parent: RenderId) -> bool {
        self.parent(child) == Some(parent)
    }

    /// The parent of a current id, from its header. Reads no node, so it is sound while the node
    /// (or any other) is entered.
    pub(crate) fn parent(&self, id: RenderId) -> Option<RenderId> {
        let slot = self.slot(id)?;
        // SAFETY: a header field projection (invariant 4).
        unsafe { (*slot).parent.get() }
    }

    /// Enters `id`: sets its in-flight bit and lends its node until the entry drops.
    ///
    /// A slot already in flight is refused with [`EnterRefusal::InFlight`] before its state is
    /// read, since the entry up the stack may hold its `&mut RenderNode`. A stale id, or a slot
    /// that holds no node, is [`EnterRefusal::Missing`] and leaves the bit as it found it.
    #[inline]
    pub(crate) fn enter(&self, id: RenderId) -> Result<EnteredNode<'_, 'tree>, EnterRefusal> {
        let slot = self.slot(id).ok_or(EnterRefusal::Missing)?;
        if self.in_flight(slot).replace(true) {
            return Err(EnterRefusal::InFlight);
        }
        // SAFETY: the state cell is a field projection of a slot inside the leased allocation
        // (invariants 1 and 2). The bit was clear and is now set, so no entry of this slot is
        // live and no reference into its state exists (invariant 3); the `&mut` to the payload
        // taken here becomes the entry's pointer at once.
        let node = unsafe {
            let state = UnsafeCell::raw_get(&raw const (*slot).state);
            match *state {
                SlotState::Occupied(ref mut node) => Some(NonNull::from(node)),
                SlotState::Vacant { .. } | SlotState::Retired => None,
            }
        };
        let Some(node) = node else {
            self.in_flight(slot).set(false);
            return Err(EnterRefusal::Missing);
        };
        Ok(EnteredNode {
            lease: self,
            id,
            node,
        })
    }

    /// Slots in the slab: an upper bound on how many nodes a walk can reach.
    pub(crate) fn capacity(&self) -> usize {
        self.len
    }
}

/// One node entered through a [`TreeLease`]: its slot's in-flight bit is set while this value
/// lives, and the node is reachable only through it. Dropping it clears the bit, on unwind too.
pub(crate) struct EnteredNode<'lease, 'tree> {
    lease: &'lease TreeLease<'tree>,
    id: RenderId,
    node: NonNull<RenderNode>,
}

impl EnteredNode<'_, '_> {
    /// The id this entry was made for.
    #[inline]
    pub(crate) fn id(&self) -> RenderId {
        self.id
    }

    /// The entered node, shared for as long as this borrow of the entry.
    #[inline]
    pub(crate) fn node(&self) -> &RenderNode {
        // SAFETY: the pointer came from the lease, whose slab borrow outlives this entry
        // (invariant 1), with the allocation's provenance (invariant 2). This entry holds the
        // slot's in-flight bit, so it is the only way to the node (invariant 3), and `&self`
        // excludes the `&mut` that `node_mut` lends.
        unsafe { self.node.as_ref() }
    }

    /// The entered node, exclusive for as long as this borrow of the entry.
    #[inline]
    pub(crate) fn node_mut(&mut self) -> &mut RenderNode {
        // SAFETY: as in `node`; `&mut self` excludes every other reference this entry lent.
        unsafe { self.node.as_mut() }
    }
}

impl Drop for EnteredNode<'_, '_> {
    fn drop(&mut self) {
        // The id resolves: no generation changes while the lease lives (invariant 1).
        if let Some(slot) = self.lease.slot(self.id) {
            self.lease.in_flight(slot).set(false);
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
        fn paint(&self, _ctx: &mut crate::context::PaintContext<'_, Leaf>) {}
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
        let entered = lease.enter(b).expect("BUG: b is live");
        assert_eq!(entered.id(), b);
        assert_eq!(
            lease.enter(b).err(),
            Some(EnterRefusal::InFlight),
            "a second enter is refused"
        );
        drop(entered);
        assert!(lease.enter(b).is_ok(), "dropping the entry clears the bit");
        assert_eq!(
            lease.enter(a).err(),
            Some(EnterRefusal::Missing),
            "a stale id names no node"
        );
        let never = RenderId::new_gen(99, flui_foundation::generation::Generation::FIRST);
        assert_eq!(lease.enter(never).err(), Some(EnterRefusal::Missing));
    }

    #[test]
    fn lease_refuses_a_vacant_slot_and_leaves_its_bit_clear() {
        let mut slab = NodeSlab::new();
        let a = slab.insert(node());
        let _ = slab.remove(a);
        // The id the vacant slot's next occupant will carry matches its generation.
        let next = RenderId::new_gen(
            a.index(),
            flui_foundation::generation::Generation::try_from(slab.slots[0].generation)
                .expect("BUG: a vacant slot's generation is an id's"),
        );

        let lease = slab.lease();
        assert_eq!(lease.enter(next).err(), Some(EnterRefusal::Missing));
        assert_eq!(
            lease.enter(next).err(),
            Some(EnterRefusal::Missing),
            "a refused vacant slot is not left in flight"
        );
    }

    #[test]
    fn lease_reads_the_header_parent_while_a_node_is_borrowed() {
        let mut slab = NodeSlab::new();
        let a = slab.insert(node());
        let b = slab.insert(node());
        slab.set_parent(b, Some(a));

        let lease = slab.lease();
        let mut held = lease.enter(a).expect("BUG: a is live");
        let _live: &mut RenderNode = held.node_mut();
        assert_eq!(lease.parent(b), Some(a));
        assert!(lease.is_child_of(b, a));
        assert_eq!(lease.enter(a).err(), Some(EnterRefusal::InFlight));
        drop(held);
    }

    static_assertions::assert_not_impl_any!(EnteredNode<'_, '_>: Send, Sync);
}
